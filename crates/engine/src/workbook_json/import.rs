//! Workbook JSON v1 → model. Every index, size and reference from the JSON is checked: bad input
//! is an error naming the problem (or a clamp for sizes), never a panic.

use std::sync::Arc;

use gridcraft_core::{CellError, CellRef, MAX_COLS, MAX_ROWS, RangeRef, Value};
use gridcraft_model::style::Protection;
use gridcraft_model::{
    Alignment, Anchor, BorderLine, BorderStyle, Borders, Cell, CfOperator, CfRule, CfValueKind, Color, Comment, CondFormat, DefinedName, ErrorStyle,
    Fill, Font, Formula, HAlign, Hyperlink, Image, LineInfo, NumFmt, PatternType, Sheet, Style, StyleId, Table, TableColumn, Underline, VAlign,
    Validation, ValidationKind, VertAlign, Visibility, Workbook,
};
use serde_json::Value as Json;

use super::*;

/// Caps on what a workbook JSON may ask for (beyond the sheet's own row/column limits).
const MAX_SHEETS: usize = 4096;
const MAX_STYLES: usize = 65_536;
const MAX_MERGES_CHECKED: usize = 20_000;

type R<T> = std::result::Result<T, String>;

/// Media bytes and content type for a media id.
pub type MediaLookup<'a> = dyn Fn(&str) -> Option<(Arc<Vec<u8>>, String)> + 'a;

fn pt_to_px(pt: f64, max_px: f32) -> f32 {
    if !pt.is_finite() || pt <= 0.0 { 0.0 } else { ((pt * 4.0 / 3.0) as f32).min(max_px) }
}

fn color(s: Option<&str>, what: &str) -> R<Color> {
    match s {
        None => Ok(Color::Auto),
        Some(t) if t.trim().is_empty() => Ok(Color::Auto),
        Some(t) => Color::from_hex(t).ok_or_else(|| format!("{what}: `{t}` is not a #RRGGBB colour")),
    }
}

fn cell_at(r: u32, c: u32, what: &str) -> R<CellRef> {
    if r >= MAX_ROWS || c >= MAX_COLS {
        return Err(format!("{what}: row {r}, column {c} is outside the sheet ({MAX_ROWS} rows × {MAX_COLS} columns)"));
    }
    Ok(CellRef::new(r, c))
}

fn range(a1: &str, what: &str) -> R<RangeRef> {
    let body = a1.trim().rsplit_once('!').map(|(_, b)| b).unwrap_or(a1.trim());
    RangeRef::parse(body).ok_or_else(|| format!("{what}: `{a1}` is not a range"))
}

