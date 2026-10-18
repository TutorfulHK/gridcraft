use std::collections::BTreeMap;
use std::sync::Arc;

use gridcraft_core::{CellRef, RangeRef, Value};
use gridcraft_model::{
    Anchor, BorderLine, BorderStyle, Cell, CfOperator, CfRule, CfValueKind, Color, Comment, CondFormat, DefinedName, Fill, Formula, HAlign,
    Hyperlink, Image, LineInfo, Style, StyleId, Validation, ValidationKind, Workbook,
};
use serde_json::{Value as Json, json};

use super::*;
use crate::{DocState, Session};

fn at(a: &str) -> CellRef {
    CellRef::parse(a).unwrap()
}

fn png() -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(4, 3, image::Rgba([200, 30, 30, 255]));
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

fn no_media(_: &str) -> Option<(Arc<Vec<u8>>, String)> {
    None
}

/// A workbook with something of everything the JSON describes, built in code and recalculated.
fn rich() -> Workbook {
    let mut wb = Workbook::new();
    let bold = wb.styles.derive(StyleId::DEFAULT, |s| {
        s.font.bold = true;
        s.font.color = Color::rgb(0x11, 0x22, 0x33);
        s.fill = Fill::solid(Color::rgb(0xFF, 0xEE, 0x00));
        s.borders.bottom = BorderLine { style: BorderStyle::Double, color: Color::Auto };
        s.align.h = HAlign::Center;
        s.align.wrap = true;
    });
    let pct = wb.styles.derive(StyleId::DEFAULT, |s| s.num_fmt = gridcraft_model::NumFmt::new("0.00%"));
    let date = wb.styles.derive(StyleId::DEFAULT, |s| s.num_fmt = gridcraft_model::NumFmt::new("yyyy-mm-dd"));
    let sh = wb.sheet_mut(0).unwrap();
    sh.name = "預算 Budget".into();
    let set = |sh: &mut gridcraft_model::Sheet, a: &str, cell: Cell| sh.cells.set(at(a), cell);
    set(sh, "A1", Cell { style: bold, ..Cell::value(Value::Text("項目".into())) });
    set(sh, "B1", Cell { style: bold, ..Cell::value(Value::Text("金额 Amount".into())) });
    set(sh, "A2", Cell::value(Value::Text("Rent 租金".into())));
    set(sh, "B2", Cell::value(Value::Number(1200.5)));
    set(sh, "A3", Cell::value(Value::Text("Food".into())));
    set(sh, "B3", Cell::value(Value::Number(300.0)));
    set(sh, "B4", Cell::formula(Formula::new("=SUM(B2:B3)")));
    set(sh, "C2", Cell { style: pct, ..Cell::formula(Formula::new("=B2/B4")) });
    set(sh, "D2", Cell { style: date, ..Cell::value(Value::Number(46000.0)) });
    set(sh, "E2", Cell::value(Value::Bool(true)));
    set(sh, "E3", Cell::formula(Formula::new("=1/0")));
    set(sh, "F1", Cell::formula(Formula::new("=SEQUENCE(3)")));
    set(sh, "G5", Cell { style: bold, ..Cell::default() });
    sh.merges.push(RangeRef::parse("A6:C7").unwrap());
    sh.freeze = Some((1, 0));
    sh.cols.insert(0, LineInfo { size: Some(120.0), ..Default::default() });
    sh.cols.insert(3, LineInfo { size: Some(80.0), hidden: true, ..Default::default() });
    sh.rows.insert(0, LineInfo { size: Some(30.0), custom: true, ..Default::default() });
    sh.rows.insert(8, LineInfo { hidden: true, ..Default::default() });
    sh.cond_formats.push(CondFormat {
        ranges: vec![RangeRef::parse("B2:B3").unwrap()],
        rule: CfRule::CellIs {
            op: CfOperator::Greater,
            a: "500".into(),
            b: None,
            style: Box::new(Style { font: Default::default(), ..Style::default() }),
        },
        priority: 1,
        stop_if_true: false,
    });
    sh.cond_formats.push(CondFormat {
        ranges: vec![RangeRef::parse("B2:B4").unwrap()],
        rule: CfRule::ColorScale { stops: vec![(CfValueKind::Min, Color::rgb(255, 255, 255)), (CfValueKind::Max, Color::rgb(0, 128, 0))] },
        priority: 2,
        stop_if_true: false,
    });
    sh.validations.push(Validation {
        ranges: vec![RangeRef::parse("A2:A10").unwrap()],
        kind: ValidationKind::List,
        f1: "\"Rent,Food,Other\"".into(),
        ..Default::default()
    });
    sh.hyperlinks.insert(at("A3"), Hyperlink { target: "https://example.org/".into(), tooltip: Some("Example".into()) });
    sh.comments.insert(
        at("B2"),
        Comment { author: "老師".into(), text: "Check this".into(), replies: vec![], threaded: false, resolved: false, visible: false },
    );
    sh.images.push(Image {
        id: 1,
        anchor: Anchor { cell: at("H2"), dx: 4.0, dy: 8.0, width: 64.0, height: 48.0 },
        data: png(),
        mime: "image/png".into(),
        alt: "logo".into(),
    });
    let mut second = gridcraft_model::Sheet::new("Second");
    second.cells.set(at("A1"), Cell::formula(Formula::new("='預算 Budget'!B4*2")));
    wb.sheets.push(Arc::new(second));
    wb.names.push(DefinedName { name: "Total".into(), scope: None, formula: "'預算 Budget'!$B$4".into(), comment: String::new(), hidden: false });
    DocState::new(wb, None, "t".into()).wb.as_ref().clone()
}

