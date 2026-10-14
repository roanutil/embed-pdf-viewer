# Stamp: Swift Wrapper Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Export the workspace-scoped `epdf-plugin-stamp` through UniFFI as `StampHandle`, give it an asset engine the native host can actually run, and ship `StampCapability` in the `EmbedPDF` Swift package, so a Swift app can import a stamp library PDF, list and preview its assets, and place one on a page.

**Architecture:** The recipe in `2026-10-01-swift-package.md` Appendix A §A.0, adapted for a workspace plugin (no document id, no `instance-closed`). A stamp library is a PDF, and the plugin opens library bytes on an "asset engine" (`crates/plugins/stamp/src/services/asset_engine.rs`). The kernel's `NativeEngine` can't serve that today: it only opens `{"$host": id}` input, answers renders as raw pixels where the plugin wants encoded bytes (`image_to_preview`, line 97), and has no `pieceInfo` or named-page paths. So the host builds a second `NativeEngine` for assets, on its own `EngineWorker` thread (PDFium's globals are per thread, `crates/pdfium/src/library.rs:4-10`), with a PNG encoder installed and an encoded render mode. `epdf-engine` gains the named-page kinds the plugin's canonical libraries need. Library persistence goes through a `StampLibraryStore` foreign trait that Swift implements over files.

**Tech Stack:** Rust 2024, `uniffi =0.32.0`, `png` 0.17 (new, in `epdf-viewer-host` only), Swift 6.2 tools, Swift Testing, `swift-custom-dump` 1.3, ImageIO, Xcode 27.0, iPhone 17 simulator.

**Spec:** `docs/superpowers/plans/2026-10-06-swift-plugins/00-umbrella.md`, `docs/superpowers/plans/2026-10-01-swift-package.md` (Global Constraints, Appendix A §A.0 and §A.3 "stamp"), and the TypeScript wire for the kinds this plan adds: `packages/engine/core/src/wire/worker-protocol.ts:650-657` (`pages.renderEncoded`), `:728-746` (`pages.setName`, `pages.removeName`), `:900-917` (`pieceInfo.*`), and `packages/engine/services/src/features/pages/PagesMutator.ts:254-300`.

## Global Constraints

Everything under "Global Constraints" in `00-umbrella.md` applies. The ones this plan is most likely to break:

- `uniffi` is pinned at `=0.32.0`.
- UniFFI names are unique across the workspace. Every record here starts with `Stamp`.
- No `serde_json::Value` across the boundary. A `BinarySource` crosses as `Data` (`Vec<u8>`) and is wrapped as `{"$bytes": base64}` on the Rust side, which costs a base64 round trip per asset; we accept that for the sizes stamps have (tens of kilobytes).
- PDFium never crosses threads. The asset engine gets its own `EngineWorker`; it never shares the document engine's thread.
- `stamp` is workspace-scoped (`crates/plugins/stamp/src/lib.rs:48`): `ViewerHost::stamp()` takes no document id.
- Stamp needs plan 05 (annotation) merged to place assets, and plan 10 (page-edit) merged for the `pages.insert`, `pages.insertBlank`, `pages.delete` and `pages.extract` rows in `translate.rs`, which Appendix C assigns to page-edit. If plan 10 hasn't merged, wait for it rather than adding those rows here.
- Named pages need the fork's `epdf_named_pages.h`, which the 2026-08-30 PDFium slices don't ship.

## Review Focus

1. A library PDF with no `/Names /Pages` registry (a plain PDF someone drops in). People expect one asset per page, labelled from the page index, not an import failure. Pinned in Task 4 (`a_plain_pdf_imports_one_asset_per_page`).
2. Bytes that aren't a PDF passed to `importLibrary`. People expect `invalid-input`-class `InvalidArg`, not a PDFium crash or a half-created library. Pinned in Task 4 (`importing_non_pdf_bytes_is_refused`) and Task 6 (`importingAPNGThrows`).
3. Placing an asset into a document that has since closed. People expect a typed error, and the armed state for that document to clear. Pinned in Task 6 (`placingIntoAClosedDocumentThrows`).
4. Destroying the viewer while the asset engine is mid-import. People expect both engine threads to exit (no leaked PDFium thread). Pinned in Task 3 (`dropping_the_host_joins_the_asset_engine_thread`).
5. `createAssetFromAnnotations` on the native engine, which has no `annotations.exportAppearance`. People expect `unsupported`, said once, not a silent empty asset. Pinned in Task 4 (`create_asset_from_annotations_is_unsupported_natively`).

## Cross-plugin tests

| Integration | Kind | Owner |
|---|---|---|
| annotation (place an asset, `placeAsset`) | optional | this plan, Task 6 (`aPlacedStampIsAnAnnotation`); plan 05 has merged |
| actions (dynamic, form-backed assets evaluated in a detached realm) | optional | plan 15 (actions). It adds `aDynamicStampEvaluatesOnArm` with `EmbedPDF_Dynamic_Approval_Stamp.pdf` once a `ScriptSandbox` exists. Until then a dynamic asset arms with its static appearance, which this plan's tests don't assert either way. |
| form (form-backed assets read `forms.list` on the asset document) | Rust crate dependency | plan 06 maps `forms.list`; nothing here tests it |
| signature (signature-kind libraries, `placeMark`) | consumer | plan 16 |

## File Structure

```
crates/pdfium-sys/wrapper.h                       + #include "epdf_named_pages.h"
crates/pdfium/src/named_pages.rs                  new
crates/pdfium/tests/named_pages.rs                new
crates/epdf-engine/src/features/pages.rs          + namedPages in the page list snapshot
crates/epdf-engine/src/features/pages_write.rs    + set_name, remove_name
crates/epdf-engine/src/host.rs                    + "pages.setName", "pages.removeName"
crates/epdf-engine/tests/named_pages.rs           new
crates/epdf-viewer-host/Cargo.toml                + png, epdf-plugin-stamp
crates/epdf-viewer-host/src/png_encoder.rs        new: ImageEncoder over `png`
crates/epdf-viewer-host/src/engine_worker.rs      + spawn_with
crates/epdf-viewer-host/src/native_engine.rs      + NativeEngine::assets, bytes input, encoded renders
crates/epdf-viewer-host/src/translate.rs          + encoded render, pieceInfo, page.pieceInfo, pages.setName/removeName/flatten
crates/epdf-viewer-host/tests/translate.rs        + tests
crates/epdf-viewer-host/tests/asset_engine.rs     new
crates/plugins/stamp/{Cargo.toml,src/lib.rs,src/contract.rs}
crates/plugins/stamp/src/ffi.rs                   new
crates/plugins/stamp/tests/ffi.rs                 new
crates/epdf-viewer-host/{src/api.rs,src/session.rs,src/lib.rs,tests/viewer_host.rs}
platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Stamp.swift
platforms/apple/EmbedPDF/Tests/EmbedPDFTests/StampTests.swift
platforms/apple/build-xcframework.sh              + stamps fixture
.github/workflows/plugin-rust.yml, platforms/apple/README.md
```

---

### Task 1: Worktree and prerequisites

**Files:**
- Modify: `crates/pdfium-sys/wrapper.h`

**Interfaces:**
- Consumes: `research/swift-plugins` with plans 01, 05 and 10 merged.
- Produces: `pdfium_sys::EPDFDoc_SetNamedPage`, `EPDFDoc_RemoveNamedPage`, `EPDFDoc_GetNamedPageCount`, `EPDFDoc_GetNamedPageAt`.

- [x] **Step 1: Create the worktree**

```bash
git worktree add .claude/worktrees/swift-stamp -b research/swift-stamp research/swift-plugins
cd .claude/worktrees/swift-stamp
```

- [x] **Step 2: Check the prerequisites**

```bash
grep -n '"pages.insert"\|"pages.extract"' crates/epdf-viewer-host/src/translate.rs
for t in darwin-arm64 ios-arm64 ios-sim-arm64; do
  ls packages/engine/runtime/build/libpdfium/$t/include/epdf_named_pages.h || echo "MISSING $t"
done
```

Expected: both `translate.rs` rows (from plan 10), three header paths. A missing header means umbrella Task 0b hasn't run, or this worktree fetched the old slices: run `bash packages/engine/runtime/scripts/fetch-libpdfium.sh <slice>` for each, and if the header is still missing, stop and finish Task 0b.

- [x] **Step 3: Include the header**

In `crates/pdfium-sys/wrapper.h`, after `epdf_form.h` (and after `epdf_measure.h` if plan 08 added it):

```c
#include "epdf_named_pages.h"
```

- [x] **Step 4: Confirm the bindings**

Run: `cargo build -p pdfium-sys && grep -c 'pub fn EPDFDoc_SetNamedPage\|pub fn EPDFDoc_RemoveNamedPage\|pub fn EPDFDoc_GetNamedPageAt' target/debug/build/pdfium-sys-*/out/bindings.rs`
Expected: `3`.

- [x] **Step 5: Commit**

```bash
git add crates/pdfium-sys/wrapper.h
git commit -m "Bind the fork's named-page registry in pdfium-sys"
```

### Task 2: Named pages in `pdfium` and `epdf-engine`

**Files:**
- Create: `crates/pdfium/src/named_pages.rs`, `crates/pdfium/tests/named_pages.rs`
- Modify: `crates/pdfium/src/lib.rs`
- Modify: `crates/epdf-engine/src/features/pages.rs`, `crates/epdf-engine/src/features/pages_write.rs`, `crates/epdf-engine/src/host.rs`
- Create: `crates/epdf-engine/tests/named_pages.rs`

**Interfaces:**
- Produces: `pages.list` results carry `namedPages: [{tree: "pages" | "templates", name, pageObjectNumber}]` (`packages/engine/core/src/dto/NamedPage.ts`); wire kinds `pages.setName` (`{name, pageObjectNumber, replace?}`) and `pages.removeName` (`{name}`), both answering the fresh page list as `{layout}` inside a mutation envelope.

- [x] **Step 1: Write the failing `pdfium` test**

```rust
// crates/pdfium/tests/named_pages.rs
use pdfium::named_pages::NamedPageTree;
use pdfium::{Document, Library};

fn report() -> Vec<u8> {
    std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../pdfium-multiplatform-prototype/fixtures/report.pdf"))
        .unwrap()
}

#[test]
fn a_name_set_reads_back_and_removes() {
    let library = Library::init();
    let doc = Document::load_mem(&library, report(), None).unwrap();
    let pon = doc.page_object_number(0).unwrap();
    assert!(doc.named_pages(NamedPageTree::Pages).unwrap().is_empty());
    doc.set_named_page("approved=Approved", pon).unwrap();
    let names = doc.named_pages(NamedPageTree::Pages).unwrap();
    assert_eq!(names, vec![("approved=Approved".to_string(), pon)]);
    assert!(doc.remove_named_page("approved=Approved").unwrap());
    assert!(!doc.remove_named_page("approved=Approved").unwrap());
}

#[test]
fn an_empty_name_is_refused() {
    let library = Library::init();
    let doc = Document::load_mem(&library, report(), None).unwrap();
    let pon = doc.page_object_number(0).unwrap();
    assert!(doc.set_named_page("", pon).is_err());
}
```

- [x] **Step 2: Run it to verify it fails**

Run: `cargo test -p pdfium --test named_pages`
Expected: FAIL, `could not find named_pages in pdfium`.

- [x] **Step 3: Write `crates/pdfium/src/named_pages.rs`**

```rust
//! The catalog's `/Names /Pages` and `/Names /Templates` trees.
//!
//! Wraps the fork's `epdf_named_pages.h`. Names are layout: setting or
//! removing one changes no page's identity or order.

use crate::{Document, PdfiumError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NamedPageTree {
    Pages,
    Templates,
}

impl NamedPageTree {
    /// `EPDF_NAMED_PAGE_TREE_*`: 0 is `/Pages`, 1 is `/Templates`.
    fn code(self) -> i32 {
        match self {
            NamedPageTree::Pages => 0,
            NamedPageTree::Templates => 1,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            NamedPageTree::Pages => "pages",
            NamedPageTree::Templates => "templates",
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

impl Document {
    /// Every entry of `tree` in key order, as `(name, page object number)`.
    /// An entry whose page is not in the page tree is skipped.
    pub fn named_pages(&self, tree: NamedPageTree) -> Result<Vec<(String, u32)>, PdfiumError> {
        // SAFETY: the handle is live for the borrow.
        let doc = unsafe { self.as_raw() };
        let count = unsafe { pdfium_sys::EPDFDoc_GetNamedPageCount(doc, tree.code()) };
        let mut out = Vec::new();
        for i in 0..count.max(0) {
            let mut pon = 0u32;
            let name = crate::text::read_utf16_opt(|buf, len| unsafe {
                pdfium_sys::EPDFDoc_GetNamedPageAt(doc, tree.code(), i, buf, len, &mut pon)
            })?;
            if let (Some(name), true) = (name, pon != 0) {
                out.push((name, pon));
            }
        }
        Ok(out)
    }

    pub fn set_named_page(&self, name: &str, page_object_number: u32) -> Result<(), PdfiumError> {
        if name.is_empty() {
            return Err(PdfiumError::InvalidArgument("a named page needs a non-empty name"));
        }
        let key = wide(name);
        let ok = unsafe {
            pdfium_sys::EPDFDoc_SetNamedPage(self.as_raw(), key.as_ptr(), page_object_number)
        };
        if ok == 0 { Err(PdfiumError::WriteFailed("named page")) } else { Ok(()) }
    }

    /// Whether a registration was there to remove.
    pub fn remove_named_page(&self, name: &str) -> Result<bool, PdfiumError> {
        if name.is_empty() {
            return Err(PdfiumError::InvalidArgument("a named page needs a non-empty name"));
        }
        let key = wide(name);
        Ok(unsafe { pdfium_sys::EPDFDoc_RemoveNamedPage(self.as_raw(), key.as_ptr()) } != 0)
    }
}
```

Check `EPDFDoc_GetNamedPageAt`'s exact parameter order against `epdf_named_pages.h:67-80` before running; the call above assumes `(document, tree, index, buffer, buflen, out_page_obj_num)`. If the header orders them differently, reorder the call and keep the function's signature. `read_utf16_opt` is the helper plan 08 moved into `text.rs`; if plan 08 hasn't merged, move `action.rs:379`'s `read_utf16` there now under that name. Add `PdfiumError::WriteFailed(&'static str)` if it's absent. Add `pub mod named_pages;` to `lib.rs`, and `Document::page_object_number(index) -> Result<u32, PdfiumError>` if the crate has none under another name (`EPDFDoc_GetPageObjectNumberByIndex`).

- [x] **Step 4: Run the `pdfium` test**

Run: `cargo test -p pdfium --test named_pages`
Expected: PASS, 2 tests.

- [x] **Step 5: Write the failing engine test**

```rust
// crates/epdf-engine/tests/named_pages.rs
use epdf_engine::host::Host;
use serde_json::json;

mod common;
use common::{call, open_fixture};

const REPORT: &str = "pdfium-multiplatform-prototype/fixtures/report.pdf";

#[test]
fn a_set_name_is_in_the_next_page_list() {
    let mut host = Host::new();
    let (doc, pon) = open_fixture(&mut host, REPORT);
    let set = call(&mut host, json!({ "kind": "pages.setName", "docId": doc, "name": "approved=Approved", "pageObjectNumber": pon }));
    assert!(set.get("event").is_some(), "{set}");
    let list = call(&mut host, json!({ "kind": "pages.list", "docId": doc }));
    assert_eq!(list["namedPages"], json!([{ "tree": "pages", "name": "approved=Approved", "pageObjectNumber": pon }]));
}

#[test]
fn removing_a_missing_name_is_not_found() {
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, REPORT);
    let response = host.call(
        &json!({ "kind": "pages.removeName", "jobId": 1, "docId": doc, "name": "nope" }).to_string(),
        Vec::new(),
    );
    let envelope: serde_json::Value = serde_json::from_str(&response.json).unwrap();
    assert_eq!(envelope["error"]["code"], json!("NotFound"), "{envelope}");
}

#[test]
fn replace_moves_a_name() {
    let mut host = Host::new();
    let (doc, pon) = open_fixture(&mut host, REPORT);
    call(&mut host, json!({ "kind": "pages.setName", "docId": doc, "name": "a=A", "pageObjectNumber": pon }));
    call(&mut host, json!({ "kind": "pages.setName", "docId": doc, "name": "a=B", "pageObjectNumber": pon, "replace": "a=A" }));
    let list = call(&mut host, json!({ "kind": "pages.list", "docId": doc }));
    assert_eq!(list["namedPages"].as_array().unwrap().len(), 1);
    assert_eq!(list["namedPages"][0]["name"], json!("a=B"));
}
```

`common::{call, open_fixture}` are the helpers plan 08 added to `crates/epdf-engine/tests/common/mod.rs`; add them as that plan describes if they're missing.

- [x] **Step 6: Run it to verify it fails**

Run: `cargo test -p epdf-engine --test named_pages`
Expected: FAIL, unknown request kind `pages.setName`.

- [x] **Step 7: Implement**

In `crates/epdf-engine/src/features/pages.rs`, where the page list snapshot is built, add `named_pages` to the serialized struct, skipped when empty so existing vectors stay byte-identical:

```rust
    /// `PageListSnapshot.namedPages`: both trees, `/Pages` first, each in
    /// key order.
    #[serde(rename = "namedPages", skip_serializing_if = "Vec::is_empty")]
    pub named_pages: Vec<NamedPageEntry>,
```

```rust
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedPageEntry {
    pub tree: &'static str,
    pub name: String,
    pub page_object_number: u32,
}

fn read_named_pages(doc: &pdfium::Document) -> Result<Vec<NamedPageEntry>, EngineError> {
    use pdfium::named_pages::NamedPageTree;
    let mut out = Vec::new();
    for tree in [NamedPageTree::Pages, NamedPageTree::Templates] {
        for (name, page_object_number) in doc.named_pages(tree).map_err(EngineError::from)? {
            out.push(NamedPageEntry { tree: tree.as_str(), name, page_object_number });
        }
    }
    Ok(out)
}
```

and fill it in `list_pages`. In `pages_write.rs`:

```rust
/// `PagesMutator.setName`: register `name` for the page, removing
/// `replace` first when it is a different, non-empty name.
pub fn set_name(
    doc: &pdfium::Document,
    name: &str,
    page_object_number: u32,
    replace: Option<&str>,
) -> Result<(), EngineError> {
    if name.is_empty() {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "pages.setName requires a non-empty name",
        ));
    }
    if let Some(old) = replace.filter(|r| !r.is_empty() && *r != name) {
        doc.remove_named_page(old).map_err(EngineError::from)?;
    }
    doc.set_named_page(name, page_object_number).map_err(|_| {
        EngineError::with_message(
            EngineErrorCode::Unknown,
            format!("EPDFDoc_SetNamedPage rejected '{name}' for page {page_object_number}"),
        )
    })
}

pub fn remove_name(doc: &pdfium::Document, name: &str) -> Result<(), EngineError> {
    if name.is_empty() {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "pages.removeName requires a non-empty name",
        ));
    }
    if !doc.remove_named_page(name).map_err(EngineError::from)? {
        return Err(EngineError::with_message(
            EngineErrorCode::NotFound,
            format!("pages.removeName: no /Names /Pages registration '{name}'"),
        ));
    }
    Ok(())
}
```

Route both in `host.rs` beside `"pages.move"` (line 807), copying that arm's shape: resolve the page (`page_field`) so an unknown page is `NotFound`, call the writer, then answer `{layout: <fresh pages.list>}` through the same mutation-envelope helper `pages.move` uses, with no revision bump (names are layout).

- [x] **Step 8: Run the engine tests**

Run: `cargo test -p epdf-engine --test named_pages && cargo test -p epdf-engine`
Expected: PASS, and the existing `pages.list` vectors unchanged.

- [x] **Step 9: Commit**

```bash
git add crates/pdfium crates/epdf-engine
git commit -m "Read and write named pages in the Rust engine"
```

### Task 3: The asset engine in `epdf-viewer-host`

**Files:**
- Create: `crates/epdf-viewer-host/src/png_encoder.rs`, `crates/epdf-viewer-host/tests/asset_engine.rs`
- Modify: `crates/epdf-viewer-host/Cargo.toml`, `src/lib.rs` (`pub mod png_encoder;`), `src/engine_worker.rs`, `src/native_engine.rs`, `src/translate.rs`, `tests/translate.rs`

**Interfaces:**
- Consumes: Task 2's wire kinds.
- Produces: `NativeEngine::assets(post: KernelPost) -> (NativeEngine, EngineWorker)`, an engine whose `open` takes `{"kind": "bytes", "bytes": {"$bytes": ...}}` and whose `page.render.image` answers `{source: {kind: "bytes", bytes: {"$bytes": ...}}, contentType: "image/png", width, height}`; `translate::Decode::Encoded`; `PATHS` entries for those routes, so `translate::maps` answers `pieceInfo`, `page.pieceInfo`, `pages.setName`, `pages.removeName`, `pages.flatten`.

- [x] **Step 1: Write the failing translation tests**

Append to `crates/epdf-viewer-host/tests/translate.rs`:

```rust
#[test]
fn piece_info_paths_carry_their_holder() {
    assert!(translate::maps("pieceInfo") && translate::maps("page.pieceInfo"));
    let doc = translate::translate("doc-1", "pieceInfo.read", None, &[json!("EmbedPDF")], 1).unwrap();
    assert_eq!(doc.request["kind"], json!("pieceInfo.read"));
    assert_eq!(doc.request["application"], json!("EmbedPDF"));
    assert!(doc.request.get("pageObjectNumber").is_none());
    let page = translate::translate("doc-1", "page.pieceInfo.update", Some(4), &[json!("EmbedPDF"), json!({ "set": {} })], 2).unwrap();
    assert_eq!(page.request["kind"], json!("pieceInfo.update"));
    assert_eq!(page.request["pageObjectNumber"], json!(4));
    assert_eq!(page.request["patch"], json!({ "set": {} }));
}

#[test]
fn named_page_paths_go_out_as_the_host_reads_them() {
    let set = translate::translate(
        "doc-1", "pages.setName", None,
        &[json!({ "name": "a=A", "page": { "kind": "objectNumber", "pageObjectNumber": 3 }, "replace": "a=Z" })], 1,
    ).unwrap();
    assert_eq!(set.request["kind"], json!("pages.setName"));
    assert_eq!(set.request["name"], json!("a=A"));
    assert_eq!(set.request["pageObjectNumber"], json!(3));
    assert_eq!(set.request["replace"], json!("a=Z"));
    let remove = translate::translate("doc-1", "pages.removeName", None, &[json!({ "name": "a=A" })], 2).unwrap();
    assert_eq!(remove.request["name"], json!("a=A"));
}

#[test]
fn an_encoded_render_asks_for_png_and_answers_bytes() {
    let call = translate::translate_encoded_render("doc-1", Some(2), &[json!({ "viewport": { "kind": "width", "width": 64 }, "format": "png" })], 1).unwrap();
    assert_eq!(call.request["kind"], json!("pages.renderEncoded"));
    assert_eq!(call.request["encode"], json!({ "format": "png" }));
    let response = HostResponse {
        json: r#"{"result":{"mimeType":"image/png","width":64,"height":80,"size":3}}"#.into(),
        payloads: vec![vec![1, 2, 3]],
    };
    let decoded = translate::decode(&translate::Decode::Encoded, response).unwrap();
    assert_eq!(decoded.value["contentType"], json!("image/png"));
    assert_eq!(decoded.value["source"]["kind"], json!("bytes"));
    assert_eq!(decoded.value["source"]["bytes"], json!({ "$bytes": "AQID" }));
}
```

- [x] **Step 2: Run them to verify they fail**

Run: `cargo test -p epdf-viewer-host --test translate`
Expected: FAIL to compile, no `translate_encoded_render`, no `Decode::Encoded`.

- [x] **Step 3: Extend `translate.rs`**

Add routes:

```rust
        "pieceInfo.read" | "page.pieceInfo.read" => ("pieceInfo.read", Decode::Result),
        "pieceInfo.update" | "page.pieceInfo.update" => ("pieceInfo.update", Decode::Mutation),
        "pages.setName" => ("pages.setName", Decode::Mutation),
        "pages.removeName" => ("pages.removeName", Decode::Mutation),
        "pages.flatten" => ("pages.flatten", Decode::Mutation),
```

(skip any of these plan 10 already added). Add them to `PATHS` (umbrella Task 0e), one line each in plan order, both spellings of the piece-info paths:

```rust
    "pieceInfo.read",
    "page.pieceInfo.read",
    "pieceInfo.update",
    "page.pieceInfo.update",
    "pages.setName",
    "pages.removeName",
    "pages.flatten",
```

`maps("pieceInfo")` and `maps("page.pieceInfo")` then answer through the namespace rule. `pages.setName` and `pages.flatten` fail on `every_routed_path_is_listed`'s dummy args (no page, no page list); skip them in that test with a comment naming `named_page_paths_go_out_as_the_host_reads_them` as their cover.

In `translate`'s `extra` match, the page-scoped piece-info paths share a kind with the document ones, so tell them apart with a guard on `path` (the match stays on `kind`, as every other plan's arms expect):