/// A style from its JSON form.
pub(super) fn style(js: &StyleJson) -> R<Style> {
    let f = &js.font;
    let size = if f.size.is_finite() && f.size > 0.0 { f.size.clamp(1.0, 409.0) as f32 } else { gridcraft_model::DEFAULT_FONT_SIZE };
    let name: String = f.name.chars().take(64).collect();
    let font = Font {
        name: if name.trim().is_empty() { gridcraft_model::DEFAULT_FONT.into() } else { name },
        size,
        bold: f.bold,
        italic: f.italic,
        underline: match f.underline {
            UnderlineJson::None => Underline::None,
            UnderlineJson::Single => Underline::Single,
            UnderlineJson::Double => Underline::Double,
            UnderlineJson::SingleAccounting => Underline::SingleAccounting,
            UnderlineJson::DoubleAccounting => Underline::DoubleAccounting,
        },
        strike: f.strike,
        color: color(f.color.as_deref(), "font colour")?,
        vert: match f.script {
            None => VertAlign::Baseline,
            Some(ScriptJson::Superscript) => VertAlign::Superscript,
            Some(ScriptJson::Subscript) => VertAlign::Subscript,
        },
    };
    let fill = match &js.fill {
        None => Fill::default(),
        Some(fj) => {
            let pattern = match fj.pattern {
                None => PatternType::Solid,
                Some(p) => export::PATTERNS.iter().find(|(_, j)| *j == p).map(|(m, _)| *m).unwrap_or(PatternType::Solid),
            };
            let fg = color(fj.color.as_deref(), "fill colour")?;
            if fg == Color::Auto && pattern == PatternType::Solid {
                Fill::default()
            } else {
                Fill { pattern, fg, bg: color(fj.bg_color.as_deref(), "fill background colour")? }
            }
        }
    };
    let b = &js.border;
    let borders = Borders {
        left: border_line(b.left.as_ref())?,
        right: border_line(b.right.as_ref())?,
        top: border_line(b.top.as_ref())?,
        bottom: border_line(b.bottom.as_ref())?,
        diag_down: border_line(b.diag_down.as_ref())?,
        diag_up: border_line(b.diag_up.as_ref())?,
    };
    let a = &js.align;
    let align = Alignment {
        h: match a.h {
            HAlignJson::General => HAlign::General,
            HAlignJson::Left => HAlign::Left,
            HAlignJson::Center => HAlign::Center,
            HAlignJson::Right => HAlign::Right,
            HAlignJson::Fill => HAlign::Fill,
            HAlignJson::Justify => HAlign::Justify,
            HAlignJson::CenterContinuous => HAlign::CenterAcross,
            HAlignJson::Distributed => HAlign::Distributed,
        },
        v: match a.v {
            VAlignJson::Top => VAlign::Top,
            VAlignJson::Center => VAlign::Center,
            VAlignJson::Bottom => VAlign::Bottom,
            VAlignJson::Justify => VAlign::Justify,
            VAlignJson::Distributed => VAlign::Distributed,
        },
        wrap: a.wrap,
        shrink: a.shrink,
        indent: a.indent.min(250) as u8,
        rotation: if a.rotate == 255 { 255 } else { a.rotate.clamp(-90, 90) as i16 },
    };
    let code = js.num_fmt.trim();
    let num_fmt = if code.is_empty() { NumFmt::default() } else { NumFmt::new(&js.num_fmt.chars().take(255).collect::<String>()) };
    let protection = js.protection.map(|p| Protection { locked: p.locked, hidden: p.hidden }).unwrap_or_default();
    Ok(Style { font, fill, borders, align, num_fmt, protection })
}

fn border_line(l: Option<&BorderLineJson>) -> R<BorderLine> {
    let Some(l) = l else { return Ok(BorderLine::default()) };
    let style = match l.style {
        BorderStyleJson::Thin => BorderStyle::Thin,
        BorderStyleJson::Medium => BorderStyle::Medium,
        BorderStyleJson::Thick => BorderStyle::Thick,
        BorderStyleJson::Dashed => BorderStyle::Dashed,
        BorderStyleJson::Dotted => BorderStyle::Dotted,
        BorderStyleJson::Double => BorderStyle::Double,
        BorderStyleJson::Hair => BorderStyle::Hair,
        BorderStyleJson::MediumDashed => BorderStyle::MediumDashed,
        BorderStyleJson::DashDot => BorderStyle::DashDot,
        BorderStyleJson::MediumDashDot => BorderStyle::MediumDashDot,
        BorderStyleJson::DashDotDot => BorderStyle::DashDotDot,
        BorderStyleJson::MediumDashDotDot => BorderStyle::MediumDashDotDot,
        BorderStyleJson::SlantDashDot => BorderStyle::SlantDashDot,
    };
    Ok(BorderLine { style, color: color(l.color.as_deref(), "border colour")? })
}

fn operator(op: OperatorJson) -> CfOperator {
    match op {
        OperatorJson::Between => CfOperator::Between,
        OperatorJson::NotBetween => CfOperator::NotBetween,
        OperatorJson::Equal => CfOperator::Equal,
        OperatorJson::NotEqual => CfOperator::NotEqual,
        OperatorJson::GreaterThan => CfOperator::Greater,
        OperatorJson::LessThan => CfOperator::Less,
        OperatorJson::GreaterThanOrEqual => CfOperator::GreaterOrEqual,
        OperatorJson::LessThanOrEqual => CfOperator::LessOrEqual,
    }
}

fn finite(n: f64, what: &str) -> R<f64> {
    if n.is_finite() { Ok(n) } else { Err(format!("{what}: not a finite number")) }
}