fn cell<'a>(js: &'a WorkbookJson, sheet: usize, a: &str) -> &'a CellJson {
    let c = at(a);
    js.sheets[sheet].cells.iter().find(|x| x.r == c.row && x.c == c.col).unwrap_or_else(|| panic!("no cell {a}"))
}

fn rebuild(js: &WorkbookJson, media: &MediaLookupFn) -> Workbook {
    let wb = from_json(js, media, None).unwrap();
    DocState::new(wb, None, "t".into()).wb.as_ref().clone()
}

/// Path and values of the first difference between two JSON values (for readable failures).
fn first_diff(a: &Json, b: &Json, path: &str) -> Option<String> {
    match (a, b) {
        (Json::Object(x), Json::Object(y)) => {
            for k in x.keys().chain(y.keys()) {
                if let Some(d) = first_diff(x.get(k).unwrap_or(&Json::Null), y.get(k).unwrap_or(&Json::Null), &format!("{path}.{k}")) {
                    return Some(d);
                }
            }
            None
        }
        (Json::Array(x), Json::Array(y)) => {
            for i in 0..x.len().max(y.len()) {
                if let Some(d) = first_diff(x.get(i).unwrap_or(&Json::Null), y.get(i).unwrap_or(&Json::Null), &format!("{path}[{i}]")) {
                    return Some(d);
                }
            }
            None
        }
        _ => (a != b).then(|| format!("{path}: {a} ≠ {b}")),
    }
}

fn same(a: &impl serde::Serialize, b: &impl serde::Serialize, what: &str) {
    let (a, b) = (serde_json::to_value(a).unwrap(), serde_json::to_value(b).unwrap());
    if let Some(d) = first_diff(&a, &b, "") {
        panic!("{what}: {d}");
    }
}

type MediaLookupFn = dyn Fn(&str) -> Option<(Arc<Vec<u8>>, String)>;