```rust
        "pieceInfo.read" if path == "page.pieceInfo.read" => {
            vec![("pageObjectNumber", page()?), ("application", arg(0))]
        }
        "pieceInfo.read" => vec![("application", arg(0))],
        "pieceInfo.update" if path == "page.pieceInfo.update" => {
            vec![("pageObjectNumber", page()?), ("application", arg(0)), ("patch", arg(1))]
        }
        "pieceInfo.update" => vec![("application", arg(0)), ("patch", arg(1))],
        "pages.setName" => {
            let input = arg(0);
            let pon = PageRef::from_json(input.get("page").unwrap_or(&Value::Null))
                .map(|p| Value::from(p.page_object_number))
                .ok_or_else(|| coded("invalid-input", "[epdf-viewer-host] pages.setName needs a page".into()))?;
            vec![
                ("name", input.get("name").cloned().unwrap_or(Value::Null)),
                ("pageObjectNumber", pon),
                ("replace", input.get("replace").cloned().unwrap_or(Value::Null)),
            ]
        }
        "pages.removeName" => vec![("name", arg(0).get("name").cloned().unwrap_or(Value::Null))],
```

`pages.flatten`'s arguments follow `crates/plugins/stamp/src/write/marks.rs` and `libraries.rs` call sites: read them there and map `pageObjectNumbers` and `usage` the same way.

