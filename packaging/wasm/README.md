# @tutorfulhk/gridcraft-wasm

GridCraft as a headless WebAssembly library, for web apps that keep their own grid model (Univer,
say): open `.xlsx` / `.csv` / `.tsv`, convert to and from **Workbook JSON v1** (cells with cached
values and formulas, a shared style table, sizes, merges, freeze, conditional formats, validation,
links, comments, pictures, tables; see [`docs/workbook-json.md`](../../docs/workbook-json.md)),
save `.xlsx` / `.csv` / `.tsv`, and render a sheet as an HTML table for previews. It is the GridCraft
engine (`crates/engine`) behind a thin wasm-bindgen binding (`crates/wasm`); it is not the egui app.

Run it in a **Web Worker**: calls are synchronous and a big workbook takes a while. The module never
panics on purpose, but a WebAssembly trap can't be caught inside the module: if a call throws a
`WebAssembly.RuntimeError`, restart the worker.

## API

```ts
import init, { Engine, version } from "@tutorfulhk/gridcraft-wasm";

await init();                                   // fetches gridcraft_wasm_bg.wasm next to the module
const engine = new Engine();                    // starts with a blank workbook (Sheet1)
const warnings = engine.openBytes("marks.xlsx", bytes); // .xlsx/.xlsm/.csv/.tsv/.txt; replaces the workbook
const wb = JSON.parse(engine.workbook());       // Workbook JSON v1 (values recalculated on open)
for (const m of wb.media) engine.mediaBytes(m.id); // Uint8Array of each picture

engine.putMedia("my-image-1", imageBytes);      // stage new pictures (raw bytes, no base64)
engine.fromWorkbook(JSON.stringify(wb));        // replace the workbook; media ids from workbook() need no re-upload
engine.fromWorkbook(JSON.stringify(wb), true);  // merge: same-named sheets keep charts, shapes, PivotTables…
const xlsx = engine.saveBytes("xlsx");          // Uint8Array; also "csv", "tsv", "html" (active sheet), "json"
const html = engine.sheetHtml(0);               // sheet 0 as a standalone HTML page
engine.execute("cell.set", '{"cell":"A1","input":"=1+1"}'); // any engine command (JSON in/out)
version();                                      // "0.4.0"
```

| Call | Does |
|---|---|
| `new Engine()` | an engine with a blank workbook |
| `execute(id, paramsJson) → string` | runs any engine command (`gridcraft-cli commands` lists them) |
| `openBytes(name, bytes) → string[]` | opens workbook bytes, replacing the workbook; returns reader warnings. Legacy `.xls` is an error |
| `saveBytes(format) → Uint8Array` | `"xlsx"`, `"csv"`, `"tsv"`, `"html"` or `"json"` |
| `workbook() → string` | the workbook as Workbook JSON v1 |
| `fromWorkbook(json, merge?)` | replaces the workbook with one built from Workbook JSON (recalculated) |
| `putMedia(id, bytes)` | stages a picture for the next `fromWorkbook` |
| `mediaBytes(id) → Uint8Array` | bytes of a media id the JSON references |
| `sheetHtml(index) → string` | sheet (0-based) as HTML |
| `version() → string` | library version (also `Engine.version()`) |

Errors are thrown as `Error` with the engine's message. The same operations are engine commands
(`file.openBytes`, `file.saveBytes`, `workbook.json`, `workbook.fromJson`, `media.bytes`) taking
and returning base64, for `execute` or the CLI / MCP.

## Building

```sh
cargo install wasm-bindgen-cli --locked --version 0.2.129   # the version in Cargo.lock
npm install -g binaryen                                     # wasm-opt (optional, shrinks the module)
packaging/wasm/build.sh target/wasm-pkg
```

The workflow `.github/workflows/wasm-package.yml` does the same on a tag `wasm-v<version>` and
publishes to GitHub Packages; point the scope there in `.npmrc`:
`@tutorfulhk:registry=https://npm.pkg.github.com`.

## Size

`gridcraft_wasm_bg.wasm`, measured 2026-10-11 with wasm-bindgen 0.2.129 and binaryen wasm-opt:

| Stage | Raw | gzip -9 | brotli -q 11 |
|---|---|---|---|
| after wasm-bindgen | 8.48 MB | 1.93 MB | 1.29 MB |
| + `wasm-opt -Oz` (published) | 4.29 MB | 1.64 MB | 1.17 MB |

The JS glue is about 15 KB (3.5 KB gzipped). Charts and the PDF writer are linked but small; no
cargo features strip them.

## Licence

MIT OR Apache-2.0, as GridCraft.
