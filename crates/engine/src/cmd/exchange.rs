//! Byte and Workbook JSON exchange for headless hosts (a web app running the engine in a
//! Worker): open and save workbooks as bytes, and convert to and from Workbook JSON v1
//! (`docs/workbook-json.md`). None of these touch the file system or open a dialog.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::Deserialize as _;
use serde_json::{Value as Json, json};

use super::*;
use crate::workbook_json::{self, WorkbookJson};

/// Formats `file.saveBytes` writes.
pub const SAVE_FORMATS: &[&str] = &["xlsx", "csv", "tsv", "html", "json"];

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(noundo "file.openBytes", "Open from Bytes", [], None, "{name (the extension picks the format: xlsx, xlsm, csv, tsv, txt, json), data: base64}", always, open_bytes_cmd),
        cmd!(query "workbook.json", "Workbook as JSON", [], None, "{inlineMedia?: bool} → Workbook JSON v1 (docs/workbook-json.md)", has_doc, workbook_json_cmd),
        cmd!(noundo "workbook.fromJson", "Workbook from JSON", [], None, "{workbook: Workbook JSON v1 (object or text), media?: {id: base64}, merge?: bool (keep charts, tables… of same-named sheets)}", always, from_json_cmd),
        cmd!(query "media.bytes", "Media Bytes", [], None, "{id} → {data: base64, contentType, bytes}", has_doc, media_bytes_cmd),
    ]
}

/// MIME type of a saved format.
pub fn content_type(format: &str) -> &'static str {
    match format {
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "csv" => "text/csv",
        "tsv" => "text/tab-separated-values",
        "html" => "text/html",
        "json" => "application/json",
        _ => "application/octet-stream",
    }
}

fn norm_format(format: &str) -> Result<String> {
    let f = format.trim().trim_start_matches('.').to_ascii_lowercase();
    if SAVE_FORMATS.contains(&f.as_str()) {
        Ok(f)
    } else {
        Err(bad("file.saveBytes", format!("unknown format `{format}` ({})", SAVE_FORMATS.join(", "))))
    }
}

/// Opens workbook bytes (`name`'s extension, or the bytes, give the format) as the session's
/// only workbook, replacing every open one. Returns the reader's warnings. On error the session
/// is unchanged.
pub fn open_bytes(s: &mut Session, name: &str, bytes: &[u8]) -> Result<Vec<String>> {
    let (wb, warnings) = crate::io::open_bytes(name, bytes)?;
    let title = std::path::Path::new(name).file_name().and_then(|n| n.to_str()).unwrap_or("Book").to_string();
    replace_all(s, DocState::new(wb, None, title));
    Ok(warnings)
}

fn replace_all(s: &mut Session, d: DocState) {
    while !s.documents().is_empty() {
        s.close_document(0);
    }
    s.add_document(d);
}

/// The active workbook as `format` bytes (one of [`SAVE_FORMATS`]). CSV, TSV and HTML hold one
/// sheet: `sheet`, or the active one.
pub fn save_bytes(s: &Session, format: &str, sheet: Option<usize>) -> Result<Vec<u8>> {
    let f = norm_format(format)?;
    let d = s.doc()?;
    match sheet {
        Some(i) if i >= d.wb.sheets.len() => Err(bad("file.saveBytes", format!("no sheet {i} (the workbook has {})", d.wb.sheets.len()))),
        Some(i) if i != d.wb.active_sheet => {
            let mut wb = (*d.wb).clone();
            wb.active_sheet = i;
            crate::io::save_bytes(&wb, &format!("x.{f}"))
        }
        _ => crate::io::save_bytes(&d.wb, &format!("x.{f}")),
    }
}

/// Sheet `sheet` (0-based) of the active workbook as a standalone HTML page (a table).
pub fn sheet_html(s: &Session, sheet: usize) -> Result<String> {
    let d = s.doc()?;
    if sheet >= d.wb.sheets.len() {
        return Err(bad("sheetHtml", format!("no sheet {sheet} (the workbook has {})", d.wb.sheets.len())));
    }
    Ok(crate::io::to_html(&d.wb, sheet))
}

/// The active workbook as Workbook JSON v1.
pub fn to_json(s: &Session, inline_media: bool) -> Result<WorkbookJson> {
    Ok(workbook_json::to_json(&s.doc()?.wb, inline_media))
}