Add the encoded render:

```rust
    /// `pages.renderEncoded`: width, height and type in the result, the
    /// encoded bytes as the payload, answered as the `PageImage` a local
    /// engine gives the stamp plugin (`source.kind == "bytes"`).
    Encoded,
```

```rust
/// `page.render.image` on the asset engine: the same options as
/// `pages.render`, plus the encode the plugin asked for (`format`, default
/// png).
pub fn translate_encoded_render(
    doc: &str,
    pon: Option<u32>,
    args: &[Value],
    job: u64,
) -> Result<WireCall, Error> {
    let page = pon.ok_or_else(|| {
        coded("invalid-input", "[epdf-viewer-host] page.render.image needs a page".into())
    })?;
    let options = args.first().cloned().unwrap_or(Value::Null);
    let format = options.get("format").and_then(Value::as_str).unwrap_or("png");
    let encode = match format {
        "png" => serde_json::json!({ "format": "png" }),
        other => {
            return Err(coded(
                "unsupported",
                format!("[epdf-viewer-host] the asset engine encodes png only, not {other}"),
            ));
        }
    };
    let request = serde_json::json!({
        "kind": "pages.renderEncoded",
        "jobId": job,
        "docId": doc,
        "pageObjectNumber": page,
        "options": render_options(options),
        "encode": encode,
    });
    Ok(WireCall { request, payloads: Vec::new(), decode: Decode::Encoded })
}
```

and in `decode`:

```rust
        Decode::Encoded => {
            let bytes = response
                .payloads
                .into_iter()
                .next()
                .ok_or_else(|| malformed("an encoded render without its bytes".into()))?;
            let content_type = result
                .remove("mimeType")
                .ok_or_else(|| malformed("an encoded render without a mimeType".into()))?;
            let mut image = Map::new();
            image.insert("contentType".into(), content_type);
            image.insert("width".into(), result.remove("width").unwrap_or(Value::Null));
            image.insert("height".into(), result.remove("height").unwrap_or(Value::Null));
            image.insert(
                "source".into(),
                serde_json::json!({ "kind": "bytes", "bytes": epdf_kernel::bytes::Bytes(bytes).to_json() }),
            );
            Value::Object(image)
        }
```

- [x] **Step 4: Run the translation tests**

Run: `cargo test -p epdf-viewer-host --test translate`
Expected: PASS.

- [x] **Step 5: Write the failing asset-engine test**

`packages/default-stamps/en/stamps.pdf` is the shipped English stamp library: a canonical library with a `/Names /Pages` registry.

```rust
// crates/epdf-viewer-host/tests/asset_engine.rs
use std::rc::Rc;

use epdf_ffi::KernelThread;
use epdf_kernel::bytes::Bytes;
use epdf_kernel::engine::Engine;
use epdf_viewer_host::native_engine::NativeEngine;
use futures::executor::block_on;
use serde_json::json;

fn stamps() -> Vec<u8> {
    std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../packages/default-stamps/en/stamps.pdf")).unwrap()
}

#[test]
fn the_asset_engine_opens_bytes_and_renders_png() {
    let thread = KernelThread::spawn(|post| Ok::<_, epdf_kernel::Error>(NativeEngine::assets(post))).unwrap();
    let rendered = block_on(thread.call(|(engine, _worker)| {
        let opened = engine.open(json!({ "kind": "bytes", "id": "t", "bytes": Bytes(stamps()).to_json() }), None);
        let image = epdf_kernel::rt::block_on_kernel(async move {
            let doc = opened.await?;
            let pages = doc.call("pages.list", None, Vec::new()).await?;
            let pon = pages["pages"][0]["pageObjectNumber"].as_u64().unwrap() as u32;
            assert!(pages["namedPages"].as_array().is_some_and(|n| !n.is_empty()), "{pages}");
            let image = doc
                .call("page.render.image", Some(pon), vec![json!({ "viewport": { "kind": "width", "width": 64 }, "format": "png" })])
                .await?;
            doc.call("close", None, Vec::new()).await?;
            Ok::<_, epdf_kernel::Error>(image)
        });
        image
    }))
    .unwrap()
    .unwrap();
    assert_eq!(rendered["contentType"], json!("image/png"));
    let png = Bytes::from_json(&rendered["source"]["bytes"]).unwrap().0;
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
}

#[test]
fn dropping_the_host_joins_the_asset_engine_thread() {
    let thread = KernelThread::spawn(|post| Ok::<_, epdf_kernel::Error>(NativeEngine::assets(post))).unwrap();
    let witness = block_on(thread.call(|(_engine, worker)| worker.alive())).unwrap();
    assert!(witness.upgrade().is_some());
    block_on(thread.call(|(_engine, worker)| worker.join())).unwrap();
    drop(thread);
    assert!(witness.upgrade().is_none());
}
```

`rt::block_on_kernel` stands for however the existing host tests drive a kernel future to completion on the kernel thread; `crates/epdf-viewer-host/tests/viewer_host.rs` drives through `ViewerHost`, so if no such helper exists, write this test through `ViewerHost` with `stamp: Some(StampConfig::default())` after Task 5 instead, calling `StampHandle::import_library` and `asset_preview`, and keep the two assertions (PNG magic, `namedPages` non-empty via a non-empty asset list).

- [x] **Step 6: Run it to verify it fails**

Run: `cargo test -p epdf-viewer-host --test asset_engine`
Expected: FAIL to compile, no `NativeEngine::assets`.

- [x] **Step 7: Write the encoder, the worker constructor and the asset engine**

`crates/epdf-viewer-host/Cargo.toml`:

```toml
png = "0.17"
```

`crates/epdf-viewer-host/src/png_encoder.rs`:

```rust
//! The asset engine's image encoder: `pages.renderEncoded` needs one, and
//! a stamp preview is a PNG. Straight alpha in, straight alpha out.

use epdf_engine::error::{EngineError, EngineErrorCode};
use epdf_engine::features::render::raster::PageRaster;
use epdf_engine::features::server::{EncodedImage, ImageEncoder, RenderEncode};

pub struct PngEncoder;

impl ImageEncoder for PngEncoder {
    fn encode(&self, raster: &PageRaster, encode: &RenderEncode) -> Result<EncodedImage, EngineError> {
        if !matches!(encode, RenderEncode::Png) {
            return Err(EngineError::with_message(
                EngineErrorCode::NotImplemented,
                "the native asset engine encodes png only",
            ));
        }
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, raster.width, raster.height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().map_err(failed)?;
            let row = raster.width as usize * 4;
            let mut packed = Vec::with_capacity(row * raster.height as usize);
            for y in 0..raster.height as usize {
                let start = y * raster.stride as usize;
                packed.extend_from_slice(&raster.rgba[start..start + row]);
            }
            writer.write_image_data(&packed).map_err(failed)?;
        }
        Ok(EncodedImage {
            content_type: "image/png".into(),
            width: raster.width,
            height: raster.height,
            bytes,
        })
    }
}

fn failed(e: png::EncodingError) -> EngineError {
    EngineError::with_message(EngineErrorCode::Unknown, format!("png encoding failed: {e}"))
}
```

In `engine_worker.rs`, make `spawn` a call to a new `spawn_with`:

```rust
    pub fn spawn() -> EngineWorker {
        EngineWorker::spawn_with(Host::new)
    }

    /// `spawn`, with the host built by `setup` on the owner thread: the
    /// asset engine's host has an image encoder installed.
    pub fn spawn_with(setup: impl FnOnce() -> Host + Send + 'static) -> EngineWorker {
        // the body of today's `spawn`, with `OwnerThread::start(setup)`
    }
```

In `native_engine.rs`, add the encoded mode and the constructor:

```rust
struct Inner {
    worker: EngineWorker,
    post: KernelPost,
    documents: Cell<u64>,
    /// The asset engine: renders answer encoded bytes, not raw pixels.
    encoded_renders: bool,
}

impl NativeEngine {
    pub fn new(worker: EngineWorker, post: KernelPost) -> NativeEngine {
        NativeEngine::with(worker, post, false)
    }

    /// The stamp plugin's asset engine: its own engine thread, with a PNG
    /// encoder, opening library bytes and answering previews as encoded
    /// bytes. The caller joins the worker when the session ends.
    pub fn assets(post: KernelPost) -> (NativeEngine, EngineWorker) {
        let worker = EngineWorker::spawn_with(|| {
            epdf_engine::host::Host::new().with_encoder(Box::new(crate::png_encoder::PngEncoder))
        });
        (NativeEngine::with(worker.clone(), post, true), worker)
    }

    fn with(worker: EngineWorker, post: KernelPost, encoded_renders: bool) -> NativeEngine {
        NativeEngine { inner: Rc::new(Inner { worker, post, documents: Cell::new(0), encoded_renders }) }
    }
}
```

In `NativeDocument::call`, route the render:

```rust
    fn call(&self, path: &str, pon: Option<u32>, args: Vec<Value>) -> EngineCall<Value> {
        if self.inner.encoded_renders && path == "page.render.image" {
            return self.inner.send(&self.id, |job| {
                translate::translate_encoded_render(&self.id, pon, &args, job)
            });
        }
        self.inner.send(&self.id, |job| translate::translate(&self.id, path, pon, &args, job))
    }
```