fn cf_value(v: &CfValueJson) -> R<CfValueKind> {
    Ok(match v {
        CfValueJson::Min => CfValueKind::Min,
        CfValueJson::Max => CfValueKind::Max,
        CfValueJson::Number { value } => CfValueKind::Number(finite(*value, "threshold")?),
        CfValueJson::Percent { value } => CfValueKind::Percent(finite(*value, "threshold")?),
        CfValueJson::Percentile { value } => CfValueKind::Percentile(finite(*value, "threshold")?),
        CfValueJson::Formula { value } => CfValueKind::Formula(strip_eq(value)),
    })
}

fn strip_eq(f: &str) -> String {
    f.trim().strip_prefix('=').unwrap_or(f.trim()).to_string()
}

fn cf_rule(r: &CfRuleJson) -> R<CfRule> {
    let st = |s: &StyleJson| style(s).map(Box::new);
    let col = |c: &str| color(Some(c), "conditional format colour");
    Ok(match r {
        CfRuleJson::CellIs { operator: op, formula1, formula2, style: s } => {
            CfRule::CellIs { op: operator(*op), a: strip_eq(formula1), b: formula2.as_deref().map(strip_eq), style: st(s)? }
        }
        CfRuleJson::Expression { formula, style: s } => CfRule::Expression { formula: strip_eq(formula), style: st(s)? },
        CfRuleJson::ContainsText { text, style: s } => CfRule::ContainsText { text: text.clone(), style: st(s)? },
        CfRuleJson::NotContainsText { text, style: s } => CfRule::NotContainsText { text: text.clone(), style: st(s)? },
        CfRuleJson::BeginsWith { text, style: s } => CfRule::BeginsWith { text: text.clone(), style: st(s)? },
        CfRuleJson::EndsWith { text, style: s } => CfRule::EndsWith { text: text.clone(), style: st(s)? },
        CfRuleJson::Blanks { style: s } => CfRule::Blanks { style: st(s)? },
        CfRuleJson::NoBlanks { style: s } => CfRule::NoBlanks { style: st(s)? },
        CfRuleJson::Errors { style: s } => CfRule::Errors { style: st(s)? },
        CfRuleJson::NoErrors { style: s } => CfRule::NoErrors { style: st(s)? },
        CfRuleJson::Duplicate { style: s } => CfRule::Duplicate { style: st(s)? },
        CfRuleJson::Unique { style: s } => CfRule::Unique { style: st(s)? },
        CfRuleJson::Top10 { bottom, percent, rank, style: s } => {
            CfRule::Top10 { bottom: *bottom, percent: *percent, rank: (*rank).clamp(1, 1000), style: st(s)? }
        }
        CfRuleJson::AboveAverage { below, equal, std_dev, style: s } => {
            CfRule::AboveAverage { below: *below, equal: *equal, std_dev: (*std_dev).min(3) as u8, style: st(s)? }
        }
        CfRuleJson::TimePeriod { period, style: s } => CfRule::TimePeriod { period: period.clone(), style: st(s)? },
        CfRuleJson::ColorScale { stops } => {
            if !(2..=3).contains(&stops.len()) {
                return Err(format!("a colour scale has 2 or 3 stops, not {}", stops.len()));
            }
            CfRule::ColorScale { stops: stops.iter().map(|s| Ok((cf_value(&s.value)?, col(&s.color)?))).collect::<R<Vec<_>>>()? }
        }
        CfRuleJson::DataBar { min, max, color: c, gradient, show_value } => {
            CfRule::DataBar { min: cf_value(min)?, max: cf_value(max)?, color: col(c)?, gradient: *gradient, show_value: *show_value }
        }
        CfRuleJson::IconSet { set, thresholds, reverse, show_value } => {
            if thresholds.len() > 5 {
                return Err(format!("an icon set has at most 5 thresholds, not {}", thresholds.len()));
            }
            CfRule::IconSet {
                set: set.clone(),
                thresholds: thresholds.iter().map(cf_value).collect::<R<Vec<_>>>()?,
                reverse: *reverse,
                show_value: *show_value,
            }
        }
    })
}

fn ranges(list: &[String], what: &str) -> R<Vec<RangeRef>> {
    if list.is_empty() {
        return Err(format!("{what}: no ranges"));
    }
    list.iter().map(|r| range(r, what)).collect()
}

