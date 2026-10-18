# Workbook JSON v1

A portable JSON form of a workbook for web hosts that keep their own grid model (Univer's
`IWorkbookData`, for example). Produced by `workbook.json` / `Engine.workbook()`, read by
`workbook.fromJson` / `Engine.fromWorkbook()`. Serde types: `crates/engine/src/workbook_json/mod.rs`.

It is **not** the serde form of the GridCraft model. Conventions:

- Rows and columns are **0-based** (`A1` is `r: 0, c: 0`).
- Sizes are **points** (column widths, row heights, picture offsets and sizes).
- Colours are `"#RRGGBB"`; `null` means automatic. Theme colours are resolved to RGB.
- Cell values are the **cached, calculated** values: the engine recalculates on open and on
  `fromJson`, so a cached `v` on input is only a hint.
- Optional fields may be left out on input; defaults are listed below.

```jsonc
{
  "version": 1,
  "activeSheet": 0,
  "styles": [ /* StyleJson; index 0 is the default style */ ],
  "definedNames": [{ "name": "Total", "ref": "Sheet1!$B$4", "scope": 0?, "hidden"?: true, "comment"?: "" }],
  "media": [{ "id": "image1", "contentType": "image/png", "name": "image1.png", "data"?: "base64…" }],
  "sheets": [{
    "name": "Sheet1", "hidden": false, "veryHidden"?: true,
    "rowCount": 12, "colCount": 9,            // smallest grid holding the content; ignored on input
    "defaultColWidth": 48, "defaultRowHeight": 15,
    "cols": [{ "index": 0, "width": 90 | null, "hidden": false, "level"?: 1, "s"?: 3 }],
    "rows": [{ "index": 0, "height": 22.5 | null, "hidden": false, "auto"?: true, "level"?: 1, "s"?: 3 }],
    "freeze": { "rows": 1, "cols": 0 } | null,
    "merges": [{ "r1": 5, "c1": 0, "r2": 6, "c2": 2 }],   // "A6:C7" also accepted on input
    "cells": [{ "r": 3, "c": 1, "v": 1500.5, "f"?: "=SUM(B2:B3)", "t"?: "n", "s"?: 2,
                "array"?: "B4:B6", "spill"?: true }],
    "conditionalFormats"?: [ ... ], "validations"?: [ ... ], "hyperlinks"?: [ ... ],
    "comments"?: [ ... ], "images"?: [ ... ], "tables"?: [ ... ],
    "tabColor"?: "#FF0000", "showGridlines": true, "zoom": 100, "rightToLeft"?: true
  }]
}
```

## Cells

| Field | Meaning |
|---|---|
| `v` | number, string, bool or `null` (empty). Errors are their text (`"#DIV/0!"`) |
| `t` | `n` number, `s` text, `b` bool, `e` error, `d` number shown as a date/time (`v` is the serial, 1 = 1900-01-01). Omitted when `v` is `null`. On input it is inferred from the JSON type when missing; `"=x"` with `t: "s"` stays text |
| `f` | formula text with `=`. Omitted for constants |
| `s` | index into `styles`; omitted for style 0. A cell with only `s` is a formatted blank |
| `array` | range of a legacy (Ctrl+Shift+Enter) array formula anchored at this cell |
| `spill` | a value spilled by a dynamic-array formula elsewhere; on input only its style is kept (the formula recreates it) |

## Styles

```jsonc
{ "font": { "name": "Calibri", "size": 11, "bold": false, "italic": false,
            "underline": "none|single|double|singleAccounting|doubleAccounting",
            "strike": false, "color": null, "script"?: "superscript|subscript" },
  "fill": { "color": "#FFEE00", "pattern"?: "darkGrid", "bgColor"?: "#FFFFFF" } | null,
  "border": { "top": { "style": "thin", "color": null } | null, "right": …, "bottom": …, "left": …,
              "diagDown"?: …, "diagUp"?: … },
  "align": { "h": "general|left|center|right|fill|justify|centerContinuous|distributed",
             "v": "top|center|bottom|justify|distributed", "wrap": false, "indent": 0,
             "rotate": 0 /* -90..90, 255 = stacked */, "shrink"?: true },
  "numFmt": "0.00%",
  "protection"?: { "locked": true, "hidden": false } }
```

Border styles and fill patterns use ECMA-376 names (`thin`, `mediumDashDot`, `gray125`, …). Every
field has a default, so `{}` is the default style and `{"font": {"bold": true}}` is bold.

## Sheet extras

- `hyperlinks`: `{r, c, url, tooltip?}` (`url` may be `Sheet2!A1`).
- `comments`: `{r, c, text, author, replies?: [{author, text}], threaded?, resolved?, visible?}`.
- `images`: `{media, anchor: {r, c, dx, dy}, w, h, alt?}`; `media` is an id in the workbook's
  `media`. Bytes come from `mediaBytes(id)` / `media.bytes`, or inline with
  `workbook.json {inlineMedia: true}`. On input an id resolves from `putMedia` / the `media`
  param first, then from the pictures of the workbook open at the time.
- `tables`: `{name, ref: "B4:I11", headerRow, totalsRow, columns: [{name, totals?: "sum|average|…",
  totalsLabel?, formula?}], style, bandedRows, bandedCols, firstCol, lastCol, filterButton}`.
  Needed for structured references (`Sales[Q1]`).
- `validations`: `{ranges: ["A2:A10"], type: "any|whole|decimal|list|date|time|textLength|custom",
  operator: "between|notBetween|equal|notEqual|greaterThan|lessThan|greaterThanOrEqual|lessThanOrEqual",
  formula1, formula2?, allowBlank, showDropdown, inputTitle?, inputMessage?, showInput, errorTitle?,
  errorMessage?, errorStyle: "stop|warning|information", showError}`.
- `conditionalFormats`: `{ranges, priority, stopIfTrue?, rule}` where `rule.type` is one of
  `cellIs {operator, formula1, formula2?, style}`, `expression {formula, style}`,
  `containsText|notContainsText|beginsWith|endsWith {text, style}`,
  `blanks|noBlanks|errors|noErrors|duplicate|unique {style}`, `top10 {bottom, percent, rank, style}`,
  `aboveAverage {below, equal, stdDev, style}`, `timePeriod {period, style}`,
  `colorScale {stops: [{value, color}]}` (2–3 stops), `dataBar {min, max, color, gradient, showValue}`,
  `iconSet {set, thresholds, reverse, showValue}`. Thresholds are `{type: "min"|"max"}` or
  `{type: "number"|"percent"|"percentile"|"formula", value}`. A rule's `style` is a full style whose
  defaults mean "unchanged".

## Not in v1

Charts, shapes, sparklines, PivotTables, autofilters, print settings, sheet protection, the
theme, document properties and calculation settings. To keep them through an edit, rebuild with
`merge: true`: sheets whose names match the open workbook's keep those parts.

## Validation

`fromJson` rejects, with a message naming the place: another `version`, no sheets, invalid or
duplicate sheet names, a cell/merge/link/comment/picture outside 1,048,576 × 16,384, a style index
not in `styles`, a value that doesn't match its `t`, bad colours, bad ranges, unknown media, a
bad table (name, width, overlap). Sizes, zoom, indent, rotation and font sizes are clamped; an
overlapping merge is dropped; `activeSheet` is clamped.
