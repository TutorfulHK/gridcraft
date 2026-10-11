//! Model → Workbook JSON v1.

use std::collections::{BTreeMap, HashMap};

use gridcraft_core::{CellRef, RangeRef, Value};
use gridcraft_model::{
    BorderLine, BorderStyle, CfOperator, CfRule, CfValueKind, Color, ErrorStyle, HAlign, PatternType, Sheet, Style, StyleId, Theme, TotalsFn,
    Underline, VAlign, ValidationKind, VertAlign, Visibility, Workbook,
};
use gridcraft_numfmt::NumberFormat;
use serde_json::Value as Json;

use super::*;

/// Model sizes are pixels at 96 dpi; the JSON uses points, rounded to 0.01.
pub(super) fn px_to_pt(px: f32) -> f64 {
    let pt = px as f64 * 0.75;
    if pt.is_finite() { (pt * 100.0).round() / 100.0 } else { 0.0 }
}

fn hex(c: Color, theme: &Theme) -> Option<String> {
    c.hex(theme)
}

/// The workbook's distinct pictures: (id, content type, bytes), in sheet order. Identical bytes
/// share an id. The ids are what [`to_json`] writes in `images[].media`.
pub fn media_table(wb: &Workbook) -> Vec<(String, String, &[u8])> {
    let mut out: Vec<(String, String, &[u8])> = Vec::new();
    for sh in &wb.sheets {
        for im in &sh.images {
            if !out.iter().any(|(_, _, b)| *b == im.data.as_slice()) {
                out.push((format!("image{}", out.len() + 1), im.mime.clone(), &im.data));
            }
        }
    }
    out
}

fn ext_for(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpeg",
        "image/gif" => "gif",
        "image/bmp" => "bmp",
        "image/svg+xml" => "svg",
        "image/webp" => "webp",
        "image/tiff" => "tiff",
        _ => "bin",
    }
}

/// Cell styles numbered as the JSON's `styles` table: model id → JSON index, deduplicated by
/// their JSON form (two model styles can read the same, e.g. a theme colour and its RGB).
struct Styles<'a> {
    wb: &'a Workbook,
    by_id: HashMap<StyleId, u32>,
    by_json: HashMap<String, u32>,
    list: Vec<StyleJson>,
    dates: HashMap<StyleId, bool>,
}