/// A cell value from `v` and its type `t` (inferred from the JSON type when missing).
fn value(v: &Json, t: Option<CellType>, at: CellRef) -> R<Value> {
    let bad = |want: &str| format!("cell {}: `{v}` is not {want}", at.a1());
    Ok(match t {
        None => match v {
            Json::Null => Value::Empty,
            Json::Bool(b) => Value::Bool(*b),
            Json::Number(n) => Value::Number(n.as_f64().ok_or_else(|| bad("a number"))?),
            Json::String(s) => Value::Text(Arc::from(s.as_str())),
            _ => return Err(bad("a number, string, bool or null")),
        },
        Some(CellType::Number | CellType::Date) => match v {
            Json::Null => Value::Empty,
            Json::Number(n) => Value::Number(n.as_f64().filter(|x| x.is_finite()).ok_or_else(|| bad("a number"))?),
            Json::String(s) => Value::Number(gridcraft_core::parse::parse_number_text(s).ok_or_else(|| bad("a number"))?),
            Json::Bool(b) => Value::Number(if *b { 1.0 } else { 0.0 }),
            _ => return Err(bad("a number")),
        },
        Some(CellType::Text) => match v {
            Json::Null => Value::Empty,
            Json::String(s) => Value::Text(Arc::from(s.as_str())),
            Json::Number(n) => Value::Text(Arc::from(n.to_string().as_str())),
            Json::Bool(b) => Value::Text(Arc::from(if *b { "TRUE" } else { "FALSE" })),
            _ => return Err(bad("text")),
        },
        Some(CellType::Bool) => match v {
            Json::Null => Value::Empty,
            Json::Bool(b) => Value::Bool(*b),
            Json::Number(n) => Value::Bool(n.as_f64().is_some_and(|x| x != 0.0)),
            Json::String(s) if s.eq_ignore_ascii_case("true") => Value::Bool(true),
            Json::String(s) if s.eq_ignore_ascii_case("false") => Value::Bool(false),
            _ => return Err(bad("TRUE or FALSE")),
        },
        Some(CellType::Error) => match v {
            Json::String(s) => Value::Error(CellError::parse(s.trim()).unwrap_or(CellError::Value)),
            _ => Value::Error(CellError::Value),
        },
    })
}