`Engine::open` already accepts the stamp plugin's `{"kind": "bytes", "bytes": ...}` input beside `{"$host": id}`: umbrella Task 0e reads `bytes` whatever the `kind`.

- [x] **Step 8: Run the host tests**

Run: `cargo test -p epdf-viewer-host`
Expected: PASS, including `asset_engine`.

- [x] **Step 9: Commit**

```bash
git add crates/epdf-viewer-host Cargo.lock
git commit -m "Add a native asset engine for stamp libraries"
```

### Task 4: The stamp FFI module

**Files:**
- Modify: `crates/plugins/stamp/Cargo.toml`, `crates/plugins/stamp/src/lib.rs`, `crates/plugins/stamp/src/contract.rs`
- Create: `crates/plugins/stamp/src/ffi.rs`, `crates/plugins/stamp/tests/ffi.rs`

**Interfaces:**
- Consumes: `epdf_ffi::{AnnotationRef, CancelToken, KernelBound, KernelPost, PageRef, Point, Size, Subscription, ViewerError}` (`AnnotationRef` from plan 05).
- Produces (`epdf_plugin_stamp::ffi`): `StampHandle::new(cap: StampCapability, post: KernelPost) -> Arc<Self>`; `StampConfig`; records `StampAsset`, `StampAssetPreview`, `StampMark`, `StampAddAssetInput`, `StampImportOptions`, `StampCreateLibraryOptions`, `StampLibraryPatch`, `StampAssetPatch`, `StampAssetFilter`, `StampBatchResult`, `StampBatchFailure`, `StampErrorInfo`, `StampEvent`, `StoredStampLibrary`; derived in place: `StampLibrary`, `StampAssetKind`, `StampLibraryChangeReason`; foreign traits `StampEventListener`, `StampLibraryStore`; object `StampPersistence`.

The members, from `crates/plugins/stamp/src/harness.rs` `MEMBERS` (lines 50-84), under their FFI names: `libraries`, `library`, `assets`, `asset`, `asset_preview`, `render_asset_preview`, `read_asset_bytes`, `export_library`, `can_place`, `can_import`, `create_library`, `update_library`, `import_library`, `delete_library`, `create_asset`, `create_asset_from_annotations`, `update_asset`, `delete_asset`, `move_asset`, `duplicate_asset`, `arm_asset`, `place_asset`, `place_asset_on_pages`, `disarm`, `armed_asset`, and `subscribe` for the eight `on*` hooks. `restore` and `persist` wrap `persistence.rs`'s `restore_stamp_libraries` and `persist_stamp_libraries`, which aren't capability members but are how an embedder keeps custom libraries.

- [ ] **Step 1: Feature, scaffolding, in-place derives**

`Cargo.toml` gets the same `uniffi` feature, optional `epdf-ffi` and `uniffi` dependencies, dev-dependencies and `[[test]] name = "ffi" required-features = ["uniffi"]` block plan 08 Task 3 Step 1 shows, with `stamp` in place of `measurement`. `lib.rs` gets `#[cfg(feature = "uniffi")] pub mod ffi;` and `#[cfg(feature = "uniffi")] uniffi::setup_scaffolding!();`.

In `contract.rs`:

```rust
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StampAssetKind { Stamp, Signature, Initials }

#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StampLibrary { /* unchanged */ }

#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StampLibraryChangeReason { /* unchanged */ }
```

- [ ] **Step 2: Write the failing tests**

The fake engine can't open library bytes, so these tests drive the handle's own behaviour: listing, unknown ids, arming errors, events and persistence over a fake store. Task 6 and the host test in Task 5 cover import over real PDFium.

```rust
// crates/plugins/stamp/tests/ffi.rs
#![cfg(feature = "uniffi")]

use std::sync::{Arc, Mutex};

use epdf_ffi::{CancelToken, KernelThread, ViewerError};
use epdf_plugin_stamp::ffi::{
    StampAddAssetInput, StampEvent, StampEventListener, StampHandle, StampLibraryStore,
    StoredStampLibrary,
};
use epdf_plugin_stamp::{STAMP, StampConfig, stamp_plugin};
use futures::executor::block_on;
use plugin_conformance::test_kernel::{TestKernel, one_document};

#[derive(Default)]
struct Events(Mutex<Vec<StampEvent>>);

impl StampEventListener for Events {
    fn on_event(&self, event: StampEvent) {
        self.0.lock().unwrap().push(event);
    }
}

#[derive(Default)]
struct Store(Mutex<Vec<StoredStampLibrary>>);

impl StampLibraryStore for Store {
    fn list(&self) -> Result<Vec<StoredStampLibrary>, ViewerError> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn put(&self, id: String, bytes: Vec<u8>) -> Result<(), ViewerError> {
        let mut rows = self.0.lock().unwrap();
        rows.retain(|r| r.id != id);
        rows.push(StoredStampLibrary { id, bytes });
        Ok(())
    }
    fn delete(&self, id: String) -> Result<(), ViewerError> {
        self.0.lock().unwrap().retain(|r| r.id != id);
        Ok(())
    }
}

fn handle() -> (KernelThread<TestKernel>, Arc<StampHandle>, String) {
    let thread =
        KernelThread::spawn(|_| TestKernel::open(vec![stamp_plugin(StampConfig::default())], one_document()))
            .unwrap();
    let post = thread.post();
    let (handle, document_id) = block_on(thread.call(move |k| {
        let cap = k.kernel.capability(&STAMP, None).unwrap();
        (StampHandle::new(cap, post), k.document_id.clone())
    }))
    .unwrap();
    (thread, handle, document_id)
}

fn code(error: &ViewerError) -> Option<&str> {
    match error {
        ViewerError::Kernel { code, .. } => code.as_deref(),
        ViewerError::Closed => None,
    }
}

fn name(error: &ViewerError) -> Option<&str> {
    match error {
        ViewerError::Kernel { name, .. } => Some(name),
        ViewerError::Closed => None,
    }
}

#[test]
fn a_fresh_workspace_has_no_libraries() {
    let (_thread, stamp, _) = handle();
    assert!(block_on(stamp.libraries(None)).unwrap().is_empty());
    assert!(block_on(stamp.assets(None)).unwrap().is_empty());
    assert!(block_on(stamp.can_import()).unwrap());
}

#[test]
fn unknown_ids_read_as_none() {
    let (_thread, stamp, _) = handle();
    assert!(block_on(stamp.library("nope".into())).unwrap().is_none());
    assert!(block_on(stamp.asset("nope".into())).unwrap().is_none());
    assert!(block_on(stamp.asset_preview("nope".into())).unwrap().is_none());
    assert!(block_on(stamp.read_asset_bytes("nope".into())).unwrap().is_none());
}

#[test]
fn arming_an_unknown_asset_is_refused() {
    let (_thread, stamp, document_id) = handle();
    let error = block_on(stamp.arm_asset(document_id, "nope".into(), None, CancelToken::new())).unwrap_err();
    assert!(code(&error).is_some() || name(&error).is_some(), "{error:?}");
    assert!(block_on(stamp.armed_asset("doc".into())).unwrap().is_none());
}

#[test]
fn importing_non_pdf_bytes_is_refused() {
    let (_thread, stamp, _) = handle();
    let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    let error = block_on(stamp.import_library(png, Default::default(), CancelToken::new())).unwrap_err();
    assert_eq!(code(&error), Some("InvalidArg"), "{error:?}");
    assert!(block_on(stamp.libraries(None)).unwrap().is_empty());
}

#[test]
fn creating_a_library_without_canonical_services_says_so() {
    let (_thread, stamp, _) = handle();
    // The fake engine lists no `pieceInfo` service.
    let error = block_on(stamp.create_library("Mine".into(), Default::default(), CancelToken::new())).unwrap_err();
    assert_eq!(code(&error), Some("NotImplemented"), "{error:?}");
}

#[test]
fn create_asset_from_annotations_is_unsupported_natively() {
    let (_thread, stamp, document_id) = handle();
    let error = block_on(stamp.create_asset_from_annotations(
        document_id,
        epdf_ffi::PageRef { page_object_number: 1 },
        Vec::new(),
        StampAddAssetInput::default(),
        CancelToken::new(),
    ))
    .unwrap_err();
    assert!(matches!(code(&error), Some("NotImplemented") | Some("unsupported") | Some("not-found")), "{error:?}");
}

#[test]
fn restore_reads_nothing_from_an_empty_store() {
    let (_thread, stamp, _) = handle();
    let restored = block_on(stamp.restore(Arc::new(Store::default()), CancelToken::new())).unwrap();
    assert!(restored.is_empty());
}

#[test]
fn persist_returns_a_stoppable_object() {
    let (_thread, stamp, _) = handle();
    let persistence = stamp.persist(Arc::new(Store::default()), Vec::new(), None);
    persistence.stop();
    persistence.stop();
}

#[test]
fn subscribe_and_cancel_do_not_leak() {
    let (thread, stamp, _) = handle();
    let events = Arc::new(Events::default());
    let subscription = stamp.subscribe(events.clone());
    subscription.cancel();
    block_on(thread.call(|_| ())).unwrap();
    assert!(events.0.lock().unwrap().is_empty());
}
```

If `create_asset_from_annotations` answers a code outside the three the test allows, that's this plan's Review Focus item 5 not holding: read what the plugin does when `page.annotations.exportAppearance` is absent (`write/assets.rs:291`), and make the FFI answer `unsupported` there if the plugin throws something less clear.

- [ ] **Step 3: Run them to verify they fail**

Run: `cargo test -p epdf-plugin-stamp --features uniffi --test ffi`
Expected: FAIL to compile, no `ffi` module.

- [ ] **Step 4: Write `crates/plugins/stamp/src/ffi.rs`**