#[test]
fn values_types_formulas_and_styles() {
    let wb = rich();
    let js = to_json(&wb, false);
    assert_eq!(js.version, 1);
    assert_eq!(js.sheets.len(), 2);
    assert_eq!(js.sheets[0].name, "預算 Budget");
    // Cached, calculated values.
    let b4 = cell(&js, 0, "B4");
    assert_eq!((b4.v.clone(), b4.f.as_deref(), b4.t), (json!(1500.5), Some("=SUM(B2:B3)"), Some(CellType::Number)));
    assert_eq!(cell(&js, 1, "A1").v, json!(3001.0));
    assert_eq!(cell(&js, 0, "A1").v, json!("項目"));
    assert_eq!(cell(&js, 0, "A1").t, Some(CellType::Text));
    assert_eq!(cell(&js, 0, "D2").t, Some(CellType::Date));
    assert_eq!((cell(&js, 0, "E2").v.clone(), cell(&js, 0, "E2").t), (json!(true), Some(CellType::Bool)));
    assert_eq!((cell(&js, 0, "E3").v.clone(), cell(&js, 0, "E3").t), (json!("#DIV/0!"), Some(CellType::Error)));
    // A spilled array: the anchor has the formula, the rest are flagged.
    assert_eq!(cell(&js, 0, "F1").f.as_deref(), Some("=SEQUENCE(3)"));
    assert!(cell(&js, 0, "F3").spill && cell(&js, 0, "F3").v == json!(3.0));
    // Styles: 0 is the default; the bold header style is shared.
    let s = cell(&js, 0, "A1").s.unwrap() as usize;
    assert_eq!(cell(&js, 0, "B1").s, Some(s as u32));
    let st = &js.styles[s];
    assert!(st.font.bold && st.align.wrap && st.align.h == HAlignJson::Center);
    assert_eq!(st.font.color.as_deref(), Some("#112233"));
    assert_eq!(st.fill.as_ref().and_then(|f| f.color.as_deref()), Some("#FFEE00"));
    assert_eq!(st.border.bottom.as_ref().map(|b| b.style), Some(BorderStyleJson::Double));
    assert_eq!(js.styles[cell(&js, 0, "C2").s.unwrap() as usize].num_fmt, "0.00%");
    assert_eq!(js.styles[0], StyleJson { num_fmt: "General".into(), ..StyleJson::default() });
    assert!(cell(&js, 0, "B2").s.is_none());
    // A styled blank cell.
    assert_eq!((cell(&js, 0, "G5").v.clone(), cell(&js, 0, "G5").t), (Json::Null, None));
    // Geometry in points.
    let sh = &js.sheets[0];
    assert_eq!(sh.cols[0], ColJson { index: 0, width: Some(90.0), hidden: false, level: 0, s: None });
    assert!(sh.cols[1].hidden);
    assert_eq!(sh.rows[0].height, Some(22.5));
    assert!(sh.rows[1].hidden);
    assert_eq!(sh.default_col_width, 48.0);
    assert_eq!(sh.freeze, Some(FreezeJson { rows: 1, cols: 0 }));
    assert_eq!(sh.merges, vec![MergeJson::Rect { r1: 5, c1: 0, r2: 6, c2: 2 }]);
    assert!(sh.row_count >= 9 && sh.col_count >= 8);
    assert_eq!(js.defined_names[0].reference, "'預算 Budget'!$B$4");
    assert_eq!(sh.images[0].media, "image1");
    assert_eq!(js.media[0].content_type, "image/png");
    assert_eq!(sh.comments[0].author, "老師");
    assert_eq!(sh.hyperlinks[0].url, "https://example.org/");
    assert_eq!(sh.validations[0].kind, ValidationKindJson::List);
    assert_eq!(sh.conditional_formats.len(), 2);
}

#[test]
fn json_model_json_is_stable() {
    let wb = rich();
    let js = to_json(&wb, true);
    let data = png();
    let lookup = move |id: &str| (id == "image1").then(|| (Arc::new(data.clone()), "image/png".to_string()));
    let again = to_json(&rebuild(&js, &lookup), true);
    same(&js, &again, "json → model → json");
    // Through text as well (what a host does).
    let text = serde_json::to_string(&js).unwrap();
    let parsed: WorkbookJson = serde_json::from_str(&text).unwrap();
    assert_eq!(parsed, js);
}