/// Builds a workbook from Workbook JSON v1. `media` resolves `images[].media` ids. With `base`,
/// each sheet whose name matches one of `base`'s keeps what the JSON doesn't describe (charts,
/// shapes, tables, PivotTables, sparklines, filters, print settings, protection), and the
/// workbook keeps `base`'s theme, properties, calculation settings and named cell styles.
pub fn from_json(js: &WorkbookJson, media: &MediaLookup, base: Option<&Workbook>) -> R<Workbook> {
    if js.version != VERSION {
        return Err(format!("unsupported workbook JSON version {} (this engine reads version {VERSION})", js.version));
    }
    if js.sheets.is_empty() {
        return Err("a workbook needs at least one sheet".into());
    }
    if js.sheets.len() > MAX_SHEETS {
        return Err(format!("too many sheets ({}; the limit is {MAX_SHEETS})", js.sheets.len()));
    }
    if js.styles.len() > MAX_STYLES {
        return Err(format!("too many styles ({}; the limit is {MAX_STYLES})", js.styles.len()));
    }
    let mut wb = match base {
        Some(b) => {
            let mut w = Workbook { sheets: vec![], names: vec![], ..b.clone() };
            w.styles.rebuild_index();
            w
        }
        None => Workbook { sheets: vec![], ..Workbook::new() },
    };
    let mut style_ids = Vec::with_capacity(js.styles.len());
    for (i, s) in js.styles.iter().enumerate() {
        style_ids.push(wb.styles.intern(style(s).map_err(|e| format!("style {i}: {e}"))?));
    }
    let sid = |s: Option<u32>, what: &str| -> R<StyleId> {
        match s {
            None => Ok(style_ids.first().copied().unwrap_or(StyleId::DEFAULT)),
            Some(i) => style_ids
                .get(i as usize)
                .copied()
                .or_else(|| (i == 0).then_some(StyleId::DEFAULT))
                .ok_or_else(|| format!("{what}: style {i} is not in `styles` ({} entries)", style_ids.len())),
        }
    };

    let mut table_names = Vec::new();
    for (si, sj) in js.sheets.iter().enumerate() {
        wb.check_sheet_name(&sj.name, None).map_err(|e| format!("sheet {si} `{}`: {e}", sj.name))?;
        let kept = base.and_then(|b| b.sheet_index(&sj.name).and_then(|i| b.sheet(i)));
        let sheet = build_sheet(sj, kept, &sid, media, &mut table_names).map_err(|e| format!("sheet `{}`: {e}", sj.name))?;
        wb.sheets.push(Arc::new(sheet));
    }
    // Object ids are unique in the workbook: number the new tables and pictures after the kept
    // charts and shapes.
    let mut next = wb.sheets.iter().flat_map(|s| s.charts.iter().map(|c| c.id).chain(s.shapes.iter().map(|x| x.id))).max().unwrap_or(0);
    for sh in wb.sheets.iter_mut() {
        let sh = Arc::make_mut(sh);
        for t in &mut sh.tables {
            next = next.saturating_add(1);
            t.id = next;
        }
        for im in &mut sh.images {
            next = next.saturating_add(1);
            im.id = next;
        }
    }
    let n = wb.sheets.len();
    for (i, nj) in js.defined_names.iter().enumerate() {
        let name = nj.name.trim();
        if name.is_empty() || name.chars().count() > 255 {
            return Err(format!("defined name {i}: a name has 1–255 characters"));
        }
        if let Some(sc) = nj.scope
            && sc >= n
        {
            return Err(format!("defined name `{name}`: scope {sc} is not a sheet (there are {n})"));
        }
        wb.names.push(DefinedName {
            name: name.to_string(),
            scope: nj.scope,
            formula: strip_eq(&nj.reference),
            comment: nj.comment.clone(),
            hidden: nj.hidden,
        });
    }
    wb.active_sheet = js.active_sheet.min(n - 1);
    Ok(wb)
}

