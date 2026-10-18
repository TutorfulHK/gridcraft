//! Workbook JSON v1: a portable, documented JSON form of a workbook for web hosts (see
//! `docs/workbook-json.md`). It is not the serde form of the model: it names things the way a
//! spreadsheet UI does (0-based row/column indices, sizes in points, colours as `#RRGGBB`,
//! styles as a shared table the cells index), so a JavaScript host can map it straight onto its
//! own grid model (Univer's `IWorkbookData`, say) and back.
//!
//! [`to_json`] reads a workbook (cell values are the cached, calculated values); [`from_json`]
//! builds one, validating every index and size (hostile input errors, it never panics).

mod export;
mod import;
#[cfg(test)]
mod tests;

pub use export::{media_table, to_json};
pub use import::from_json;

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

/// The schema version this engine reads and writes.
pub const VERSION: u32 = 1;

fn is_false(b: &bool) -> bool {
    !*b
}
fn is_zero(n: &u32) -> bool {
    *n == 0
}
fn yes() -> bool {
    true
}
fn hundred() -> u16 {
    100
}
fn default_col_width() -> f64 {
    export::px_to_pt(gridcraft_model::DEFAULT_COL_WIDTH)
}
fn default_row_height() -> f64 {
    export::px_to_pt(gridcraft_model::DEFAULT_ROW_HEIGHT)
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkbookJson {
    pub version: u32,
    pub sheets: Vec<SheetJson>,
    /// Shared cell styles; cells refer to them by index (`s`). Index 0 is the default style.
    #[serde(default)]
    pub styles: Vec<StyleJson>,
    #[serde(default)]
    pub defined_names: Vec<NameJson>,
    #[serde(default)]
    pub active_sheet: usize,
    /// Pictures the sheets use (`images[].media`). Bytes come from `mediaBytes(id)`, or inline
    /// as base64 `data` when asked for.
    #[serde(default)]
    pub media: Vec<MediaJson>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SheetJson {
    pub name: String,
    #[serde(default)]
    pub hidden: bool,
    /// Hidden so that only code can show it again (Excel's "very hidden").
    #[serde(default, skip_serializing_if = "is_false")]
    pub very_hidden: bool,
    /// Smallest grid holding all content (rows, columns): hosts usually pad it. Ignored on input.
    #[serde(default)]
    pub row_count: u32,
    #[serde(default)]
    pub col_count: u32,
    /// Points.
    #[serde(default = "default_col_width")]
    pub default_col_width: f64,
    #[serde(default = "default_row_height")]
    pub default_row_height: f64,
    #[serde(default)]
    pub cols: Vec<ColJson>,
    #[serde(default)]
    pub rows: Vec<RowJson>,
    #[serde(default)]
    pub freeze: Option<FreezeJson>,
    #[serde(default)]
    pub merges: Vec<MergeJson>,
    #[serde(default)]
    pub cells: Vec<CellJson>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditional_formats: Vec<CondFormatJson>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub validations: Vec<ValidationJson>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hyperlinks: Vec<HyperlinkJson>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<CommentJson>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<ImageJson>,
    /// Excel tables (ListObjects): structured references (`Sales[Q1]`) in formulas need them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tables: Vec<TableJson>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_color: Option<String>,
    #[serde(default = "yes")]
    pub show_gridlines: bool,
    /// Percent, 10–400.
    #[serde(default = "hundred")]
    pub zoom: u16,
    #[serde(default, skip_serializing_if = "is_false")]
    pub right_to_left: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColJson {
    pub index: u32,
    /// Points; `null` = the sheet's default width.
    #[serde(default)]
    pub width: Option<f64>,
    #[serde(default)]
    pub hidden: bool,
    /// Outline (grouping) level 1–7.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub level: u32,
    /// Style of the whole column (index into `styles`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub s: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RowJson {
    pub index: u32,
    /// Points; `null` = the sheet's default height.
    #[serde(default)]
    pub height: Option<f64>,
    #[serde(default)]
    pub hidden: bool,
    /// The height follows the content (it was not set by hand).
    #[serde(default, skip_serializing_if = "is_false")]
    pub auto: bool,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub level: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub s: Option<u32>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FreezeJson {
    pub rows: u32,
    pub cols: u32,
}

/// A merged block: `{r1, c1, r2, c2}` (0-based, inclusive), or `"A1:B2"` on input.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MergeJson {
    Rect { r1: u32, c1: u32, r2: u32, c2: u32 },
    A1(String),
}

/// What kind of value a cell holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CellType {
    /// Number.
    #[serde(rename = "n")]
    Number,
    /// Text.
    #[serde(rename = "s")]
    Text,
    /// TRUE / FALSE.
    #[serde(rename = "b")]
    Bool,
    /// Error (`v` is its text, `#DIV/0!`).
    #[serde(rename = "e")]
    Error,
    /// A number shown as a date or time (`v` is the serial number, 1 = 1900-01-01).
    #[serde(rename = "d")]
    Date,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CellJson {
    pub r: u32,
    pub c: u32,
    /// The value (for a formula, its last calculated result): number, string, bool or null.
    #[serde(default)]
    pub v: Json,
    /// Formula text with the leading `=`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub f: Option<String>,
    /// Omitted for an empty value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub t: Option<CellType>,
    /// Style index; omitted for the default style 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub s: Option<u32>,
    /// Range of a legacy array formula (Ctrl+Shift+Enter) anchored here, `A1:B3`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub array: Option<String>,
    /// A value spilled by a dynamic-array formula elsewhere: shown, but not stored (the formula
    /// recreates it). On input such a cell keeps only its style.
    #[serde(default, skip_serializing_if = "is_false")]
    pub spill: bool,
}

// ---------------------------------------------------------------- styles

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StyleJson {
    pub font: FontJson,
    pub fill: Option<FillJson>,
    pub border: BorderJson,
    pub align: AlignJson,
    /// Number format code (`General`, `0.00%`, `yyyy-mm-dd`).
    pub num_fmt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protection: Option<ProtectionJson>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FontJson {
    pub name: String,
    /// Points.
    pub size: f64,
    pub bold: bool,
    pub italic: bool,
    pub underline: UnderlineJson,
    pub strike: bool,
    /// `#RRGGBB`; `null` = automatic (black).
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script: Option<ScriptJson>,
}

impl Default for FontJson {
    fn default() -> Self {
        FontJson {
            name: gridcraft_model::DEFAULT_FONT.into(),
            size: gridcraft_model::DEFAULT_FONT_SIZE as f64,
            bold: false,
            italic: false,
            underline: UnderlineJson::None,
            strike: false,
            color: None,
            script: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UnderlineJson {
    #[default]
    None,
    Single,
    Double,
    SingleAccounting,
    DoubleAccounting,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScriptJson {
    Superscript,
    Subscript,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FillJson {
    /// The fill colour (a pattern's foreground).
    pub color: Option<String>,
    /// Omitted for a solid fill.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern: Option<PatternJson>,
    /// A pattern's background colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bg_color: Option<String>,
}

/// Fill patterns, named as in ECMA-376.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PatternJson {
    Solid,
    MediumGray,
    DarkGray,
    LightGray,
    Gray125,
    Gray0625,
    DarkHorizontal,
    DarkVertical,
    DarkDown,
    DarkUp,
    DarkGrid,
    DarkTrellis,
    LightHorizontal,
    LightVertical,
    LightDown,
    LightUp,
    LightGrid,
    LightTrellis,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BorderJson {
    pub top: Option<BorderLineJson>,
    pub right: Option<BorderLineJson>,
    pub bottom: Option<BorderLineJson>,
    pub left: Option<BorderLineJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diag_down: Option<BorderLineJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diag_up: Option<BorderLineJson>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BorderLineJson {
    pub style: BorderStyleJson,
    #[serde(default)]
    pub color: Option<String>,
}

/// Border line styles, named as in ECMA-376.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BorderStyleJson {
    Thin,
    Medium,
    Thick,
    Dashed,
    Dotted,
    Double,
    Hair,
    MediumDashed,
    DashDot,
    MediumDashDot,
    DashDotDot,
    MediumDashDotDot,
    SlantDashDot,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AlignJson {
    pub h: HAlignJson,
    pub v: VAlignJson,
    pub wrap: bool,
    pub indent: u32,
    /// Degrees -90..=90 (counter-clockwise), or 255 for stacked vertical text.
    pub rotate: i32,
    #[serde(skip_serializing_if = "is_false")]
    pub shrink: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HAlignJson {
    #[default]
    General,
    Left,
    Center,
    Right,
    Fill,
    Justify,
    CenterContinuous,
    Distributed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VAlignJson {
    Top,
    Center,
    #[default]
    Bottom,
    Justify,
    Distributed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProtectionJson {
    pub locked: bool,
    pub hidden: bool,
}

impl Default for ProtectionJson {
    fn default() -> Self {
        ProtectionJson { locked: true, hidden: false }
    }
}

// ---------------------------------------------------------------- names, media

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NameJson {
    pub name: String,
    /// What the name refers to, without `=`: `Sheet1!$A$1:$B$4`, `0.07`, a formula.
    #[serde(rename = "ref")]
    pub reference: String,
    /// Sheet index for a sheet-level name; omitted for a workbook-level one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<usize>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub hidden: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaJson {
    pub id: String,
    #[serde(default)]
    pub content_type: String,
    #[serde(default)]
    pub name: String,
    /// Base64 bytes, only when asked for (`workbook.json {inlineMedia: true}`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
}

// ---------------------------------------------------------------- sheet extras

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HyperlinkJson {
    pub r: u32,
    pub c: u32,
    /// URL, `mailto:`, or a place in the workbook (`Sheet2!A1`).
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tooltip: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CommentJson {
    pub r: u32,
    pub c: u32,
    pub text: String,
    #[serde(default)]
    pub author: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub replies: Vec<ReplyJson>,
    /// A threaded comment (otherwise a note).
    #[serde(default, skip_serializing_if = "is_false")]
    pub threaded: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub resolved: bool,
    /// A note shown all the time.
    #[serde(default, skip_serializing_if = "is_false")]
    pub visible: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReplyJson {
    pub author: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageJson {
    /// Id of an entry in the workbook's `media`.
    pub media: String,
    pub anchor: AnchorJson,
    /// Size in points.
    pub w: f64,
    pub h: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub alt: String,
}

/// Top-left corner: a cell plus an offset in points.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AnchorJson {
    pub r: u32,
    pub c: u32,
    #[serde(default)]
    pub dx: f64,
    #[serde(default)]
    pub dy: f64,
}

// ---------------------------------------------------------------- tables

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableJson {
    /// Unique in the workbook; formulas refer to it (`Sales[Q1]`).
    pub name: String,
    /// The whole table, header and totals rows included: `B4:I11`.
    #[serde(rename = "ref")]
    pub range: String,
    #[serde(default = "yes")]
    pub header_row: bool,
    #[serde(default)]
    pub totals_row: bool,
    /// One per column of `ref`, left to right.
    pub columns: Vec<TableColumnJson>,
    /// Table style name (`TableStyleMedium2`); empty for none.
    #[serde(default)]
    pub style: String,
    #[serde(default = "yes")]
    pub banded_rows: bool,
    #[serde(default)]
    pub banded_cols: bool,
    #[serde(default)]
    pub first_col: bool,
    #[serde(default)]
    pub last_col: bool,
    #[serde(default = "yes")]
    pub filter_button: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableColumnJson {
    pub name: String,
    /// Totals-row function.
    #[serde(default)]
    pub totals: TotalsJson,
    /// Totals-row label (or custom formula).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub totals_label: Option<String>,
    /// Calculated-column formula.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formula: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TotalsJson {
    #[default]
    None,
    Average,
    Count,
    CountNums,
    Max,
    Min,
    Sum,
    StdDev,
    Var,
    Custom,
}

// ---------------------------------------------------------------- conditional formats, validation

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CondFormatJson {
    /// A1 ranges the rule covers.
    pub ranges: Vec<String>,
    #[serde(default)]
    pub priority: u32,
    #[serde(default, skip_serializing_if = "is_false")]
    pub stop_if_true: bool,
    pub rule: CfRuleJson,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OperatorJson {
    Between,
    NotBetween,
    Equal,
    NotEqual,
    GreaterThan,
    LessThan,
    GreaterThanOrEqual,
    LessThanOrEqual,
}

/// A conditional-formatting rule. Formulas have no leading `=` requirement (either works) and
/// are relative to the top-left cell of the first range. `style` holds only what the rule
/// changes, as a full style object whose defaults mean "unchanged".
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum CfRuleJson {
    CellIs {
        operator: OperatorJson,
        formula1: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        formula2: Option<String>,
        style: StyleJson,
    },
    Expression {
        formula: String,
        style: StyleJson,
    },
    ContainsText {
        text: String,
        style: StyleJson,
    },
    NotContainsText {
        text: String,
        style: StyleJson,
    },
    BeginsWith {
        text: String,
        style: StyleJson,
    },
    EndsWith {
        text: String,
        style: StyleJson,
    },
    Blanks {
        style: StyleJson,
    },
    NoBlanks {
        style: StyleJson,
    },
    Errors {
        style: StyleJson,
    },
    NoErrors {
        style: StyleJson,
    },
    Duplicate {
        style: StyleJson,
    },
    Unique {
        style: StyleJson,
    },
    /// Top/bottom N items or N percent.
    Top10 {
        #[serde(default)]
        bottom: bool,
        #[serde(default)]
        percent: bool,
        rank: u32,
        style: StyleJson,
    },
    #[serde(rename_all = "camelCase")]
    AboveAverage {
        #[serde(default)]
        below: bool,
        #[serde(default)]
        equal: bool,
        #[serde(default)]
        std_dev: u32,
        style: StyleJson,
    },
    /// `yesterday`, `today`, `tomorrow`, `last7Days`, `lastWeek`, `thisWeek`, `nextWeek`,
    /// `lastMonth`, `thisMonth`, `nextMonth`.
    TimePeriod {
        period: String,
        style: StyleJson,
    },
    ColorScale {
        stops: Vec<ColorStopJson>,
    },
    #[serde(rename_all = "camelCase")]
    DataBar {
        min: CfValueJson,
        max: CfValueJson,
        color: String,
        #[serde(default = "yes")]
        gradient: bool,
        #[serde(default = "yes")]
        show_value: bool,
    },
    /// `set` is an icon set name: `3Arrows`, `3TrafficLights1`, `4Rating`, `5Quarters`…
    #[serde(rename_all = "camelCase")]
    IconSet {
        set: String,
        thresholds: Vec<CfValueJson>,
        #[serde(default)]
        reverse: bool,
        #[serde(default = "yes")]
        show_value: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ColorStopJson {
    pub value: CfValueJson,
    pub color: String,
}

/// A threshold: `{"type": "min"}`, `{"type": "percent", "value": 50}`,
/// `{"type": "formula", "value": "$A$1"}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum CfValueJson {
    Min,
    Max,
    Number { value: f64 },
    Percent { value: f64 },
    Percentile { value: f64 },
    Formula { value: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationJson {
    pub ranges: Vec<String>,
    #[serde(rename = "type")]
    pub kind: ValidationKindJson,
    #[serde(default = "between")]
    pub operator: OperatorJson,
    /// First criterion: a value, a formula, or for a list `"a,b,c"` or a range `=$A$1:$A$5`.
    #[serde(default)]
    pub formula1: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formula2: Option<String>,
    #[serde(default = "yes")]
    pub allow_blank: bool,
    /// In-cell dropdown for a list.
    #[serde(default = "yes")]
    pub show_dropdown: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub input_title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub input_message: String,
    #[serde(default = "yes")]
    pub show_input: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub error_title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub error_message: String,
    #[serde(default)]
    pub error_style: ErrorStyleJson,
    #[serde(default = "yes")]
    pub show_error: bool,
}

fn between() -> OperatorJson {
    OperatorJson::Between
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ValidationKindJson {
    Any,
    Whole,
    Decimal,
    List,
    Date,
    Time,
    TextLength,
    Custom,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorStyleJson {
    #[default]
    Stop,
    Warning,
    Information,
}