#[test]
fn samples_round_trip_through_json_and_xlsx() {
    for (name, _) in crate::sample::SAMPLES {
        let wb = DocState::new(crate::sample::build(name).unwrap(), None, name.to_string()).wb.as_ref().clone();
        let js = to_json(&wb, false);
        if *name == "sales" {
            assert_eq!(js.sheets[0].tables[0].name, "Sales");
        }
        let rebuilt = rebuild(&js, &no_media);
        same(&js, &to_json(&rebuilt, false), &format!("sample {name}: json → model → json"));
        // JSON → xlsx → JSON keeps every cell, value, formula and style.
        let bytes = gridcraft_xlsx::write_xlsx(&rebuilt).unwrap();
        let (back, _) = crate::io::open_bytes("x.xlsx", &bytes).unwrap();
        let back = DocState::new(back, None, "x".into()).wb.as_ref().clone();
        let js2 = to_json(&back, false);
        for (a, b) in js.sheets.iter().zip(&js2.sheets) {
            assert_eq!(a.cells, b.cells, "sample {name}, sheet {}: cells after xlsx", a.name);
            assert_eq!(a.merges, b.merges);
            assert_eq!(a.freeze, b.freeze);
        }
        assert_eq!(js.styles, js2.styles, "sample {name}: styles after xlsx");
    }
}

#[test]
fn rich_workbook_round_trips_through_xlsx() {
    let wb = rich();
    let js = to_json(&wb, false);
    let bytes = gridcraft_xlsx::write_xlsx(&wb).unwrap();
    let (back, _) = crate::io::open_bytes("x.xlsx", &bytes).unwrap();
    let js2 = to_json(&DocState::new(back, None, "x".into()).wb, false);
    for (a, b) in js.sheets.iter().zip(&js2.sheets) {
        assert_eq!(a.name, b.name);
        assert_eq!(a.cells, b.cells, "sheet {}", a.name);
        assert_eq!(a.merges, b.merges);
        assert_eq!(a.freeze, b.freeze);
        assert_eq!(a.cols, b.cols);
        assert_eq!(a.hyperlinks, b.hyperlinks);
        assert_eq!(a.comments, b.comments);
        assert_eq!(a.validations, b.validations);
        assert_eq!(a.images, b.images);
        assert_eq!(a.conditional_formats.len(), b.conditional_formats.len());
    }
    assert_eq!(js.styles, js2.styles);
    assert_eq!(js.defined_names, js2.defined_names);
}

#[test]
fn formulas_are_recalculated_on_build() {
    let js: WorkbookJson = serde_json::from_value(json!({
        "version": 1,
        "sheets": [{"name": "S", "cells": [
            {"r": 0, "c": 0, "v": 2},
            {"r": 1, "c": 0, "v": 3, "t": "n"},
            {"r": 2, "c": 0, "v": 999, "f": "=SUM(A1:A2)"},
            {"r": 3, "c": 0, "v": "=not a formula", "t": "s"},
            {"r": 4, "c": 0, "v": "45000", "t": "d"},
            {"r": 5, "c": 0, "v": "#N/A", "t": "e"},
            {"r": 6, "c": 0, "v": "TRUE", "t": "b"}
        ]}]
    }))
    .unwrap();
    let wb = rebuild(&js, &no_media);
    let sh = wb.sheet(0).unwrap();
    assert_eq!(sh.value(at("A3")), Value::Number(5.0));
    assert_eq!(sh.value(at("A4")), Value::Text("=not a formula".into()));
    assert_eq!(sh.value(at("A5")), Value::Number(45000.0));
    assert_eq!(sh.value(at("A6")), Value::Error(gridcraft_core::CellError::NA));
    assert_eq!(sh.value(at("A7")), Value::Bool(true));
    // Defaults fill in what a minimal sheet leaves out.
    assert_eq!(sh.default_col_width, gridcraft_model::DEFAULT_COL_WIDTH);
    assert!(sh.show_gridlines);
}