```rust
//! The stamp plugin through uniffi: workspace-scoped, so the handle needs
//! no document and every placement names one.
//!
//! A `BinarySource` crosses as bytes and becomes `{"$bytes": base64}`
//! here; the plugin's batch results and engine errors are JSON and decode
//! into typed records, and a value that does not decode is
//! `invalid-state`.

use std::rc::Rc;
use std::sync::{Arc, Mutex};

use epdf_ffi::{
    AnnotationRef, CancelToken, KernelBound, KernelPost, PageRef, Point, Size, Subscription,
    ViewerError,
};
use epdf_kernel::bytes::Bytes;
use epdf_kernel::rt::Promise;
use epdf_kernel::{Error, Unsubscribe};
use serde_json::Value;

use crate::persistence::{
    PersistOptions, StampLibraryStore as CoreStore, StoredLibrary, persist_stamp_libraries,
    restore_stamp_libraries,
};
use crate::{
    AddAssetInput, AssetPatch, CreateLibraryOptions, ImportLibraryOptions, LibraryPatch,
    MarkSource, StampAssetFilter as CoreAssetFilter, StampAssetKind, StampCapability,
    StampLibrary, StampLibraryChangeReason, StampLibraryFilter,
};

fn invalid_state(what: &str, e: impl std::fmt::Display) -> Error {
    Error::named("InvalidState", format!("[stamp] {what} does not decode: {e}"))
        .with("code", "invalid-state")
}

fn source(bytes: Vec<u8>) -> Value {
    Bytes(bytes).to_json()
}

// ── config ──────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct StampConfig {
    /// Cached preview width in device pixels. `None` is 256.
    #[uniffi(default = None)]
    pub preview_width: Option<f64>,
    /// Evaluate form-backed assets on arm. `None` is true; scripting is the
    /// target document's actions plugin's switch.
    #[uniffi(default = None)]
    pub dynamic: Option<bool>,
}

impl StampConfig {
    /// The plugin's config with the host's asset engine factory.
    pub fn into_core(self, asset_engine: Option<crate::AssetEngine>) -> crate::StampConfig {
        crate::StampConfig {
            asset_engine,
            preview_width: self.preview_width,
            dynamic: self.dynamic,
            host: crate::StampHost::default(),
        }
    }
}

// ── records ─────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct StampAsset {
    /// `libraryId:name`, stable.
    pub id: String,
    pub library_id: String,
    pub kind: StampAssetKind,
    pub name: String,
    pub label: String,
    /// Intrinsic size in PDF points.
    pub size: Size,
    /// The library page that is this asset.
    pub page: PageRef,
    pub subject: Option<String>,
    pub categories: Option<Vec<String>>,
}

impl From<crate::StampAsset> for StampAsset {
    fn from(a: crate::StampAsset) -> Self {
        StampAsset {
            id: a.id,
            library_id: a.library_id,
            kind: a.kind,
            name: a.name,
            label: a.label,
            size: a.size.into(),
            page: a.page.into(),
            subject: a.subject,
            categories: a.categories,
        }
    }
}

/// A cached, paintable render. `bytes` is `None` when the engine answered
/// none.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct StampAssetPreview {
    pub bytes: Option<Vec<u8>>,
    pub mime_type: Option<String>,
}

impl From<crate::StampAssetPreview> for StampAssetPreview {
    fn from(p: crate::StampAssetPreview) -> Self {
        StampAssetPreview { bytes: p.bytes, mime_type: p.mime_type }
    }
}

/// How a new asset's artwork is authored.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum StampMark {
    /// Strokes in PDF points.
    Ink { strokes: Vec<Vec<Point>>, color: Option<String>, stroke_width: Option<f64> },
    Text { text: String, font_family: String, color: Option<String>, font_size: Option<f64> },
    /// PNG or JPEG bytes.
    Image { bytes: Vec<u8> },
    /// One page of a PDF.
    Pdf { bytes: Vec<u8>, page_index: Option<u32> },
}

impl From<StampMark> for MarkSource {
    fn from(m: StampMark) -> Self {
        match m {
            StampMark::Ink { strokes, color, stroke_width } => MarkSource::Ink {
                strokes: strokes
                    .into_iter()
                    .map(|s| s.into_iter().map(|p| (p.x, p.y)).collect())
                    .collect(),
                color,
                stroke_width,
            },
            StampMark::Text { text, font_family, color, font_size } => {
                MarkSource::Text { text, font_family, color, font_size }
            }
            StampMark::Image { bytes } => MarkSource::Image { source: source(bytes) },
            StampMark::Pdf { bytes, page_index } => {
                MarkSource::Pdf { source: source(bytes), page_index: page_index.map(f64::from) }
            }
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct StampAddAssetInput {
    #[uniffi(default = None)]
    pub library_id: Option<String>,
    #[uniffi(default = None)]
    pub name: Option<String>,
    #[uniffi(default = None)]
    pub label: Option<String>,
    #[uniffi(default = None)]
    pub kind: Option<StampAssetKind>,
    #[uniffi(default = None)]
    pub subject: Option<String>,
    #[uniffi(default = None)]
    pub categories: Option<Vec<String>>,
    /// The artwork as bytes (PDF, PNG or JPEG). Give this or `mark`.
    #[uniffi(default = None)]
    pub source: Option<Vec<u8>>,
    #[uniffi(default = None)]
    pub mark: Option<StampMark>,
    #[uniffi(default = None)]
    pub preview: Option<Vec<u8>>,
    #[uniffi(default = None)]
    pub size: Option<Size>,
}

impl From<StampAddAssetInput> for AddAssetInput {
    fn from(i: StampAddAssetInput) -> Self {
        AddAssetInput {
            library_id: i.library_id,
            name: i.name,
            label: i.label,
            kind: i.kind,
            subject: i.subject,
            categories: i.categories,
            source: i.source.map(source),
            mark: i.mark.map(Into::into),
            preview: i.preview.map(source),
            size: i.size.map(Into::into),
        }
    }
}

/// `asset_name` (per-page labels from a closure) is not exported; a plain
/// PDF's assets are labelled from their page index.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct StampImportOptions {
    #[uniffi(default = None)]
    pub name: Option<String>,
    #[uniffi(default = None)]
    pub categories: Option<Vec<String>>,
    #[uniffi(default = None)]
    pub kind: Option<StampAssetKind>,
    #[uniffi(default = None)]
    pub library_kind: Option<String>,
}

impl From<StampImportOptions> for ImportLibraryOptions {
    fn from(o: StampImportOptions) -> Self {
        ImportLibraryOptions {
            name: o.name,
            categories: o.categories,
            kind: o.kind,
            library_kind: o.library_kind,
            asset_name: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct StampCreateLibraryOptions {
    #[uniffi(default = None)]
    pub id: Option<String>,
    #[uniffi(default = None)]
    pub kind: Option<String>,
    #[uniffi(default = None)]
    pub categories: Option<Vec<String>>,
}

#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct StampLibraryPatch {
    #[uniffi(default = None)]
    pub name: Option<String>,
    #[uniffi(default = None)]
    pub categories: Option<Vec<String>>,
}

/// `clear_subject` removes the subject override; `subject` sets it.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct StampAssetPatch {
    #[uniffi(default = None)]
    pub label: Option<String>,
    #[uniffi(default = None)]
    pub subject: Option<String>,
    #[uniffi(default = false)]
    pub clear_subject: bool,
    #[uniffi(default = None)]
    pub categories: Option<Vec<String>>,
}

#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct StampAssetFilter {
    #[uniffi(default = None)]
    pub library_id: Option<String>,
    #[uniffi(default = None)]
    pub kind: Option<StampAssetKind>,
    #[uniffi(default = None)]
    pub category: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct StampErrorInfo {
    pub code: String,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct StampBatchFailure {
    pub page: PageRef,
    pub error: StampErrorInfo,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct StampBatchResult {
    pub applied: Vec<AnnotationRef>,
    pub skipped: Vec<PageRef>,
    pub failed: Vec<StampBatchFailure>,
}

impl TryFrom<crate::BatchResult> for StampBatchResult {
    type Error = Error;

    fn try_from(r: crate::BatchResult) -> Result<Self, Error> {
        let annotation = |v: Value| {
            serde_json::from_value::<epdf_annotation_core::AnnotationRef>(v)
                .map(AnnotationRef::from)
                .map_err(|e| invalid_state("a placed annotation", e))
        };
        let page = |v: Value| {
            epdf_kernel::geometry::PageRef::from_json(&v)
                .map(PageRef::from)
                .ok_or_else(|| invalid_state("a skipped page", &v))
        };
        Ok(StampBatchResult {
            applied: r.applied.into_iter().map(annotation).collect::<Result<_, _>>()?,
            skipped: r.skipped.into_iter().map(page).collect::<Result<_, _>>()?,
            failed: r
                .failed
                .into_iter()
                .map(|f| {
                    let code = f.error.get("code").and_then(Value::as_str);
                    let message = f.error.get("message").and_then(Value::as_str);
                    match (code, message) {
                        (Some(code), Some(message)) => Ok(StampBatchFailure {
                            page: f.page.into(),
                            error: StampErrorInfo { code: code.into(), detail: message.into() },
                        }),
                        _ => Err(invalid_state("a placement error", &f.error)),
                    }
                })
                .collect::<Result<_, _>>()?,
        })
    }
}

// ── events ──────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum StampEvent {
    /// A library's canonical bytes changed: persist it.
    LibraryChanged { library_id: String, reason: StampLibraryChangeReason },
    LibraryCreated { library_id: String, library: Option<StampLibrary> },
    LibraryUpdated { library_id: String, library: Option<StampLibrary> },
    LibraryDeleted { library_id: String },
    AssetCreated { asset_id: String, library_id: String, asset: Option<StampAsset> },
    AssetUpdated { asset_id: String, library_id: String, asset: Option<StampAsset> },
    AssetDeleted { asset_id: String, library_id: String },
    ArmChanged { document_id: String, asset_id: Option<String> },
}

/// Workspace events never end on their own: the stream finishes when the
/// viewer is destroyed.
#[uniffi::export(with_foreign)]
pub trait StampEventListener: Send + Sync {
    fn on_event(&self, event: StampEvent);
}

// ── persistence port ────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct StoredStampLibrary {
    pub id: String,
    pub bytes: Vec<u8>,
}

/// Where custom libraries live: bytes by library id. Swift implements it
/// over files. Called on the kernel thread, synchronously; keep it to
/// local disk.
#[uniffi::export(with_foreign)]
pub trait StampLibraryStore: Send + Sync {
    fn list(&self) -> Result<Vec<StoredStampLibrary>, ViewerError>;
    fn put(&self, id: String, bytes: Vec<u8>) -> Result<(), ViewerError>;
    fn delete(&self, id: String) -> Result<(), ViewerError>;
}

/// The Rust store over the foreign one.
struct ForeignStore(Arc<dyn StampLibraryStore>);

fn settle<T: Clone + 'static>(result: Result<T, ViewerError>) -> Promise<T> {
    match result {
        Ok(value) => Promise::resolved(value),
        Err(ViewerError::Kernel { name, code, detail }) => {
            let mut error = Error::named(name, detail);
            if let Some(code) = code {
                error = error.with("code", code);
            }
            Promise::rejected(error)
        }
        Err(ViewerError::Closed) => Promise::rejected(Error::new("[stamp] the store is closed")),
    }
}

impl CoreStore for ForeignStore {
    fn list(&self) -> Promise<Vec<StoredLibrary>> {
        settle(self.0.list().map(|rows| {
            rows.into_iter().map(|r| StoredLibrary { id: r.id, bytes: r.bytes }).collect()
        }))
    }
    fn put(&self, id: &str, bytes: Vec<u8>) -> Promise<()> {
        settle(self.0.put(id.to_string(), bytes))
    }
    fn delete(&self, id: &str) -> Promise<()> {
        settle(self.0.delete(id.to_string()))
    }
}

/// A running `persist`. `stop` is idempotent.
#[derive(uniffi::Object)]
pub struct StampPersistence {
    stop: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

#[uniffi::export]
impl StampPersistence {
    pub fn stop(&self) {
        if let Some(stop) = self.stop.lock().unwrap().take() {
            stop();
        }
    }
}

// ── the handle ──────────────────────────────────────────────────────────────

#[derive(uniffi::Object)]
pub struct StampHandle {
    cap: KernelBound<StampCapability>,
    post: KernelPost,
}

impl StampHandle {
    /// On the kernel thread.
    pub fn new(cap: StampCapability, post: KernelPost) -> Arc<Self> {
        Arc::new(StampHandle { cap: KernelBound::new(cap, post.clone()), post })
    }
}

#[uniffi::export]
impl StampHandle {
    /// Every library, or those of the given kinds (`stamps`, `signatures`).
    pub async fn libraries(&self, kinds: Option<Vec<String>>) -> Result<Vec<StampLibrary>, ViewerError> {
        self.cap
            .call(move |c| {
                let filter = kinds.map(|kind| StampLibraryFilter { kind: Some(kind) });
                Ok(c.list_libraries(filter.as_ref()))
            })
            .await
    }

    pub async fn library(&self, id: String) -> Result<Option<StampLibrary>, ViewerError> {
        self.cap.call(move |c| Ok(c.get_library(&id))).await
    }

    pub async fn assets(&self, filter: Option<StampAssetFilter>) -> Result<Vec<StampAsset>, ViewerError> {
        self.cap
            .call(move |c| {
                let filter = filter.map(|f| CoreAssetFilter {
                    library_id: f.library_id,
                    kind: f.kind,
                    category: f.category,
                });
                Ok(c.list_assets(filter.as_ref()).into_iter().map(Into::into).collect())
            })
            .await
    }

    pub async fn asset(&self, id: String) -> Result<Option<StampAsset>, ViewerError> {
        self.cap.call(move |c| Ok(c.get_asset(&id).map(Into::into))).await
    }

    /// The cached preview, made at import. `None` before then.
    pub async fn asset_preview(&self, id: String) -> Result<Option<StampAssetPreview>, ViewerError> {
        self.cap.call(move |c| Ok(c.get_asset_preview(&id).map(Into::into))).await
    }

    /// A fresh render `width` device pixels wide.
    pub async fn render_asset_preview(
        &self,
        id: String,
        width: f64,
        cancel: Arc<CancelToken>,
    ) -> Result<Option<StampAssetPreview>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(c.render_asset_preview(&id, width)),
                |p: Option<crate::StampAssetPreview>| p.map(Into::into),
            )
            .await
    }

    /// The asset's own one-page PDF.
    pub async fn read_asset_bytes(&self, id: String) -> Result<Option<Vec<u8>>, ViewerError> {
        self.cap.call(move |c| Ok(c.read_asset_bytes(&id))).await
    }

    /// The library's canonical PDF.
    pub async fn export_library(&self, id: String, cancel: Arc<CancelToken>) -> Result<Vec<u8>, ViewerError> {
        self.cap.call_promise_cancellable(cancel, move |c| Ok(c.export_library(&id)), |b| b).await
    }

    /// Whether `document_id` (or the active document) can take a stamp.
    pub async fn can_place(&self, document_id: Option<String>) -> Result<bool, ViewerError> {
        self.cap.call(move |c| Ok(c.can_place(document_id.as_deref()))).await
    }

    pub async fn can_import(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(c.can_import())).await
    }

    /// Returns the new library's id.
    pub async fn create_library(
        &self,
        name: String,
        options: StampCreateLibraryOptions,
        cancel: Arc<CancelToken>,
    ) -> Result<String, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let opts = CreateLibraryOptions { id: options.id, kind: options.kind, categories: options.categories };
                    Ok(c.create_library(&name, opts))
                },
                |id| id,
            )
            .await
    }

    pub async fn update_library(
        &self,
        id: String,
        patch: StampLibraryPatch,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(c.update_library(&id, LibraryPatch { name: patch.name, categories: patch.categories })),
                |()| (),
            )
            .await
    }

    /// Import a library PDF. Returns the new library's id. `InvalidArg` for
    /// bytes that aren't a PDF.
    pub async fn import_library(
        &self,
        bytes: Vec<u8>,
        options: StampImportOptions,
        cancel: Arc<CancelToken>,
    ) -> Result<String, ViewerError> {
        self.cap
            .call_promise_cancellable(cancel, move |c| Ok(c.import_library(source(bytes), options.into())), |id| id)
            .await
    }

    pub async fn delete_library(&self, id: String, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap.call_promise_cancellable(cancel, move |c| Ok(c.delete_library(&id)), |()| ()).await
    }

    /// Returns the new asset's id.
    pub async fn create_asset(
        &self,
        input: StampAddAssetInput,
        cancel: Arc<CancelToken>,
    ) -> Result<String, ViewerError> {
        self.cap.call_promise_cancellable(cancel, move |c| Ok(c.create_asset(input.into())), |id| id).await
    }

    /// Needs `annotations.exportAppearance`, which the native engine has
    /// not mapped: answers `unsupported` there.
    pub async fn create_asset_from_annotations(
        &self,
        document_id: String,
        page: PageRef,
        annotations: Vec<AnnotationRef>,
        input: StampAddAssetInput,
        cancel: Arc<CancelToken>,
    ) -> Result<String, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let refs = annotations
                        .into_iter()
                        .map(|a| serde_json::to_value(epdf_annotation_core::AnnotationRef::from(a)))
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|e| Error::new(e.to_string()))?;
                    Ok(c.create_asset_from_annotations(&document_id, page.into(), refs, input.into()))
                },
                |id| id,
            )
            .await
    }

    pub async fn update_asset(&self, id: String, patch: StampAssetPatch, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let subject = if patch.clear_subject { Some(None) } else { patch.subject.map(Some) };
                    Ok(c.update_asset(&id, AssetPatch { label: patch.label, subject, categories: patch.categories }))
                },
                |()| (),
            )
            .await
    }

    pub async fn delete_asset(&self, id: String, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap.call_promise_cancellable(cancel, move |c| Ok(c.delete_asset(&id)), |()| ()).await
    }

    /// Returns the asset's new id (ids carry the library).
    pub async fn move_asset(&self, id: String, to_library_id: String, cancel: Arc<CancelToken>) -> Result<String, ViewerError> {
        self.cap.call_promise_cancellable(cancel, move |c| Ok(c.move_asset(&id, &to_library_id)), |id| id).await
    }

    pub async fn duplicate_asset(&self, id: String, label: Option<String>, cancel: Arc<CancelToken>) -> Result<String, ViewerError> {
        self.cap.call_promise_cancellable(cancel, move |c| Ok(c.duplicate_asset(&id, label)), |id| id).await
    }

    /// Arm `asset_id` for the next tap in `document_id`.
    pub async fn arm_asset(
        &self,
        document_id: String,
        asset_id: String,
        target_width: Option<f64>,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(cancel, move |c| Ok(c.arm_asset(&document_id, &asset_id, target_width)), |()| ())
            .await
    }

    /// Place `asset_id` on `page` of `document_id` at `at` (page space).
    pub async fn place_asset(
        &self,
        document_id: String,
        asset_id: String,
        page: PageRef,
        at: Point,
        target_width: Option<f64>,
        rotation: Option<f64>,
        cancel: Arc<CancelToken>,
    ) -> Result<AnnotationRef, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let placement = epdf_plugin_annotation::StampPlacement {
                        page: epdf_annotation_core::PageRef::new(page.page_object_number),
                        at: at.into(),
                        target_width,
                        rotation,
                    };
                    Ok(c.place_asset(&document_id, &asset_id, placement))
                },
                AnnotationRef::from,
            )
            .await
    }

    /// The same placement on every page (`pages` is `None`) or those listed.
    pub async fn place_asset_on_pages(
        &self,
        document_id: String,
        asset_id: String,
        pages: Option<Vec<PageRef>>,
        at: Point,
        target_width: Option<f64>,
        rotation: Option<f64>,
        cancel: Arc<CancelToken>,
    ) -> Result<StampBatchResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let pages = pages.map(|p| p.into_iter().map(Into::into).collect());
                    Ok(c.place_asset_on_pages(&document_id, &asset_id, pages, at.into(), target_width, rotation))
                },
                |r: crate::BatchResult| StampBatchResult::try_from(r).map_err(ViewerError::from),
            )
            .await?
    }

    pub async fn disarm(&self, document_id: String) -> Result<(), ViewerError> {
        self.cap.call(move |c| c.disarm(&document_id)).await
    }

    pub async fn armed_asset(&self, document_id: String) -> Result<Option<StampAsset>, ViewerError> {
        self.cap.call(move |c| Ok(c.get_armed_asset(&document_id).map(Into::into))).await
    }

    /// Load every library `store` holds. Returns the restored ids.
    pub async fn restore(
        &self,
        store: Arc<dyn StampLibraryStore>,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<String>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(restore_stamp_libraries(c, Rc::new(ForeignStore(store)), Rc::new(|_, _| {}))),
                |ids| ids,
            )
            .await
    }

    /// Write each changed library to `store`, debounced (`None` is the
    /// plugin's default), except the ids in `except`. Runs until `stop`.
    pub fn persist(
        &self,
        store: Arc<dyn StampLibraryStore>,
        except: Vec<String>,
        debounce_ms: Option<f64>,
    ) -> Arc<StampPersistence> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.cap.fire(move |c| {
            let stop = persist_stamp_libraries(
                c,
                Rc::new(ForeignStore(store)),
                PersistOptions { except, debounce_ms },
                Rc::new(|_, _| {}),
            );
            slot.fill(Unsubscribe::new(move || stop()));
        });
        Arc::new(StampPersistence { stop: Mutex::new(Some(Box::new(move || subscription.cancel()))) })
    }

    /// Every hook in one registration.
    pub fn subscribe(&self, listener: Arc<dyn StampEventListener>) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.cap.fire(move |c| {
            let l = listener.clone();
            let offs = vec![
                c.on_library_changed({
                    let l = l.clone();
                    move |e| {
                        l.on_event(StampEvent::LibraryChanged { library_id: e.library_id.clone(), reason: e.reason });
                        Ok(())
                    }
                }),
                c.on_library_created({
                    let l = l.clone();
                    move |e| {
                        l.on_event(StampEvent::LibraryCreated { library_id: e.library_id.clone(), library: e.library.clone() });
                        Ok(())
                    }
                }),
                c.on_library_updated({
                    let l = l.clone();
                    move |e| {
                        l.on_event(StampEvent::LibraryUpdated { library_id: e.library_id.clone(), library: e.library.clone() });
                        Ok(())
                    }
                }),
                c.on_library_deleted({
                    let l = l.clone();
                    move |e| {
                        l.on_event(StampEvent::LibraryDeleted { library_id: e.library_id.clone() });
                        Ok(())
                    }
                }),
                c.on_asset_created({
                    let l = l.clone();
                    move |e| {
                        l.on_event(StampEvent::AssetCreated {
                            asset_id: e.asset_id.clone(),
                            library_id: e.library_id.clone(),
                            asset: e.asset.clone().map(Into::into),
                        });
                        Ok(())
                    }
                }),
                c.on_asset_updated({
                    let l = l.clone();
                    move |e| {
                        l.on_event(StampEvent::AssetUpdated {
                            asset_id: e.asset_id.clone(),
                            library_id: e.library_id.clone(),
                            asset: e.asset.clone().map(Into::into),
                        });
                        Ok(())
                    }
                }),
                c.on_asset_deleted({
                    let l = l.clone();
                    move |e| {
                        l.on_event(StampEvent::AssetDeleted { asset_id: e.asset_id.clone(), library_id: e.library_id.clone() });
                        Ok(())
                    }
                }),
                c.on_arm_changed(move |e| {
                    l.on_event(StampEvent::ArmChanged { document_id: e.document_id.clone(), asset_id: e.asset_id.clone() });
                    Ok(())
                }),
            ];
            slot.fill(Unsubscribe::new(move || offs.iter().for_each(Unsubscribe::call)));
        });
        subscription
    }
}
```

