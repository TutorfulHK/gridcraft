use serde_json::Value;

use super::*;

/// XLSX bytes of the engine's sales sample (built in code).
fn sample_xlsx() -> Vec<u8> {
    let mut b = Book::new();
    b.execute("file.new", r#"{"sample":"sales"}"#).unwrap();
    b.save_bytes("xlsx").unwrap()
}

fn workbook(b: &Book) -> Value {
    serde_json::from_str(&b.workbook().unwrap()).unwrap()
}

#[test]
fn xlsx_json_xlsx_is_stable() {
    let mut b = Book::new();
    b.open_bytes("sales.xlsx", &sample_xlsx()).unwrap();
    let js = workbook(&b);
    assert_eq!(js["version"], 1);
    assert_eq!(js["sheets"][0]["name"], "Sales");
    assert!(js["sheets"][0]["cells"].as_array().unwrap().iter().any(|c| c["f"].as_str().is_some_and(|f| f.starts_with("=SUM("))));
    b.from_workbook(&js.to_string(), false).unwrap();
    let xlsx = b.save_bytes("xlsx").unwrap();
    assert!(xlsx.starts_with(b"PK"));
    let mut again = Book::new();
    again.open_bytes("again.xlsx", &xlsx).unwrap();
    assert_eq!(workbook(&again)["sheets"], js["sheets"]);
    assert_eq!(workbook(&again)["styles"], js["styles"]);
    assert!(b.sheet_html(0).unwrap().contains("Quarterly Sales Dashboard"));
    assert!(String::from_utf8(b.save_bytes("csv").unwrap()).unwrap().contains("Quarterly Sales Dashboard"));
}

#[test]
fn chinese_text_and_cached_values_survive() {
    let mut b = Book::new();
    b.execute("range.setValues", r#"{"range":"A1","values":[["學生","分數"],["陳大文",88],["李小明",92]]}"#).unwrap();
    b.execute("cell.set", r#"{"cell":"B4","input":"=AVERAGE(B2:B3)"}"#).unwrap();
    let xlsx = b.save_bytes("xlsx").unwrap();
    let mut o = Book::new();
    o.open_bytes("成績.xlsx", &xlsx).unwrap();
    let js = workbook(&o);
    let cells = js["sheets"][0]["cells"].as_array().unwrap();
    assert!(cells.iter().any(|c| c["v"] == "陳大文"));
    let avg = cells.iter().find(|c| c["r"] == 3 && c["c"] == 1).unwrap();
    assert_eq!((avg["f"].as_str(), avg["v"].as_f64()), (Some("=AVERAGE(B2:B3)"), Some(90.0)));
    let csv = String::from_utf8(o.save_bytes("csv").unwrap()).unwrap();
    assert!(csv.contains("李小明"));
}

#[test]
fn staged_media_feeds_from_workbook() {
    let mut b = Book::new();
    // A 1×1 PNG.
    let png: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
        0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8, 0xCF,
        0xC0, 0xF0, 0x1F, 0x00, 0x05, 0x00, 0x01, 0xFF, 0x89, 0x99, 0x3D, 0x1D, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60,
        0x82,
    ];
    b.put_media("logo", png);
    let js = r#"{"version":1,"sheets":[{"name":"S","cells":[{"r":0,"c":0,"v":"hi"}],
        "images":[{"media":"logo","anchor":{"r":1,"c":1,"dx":0,"dy":0},"w":30,"h":30,"alt":"logo"}]}]}"#;
    b.from_workbook(js, false).unwrap();
    let out = workbook(&b);
    let id = out["sheets"][0]["images"][0]["media"].as_str().unwrap().to_string();
    assert_eq!(b.media_bytes(&id).unwrap(), png);
    assert_eq!(out["media"][0]["contentType"], "image/png");
    // A workbook from workbook() rebuilds without staging again, and survives xlsx.
    b.from_workbook(&out.to_string(), false).unwrap();
    assert_eq!(b.media_bytes(&id).unwrap(), png);
    let mut again = Book::new();
    again.open_bytes("p.xlsx", &b.save_bytes("xlsx").unwrap()).unwrap();
    assert_eq!(again.media_bytes(&id).unwrap(), png);
}

#[test]
fn junk_is_an_error_not_a_crash() {
    let mut b = Book::new();
    assert!(b.open_bytes("x.xlsx", b"PK\x03\x04 not a zip").is_err());
    assert!(b.open_bytes("x.xls", &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0]).is_err());
    assert!(b.open_bytes("x.png", &[0x89, b'P', b'N', b'G', 0, 0, 0]).is_err());
    assert!(b.from_workbook("{", false).is_err());
    assert!(b.from_workbook("[]", false).is_err());
    assert!(b.from_workbook(r#"{"version":7,"sheets":[{"name":"S"}]}"#, false).is_err());
    assert!(
        b.from_workbook(r#"{"version":1,"sheets":[{"name":"S","images":[{"media":"missing","anchor":{"r":0,"c":0},"w":1,"h":1}]}]}"#, true).is_err()
    );
    assert!(b.from_workbook(r#"{"version":1,"sheets":[{"name":"S","cells":[{"r":-1,"c":0}]}]}"#, false).is_err());
    assert!(b.execute("no.such.command", "{}").is_err());
    assert!(b.execute("file.new", "{oops").is_err());
    assert!(b.save_bytes("exe").is_err());
    assert!(b.media_bytes("nope").is_err());
    assert!(b.sheet_html(7).is_err());
    assert!(b.sheet_html(u32::MAX).is_err());
    // Still a working blank workbook after all that.
    assert_eq!(workbook(&b)["sheets"][0]["name"], "Sheet1");
    assert!(b.open_bytes("empty.csv", b"").is_ok());
    assert!(!version().is_empty());
}