#[test]
fn merges_accept_a1_and_drop_overlaps() {
    let js: WorkbookJson = serde_json::from_value(json!({
        "version": 1,
        "sheets": [{"name": "S", "merges": ["A1:B2", {"r1": 1, "c1": 1, "r2": 3, "c2": 3}, "D5", {"r1": 9, "c1": 0, "r2": 9, "c2": 4}]}]
    }))
    .unwrap();
    let wb = from_json(&js, &no_media, None).unwrap();
    let m: Vec<String> = wb.sheet(0).unwrap().merges.iter().map(|r| r.a1()).collect();
    assert_eq!(m, vec!["A1:B2", "A10:E10"]);
}

#[test]
fn hostile_workbooks_error_or_clamp() {
    let bad = |v: Json| {
        let js: WorkbookJson = match serde_json::from_value(v.clone()) {
            Ok(j) => j,
            Err(_) => return,
        };
        assert!(from_json(&js, &no_media, None).is_err(), "should fail: {v}");
    };
    bad(json!({"version": 2, "sheets": [{"name": "S"}]}));
    bad(json!({"version": 1, "sheets": []}));
    bad(json!({"version": 1, "sheets": [{"name": ""}]}));
    bad(json!({"version": 1, "sheets": [{"name": "a/b"}]}));
    bad(json!({"version": 1, "sheets": [{"name": "S"}, {"name": "s"}]}));
    bad(json!({"version": 1, "sheets": [{"name": "S", "cells": [{"r": 1048576, "c": 0, "v": 1}]}]}));
    bad(json!({"version": 1, "sheets": [{"name": "S", "cells": [{"r": 0, "c": 16384, "v": 1}]}]}));
    bad(json!({"version": 1, "sheets": [{"name": "S", "cells": [{"r": 0, "c": 0, "v": 1, "s": 7}]}]}));
    bad(json!({"version": 1, "sheets": [{"name": "S", "cells": [{"r": 0, "c": 0, "v": "x", "t": "n"}]}]}));
    bad(json!({"version": 1, "sheets": [{"name": "S", "cells": [{"r": 0, "c": 0, "v": [1], "t": "s"}]}]}));
    bad(json!({"version": 1, "sheets": [{"name": "S", "cells": [{"r": 3, "c": 0, "f": "=A1", "array": "A1:A3"}]}]}));
    bad(json!({"version": 1, "sheets": [{"name": "S", "merges": [{"r1": 0, "c1": 0, "r2": 4294967295u32, "c2": 0}]}]}));
    bad(json!({"version": 1, "sheets": [{"name": "S", "merges": ["nonsense"]}]}));
    bad(json!({"version": 1, "sheets": [{"name": "S", "cols": [{"index": 99999}]}]}));
    bad(json!({"version": 1, "sheets": [{"name": "S", "rows": [{"index": 4294967295u32}]}]}));
    bad(json!({"version": 1, "sheets": [{"name": "S", "tabColor": "red"}]}));
    bad(json!({"version": 1, "sheets": [{"name": "S", "images": [{"media": "nope", "anchor": {"r": 0, "c": 0}, "w": 10, "h": 10}]}]}));
    bad(json!({"version": 1, "sheets": [{"name": "S", "hyperlinks": [{"r": 0, "c": 99999, "url": "x"}]}]}));
    bad(json!({"version": 1, "sheets": [{"name": "S", "validations": [{"ranges": [], "type": "list"}]}]}));
    bad(json!({"version": 1, "sheets": [{"name": "S", "conditionalFormats": [{"ranges": ["A1"], "rule": {"type": "colorScale", "stops": []}}]}]}));
    bad(json!({"version": 1, "sheets": [{"name": "S"}], "styles": [{"font": {"color": "#12"}}]}));
    bad(json!({"version": 1, "sheets": [{"name": "S"}], "definedNames": [{"name": "x", "ref": "1", "scope": 5}]}));
    bad(json!({"version": 1, "sheets": [{"name": "S"}], "definedNames": [{"name": " ", "ref": "1"}]}));
    let table = |name: &str, r: &str, cols: usize| json!({"name": name, "ref": r, "columns": (0..cols).map(|i| json!({"name": format!("C{i}")})).collect::<Vec<_>>()});
    bad(json!({"version": 1, "sheets": [{"name": "S", "tables": [table("T", "A1:B3", 3)]}]}));
    bad(json!({"version": 1, "sheets": [{"name": "S", "tables": [table("1T", "A1:B3", 2)]}]}));
    bad(json!({"version": 1, "sheets": [{"name": "S", "tables": [table("T", "A1:B1", 2)]}]}));
    bad(json!({"version": 1, "sheets": [{"name": "S", "tables": [table("T", "A1:B3", 2), table("U", "B2:C4", 2)]}]}));
    bad(json!({"version": 1, "sheets": [{"name": "S", "tables": [table("T", "A1:B3", 2)]}, {"name": "R", "tables": [table("t", "A1:B3", 2)]}]}));
    // Clamped rather than refused: absurd sizes, zoom, indent, rotation, font size, active sheet.
    let js: WorkbookJson = serde_json::from_value(json!({
        "version": 1, "activeSheet": 99,
        "sheets": [{"name": "S", "zoom": 9999, "defaultColWidth": -5, "defaultRowHeight": 1e308,
            "cols": [{"index": 0, "width": 1e300}], "rows": [{"index": 0, "height": -1}],
            "freeze": {"rows": 4294967295u32, "cols": 0},
            "cells": [{"r": 0, "c": 0, "v": 1, "s": 1}, {"r": 0, "c": 1, "f": "=((((((((((1))))))))))+", "v": null}]}],
        "styles": [{}, {"font": {"size": 1e9, "name": ""}, "align": {"indent": 4000000000u32, "rotate": -5000}}]
    }))
    .unwrap();
    let wb = rebuild(&js, &no_media);
    let sh = wb.sheet(0).unwrap();
    assert_eq!(sh.zoom, 400);
    assert_eq!(wb.active_sheet, 0);
    assert!(sh.col_width(0) <= 1790.0 && sh.default_row_height <= 546.0 && sh.default_col_width > 0.0);
    assert_eq!(sh.freeze, Some((gridcraft_core::MAX_ROWS - 1, 0)));
    let st = wb.styles.get(sh.cell(at("A1")).unwrap().style);
    assert_eq!((st.font.size, st.align.indent, st.align.rotation), (409.0, 250, -90));
    assert_eq!(st.font.name, gridcraft_model::DEFAULT_FONT);
}