/// Bytes and content type of a media id of the active workbook (the ids [`to_json`] gives).
pub fn media_bytes(s: &Session, id: &str) -> Result<(Vec<u8>, String)> {
    let d = s.doc()?;
    workbook_json::media_table(&d.wb)
        .into_iter()
        .find(|(m, _, _)| m == id)
        .map(|(_, mime, data)| (data.to_vec(), mime))
        .ok_or_else(|| bad("media.bytes", format!("no media `{id}`")))
}

/// Replaces the active workbook (keeping its title) with one built from `js`, or opens it when
/// none is open. Media ids resolve from `extra` first, then from the active workbook's pictures
/// (the ids `workbook.json` gave out). `merge` keeps what JSON doesn't describe from same-named
/// sheets of the active workbook. The result is recalculated; undo history starts afresh.
pub fn from_json(s: &mut Session, js: &WorkbookJson, extra: &BTreeMap<String, Arc<Vec<u8>>>, merge: bool) -> Result<()> {
    let wb = {
        let current = s.active().map(|d| d.wb.clone());
        let own = current.as_deref().map(workbook_json::media_table).unwrap_or_default();
        let lookup = |id: &str| -> Option<(Arc<Vec<u8>>, String)> {
            if let Some(b) = extra.get(id) {
                return Some((b.clone(), String::new()));
            }
            own.iter().find(|(m, _, _)| m == id).map(|(_, mime, data)| (Arc::new(data.to_vec()), mime.clone()))
        };
        let base = if merge { current.as_deref() } else { None };
        workbook_json::from_json(js, &lookup, base).map_err(|e| bad("workbook.fromJson", e))?
    };
    match s.active_mut() {
        Some(d) => {
            let (path, title) = (d.path.clone(), d.title.clone());
            *d = DocState::new(wb, path, title);
        }
        None => {
            let i = s.add_document(DocState::new(wb, None, "Book1".into()));
            s.set_active(i);
        }
    }
    Ok(())
}

fn open_bytes_cmd(s: &mut Session, p: &Json) -> Result<Json> {
    let data = str_param(p, "data").ok_or_else(|| bad("file.openBytes", "missing `data` (base64)"))?;
    let name = str_param(p, "name").unwrap_or("Book.xlsx");
    super::file::open(s, &json!({"name": name, "base64": data}))
}

fn workbook_json_cmd(s: &mut Session, p: &Json) -> Result<Json> {
    let js = to_json(s, bool_param(p, "inlineMedia").unwrap_or(false))?;
    serde_json::to_value(js).map_err(|e| EngineError::Other(e.to_string()))
}

/// Parses Workbook JSON given as an object or as JSON text.
pub fn parse_workbook_json(raw: &Json) -> std::result::Result<WorkbookJson, String> {
    match raw {
        Json::String(text) => serde_json::from_str(text),
        other => WorkbookJson::deserialize(other),
    }
    .map_err(|e| format!("workbook JSON: {e}"))
}

fn from_json_cmd(s: &mut Session, p: &Json) -> Result<Json> {
    let raw = p.get("workbook").ok_or_else(|| bad("workbook.fromJson", "missing `workbook` (Workbook JSON v1)"))?;
    let js = parse_workbook_json(raw).map_err(|e| bad("workbook.fromJson", e))?;
    let mut extra = BTreeMap::new();
    if let Some(m) = p.get("media") {
        let m = m.as_object().ok_or_else(|| bad("workbook.fromJson", "`media` must be an object of id → base64"))?;
        for (id, data) in m {
            let b64 = data.as_str().or_else(|| data.get("data").and_then(Json::as_str)).unwrap_or("");
            let bytes = crate::io::base64_decode(b64).ok_or_else(|| bad("workbook.fromJson", format!("media `{id}` is not base64")))?;
            extra.insert(id.clone(), Arc::new(bytes));
        }
    }
    from_json(s, &js, &extra, bool_param(p, "merge").unwrap_or(false))?;
    let d = s.doc()?;
    Ok(json!({
        "sheets": d.wb.sheets.iter().map(|sh| sh.name.clone()).collect::<Vec<_>>(),
        "cells": d.wb.sheets.iter().map(|sh| sh.cells.len()).sum::<usize>(),
    }))
}

fn media_bytes_cmd(s: &mut Session, p: &Json) -> Result<Json> {
    let id = str_param(p, "id").ok_or_else(|| bad("media.bytes", "missing `id`"))?;
    let (bytes, mime) = media_bytes(s, id)?;
    Ok(json!({"data": crate::io::base64_encode(&bytes), "contentType": mime, "bytes": bytes.len()}))
}