Check three signatures against the crate before running, and adapt the call, not the FFI shape: `restore_stamp_libraries`'s `Warn` type (`persistence.rs:80`), `persist_stamp_libraries`'s return (`Rc<dyn Fn()>`, line 154), and the `PageRef` type `place_asset_on_pages` takes (`epdf_kernel::geometry::PageRef`, from `write/placement.rs:308`). `persist`'s `stop` closure isn't `Send`, so it runs on the kernel thread through the subscription slot; `StampPersistence::stop` only cancels that subscription.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p epdf-plugin-stamp --features uniffi --test ffi && cargo test -p epdf-plugin-stamp`
Expected: PASS, 9 FFI tests, and the conformance vectors without `uniffi`.

- [ ] **Step 6: Commit**

```bash
git add crates/plugins/stamp
git commit -m "Export the stamp plugin through uniffi"
```

### Task 5: Host wiring

**Files:**
- Modify: `crates/epdf-viewer-host/Cargo.toml`, `src/api.rs`, `src/session.rs`, `src/lib.rs`, `tests/viewer_host.rs`

**Interfaces:**
- Consumes: Task 3's `NativeEngine::assets`, Task 4's `StampHandle`, `ffi::StampConfig::into_core`.
- Produces: `ViewerConfig.stamp: Option<StampConfig>`; `ViewerHost::stamp() -> Result<Arc<StampHandle>, ViewerError>`.

- [ ] **Step 1: Write the failing host test**

```rust
#[test]
fn a_stamp_library_imports_and_places_over_pdfium() {
    let host = ViewerHost::new(
        ViewerConfig {
            interaction: true,
            annotation: Some(Default::default()),
            stamp: Some(Default::default()),
            ..Default::default()
        },
        Arc::new(NullDelegate),
    )
    .unwrap();
    block_on(host.start()).unwrap();
    let stamp = block_on(host.stamp()).unwrap();
    let library = block_on(stamp.import_library(read_repo_file("packages/default-stamps/en/stamps.pdf"), Default::default(), CancelToken::new())).unwrap();
    let assets = block_on(stamp.assets(Some(StampAssetFilter { library_id: Some(library), ..Default::default() }))).unwrap();
    assert!(!assets.is_empty());
    let preview = block_on(stamp.asset_preview(assets[0].id.clone())).unwrap().unwrap();
    assert_eq!(preview.mime_type.as_deref(), Some("image/png"));
    let doc = block_on(host.open_document(fixture("report"), OpenDocumentOptions::default())).unwrap();
    let page = block_on(host.pages(doc.id.clone())).unwrap()[0].page;
    block_on(stamp.place_asset(doc.id, assets[0].id.clone(), page, Point { x: 100.0, y: 100.0 }, None, None, CancelToken::new())).unwrap();
}