#[test]
fn merge_keeps_what_json_does_not_describe() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "sales"})).unwrap();
    let charts = |s: &Session| s.doc().unwrap().wb.sheets.iter().map(|sh| sh.charts.len()).sum::<usize>();
    let n = charts(&s);
    assert!(n > 0, "the sales sample has a chart");
    let mut js = serde_json::to_value(crate::cmd::exchange::to_json(&s, false).unwrap()).unwrap();
    js["sheets"][0]["cells"].as_array_mut().unwrap().push(json!({"r": 200, "c": 0, "v": "added"}));
    s.execute("workbook.fromJson", json!({"workbook": js, "merge": true})).unwrap();
    assert_eq!(charts(&s), n);
    let sh = s.doc().unwrap().wb.sheets[0].clone();
    assert_eq!(sh.value(CellRef::new(200, 0)), Value::Text("added".into()));
    s.execute("workbook.fromJson", json!({"workbook": js})).unwrap();
    assert_eq!(charts(&s), 0);
}

#[test]
fn commands_open_save_and_media() {
    let mut s = Session::new();
    s.new_workbook();
    let wb = rich();
    let bytes = gridcraft_xlsx::write_xlsx(&wb).unwrap();
    let r = s.execute("file.openBytes", json!({"name": "rich.xlsx", "data": crate::io::base64_encode(&bytes)})).unwrap();
    assert_eq!(r["sheets"][0], "預算 Budget");
    assert_eq!(s.documents().len(), 1, "replaces the untouched blank book");
    let js = s.execute("workbook.json", json!({})).unwrap();
    assert_eq!(js["sheets"][0]["cells"][0]["v"], "項目");
    assert!(js["media"][0].get("data").is_none());
    let m = s.execute("media.bytes", json!({"id": "image1"})).unwrap();
    assert_eq!(crate::io::base64_decode(m["data"].as_str().unwrap()).unwrap(), png());
    assert!(s.execute("media.bytes", json!({"id": "image9"})).is_err());
    // Rebuild from the JSON: media ids resolve against the open workbook.
    s.execute("workbook.fromJson", json!({"workbook": js.to_string()})).unwrap();
    assert_eq!(s.doc().unwrap().wb.sheets[0].images.len(), 1);
    // …or from inline media.
    let inline = s.execute("workbook.json", json!({"inlineMedia": true})).unwrap();
    let mut fresh = Session::new();
    let media: BTreeMap<String, String> = [("image1".to_string(), inline["media"][0]["data"].as_str().unwrap().to_string())].into();
    fresh.execute("workbook.fromJson", json!({"workbook": inline, "media": media})).unwrap();
    assert_eq!(fresh.execute("workbook.json", json!({})).unwrap(), s.execute("workbook.json", json!({})).unwrap());
    // Saving.
    let x = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
    assert_eq!(x["contentType"], "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet");
    assert_eq!(x["data"], x["base64"]);
    let csv = s.execute("file.saveBytes", json!({"format": "csv", "sheet": 1})).unwrap();
    assert_eq!(crate::io::base64_decode(csv["data"].as_str().unwrap()).unwrap(), b"3001\r\n".to_vec());
    let html = s.execute("file.saveBytes", json!({"format": "html"})).unwrap();
    assert!(String::from_utf8(crate::io::base64_decode(html["data"].as_str().unwrap()).unwrap()).unwrap().contains("租金"));
    assert!(s.execute("file.saveBytes", json!({"format": "pdfx"})).is_err());
    assert!(s.execute("file.saveBytes", json!({"format": "csv", "sheet": 9})).is_err());
    // Bad input.
    assert!(s.execute("workbook.fromJson", json!({"workbook": "{"})).is_err());
    assert!(s.execute("workbook.fromJson", json!({"workbook": {"version": 1, "sheets": []}})).is_err());
    assert!(s.execute("workbook.fromJson", json!({"workbook": inline, "media": {"image1": "***"}})).is_err());
    assert!(s.execute("file.openBytes", json!({"name": "x.xlsx", "data": "UEsDBAo="})).is_err());
    assert!(s.execute("file.openBytes", json!({"name": "x.xls", "data": crate::io::base64_encode(&[0xD0, 0xCF, 0x11, 0xE0, 0, 0, 0, 1])})).is_err());
}

#[test]
fn headless_open_replaces_every_workbook() {
    let mut s = Session::new();
    s.new_workbook();
    s.new_workbook();
    let csv = "名字,分數\nAda,90\n";
    let warnings = crate::cmd::exchange::open_bytes(&mut s, "成績.csv", csv.as_bytes()).unwrap();
    assert!(warnings.is_empty());
    assert_eq!(s.documents().len(), 1);
    let js = crate::cmd::exchange::to_json(&s, false).unwrap();
    assert_eq!(js.sheets[0].name, "成績");
    assert_eq!(cell(&js, 0, "B2").v, json!(90.0));
    assert!(crate::cmd::exchange::open_bytes(&mut s, "x.xls", &[0xD0, 0xCF, 0x11, 0xE0, 0, 1, 2]).is_err());
    assert_eq!(s.documents().len(), 1, "a failed open keeps the workbook");
    assert!(crate::cmd::exchange::sheet_html(&s, 0).unwrap().contains("Ada"));
    assert!(crate::cmd::exchange::sheet_html(&s, 3).is_err());
}
