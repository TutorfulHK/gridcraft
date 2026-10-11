//! GridCraft as a headless WebAssembly library for web hosts (run it in a Web Worker): open
//! `.xlsx` / `.csv` / `.tsv` workbooks, convert them to and from Workbook JSON
//! (`docs/workbook-json.md`), save `.xlsx` / `.csv` / `.tsv`, and render a sheet as HTML.
//!
//! A thin binding over engine commands: [`Book`] holds the logic (plain Rust, tested natively)
//! and [`Engine`] exports it with wasm-bindgen. A panic aborts on wasm (there is no unwinding), so
//! nothing here may panic; the host restarts the worker if the module traps.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::sync::Arc;

use gridcraft_engine::Session;
use gridcraft_engine::cmd::exchange;
use serde_json::{Value, json};
use wasm_bindgen::prelude::*;

/// The library's state and logic: one open workbook.
pub struct Book {
    session: Session,
    /// Picture bytes staged with [`Book::put_media`] for the next [`Book::from_workbook`].
    staged: BTreeMap<String, Arc<Vec<u8>>>,
}

impl Default for Book {
    fn default() -> Self {
        Self::new()
    }
}

impl Book {
    /// A blank workbook (one sheet, `Sheet1`).
    pub fn new() -> Self {
        let mut session = Session::new();
        session.new_workbook();
        Book { session, staged: BTreeMap::new() }
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    /// Run any engine command; params and result are JSON text.
    pub fn execute(&mut self, id: &str, params_json: &str) -> Result<String, String> {
        let params: Value =
            if params_json.trim().is_empty() { json!({}) } else { serde_json::from_str(params_json).map_err(|e| format!("params: {e}"))? };
        let v = self.session.run(id, params)?;
        serde_json::to_string(&v).map_err(|e| e.to_string())
    }

    /// Open workbook bytes (`.xlsx`, `.xlsm`, `.csv`, `.tsv`, `.txt`; `name`'s extension and the
    /// bytes give the format), replacing the open workbook. Returns the reader's warnings. Clears
    /// staged media.
    pub fn open_bytes(&mut self, name: &str, bytes: &[u8]) -> Result<Vec<String>, String> {
        let warnings = exchange::open_bytes(&mut self.session, name, bytes).map_err(|e| e.to_string())?;
        self.staged.clear();
        Ok(warnings)
    }

    /// The workbook as `xlsx`, `csv`, `tsv`, `html` or `json` bytes (CSV, TSV and HTML: the active
    /// sheet).
    pub fn save_bytes(&self, format: &str) -> Result<Vec<u8>, String> {
        exchange::save_bytes(&self.session, format, None).map_err(|e| e.to_string())
    }

    /// The workbook as Workbook JSON v1 text.
    pub fn workbook(&self) -> Result<String, String> {
        let js = exchange::to_json(&self.session, false).map_err(|e| e.to_string())?;
        serde_json::to_string(&js).map_err(|e| e.to_string())
    }

    /// Replace the workbook with one built from Workbook JSON v1 text. With `merge`, sheets whose
    /// names match keep what the JSON doesn't describe (charts, shapes, PivotTables, sparklines,
    /// filters, print settings).
    pub fn from_workbook(&mut self, json_text: &str, merge: bool) -> Result<(), String> {
        let raw: Value = serde_json::from_str(json_text).map_err(|e| format!("workbook JSON: {e}"))?;
        let js = exchange::parse_workbook_json(&raw)?;
        exchange::from_json(&mut self.session, &js, &self.staged, merge).map_err(|e| e.to_string())
    }

    /// Stage picture bytes for the next [`Book::from_workbook`] (avoids base64 for big blobs).
    pub fn put_media(&mut self, id: &str, bytes: &[u8]) {
        self.staged.insert(id.to_string(), Arc::new(bytes.to_vec()));
    }

    /// Bytes of a media id the workbook JSON references.
    pub fn media_bytes(&self, id: &str) -> Result<Vec<u8>, String> {
        exchange::media_bytes(&self.session, id).map(|(b, _)| b).map_err(|e| e.to_string())
    }

    /// Sheet `sheet` (0-based) as a standalone HTML page (a table), for previews.
    pub fn sheet_html(&self, sheet: u32) -> Result<String, String> {
        let i = usize::try_from(sheet).map_err(|e| e.to_string())?;
        exchange::sheet_html(&self.session, i).map_err(|e| e.to_string())
    }
}

fn js(e: String) -> JsError {
    JsError::new(&e)
}

/// The JS-facing engine: one open workbook (a blank one to start).
#[wasm_bindgen]
pub struct Engine {
    book: Book,
}

#[wasm_bindgen]
impl Engine {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Engine {
        Engine { book: Book::new() }
    }

    /// Run any engine command by id with JSON params; returns the JSON result.
    pub fn execute(&mut self, id: &str, params_json: &str) -> Result<String, JsError> {
        self.book.execute(id, params_json).map_err(js)
    }

    /// Open workbook bytes, replacing the workbook; returns the reader's warnings.
    #[wasm_bindgen(js_name = openBytes)]
    pub fn open_bytes(&mut self, name: &str, bytes: &[u8]) -> Result<Vec<String>, JsError> {
        self.book.open_bytes(name, bytes).map_err(js)
    }

    /// `xlsx`, `csv`, `tsv`, `html` or `json`.
    #[wasm_bindgen(js_name = saveBytes)]
    pub fn save_bytes(&self, format: &str) -> Result<Vec<u8>, JsError> {
        self.book.save_bytes(format).map_err(js)
    }

    /// The workbook as Workbook JSON v1 text.
    pub fn workbook(&self) -> Result<String, JsError> {
        self.book.workbook().map_err(js)
    }

    /// Replace the workbook with one built from Workbook JSON v1 text; `merge` keeps charts and
    /// other parts the JSON doesn't describe on same-named sheets.
    #[wasm_bindgen(js_name = fromWorkbook)]
    pub fn from_workbook(&mut self, json: &str, merge: Option<bool>) -> Result<(), JsError> {
        self.book.from_workbook(json, merge.unwrap_or(false)).map_err(js)
    }

    #[wasm_bindgen(js_name = putMedia)]
    pub fn put_media(&mut self, id: &str, bytes: &[u8]) {
        self.book.put_media(id, bytes)
    }

    #[wasm_bindgen(js_name = mediaBytes)]
    pub fn media_bytes(&self, id: &str) -> Result<Vec<u8>, JsError> {
        self.book.media_bytes(id).map_err(js)
    }

    #[wasm_bindgen(js_name = sheetHtml)]
    pub fn sheet_html(&self, sheet: u32) -> Result<String, JsError> {
        self.book.sheet_html(sheet).map_err(js)
    }

    /// The library version (also the free function `version()`).
    #[wasm_bindgen(js_name = version)]
    pub fn engine_version() -> String {
        version()
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

/// The library version.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[cfg(test)]
mod tests;