#[test]
fn a_plain_pdf_imports_one_asset_per_page() {
    let host = ViewerHost::new(ViewerConfig { stamp: Some(Default::default()), ..Default::default() }, Arc::new(NullDelegate)).unwrap();
    block_on(host.start()).unwrap();
    let stamp = block_on(host.stamp()).unwrap();
    let report = fixture("report");
    let library = block_on(stamp.import_library(report, Default::default(), CancelToken::new())).unwrap();
    let pages = 3; // report.pdf's page count; read it from `host.pages` if the fixture changes
    let assets = block_on(stamp.assets(Some(StampAssetFilter { library_id: Some(library), ..Default::default() }))).unwrap();
    assert_eq!(assets.len(), pages);
}
```

`read_repo_file` reads a path relative to the workspace root; add it next to `fixture` if the file has none. Check `report.pdf`'s page count with `host.pages` once and put the number in the test.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p epdf-viewer-host --test viewer_host stamp`
Expected: FAIL to compile, no `stamp` on `ViewerConfig`.

- [ ] **Step 3: Wire the host**

`Cargo.toml`:

```toml
epdf-plugin-stamp = { path = "../plugins/stamp", default-features = false, features = ["uniffi"] }
```

`api.rs`, in plan-number order:

```rust
    /// Install `stampPlugin(config)`: workspace stamp libraries, on their
    /// own asset engine thread. Placing needs `annotation`.
    #[uniffi(default = None)]
    pub stamp: Option<epdf_plugin_stamp::ffi::StampConfig>,
```

`session.rs`: the asset engine is built on first use and joined with the session. Add a field and the branch:

```rust
pub struct Session {
    pub kernel: Kernel,
    worker: EngineWorker,
    /// The stamp plugin's asset engine thread, once its factory has run.
    asset_worker: Rc<RefCell<Option<EngineWorker>>>,
    on_change: Unsubscribe,
}
```

```rust
        let asset_worker: Rc<RefCell<Option<EngineWorker>>> = Rc::default();
        if let Some(stamp) = config.stamp {
            let slot = asset_worker.clone();
            let assets_post = post.clone();
            let factory: Rc<dyn Fn() -> Result<epdf_plugin_stamp::AssetEngineRef, Error>> =
                Rc::new(move || {
                    let (engine, worker) = NativeEngine::assets(assets_post.clone());
                    *slot.borrow_mut() = Some(worker);
                    Ok(epdf_plugin_stamp::AssetEngineRef::Ready(Rc::new(engine)))
                });
            plugins.push(epdf_plugin_stamp::stamp_plugin(
                stamp.into_core(Some(epdf_plugin_stamp::AssetEngine::Factory(factory))),
            ));
        }
```

and in `Drop for Session`, after `self.worker.join()`:

```rust
        if let Some(worker) = self.asset_worker.borrow_mut().take() {
            worker.join();
        }
```

`post` must be cloned before `NativeEngine::new(worker.clone(), post)` takes it; move that clone above. `lib.rs`:

```rust
    /// The workspace stamp capability. Needs `ViewerConfig.stamp`.
    pub async fn stamp(&self) -> Result<Arc<StampHandle>, ViewerError> {
        let post = self.post.clone();
        self.call(move |s| {
            let cap = s.kernel.capability(&epdf_plugin_stamp::STAMP, None)?;
            Ok(StampHandle::new(cap, post))
        })
        .await
    }
```

- [ ] **Step 4: Run the host tests**

Run: `cargo test -p epdf-ffi -p epdf-viewer-host`
Expected: PASS. The existing `dropping_the_host_joins_both_threads` still passes; with stamp on and an import done, there are three threads to join, and Task 3's test covers the third.

- [ ] **Step 5: Commit**

```bash
git add crates/epdf-viewer-host
git commit -m "Add stamp to ViewerHost with a lazily started asset engine"
```

### Task 6: `StampCapability` in Swift

**Files:**
- Create: `platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Stamp.swift`, `platforms/apple/EmbedPDF/Tests/EmbedPDFTests/StampTests.swift`
- Modify: `platforms/apple/build-xcframework.sh`

**Interfaces:**
- Consumes: generated `StampHandle`, `ViewerHost.stamp()`; plan 05's `PDFViewer.annotations(for:)`.
- Produces: `StampCapability`, `PDFViewer.stamps()`, `FileStampLibraryStore`.

- [ ] **Step 1: Add the fixture**

In `platforms/apple/build-xcframework.sh`, add to the `FIXTURES` list (umbrella Task 0f), one line each, after the last plan's lines:

```bash
  packages/default-stamps/en/stamps.pdf
```

- [ ] **Step 2: Write the failing Swift tests**

```swift
// platforms/apple/EmbedPDF/Tests/EmbedPDFTests/StampTests.swift
import CustomDump
import EmbedPDF
import Foundation
import Testing

extension BaseSuite {
  @Suite struct StampTests {
    func started() async throws -> (PDFViewer, StampCapability) {
      let viewer = try PDFViewer(
        ViewerConfig(interaction: true, annotation: AnnotationConfig(), stamp: StampConfig()))
      try await viewer.start()
      return (viewer, try await viewer.stamps())
    }

    @Test func aLibraryImportsWithPreviews() async throws {
      let (_, stamps) = try await started()
      let library = try await stamps.importLibrary(Fixture.data("stamps"))
      let assets = try await stamps.assets(StampAssetFilter(libraryId: library))
      #expect(!assets.isEmpty)
      let preview = try #require(try await stamps.assetPreview(assets[0].id))
      expectNoDifference(preview.mimeType, "image/png")
      #expect(preview.image != nil)
    }

    @Test func importingAPNGThrows() async throws {
      let (_, stamps) = try await started()
      let png = Data([0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A])
      await #expect {
        _ = try await stamps.importLibrary(png)
      } throws: { error in
        (error as? PDFError)?.code == "InvalidArg"
      }
      expectNoDifference(try await stamps.libraries().count, 0)
    }

    @Test func aPlacedStampIsAnAnnotation() async throws {
      let (viewer, stamps) = try await started()
      let library = try await stamps.importLibrary(Fixture.data("stamps"))
      let asset = try #require(try await stamps.assets(StampAssetFilter(libraryId: library)).first)
      let document = try await viewer.open(Fixture.data("report"))
      let page = try await viewer.pages(of: document)[0].page
      let ref = try await stamps.place(asset.id, in: document, on: page, at: Point(x: 100, y: 100))
      let annotations = try await viewer.annotations(for: document)
      #expect(try await annotations.get(ref) != nil)
    }

    @Test func placingIntoAClosedDocumentThrows() async throws {
      let (viewer, stamps) = try await started()
      let library = try await stamps.importLibrary(Fixture.data("stamps"))
      let asset = try #require(try await stamps.assets(StampAssetFilter(libraryId: library)).first)
      let document = try await viewer.open(Fixture.data("report"))
      let page = try await viewer.pages(of: document)[0].page
      try await viewer.close(document)
      await #expect(throws: PDFError.self) {
        _ = try await stamps.place(asset.id, in: document, on: page, at: Point(x: 10, y: 10))
      }
      expectNoDifference(try await stamps.armedAsset(in: document), nil)
    }

    @Test func aLibraryPersistsThroughAFileStore() async throws {
      let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
      let store = try FileStampLibraryStore(directory: directory)
      do {
        let (_, stamps) = try await started()
        let persistence = stamps.persist(to: store, debounce: .zero)
        _ = try await stamps.importLibrary(Fixture.data("stamps"))
        try await Task.sleep(for: .milliseconds(100))
        persistence.stop()
      }
      let (_, again) = try await started()
      let restored = try await again.restore(from: store)
      expectNoDifference(restored.count, 1)
    }

    @Test func droppingTheStreamUnsubscribes() async throws {
      let (_, stamps) = try await started()
      let task = Task { for await _ in stamps.events {} }
      try await Task.sleep(for: .milliseconds(20))
      task.cancel()
      _ = await task.value
      _ = try await stamps.importLibrary(Fixture.data("stamps"))
    }

    @Test func importCancelsWhenTheTaskIsCancelled() async throws {
      let (_, stamps) = try await started()
      let data = try Fixture.data("stamps")
      let task = Task { try await stamps.importLibrary(data) }
      task.cancel()
      do {
        _ = try await task.value
      } catch {
        expectNoDifference((error as? PDFError)?.code, "operation-cancelled")
      }
      _ = try await stamps.libraries()
    }

    @Test func destroyingTheViewerFinishesTheEvents() async throws {
      let (viewer, stamps) = try await started()
      let events = Task { for await _ in stamps.events {} }
      try await Task.sleep(for: .milliseconds(50))
      try await viewer.destroy()
      await events.value
    }
  }
}
```

`stamp` is workspace-scoped, so the standing `aClosedDocumentsCapabilityThrows` doesn't apply (umbrella, "The template"). `placingIntoAClosedDocumentThrows` covers the document-scoped part of the API instead. Every `CancelToken` member also gets the cancellation shape `importCancelsWhenTheTaskIsCancelled` shows; add one test per member as you write it in Step 4, named `<member>CancelsWhenTheTaskIsCancelled`.

- [ ] **Step 3: Run them to verify they fail**

Run: `bash platforms/apple/build-xcframework.sh && (cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17' -only-testing:EmbedPDFTests/BaseSuite/StampTests)`
Expected: FAIL to compile, no `stamps()` on `PDFViewer`.

- [ ] **Step 4: Write `Stamp.swift`**