impl<'a> Styles<'a> {
    fn new(wb: &'a Workbook) -> Styles<'a> {
        let mut s = Styles { wb, by_id: HashMap::new(), by_json: HashMap::new(), list: Vec::new(), dates: HashMap::new() };
        s.index(StyleId::DEFAULT);
        s
    }
    fn index(&mut self, id: StyleId) -> u32 {
        if let Some(i) = self.by_id.get(&id) {
            return *i;
        }
        let js = style_json(self.wb.styles.get(id), &self.wb.theme);
        let key = serde_json::to_string(&js).unwrap_or_default();
        let i = match self.by_json.get(&key) {
            Some(i) => *i,
            None => {
                let i = u32::try_from(self.list.len()).unwrap_or(u32::MAX);
                self.list.push(js);
                self.by_json.insert(key, i);
                i
            }
        };
        self.by_id.insert(id, i);
        i
    }
    /// `Some(index)` unless it is the default style.
    fn opt(&mut self, id: StyleId) -> Option<u32> {
        Some(self.index(id)).filter(|i| *i != 0)
    }
    fn is_date(&mut self, id: StyleId) -> bool {
        if let Some(d) = self.dates.get(&id) {
            return *d;
        }
        let d = NumberFormat::parse(self.wb.styles.get(id).num_fmt.as_str()).is_date();
        self.dates.insert(id, d);
        d
    }
}

pub(super) fn style_json(s: &Style, theme: &Theme) -> StyleJson {
    let f = &s.font;
    let font = FontJson {
        name: f.name.clone(),
        size: f.size as f64,
        bold: f.bold,
        italic: f.italic,
        underline: match f.underline {
            Underline::None => UnderlineJson::None,
            Underline::Single => UnderlineJson::Single,
            Underline::Double => UnderlineJson::Double,
            Underline::SingleAccounting => UnderlineJson::SingleAccounting,
            Underline::DoubleAccounting => UnderlineJson::DoubleAccounting,
        },
        strike: f.strike,
        color: hex(f.color, theme),
        script: match f.vert {
            VertAlign::Baseline => None,
            VertAlign::Superscript => Some(ScriptJson::Superscript),
            VertAlign::Subscript => Some(ScriptJson::Subscript),
        },
    };
    let fill = match s.fill.pattern {
        PatternType::None => None,
        PatternType::Solid => Some(FillJson { color: hex(s.fill.fg, theme), pattern: None, bg_color: None }),
        p => Some(FillJson { color: hex(s.fill.fg, theme), pattern: Some(pattern_json(p)), bg_color: hex(s.fill.bg, theme) }),
    };
    let b = &s.borders;
    let line = |l: &BorderLine| border_line(l, theme);
    let border = BorderJson {
        top: line(&b.top),
        right: line(&b.right),
        bottom: line(&b.bottom),
        left: line(&b.left),
        diag_down: line(&b.diag_down),
        diag_up: line(&b.diag_up),
    };
    let a = &s.align;
    let align = AlignJson {
        h: match a.h {
            HAlign::General => HAlignJson::General,
            HAlign::Left => HAlignJson::Left,
            HAlign::Center => HAlignJson::Center,
            HAlign::Right => HAlignJson::Right,
            HAlign::Fill => HAlignJson::Fill,
            HAlign::Justify => HAlignJson::Justify,
            HAlign::CenterAcross => HAlignJson::CenterContinuous,
            HAlign::Distributed => HAlignJson::Distributed,
        },
        v: match a.v {
            VAlign::Top => VAlignJson::Top,
            VAlign::Center => VAlignJson::Center,
            VAlign::Bottom => VAlignJson::Bottom,
            VAlign::Justify => VAlignJson::Justify,
            VAlign::Distributed => VAlignJson::Distributed,
        },
        wrap: a.wrap,
        indent: a.indent as u32,
        rotate: a.rotation as i32,
        shrink: a.shrink,
    };
    let protection = (s.protection != gridcraft_model::style::Protection::default())
        .then_some(ProtectionJson { locked: s.protection.locked, hidden: s.protection.hidden });
    StyleJson { font, fill, border, align, num_fmt: s.num_fmt.as_str().to_string(), protection }
}

fn border_line(l: &BorderLine, theme: &Theme) -> Option<BorderLineJson> {
    let style = match l.style {
        BorderStyle::None => return None,
        BorderStyle::Thin => BorderStyleJson::Thin,
        BorderStyle::Medium => BorderStyleJson::Medium,
        BorderStyle::Thick => BorderStyleJson::Thick,
        BorderStyle::Dashed => BorderStyleJson::Dashed,
        BorderStyle::Dotted => BorderStyleJson::Dotted,
        BorderStyle::Double => BorderStyleJson::Double,
        BorderStyle::Hair => BorderStyleJson::Hair,
        BorderStyle::MediumDashed => BorderStyleJson::MediumDashed,
        BorderStyle::DashDot => BorderStyleJson::DashDot,
        BorderStyle::MediumDashDot => BorderStyleJson::MediumDashDot,
        BorderStyle::DashDotDot => BorderStyleJson::DashDotDot,
        BorderStyle::MediumDashDotDot => BorderStyleJson::MediumDashDotDot,
        BorderStyle::SlantDashDot => BorderStyleJson::SlantDashDot,
    };
    Some(BorderLineJson { style, color: hex(l.color, theme) })
}

pub(super) const TOTALS: &[(TotalsFn, TotalsJson)] = &[
    (TotalsFn::None, TotalsJson::None),
    (TotalsFn::Average, TotalsJson::Average),
    (TotalsFn::Count, TotalsJson::Count),
    (TotalsFn::CountNums, TotalsJson::CountNums),
    (TotalsFn::Max, TotalsJson::Max),
    (TotalsFn::Min, TotalsJson::Min),
    (TotalsFn::Sum, TotalsJson::Sum),
    (TotalsFn::StdDev, TotalsJson::StdDev),
    (TotalsFn::Var, TotalsJson::Var),
    (TotalsFn::Custom, TotalsJson::Custom),
];

pub(super) const PATTERNS: &[(PatternType, PatternJson)] = &[
    (PatternType::Solid, PatternJson::Solid),
    (PatternType::Gray50, PatternJson::MediumGray),
    (PatternType::Gray75, PatternJson::DarkGray),
    (PatternType::Gray25, PatternJson::LightGray),
    (PatternType::Gray125, PatternJson::Gray125),
    (PatternType::Gray0625, PatternJson::Gray0625),
    (PatternType::DarkHorizontal, PatternJson::DarkHorizontal),
    (PatternType::DarkVertical, PatternJson::DarkVertical),
    (PatternType::DarkDown, PatternJson::DarkDown),
    (PatternType::DarkUp, PatternJson::DarkUp),
    (PatternType::DarkGrid, PatternJson::DarkGrid),
    (PatternType::DarkTrellis, PatternJson::DarkTrellis),
    (PatternType::LightHorizontal, PatternJson::LightHorizontal),
    (PatternType::LightVertical, PatternJson::LightVertical),
    (PatternType::LightDown, PatternJson::LightDown),
    (PatternType::LightUp, PatternJson::LightUp),
    (PatternType::LightGrid, PatternJson::LightGrid),
    (PatternType::LightTrellis, PatternJson::LightTrellis),
];

fn pattern_json(p: PatternType) -> PatternJson {
    PATTERNS.iter().find(|(m, _)| *m == p).map(|(_, j)| *j).unwrap_or(PatternJson::Solid)
}

pub(super) fn operator_json(op: CfOperator) -> OperatorJson {
    match op {
        CfOperator::Between => OperatorJson::Between,
        CfOperator::NotBetween => OperatorJson::NotBetween,
        CfOperator::Equal => OperatorJson::Equal,
        CfOperator::NotEqual => OperatorJson::NotEqual,
        CfOperator::Greater => OperatorJson::GreaterThan,
        CfOperator::Less => OperatorJson::LessThan,
        CfOperator::GreaterOrEqual => OperatorJson::GreaterThanOrEqual,
        CfOperator::LessOrEqual => OperatorJson::LessThanOrEqual,
    }
}

fn cf_value(v: &CfValueKind) -> CfValueJson {
    match v {
        CfValueKind::Min => CfValueJson::Min,
        CfValueKind::Max => CfValueJson::Max,
        CfValueKind::Number(n) => CfValueJson::Number { value: *n },
        CfValueKind::Percent(n) => CfValueJson::Percent { value: *n },
        CfValueKind::Percentile(n) => CfValueJson::Percentile { value: *n },
        CfValueKind::Formula(f) => CfValueJson::Formula { value: f.clone() },
    }
}

fn cf_rule(r: &CfRule, theme: &Theme) -> CfRuleJson {
    let st = |s: &Style| style_json(s, theme);
    let color = |c: &Color| hex(*c, theme).unwrap_or_else(|| "#000000".into());
    match r {
        CfRule::CellIs { op, a, b, style } => {
            CfRuleJson::CellIs { operator: operator_json(*op), formula1: a.clone(), formula2: b.clone(), style: st(style) }
        }
        CfRule::Expression { formula, style } => CfRuleJson::Expression { formula: formula.clone(), style: st(style) },
        CfRule::ContainsText { text, style } => CfRuleJson::ContainsText { text: text.clone(), style: st(style) },
        CfRule::NotContainsText { text, style } => CfRuleJson::NotContainsText { text: text.clone(), style: st(style) },
        CfRule::BeginsWith { text, style } => CfRuleJson::BeginsWith { text: text.clone(), style: st(style) },
        CfRule::EndsWith { text, style } => CfRuleJson::EndsWith { text: text.clone(), style: st(style) },
        CfRule::Blanks { style } => CfRuleJson::Blanks { style: st(style) },
        CfRule::NoBlanks { style } => CfRuleJson::NoBlanks { style: st(style) },
        CfRule::Errors { style } => CfRuleJson::Errors { style: st(style) },
        CfRule::NoErrors { style } => CfRuleJson::NoErrors { style: st(style) },
        CfRule::Duplicate { style } => CfRuleJson::Duplicate { style: st(style) },
        CfRule::Unique { style } => CfRuleJson::Unique { style: st(style) },
        CfRule::Top10 { bottom, percent, rank, style } => CfRuleJson::Top10 { bottom: *bottom, percent: *percent, rank: *rank, style: st(style) },
        CfRule::AboveAverage { below, equal, std_dev, style } => {
            CfRuleJson::AboveAverage { below: *below, equal: *equal, std_dev: *std_dev as u32, style: st(style) }
        }
        CfRule::TimePeriod { period, style } => CfRuleJson::TimePeriod { period: period.clone(), style: st(style) },
        CfRule::ColorScale { stops } => {
            CfRuleJson::ColorScale { stops: stops.iter().map(|(v, c)| ColorStopJson { value: cf_value(v), color: color(c) }).collect() }
        }
        CfRule::DataBar { min, max, color: c, gradient, show_value } => {
            CfRuleJson::DataBar { min: cf_value(min), max: cf_value(max), color: color(c), gradient: *gradient, show_value: *show_value }
        }
        CfRule::IconSet { set, thresholds, reverse, show_value } => CfRuleJson::IconSet {
            set: set.clone(),
            thresholds: thresholds.iter().map(cf_value).collect(),
            reverse: *reverse,
            show_value: *show_value,
        },
    }
}

/// A cell value as (JSON value, type). Arrays (a dynamic-array anchor) give their first item.
fn value_json(v: &Value, date: bool) -> (Json, Option<CellType>) {
    match v {
        Value::Empty => (Json::Null, None),
        Value::Number(n) => {
            (serde_json::Number::from_f64(*n).map(Json::Number).unwrap_or(Json::Null), Some(if date { CellType::Date } else { CellType::Number }))
        }
        Value::Text(t) => (Json::String(t.to_string()), Some(CellType::Text)),
        Value::Bool(b) => (Json::Bool(*b), Some(CellType::Bool)),
        Value::Error(e) => (Json::String(e.as_str().to_string()), Some(CellType::Error)),
        Value::Array(a) => value_json(a.data.first().unwrap_or(&Value::Empty), date),
    }
}

/// The workbook as Workbook JSON v1. With `inline_media`, pictures carry their bytes as base64.
pub fn to_json(wb: &Workbook, inline_media: bool) -> WorkbookJson {
    let mut styles = Styles::new(wb);
    let media = media_table(wb);
    let media_id = |data: &[u8]| media.iter().find(|(_, _, b)| *b == data).map(|(id, _, _)| id.clone()).unwrap_or_default();
    let sheets = wb.sheets.iter().map(|sh| sheet_json(wb, sh, &mut styles, &media_id)).collect();
    let defined_names = wb
        .names
        .iter()
        .map(|n| NameJson { name: n.name.clone(), reference: n.formula.clone(), scope: n.scope, hidden: n.hidden, comment: n.comment.clone() })
        .collect();
    let media = media
        .iter()
        .map(|(id, mime, data)| MediaJson {
            id: id.clone(),
            content_type: mime.clone(),
            name: format!("{id}.{}", ext_for(mime)),
            data: inline_media.then(|| crate::io::base64_encode(data)),
        })
        .collect();
    WorkbookJson { version: VERSION, sheets, styles: styles.list, defined_names, active_sheet: wb.active_sheet, media }
}

fn sheet_json(wb: &Workbook, sh: &Sheet, styles: &mut Styles, media_id: &dyn Fn(&[u8]) -> String) -> SheetJson {
    let theme = &wb.theme;
    // Grid extent: everything that sits somewhere on the sheet.
    let mut rows_n: u32 = 0;
    let mut cols_n: u32 = 0;
    let mut grow = |c: CellRef| {
        rows_n = rows_n.max(c.row.saturating_add(1));
        cols_n = cols_n.max(c.col.saturating_add(1));
    };
    if let Some(r) = sh.used_range() {
        grow(r.end);
    }
    for m in &sh.merges {
        grow(m.end);
    }
    for c in sh.comments.keys().chain(sh.hyperlinks.keys()) {
        grow(*c);
    }
    for im in &sh.images {
        grow(im.anchor.cell);
    }
    if let Some((r, c)) = sh.freeze {
        grow(CellRef::new(r, c));
    }
    if let Some(r) = sh.rows.keys().next_back() {
        rows_n = rows_n.max(r.saturating_add(1));
    }
    if let Some(c) = sh.cols.keys().next_back() {
        cols_n = cols_n.max(c.saturating_add(1));
    }

    let cols = sh
        .cols
        .iter()
        .map(|(&index, i)| ColJson {
            index,
            width: i.size.map(px_to_pt),
            hidden: i.hidden,
            level: i.outline as u32,
            s: i.style.and_then(|s| styles.opt(s)),
        })
        .collect();
    let rows = sh
        .rows
        .iter()
        .map(|(&index, i)| RowJson {
            index,
            height: i.size.map(px_to_pt),
            hidden: i.hidden,
            auto: i.size.is_some() && !i.custom,
            level: i.outline as u32,
            s: i.style.and_then(|s| styles.opt(s)),
        })
        .collect();

    // Stored cells, then spilled values that have no stored cell, in row-major order.
    let mut cells: BTreeMap<(u32, u32), CellJson> = BTreeMap::new();
    for (at, cell) in sh.cells.iter() {
        let date = styles.is_date(cell.style);
        let s = styles.opt(cell.style);
        let own = !cell.value.is_empty() || cell.formula.is_some();
        let spilled = if own { None } else { sh.spill.get(&at) };
        let (v, t) = value_json(if own { &cell.value } else { spilled.unwrap_or(&Value::Empty) }, date);
        let f = cell.formula.as_ref().map(|f| format!("={}", f.text));
        let array = cell.formula.as_ref().and_then(|f| f.array).map(|r| r.a1());
        cells.insert((at.row, at.col), CellJson { r: at.row, c: at.col, v, f, t, s, array, spill: spilled.is_some() });
    }
    for (at, val) in &sh.spill {
        if cells.contains_key(&(at.row, at.col)) {
            continue;
        }
        let (v, t) = value_json(val, styles.is_date(StyleId::DEFAULT));
        cells.insert((at.row, at.col), CellJson { r: at.row, c: at.col, v, f: None, t, s: None, array: None, spill: true });
    }

    let merges = sh.merges.iter().map(|m: &RangeRef| MergeJson::Rect { r1: m.start.row, c1: m.start.col, r2: m.end.row, c2: m.end.col }).collect();

    let conditional_formats = sh
        .cond_formats
        .iter()
        .map(|cf| CondFormatJson {
            ranges: cf.ranges.iter().map(|r| r.a1()).collect(),
            priority: cf.priority,
            stop_if_true: cf.stop_if_true,
            rule: cf_rule(&cf.rule, theme),
        })
        .collect();
    let validations = sh
        .validations
        .iter()
        .map(|v| ValidationJson {
            ranges: v.ranges.iter().map(|r| r.a1()).collect(),
            kind: match v.kind {
                ValidationKind::Any => ValidationKindJson::Any,
                ValidationKind::Whole => ValidationKindJson::Whole,
                ValidationKind::Decimal => ValidationKindJson::Decimal,
                ValidationKind::List => ValidationKindJson::List,
                ValidationKind::Date => ValidationKindJson::Date,
                ValidationKind::Time => ValidationKindJson::Time,
                ValidationKind::TextLength => ValidationKindJson::TextLength,
                ValidationKind::Custom => ValidationKindJson::Custom,
            },
            operator: operator_json(v.op),
            formula1: v.f1.clone(),
            formula2: v.f2.clone(),
            allow_blank: v.allow_blank,
            show_dropdown: v.in_cell_dropdown,
            input_title: v.input_title.clone(),
            input_message: v.input_message.clone(),
            show_input: v.show_input,
            error_title: v.error_title.clone(),
            error_message: v.error_message.clone(),
            error_style: match v.error_style {
                ErrorStyle::Stop => ErrorStyleJson::Stop,
                ErrorStyle::Warning => ErrorStyleJson::Warning,
                ErrorStyle::Information => ErrorStyleJson::Information,
            },
            show_error: v.show_error,
        })
        .collect();
    let hyperlinks =
        sh.hyperlinks.iter().map(|(c, h)| HyperlinkJson { r: c.row, c: c.col, url: h.target.clone(), tooltip: h.tooltip.clone() }).collect();
    let comments = sh
        .comments
        .iter()
        .map(|(c, cm)| CommentJson {
            r: c.row,
            c: c.col,
            text: cm.text.clone(),
            author: cm.author.clone(),
            replies: cm.replies.iter().map(|(a, t)| ReplyJson { author: a.clone(), text: t.clone() }).collect(),
            threaded: cm.threaded,
            resolved: cm.resolved,
            visible: cm.visible,
        })
        .collect();
    let images = sh
        .images
        .iter()
        .map(|im| ImageJson {
            media: media_id(&im.data),
            anchor: AnchorJson { r: im.anchor.cell.row, c: im.anchor.cell.col, dx: px_to_pt(im.anchor.dx), dy: px_to_pt(im.anchor.dy) },
            w: px_to_pt(im.anchor.width),
            h: px_to_pt(im.anchor.height),
            alt: im.alt.clone(),
        })
        .collect();

    let tables = sh
        .tables
        .iter()
        .map(|t| TableJson {
            name: t.name.clone(),
            range: t.range.a1(),
            header_row: t.header_row,
            totals_row: t.totals_row,
            columns: t
                .columns
                .iter()
                .map(|c| TableColumnJson {
                    name: c.name.clone(),
                    totals: TOTALS.iter().find(|(m, _)| *m == c.totals).map(|(_, j)| *j).unwrap_or_default(),
                    totals_label: c.totals_label.clone(),
                    formula: c.formula.clone(),
                })
                .collect(),
            style: t.style.clone(),
            banded_rows: t.banded_rows,
            banded_cols: t.banded_cols,
            first_col: t.first_col,
            last_col: t.last_col,
            filter_button: t.filter_button,
        })
        .collect();

    SheetJson {
        name: sh.name.clone(),
        hidden: sh.visibility != Visibility::Visible,
        very_hidden: sh.visibility == Visibility::VeryHidden,
        row_count: rows_n.max(1),
        col_count: cols_n.max(1),
        default_col_width: px_to_pt(sh.default_col_width),
        default_row_height: px_to_pt(sh.default_row_height),
        cols,
        rows,
        freeze: sh.freeze.filter(|f| *f != (0, 0)).map(|(rows, cols)| FreezeJson { rows, cols }),
        merges,
        cells: cells.into_values().collect(),
        conditional_formats,
        validations,
        hyperlinks,
        comments,
        images,
        tables,
        tab_color: sh.tab_color.and_then(|c| hex(c, theme)),
        show_gridlines: sh.show_gridlines,
        zoom: sh.zoom,
        right_to_left: sh.right_to_left,
    }
}