fn build_sheet(
    sj: &SheetJson,
    kept: Option<&Sheet>,
    sid: &dyn Fn(Option<u32>, &str) -> R<StyleId>,
    media: &MediaLookup,
    taken: &mut Vec<String>,
) -> R<Sheet> {
    let mut sh = match kept {
        Some(k) => {
            let mut s = k.clone();
            s.cells = Default::default();
            s.spill.clear();
            s.spill_ranges.clear();
            s.cols.clear();
            s.rows.clear();
            s.merges.clear();
            s.cond_formats.clear();
            s.validations.clear();
            s.comments.clear();
            s.hyperlinks.clear();
            s.images.clear();
            s.tables.clear();
            s
        }
        None => Sheet::new(sj.name.clone()),
    };
    sh.name = sj.name.clone();
    sh.visibility = if sj.very_hidden {
        Visibility::VeryHidden
    } else if sj.hidden {
        Visibility::Hidden
    } else {
        Visibility::Visible
    };
    let dw = pt_to_px(sj.default_col_width, 1790.0);
    sh.default_col_width = if dw > 0.0 { dw } else { gridcraft_model::DEFAULT_COL_WIDTH };
    let dh = pt_to_px(sj.default_row_height, 546.0);
    sh.default_row_height = if dh > 0.0 { dh } else { gridcraft_model::DEFAULT_ROW_HEIGHT };
    sh.show_gridlines = sj.show_gridlines;
    sh.zoom = sj.zoom.clamp(10, 400);
    sh.right_to_left = sj.right_to_left;
    sh.tab_color = match sj.tab_color.as_deref() {
        None => None,
        Some(t) => Some(color(Some(t), "tab colour")?).filter(|c| *c != Color::Auto),
    };
    sh.freeze = match sj.freeze {
        Some(FreezeJson { rows, cols }) if rows > 0 || cols > 0 => Some((rows.min(MAX_ROWS - 1), cols.min(MAX_COLS - 1))),
        _ => None,
    };

    for col in &sj.cols {
        if col.index >= MAX_COLS {
            return Err(format!("column {} is outside the sheet", col.index));
        }
        let info = LineInfo {
            size: col.width.map(|w| pt_to_px(w, 1790.0)),
            hidden: col.hidden,
            outline: col.level.min(7) as u8,
            style: col.s.map(|s| sid(Some(s), "column")).transpose()?.filter(|s| *s != StyleId::DEFAULT),
            ..Default::default()
        };
        if info != LineInfo::default() {
            sh.cols.insert(col.index, info);
        }
    }
    for row in &sj.rows {
        if row.index >= MAX_ROWS {
            return Err(format!("row {} is outside the sheet", row.index));
        }
        let info = LineInfo {
            size: row.height.map(|h| pt_to_px(h, 546.0)),
            hidden: row.hidden,
            outline: row.level.min(7) as u8,
            style: row.s.map(|s| sid(Some(s), "row")).transpose()?.filter(|s| *s != StyleId::DEFAULT),
            custom: row.height.is_some() && !row.auto,
            ..Default::default()
        };
        if info != LineInfo::default() {
            sh.rows.insert(row.index, info);
        }
    }

    for m in &sj.merges {
        let r = match m {
            MergeJson::Rect { r1, c1, r2, c2 } => RangeRef::new(cell_at(*r1, *c1, "merge")?, cell_at(*r2, *c2, "merge")?),
            MergeJson::A1(t) => range(t, "merge")?,
        };
        if r.is_single() {
            continue;
        }
        // Overlapping merges make a file Excel repairs: keep the first.
        if sh.merges.len() < MAX_MERGES_CHECKED && sh.merges.iter().any(|x| x.intersects(&r)) {
            continue;
        }
        sh.merges.push(r);
    }

    for cj in &sj.cells {
        let at = cell_at(cj.r, cj.c, "cell")?;
        let style = sid(cj.s, &format!("cell {}", at.a1()))?;
        let f = cj.f.as_deref().map(str::trim).filter(|f| !f.is_empty() && *f != "=");
        let cell = if let Some(f) = f {
            let mut formula = Formula::new(f);
            if let Some(a) = &cj.array {
                let r = range(a, &format!("cell {} array", at.a1()))?;
                if r.start != at {
                    return Err(format!("cell {}: an array formula's range `{a}` must start at the cell", at.a1()));
                }
                formula.array = Some(r);
            }
            Cell { value: value(&cj.v, cj.t, at)?, formula: Some(Arc::new(formula)), style }
        } else if cj.spill {
            Cell { value: Value::Empty, formula: None, style }
        } else {
            Cell { value: value(&cj.v, cj.t, at)?, formula: None, style }
        };
        if cell.is_blank() {
            sh.cells.remove(at);
        } else {
            sh.cells.set(at, cell);
        }
    }

    for (i, cf) in sj.conditional_formats.iter().enumerate() {
        let what = format!("conditional format {i}");
        sh.cond_formats.push(CondFormat {
            ranges: ranges(&cf.ranges, &what)?,
            rule: cf_rule(&cf.rule).map_err(|e| format!("{what}: {e}"))?,
            priority: cf.priority,
            stop_if_true: cf.stop_if_true,
        });
    }
    for (i, v) in sj.validations.iter().enumerate() {
        sh.validations.push(Validation {
            ranges: ranges(&v.ranges, &format!("validation {i}"))?,
            kind: match v.kind {
                ValidationKindJson::Any => ValidationKind::Any,
                ValidationKindJson::Whole => ValidationKind::Whole,
                ValidationKindJson::Decimal => ValidationKind::Decimal,
                ValidationKindJson::List => ValidationKind::List,
                ValidationKindJson::Date => ValidationKind::Date,
                ValidationKindJson::Time => ValidationKind::Time,
                ValidationKindJson::TextLength => ValidationKind::TextLength,
                ValidationKindJson::Custom => ValidationKind::Custom,
            },
            op: operator(v.operator),
            f1: v.formula1.clone(),
            f2: v.formula2.clone(),
            allow_blank: v.allow_blank,
            in_cell_dropdown: v.show_dropdown,
            input_title: v.input_title.clone(),
            input_message: v.input_message.clone(),
            show_input: v.show_input,
            error_title: v.error_title.clone(),
            error_message: v.error_message.clone(),
            error_style: match v.error_style {
                ErrorStyleJson::Stop => ErrorStyle::Stop,
                ErrorStyleJson::Warning => ErrorStyle::Warning,
                ErrorStyleJson::Information => ErrorStyle::Information,
            },
            show_error: v.show_error,
        });
    }
    for h in &sj.hyperlinks {
        let at = cell_at(h.r, h.c, "hyperlink")?;
        if !h.url.trim().is_empty() {
            sh.hyperlinks.insert(at, Hyperlink { target: h.url.clone(), tooltip: h.tooltip.clone() });
        }
    }
    for c in &sj.comments {
        let at = cell_at(c.r, c.c, "comment")?;
        sh.comments.insert(
            at,
            Comment {
                author: c.author.clone(),
                text: c.text.clone(),
                replies: c.replies.iter().map(|r| (r.author.clone(), r.text.clone())).collect(),
                threaded: c.threaded,
                resolved: c.resolved,
                visible: c.visible,
            },
        );
    }
    for (i, tj) in sj.tables.iter().enumerate() {
        let what = format!("table {i} `{}`", tj.name);
        let r = range(&tj.range, &what)?;
        if !r.end.is_valid() {
            return Err(format!("{what}: `{}` is outside the sheet", tj.range));
        }
        let name = tj.name.trim();
        let name_ok = name.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_' || c == '\\')
            && name.chars().count() <= 255
            && name.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '\\'));
        if !name_ok {
            return Err(format!("{what}: not a valid table name"));
        }
        if taken.iter().any(|t| t.eq_ignore_ascii_case(name)) {
            return Err(format!("{what}: another table has that name"));
        }
        if u32::try_from(tj.columns.len()).ok() != Some(r.width()) {
            return Err(format!("{what}: {} columns for a range {} wide", tj.columns.len(), r.width()));
        }
        if r.height() <= (tj.header_row as u32 + tj.totals_row as u32) {
            return Err(format!("{what}: no room for data rows"));
        }
        if sh.tables.iter().any(|t| t.range.intersects(&r)) {
            return Err(format!("{what}: overlaps another table"));
        }
        taken.push(name.to_string());
        sh.tables.push(Table {
            id: 0,
            name: name.to_string(),
            range: r,
            header_row: tj.header_row,
            totals_row: tj.totals_row,
            columns: tj
                .columns
                .iter()
                .map(|c| TableColumn {
                    name: c.name.clone(),
                    totals: export::TOTALS.iter().find(|(_, j)| *j == c.totals).map(|(m, _)| *m).unwrap_or_default(),
                    totals_label: c.totals_label.clone(),
                    formula: c.formula.as_deref().map(strip_eq),
                })
                .collect(),
            style: tj.style.clone(),
            banded_rows: tj.banded_rows,
            banded_cols: tj.banded_cols,
            first_col: tj.first_col,
            last_col: tj.last_col,
            filter_button: tj.filter_button,
        });
    }
    for im in &sj.images {
        let at = cell_at(im.anchor.r, im.anchor.c, "image")?;
        let (data, mime) =
            media(&im.media).ok_or_else(|| format!("image: no media `{}` (stage it with putMedia or pass it in `media`)", im.media))?;
        sh.images.push(Image {
            id: 0,
            anchor: Anchor {
                cell: at,
                dx: pt_to_px(im.anchor.dx, 100_000.0),
                dy: pt_to_px(im.anchor.dy, 100_000.0),
                width: pt_to_px(im.w, 100_000.0),
                height: pt_to_px(im.h, 100_000.0),
            },
            data: data.to_vec(),
            mime: if mime.is_empty() { sniff_mime(&data).to_string() } else { mime },
            alt: im.alt.clone(),
        });
    }
    Ok(sh)
}

/// Content type of image bytes from their signature.
pub(crate) fn sniff_mime(data: &[u8]) -> &'static str {
    if data.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else if data.starts_with(&[0xFF, 0xD8]) {
        "image/jpeg"
    } else if data.starts_with(b"GIF8") {
        "image/gif"
    } else if data.starts_with(b"BM") {
        "image/bmp"
    } else {
        "application/octet-stream"
    }
}