```swift
public import CoreGraphics
public import EmbedPDFBindings
import Foundation
import ImageIO

public typealias StampConfig = EmbedPDFBindings.StampConfig
public typealias StampLibrary = EmbedPDFBindings.StampLibrary
public typealias StampAsset = EmbedPDFBindings.StampAsset
public typealias StampAssetKind = EmbedPDFBindings.StampAssetKind
public typealias StampMark = EmbedPDFBindings.StampMark
public typealias StampAddAssetInput = EmbedPDFBindings.StampAddAssetInput
public typealias StampImportOptions = EmbedPDFBindings.StampImportOptions
public typealias StampCreateLibraryOptions = EmbedPDFBindings.StampCreateLibraryOptions
public typealias StampLibraryPatch = EmbedPDFBindings.StampLibraryPatch
public typealias StampAssetPatch = EmbedPDFBindings.StampAssetPatch
public typealias StampAssetFilter = EmbedPDFBindings.StampAssetFilter
public typealias StampBatchResult = EmbedPDFBindings.StampBatchResult
public typealias StampBatchFailure = EmbedPDFBindings.StampBatchFailure
public typealias StampErrorInfo = EmbedPDFBindings.StampErrorInfo
public typealias StampLibraryChangeReason = EmbedPDFBindings.StampLibraryChangeReason
public typealias StampEvent = EmbedPDFBindings.StampEvent
public typealias StoredStampLibrary = EmbedPDFBindings.StoredStampLibrary
public typealias StampPersistence = EmbedPDFBindings.StampPersistence

/// A stamp preview: the encoded bytes and, decoded once, the image.
public struct StampPreview: @unchecked Sendable {
  public let data: Data?
  public let mimeType: String?
  /// `nil` when there are no bytes or ImageIO can't read them.
  public let image: CGImage?

  init(_ preview: StampAssetPreview) {
    data = preview.bytes
    mimeType = preview.mimeType
    image = preview.bytes.flatMap { bytes in
      CGImageSourceCreateWithData(bytes as CFData, nil).flatMap {
        CGImageSourceCreateImageAtIndex($0, 0, nil)
      }
    }
  }
}

/// The workspace's stamp libraries. Get it with `PDFViewer.stamps()`; the
/// viewer needs `ViewerConfig(stamp: StampConfig())`, plus
/// `annotation:` to place. It keeps the viewer alive.
public struct StampCapability: Sendable {
  let handle: StampHandle
  let viewer: PDFViewer

  public func libraries(kinds: [String]? = nil) async throws(PDFError) -> [StampLibrary] {
    try await bridged { try await handle.libraries(kinds: kinds) }
  }

  public func library(_ id: String) async throws(PDFError) -> StampLibrary? {
    try await bridged { try await handle.library(id: id) }
  }

  public func assets(_ filter: StampAssetFilter? = nil) async throws(PDFError) -> [StampAsset] {
    try await bridged { try await handle.assets(filter: filter) }
  }

  public func asset(_ id: String) async throws(PDFError) -> StampAsset? {
    try await bridged { try await handle.asset(id: id) }
  }

  public func assetPreview(_ id: String) async throws(PDFError) -> StampPreview? {
    try await bridged { try await handle.assetPreview(id: id) }.map(StampPreview.init)
  }

  public func renderAssetPreview(_ id: String, width: Double) async throws(PDFError) -> StampPreview? {
    try await bridgedCancellable { token in
      try await handle.renderAssetPreview(id: id, width: width, cancel: token)
    }.map(StampPreview.init)
  }

  public func assetBytes(_ id: String) async throws(PDFError) -> Data? {
    try await bridged { try await handle.readAssetBytes(id: id) }
  }

  public func exportLibrary(_ id: String) async throws(PDFError) -> Data {
    try await bridgedCancellable { token in try await handle.exportLibrary(id: id, cancel: token) }
  }

  public func canPlace(in document: PDFDocument? = nil) async throws(PDFError) -> Bool {
    try await bridged { try await handle.canPlace(documentId: document?.id) }
  }

  public func canImport() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canImport() }
  }

  public func createLibrary(_ name: String, options: StampCreateLibraryOptions = StampCreateLibraryOptions())
    async throws(PDFError) -> String
  {
    try await bridgedCancellable { token in
      try await handle.createLibrary(name: name, options: options, cancel: token)
    }
  }

  public func updateLibrary(_ id: String, _ patch: StampLibraryPatch) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.updateLibrary(id: id, patch: patch, cancel: token) }
  }

  /// Returns the library's id. Throws `InvalidArg` for bytes that aren't a PDF.
  public func importLibrary(_ data: Data, options: StampImportOptions = StampImportOptions())
    async throws(PDFError) -> String
  {
    try await bridgedCancellable { token in
      try await handle.importLibrary(bytes: data, options: options, cancel: token)
    }
  }

  public func deleteLibrary(_ id: String) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.deleteLibrary(id: id, cancel: token) }
  }

  public func createAsset(_ input: StampAddAssetInput) async throws(PDFError) -> String {
    try await bridgedCancellable { token in try await handle.createAsset(input: input, cancel: token) }
  }

  /// Throws `unsupported` on the native engine today.
  public func createAsset(
    from annotations: [AnnotationRef], in document: PDFDocument, on page: PageRef,
    input: StampAddAssetInput = StampAddAssetInput()
  ) async throws(PDFError) -> String {
    try await bridgedCancellable { token in
      try await handle.createAssetFromAnnotations(
        documentId: document.id, page: page, annotations: annotations, input: input, cancel: token)
    }
  }

  public func updateAsset(_ id: String, _ patch: StampAssetPatch) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.updateAsset(id: id, patch: patch, cancel: token) }
  }

  public func deleteAsset(_ id: String) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.deleteAsset(id: id, cancel: token) }
  }

  public func moveAsset(_ id: String, to libraryId: String) async throws(PDFError) -> String {
    try await bridgedCancellable { token in
      try await handle.moveAsset(id: id, toLibraryId: libraryId, cancel: token)
    }
  }

  public func duplicateAsset(_ id: String, label: String? = nil) async throws(PDFError) -> String {
    try await bridgedCancellable { token in try await handle.duplicateAsset(id: id, label: label, cancel: token) }
  }

  public func arm(_ assetId: String, in document: PDFDocument, targetWidth: Double? = nil) async throws(PDFError) {
    try await bridgedCancellable { token in
      try await handle.armAsset(documentId: document.id, assetId: assetId, targetWidth: targetWidth, cancel: token)
    }
  }

  public func place(
    _ assetId: String, in document: PDFDocument, on page: PageRef, at point: Point,
    targetWidth: Double? = nil, rotation: Double? = nil
  ) async throws(PDFError) -> AnnotationRef {
    try await bridgedCancellable { token in
      try await handle.placeAsset(
        documentId: document.id, assetId: assetId, page: page, at: point, targetWidth: targetWidth,
        rotation: rotation, cancel: token)
    }
  }

  public func place(
    _ assetId: String, in document: PDFDocument, onPages pages: [PageRef]?, at point: Point,
    targetWidth: Double? = nil, rotation: Double? = nil
  ) async throws(PDFError) -> StampBatchResult {
    try await bridgedCancellable { token in
      try await handle.placeAssetOnPages(
        documentId: document.id, assetId: assetId, pages: pages, at: point, targetWidth: targetWidth,
        rotation: rotation, cancel: token)
    }
  }

  public func disarm(in document: PDFDocument) async throws(PDFError) {
    try await bridged { try await handle.disarm(documentId: document.id) }
  }

  public func armedAsset(in document: PDFDocument) async throws(PDFError) -> StampAsset? {
    try await bridged { try await handle.armedAsset(documentId: document.id) }
  }

  /// Loads every library `store` holds and returns their ids.
  public func restore(from store: some StampLibraryStore) async throws(PDFError) -> [String] {
    try await bridgedCancellable { token in try await handle.restore(store: store, cancel: token) }
  }

  /// Writes changed libraries to `store` until `stop()`.
  public func persist(to store: some StampLibraryStore, except: [String] = [], debounce: Duration? = nil)
    -> StampPersistence
  {
    let ms = debounce.map { Double($0.components.seconds) * 1000 + Double($0.components.attoseconds) / 1e15 }
    return handle.persist(store: store, except: except, debounceMs: ms)
  }

  /// Workspace events. Finishes when the viewer is destroyed.
  public var events: AsyncStream<StampEvent> {
    viewer.registry.stream { handle.subscribe(listener: $0) }
  }
}

extension EventRelay: StampEventListener where Event == StampEvent {
  func onEvent(event: StampEvent) {
    continuation.yield(event)
  }
}

/// A `StampLibraryStore` over one file per library in `directory`.
public final class FileStampLibraryStore: StampLibraryStore, Sendable {
  public let directory: URL

  public init(directory: URL) throws {
    self.directory = directory
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
  }

  private func url(_ id: String) -> URL {
    let safe = id.addingPercentEncoding(withAllowedCharacters: .alphanumerics) ?? id
    return directory.appending(path: "\(safe).pdf")
  }

  public func list() throws(ViewerError) -> [StoredStampLibrary] {
    do {
      return try FileManager.default.contentsOfDirectory(at: directory, includingPropertiesForKeys: nil)
        .filter { $0.pathExtension == "pdf" }
        .sorted { $0.lastPathComponent < $1.lastPathComponent }
        .map { url in
          let id = url.deletingPathExtension().lastPathComponent.removingPercentEncoding ?? ""
          return StoredStampLibrary(id: id, bytes: try Data(contentsOf: url))
        }
    } catch {
      throw .Kernel(name: "StoreError", code: "io", detail: "\(error)")
    }
  }

  public func put(id: String, bytes: Data) throws(ViewerError) {
    do { try bytes.write(to: url(id), options: .atomic) } catch {
      throw .Kernel(name: "StoreError", code: "io", detail: "\(error)")
    }
  }

  public func delete(id: String) throws(ViewerError) {
    do { try FileManager.default.removeItem(at: url(id)) } catch CocoaError.fileNoSuchFile {
    } catch {
      throw .Kernel(name: "StoreError", code: "io", detail: "\(error)")
    }
  }
}

extension PDFViewer {
  public func stamps() async throws(PDFError) -> StampCapability {
    let handle = try await bridged { try await host.stamp() }
    return StampCapability(handle: handle, viewer: self)
  }
}
```

`ViewerError` and `StampLibraryStore` come from `EmbedPDFBindings`; the generated protocol's method signatures decide the exact `throws` spelling (UniFFI 0.32 generates `throws` for a fallible foreign method), so match what `Sources/EmbedPDFBindings/epdf_plugin_stamp.swift` declares. If `ViewerError` isn't public through `EmbedPDF`, add `public typealias ViewerError = EmbedPDFBindings.ViewerError` here; it's needed by anyone implementing their own store. If generated code calls the protocol from a non-main thread under Swift 6 strict concurrency and the class's `Sendable` conformance is refused, keep `FileStampLibraryStore` `final` with only `let` properties, which is what makes the conformance hold.

- [ ] **Step 5: Run the Swift tests**

Run: `bash platforms/apple/build-xcframework.sh && (cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17')`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add platforms/apple
git commit -m "Add StampCapability and a file-backed library store to the Swift package"
```

### Task 7: CI, docs, merge and clean up

**Files:**
- Modify: `.github/workflows/plugin-rust.yml`, `platforms/apple/README.md`, `docs/superpowers/plans/2026-10-01-swift-package.md`

- [ ] **Step 1:** Append `stamp` to `crates=( ... )` in `plugin-rust.yml`, in plan-number order.

- [ ] **Step 2:** Add the README row:

```markdown
| stamp | `StampCapability` | `stamp: StampConfig()` (with `annotation` to place) | `2026-10-06-swift-plugins/09-stamp.md` |
```

- [ ] **Step 3:** In `2026-10-01-swift-package.md` §A.3, mark `**stamp** (W), delivered by 09-stamp.md`, and note there that `create_asset_from_annotations` is `unsupported` natively until `annotations.exportAppearance` exists in `epdf-engine`.

- [ ] **Step 4: Run the definition of done**

```bash
cargo test -p epdf-plugin-stamp --features uniffi
cargo test -p epdf-ffi -p epdf-viewer-host
cargo clippy -p epdf-plugin-stamp -p epdf-viewer-host --all-targets --features epdf-plugin-stamp/uniffi -- -D warnings
cargo test -p epdf-plugin-stamp
cargo test -p pdfium -p epdf-engine
bash platforms/apple/build-xcframework.sh
(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17')
cargo build -p epdf-viewer-host -p uniffi-bindgen
./target/debug/uniffi-bindgen generate --library target/debug/libepdf_viewer_host.dylib --language kotlin --no-format --out-dir "$TMPDIR/epdf-kotlin-check"
```

Expected: every command exits 0.

- [ ] **Step 5: Commit, with the size line**

```bash
ls -l target/aarch64-apple-ios-sim/release/libepdf_viewer_host.a
git add .github/workflows/plugin-rust.yml platforms/apple/README.md docs/superpowers/plans/2026-10-01-swift-package.md
git commit -m "Test the stamp exports in CI and list them in the README

libepdf_viewer_host.a (ios-sim, release): <before> -> <after> bytes"
```

- [ ] **Step 6: Merge, rerun, clean up**

From the integration worktree:

```bash
git merge --no-ff research/swift-stamp
bash platforms/apple/build-xcframework.sh
(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17')
cargo build -p epdf-viewer-host -p uniffi-bindgen
./target/debug/uniffi-bindgen generate --library target/debug/libepdf_viewer_host.dylib --language kotlin --no-format --out-dir "$TMPDIR/epdf-kotlin-check"
git worktree remove .claude/worktrees/swift-stamp
git branch -d research/swift-stamp
git worktree prune
```

Expected: the rerun passes and both removals succeed without `--force`. Tick 09 in `00-umbrella.md`.
