# Page Edit Swift Wrapper Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Export `epdf-plugin-page-edit` through UniFFI as `PageEditHandle`, reach it from `ViewerHost::page_edit(document_id)`, and wrap it in Swift as `PageEditCapability`, with the six `pages.*` engine paths mapped so every edit runs against real PDFium.

**Architecture:** The recipe in `2026-10-01-swift-package.md` Appendix A §A.0, with two departures. The plugin answers every verb with an engine `Value`, so every record in `ffi.rs` is new and modelled on `packages/plugin/page-edit/src/contract.ts` and `packages/engine/core/src/mutation/Page*Result.ts`. And the native engine needs real work before an edit can land: `translate.rs` maps the six paths, a new `Decode::PageMutation` gives each layout page the `ref` the kernel's registry reads, extracted PDFs travel through the `HostTable` as `$hostBytes`, and `NativeDocument` stops answering `page_index` from the page list it loaded at open. `PageInfo` moves from `epdf-viewer-host` to `epdf-ffi` so the plugin can return it.

**Tech Stack:** Rust 2024, `uniffi =0.32.0`, Swift 6.2 tools, Swift Testing, `swift-custom-dump` 1.3, Xcode 27.0, iPhone 17 simulator (iOS 26.0).

**Spec:** `docs/superpowers/plans/2026-10-06-swift-plugins/00-umbrella.md` and the plan it argues from, `docs/superpowers/plans/2026-10-01-swift-package.md` (Global Constraints, Decisions, Appendix A §A.0 and §A.4, Appendix C). The plugin's own contract is `packages/plugin/page-edit/src/contract.ts`.

## Global Constraints

Every constraint in the umbrella's Global Constraints applies. The ones this plan meets head on:

- `uniffi` is pinned at `=0.32.0`.
- UniFFI names are unique across the workspace. Every record here is prefixed `PageEdit`, except `PageInfo`, which umbrella Task 0d moves to `epdf-ffi` because page edit and stage return it too.
- No `usize`, `isize`, `Rc` or `serde_json::Value` across the boundary. JavaScript numbers (`f64` indexes and counts in the plugin) cross as `u32`.
- Kotlin and Swift keywords aren't method names: `move` is `move_pages`, `delete` is `delete_pages`.
- Facades don't decide PDF semantics. The engine refuses deleting every page; Swift doesn't pre-check it.
- Swift value assertions use `expectNoDifference`.
- One shared-file block per plugin, in plan-number order (umbrella "Shared files").

## Review Focus

1. A rotation, index or count that reaches the host as a float. `js_number(90.0)` is a JSON float, and the host parses `rotation` as `u16`, `destIndex` and `count` as `u32` (`crates/epdf-engine/src/host.rs:807-905`), which serde refuses for `90.0`. People expect the edit to land. Pinned in Task 3 (`a_rotation_travels_as_a_whole_number`, `a_fractional_index_is_invalid_input`).
2. A second edit that reads what the first one left. `rotate_by` reads each page's rotation from the kernel's registry, and the registry takes the `layout` from the mutation's event (`crates/epdf-kernel/src/kernel.rs:323-327`), whose pages the host sends without `ref`. People expect two quarter turns to make a half turn. Pinned in Task 3 (`a_page_mutation_gives_every_layout_page_its_ref`) and Task 4 (`rotating_twice_reads_the_rotation_the_first_edit_left`).
3. A page inserted, then read. `NativeDocument::page_index` answers from the page list loaded at open (`crates/epdf-viewer-host/src/native_engine.rs:243-252`), so a new page is `NotFound` to every page call. People expect to render the page they just added. Pinned in Task 3 (`an_inserted_page_is_known_to_the_engine_handle`) and Task 4 (`an_inserted_blank_page_renders`).
4. Deleting every page. People expect a typed error and the document unchanged, not an empty document. Pinned in Task 5 (`deletingEveryPageThrowsAndKeepsThePages`).
5. Extracted bytes that nobody reads back. `extract` and `duplicate` park a PDF in the `HostTable` as `$hostBytes`. People expect the table to be empty again once the call returns. Pinned in Task 4 (`extracting_and_duplicating_leave_nothing_in_the_host_table`).

## Cross-plugin tests

Page edit has no optional plugin. Two integrations still matter:

- render: a page inserted by page edit renders through `RenderHandle`. Render is on the base branch, so this plan owns the test (`an_inserted_blank_page_renders`, Task 4).
- signature (plan 16): `can_edit` answers `false` once `NativeSecurity` enforces `doc.pages.assemble`. Until then it's always `true`. Plan 16 owns that test and adds it to `PageEditTests.swift`.

## File Structure

```
crates/plugins/page-edit/
  Cargo.toml                         + uniffi feature, epdf-ffi, futures dev-dep
  src/lib.rs                         + setup_scaffolding, pub mod ffi
  src/controller.rs                  + lifetime() under cfg(feature = "uniffi")
  src/ffi.rs                         new: records, PageEditHandle
  tests/ffi.rs                       new
crates/epdf-viewer-host/
  Cargo.toml                         + epdf-plugin-page-edit
  src/api.rs                         + ViewerConfig.page_edit
  src/host_table.rs                  + install_bytes_resolver
  src/translate.rs                   + six pages.* routes, Decode::PageMutation, Decode::HostBytes
  src/native_engine.rs               page_index from the live layout
  src/session.rs                     + page_edit branch, resolver install
  src/lib.rs                         + page_edit accessor
  tests/translate.rs                 + page routes and decodes, native engine page edits
  tests/viewer_host.rs               + page edit through the host
platforms/apple/EmbedPDF/
  Sources/EmbedPDF/Plugins/PageEdit.swift   new
  Tests/EmbedPDFTests/PageEditTests.swift   new
.github/workflows/plugin-rust.yml    + page-edit
platforms/apple/README.md            + plugin row
docs/superpowers/plans/2026-10-01-swift-package.md   A.4 page-edit marked delivered
```

---

### Task 1: Worktree

**Interfaces:**
- Consumes: umbrella Task 0d's `epdf_ffi::PageInfo { page: PageRef, index: u32, width: f64, height: f64, rotation: u32 }` and `epdf_ffi::page::{decode_page, decode_pages}`; Task 0e's `translate::{whole, optional_whole}`, the `$undefined` filter in `translate`, `PATHS`, and `NativeEngine::open` taking `{"bytes": <Bytes JSON>}`.

- [x] **Step 1: Create the worktree**

From the repository root, once `research/swift-plugins` exists (umbrella Task 0):

```bash
git worktree add .claude/worktrees/swift-page-edit -b research/swift-page-edit research/swift-plugins
cd .claude/worktrees/swift-page-edit
```

Fetch PDFium for `darwin-arm64`, `ios-arm64` and `ios-sim-arm64` as `platforms/apple/README.md` describes. Every later command runs in this worktree. Record the simulator archive size before any change, for the final commit message:

```bash
bash platforms/apple/build-xcframework.sh
stat -f %z target/aarch64-apple-ios-sim/release/libepdf_viewer_host.a > "$TMPDIR/page-edit-size-before"
```

- [x] **Step 2: Confirm umbrella Task 0 landed**

Run: `grep -n 'pub use page::PageInfo' crates/epdf-ffi/src/lib.rs && grep -n 'pub fn optional_whole' crates/epdf-viewer-host/src/translate.rs`
Expected: one hit each.

### Task 2: `ffi.rs` for page edit

**Files:**
- Create: `crates/plugins/page-edit/src/ffi.rs`, `crates/plugins/page-edit/tests/ffi.rs`
- Modify: `crates/plugins/page-edit/Cargo.toml`, `crates/plugins/page-edit/src/lib.rs`, `crates/plugins/page-edit/src/controller.rs`

**Interfaces:**
- Consumes: `epdf_ffi::{PageInfo, PageRef, CancelToken, KernelBound, KernelPost, ViewerError}`, `epdf_ffi::page::decode_pages` (umbrella Task 0d).
- Produces, all in `epdf_plugin_page_edit::ffi`:
  - `PageEditPlacement` enum: `After { page: PageRef }`, `Before { page: PageRef }`, `Index { index: u32 }`, `End`.
  - `PageEditTurn` enum: `Clockwise`, `CounterClockwise`, `Half` (the TypeScript's `90 | -90 | 180`).
  - `PageEditRotation` enum: `None`, `Clockwise90`, `Clockwise180`, `Clockwise270`.
  - `PageEditBlankOptions { count: Option<u32>, size: Option<Size>, placement: Option<PageEditPlacement> }`, all defaulting to `None`.
  - `PageEditBytesOptions { page_indexes: Option<Vec<u32>>, placement: Option<PageEditPlacement> }`, defaulting to `None`.
  - `PageEditLayout { pages: Vec<PageInfo> }`: what move, rotate and delete answer.
  - `PageEditInsertion { inserted: Vec<PageRef>, pages: Vec<PageInfo> }`: what the four inserts answer.
  - `PageEditHandle::new(cap: PageEditCapability, post: KernelPost) -> Arc<Self>`, and the async methods `can_edit`, `rotate_by`, `set_rotation`, `move_pages`, `delete_pages`, `insert_blank`, `insert_from_bytes`, `insert_from_document`, `duplicate`, `extract`, each taking `cancel: Arc<CancelToken>` except `can_edit`.
  - No event enum and no `subscribe`. The TypeScript contract says it outright: "Page-registry events are on `documents.onPagesChanged`; this plugin adds none." Swift reads them from `PDFViewer.documentEvents` (`.pagesChanged`).

The member list comes from `crates/plugins/page-edit/src/harness.rs` `MEMBERS`: `canEdit`, `rotateBy`, `setRotation`, `move`, `delete`, `insertBlank`, `insertFromBytes`, `insertFromDocument`, `duplicate`, `extract`. It matches `impl PageEditCapability` one to one. The plugin's results keep the engine's `cache` field (always `null` for a local engine); the records drop it.

Cancelling follows `MetadataHandle::refresh`: the plugin's promises take no abort, so a token cancelled before the job runs answers `operation-cancelled` and skips the edit, and a token cancelled later is ignored and the edit lands. Each method's doc comment says so.

- [x] **Step 1: Turn on the feature**

In `crates/plugins/page-edit/Cargo.toml`:

```toml
[features]
default = ["harness"]
harness = ["dep:plugin-conformance"]
# The uniffi exports (`ffi` module), for `epdf-viewer-host`.
uniffi = ["dep:uniffi", "dep:epdf-ffi"]

[dependencies]
# existing lines stay; add:
epdf-ffi = { path = "../../epdf-ffi", optional = true }
uniffi = { version = "=0.32.0", optional = true }

[dev-dependencies]
futures = { version = "0.3", default-features = false, features = ["std", "executor"] }
plugin-conformance = { path = "../../plugin-conformance" }
```

In `crates/plugins/page-edit/src/lib.rs`, after the `harness` module:

```rust
#[cfg(feature = "uniffi")]
pub mod ffi;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();
```

In `crates/plugins/page-edit/src/controller.rs`, inside `impl PageEditCapability`:

```rust
    /// Aborted when the document closes. The foreign handle, which can
    /// outlive the close, checks it before every call.
    #[cfg(feature = "uniffi")]
    pub(crate) fn lifetime(&self) -> &epdf_kernel::signal::AbortSignal {
        self.0.cx.lifetime()
    }
```

- [x] **Step 2: Write the failing tests**

Create `crates/plugins/page-edit/tests/ffi.rs`:

```rust
#![cfg(feature = "uniffi")]

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use epdf_ffi::{CancelToken, KernelThread, PageRef, Size, ViewerError};
use epdf_plugin_page_edit::ffi::{
    PageEditBlankOptions, PageEditHandle, PageEditPlacement, PageEditRotation, PageEditTurn,
};
use epdf_plugin_page_edit::{PAGE_EDIT, page_edit_plugin};
use futures::executor::block_on;
use plugin_conformance::test_kernel::{DOCUMENT, Reply, TestKernel, TestSpec};
use serde_json::{Value, json};

fn page(pon: u32, index: u32, rotation: u32) -> Value {
    json!({
        "ref": { "kind": "objectNumber", "pageObjectNumber": pon },
        "pageObjectNumber": pon, "index": index, "rotation": rotation,
        "size": { "width": 612.0, "height": 792.0 }
    })
}

/// Three pages, 10, 11 and 12. `pages.rotate` and `pages.delete` apply to
/// a shared layout, publish the event a real engine publishes, and answer
/// `{layout}`. `pages.insertBlank` appends page 20.
fn spec(allows: Value) -> TestSpec {
    let layout = Rc::new(RefCell::new(vec![page(10, 0, 0), page(11, 1, 0), page(12, 2, 0)]));
    let answer = |kind: &str, pages: &[Value], inserted: Option<Vec<u32>>| {
        let layout = json!({ "pageCount": pages.len(), "pages": pages });
        let mut value = json!({ "layout": layout.clone(), "cache": null });
        if let Some(inserted) = inserted {
            value["insertedPages"] = inserted
                .iter()
                .map(|p| json!({ "kind": "objectNumber", "pageObjectNumber": p }))
                .collect();
        }
        Reply { value, events: vec![json!({ "type": kind, "layout": layout })] }
    };
    let pons = |args: &[Value]| -> Vec<u64> {
        args[0]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["pageObjectNumber"].as_u64().unwrap())
            .collect()
    };
    let (rotated, deleted, inserted) = (layout.clone(), layout.clone(), layout);
    TestSpec::new(json!({
        "documents": { DOCUMENT: {
            "id": DOCUMENT, "security": { "allows": allows },
            "pages": [page(10, 0, 0), page(11, 1, 0), page(12, 2, 0)]
        } }
    }))
    .respond("pages.rotate", move |args| {
        let targets = pons(args);
        let rotation = args[1].as_f64().unwrap() as u32;
        let mut pages = rotated.borrow_mut();
        for p in pages.iter_mut() {
            if targets.contains(&p["pageObjectNumber"].as_u64().unwrap()) {
                p["rotation"] = json!(rotation);
            }
        }
        Ok(answer("pages.rotated", &pages, None))
    })
    .respond("pages.delete", move |args| {
        let targets = pons(args);
        let mut pages = deleted.borrow_mut();
        if targets.len() >= pages.len() {
            return Err(epdf_kernel::Error::engine("InvalidArg", "cannot delete every page"));
        }
        pages.retain(|p| !targets.contains(&p["pageObjectNumber"].as_u64().unwrap()));
        for (i, p) in pages.iter_mut().enumerate() {
            p["index"] = json!(i);
        }
        Ok(answer("pages.deleted", &pages, None))
    })
    .respond("pages.insertBlank", move |_| {
        let mut pages = inserted.borrow_mut();
        let index = pages.len() as u32;
        pages.push(page(20, index, 0));
        Ok(answer("pages.inserted", &pages, Some(vec![20])))
    })
    .respond("pages.extract", |_| Ok(Reply::value(json!({ "$bytes": "JVBERi0=" }))))
}

fn handle_over(spec: impl FnOnce() -> TestSpec + Send + 'static) -> (KernelThread<TestKernel>, Arc<PageEditHandle>) {
    let thread = KernelThread::spawn(|_| TestKernel::open(vec![page_edit_plugin()], spec())).unwrap();
    let post = thread.post();
    let handle = block_on(
        thread.call(move |k| PageEditHandle::new(k.capability(&PAGE_EDIT).unwrap(), post)),
    )
    .unwrap();
    (thread, handle)
}

fn handle() -> (KernelThread<TestKernel>, Arc<PageEditHandle>) {
    handle_over(|| spec(json!("*")))
}

fn code(error: &ViewerError) -> Option<&str> {
    match error {
        ViewerError::Kernel { code, .. } => code.as_deref(),
        ViewerError::Closed => None,
    }
}

fn token() -> Arc<CancelToken> {
    CancelToken::new()
}

const P10: PageRef = PageRef { page_object_number: 10 };
const P11: PageRef = PageRef { page_object_number: 11 };

#[test]
fn can_edit_follows_the_assemble_grant() {
    let (_thread, edit) = handle();
    assert!(block_on(edit.can_edit()).unwrap());
    let (_thread, denied) = handle_over(|| spec(json!(["doc.text.search"])));
    assert!(!block_on(denied.can_edit()).unwrap());
}

#[test]
fn set_rotation_answers_the_new_layout() {
    let (_thread, edit) = handle();
    let layout =
        block_on(edit.set_rotation(vec![P10], PageEditRotation::Clockwise90, token())).unwrap();
    assert_eq!(layout.pages.len(), 3);
    assert_eq!(layout.pages[0].page, P10);
    assert_eq!(layout.pages[0].rotation, 90);
    assert_eq!(layout.pages[1].rotation, 0);
}

#[test]
fn rotate_by_turns_from_the_rotation_the_last_edit_left() {
    let (_thread, edit) = handle();
    block_on(edit.rotate_by(vec![P10], PageEditTurn::Clockwise, token())).unwrap();
    let layout = block_on(edit.rotate_by(vec![P10], PageEditTurn::Clockwise, token())).unwrap();
    assert_eq!(layout.pages[0].rotation, 180);
    let layout =
        block_on(edit.rotate_by(vec![P10], PageEditTurn::CounterClockwise, token())).unwrap();
    assert_eq!(layout.pages[0].rotation, 90);
}

#[test]
fn rotate_by_with_no_pages_is_invalid_input() {
    let (_thread, edit) = handle();
    let error = block_on(edit.rotate_by(vec![], PageEditTurn::Half, token())).unwrap_err();
    assert_eq!(code(&error), Some("invalid-input"), "{error:?}");
}

#[test]
fn delete_pages_answers_the_survivors() {
    let (_thread, edit) = handle();
    let layout = block_on(edit.delete_pages(vec![P11], token())).unwrap();
    let pons: Vec<u32> = layout.pages.iter().map(|p| p.page.page_object_number).collect();
    assert_eq!(pons, [10, 12]);
}

#[test]
fn insert_blank_answers_the_inserted_pages_and_the_layout() {
    let (_thread, edit) = handle();
    let options = PageEditBlankOptions {
        count: Some(1),
        size: Some(Size { width: 300.0, height: 400.0 }),
        placement: Some(PageEditPlacement::End),
    };
    let insertion = block_on(edit.insert_blank(options, token())).unwrap();
    assert_eq!(insertion.inserted, [PageRef { page_object_number: 20 }]);
    assert_eq!(insertion.pages.len(), 4);
}

#[test]
fn a_placement_after_an_unknown_page_is_not_found() {
    let (_thread, edit) = handle();
    let options = PageEditBlankOptions {
        placement: Some(PageEditPlacement::After { page: PageRef { page_object_number: 99 } }),
        ..Default::default()
    };
    let error = block_on(edit.insert_blank(options, token())).unwrap_err();
    assert_eq!(code(&error), Some("not-found"), "{error:?}");
}

#[test]
fn extract_answers_the_pdf_bytes() {
    let (_thread, edit) = handle();
    assert_eq!(block_on(edit.extract(vec![P10], token())).unwrap(), b"%PDF-".to_vec());
}

#[test]
fn a_layout_the_record_cannot_hold_is_invalid_state() {
    let (_thread, edit) = handle_over(|| {
        spec(json!("*")).respond("pages.move", |_| Ok(Reply::value(json!({ "layout": {} }))))
    });
    let error =
        block_on(edit.move_pages(vec![P10], PageEditPlacement::End, token())).unwrap_err();
    assert_eq!(code(&error), Some("invalid-state"), "{error:?}");
}

#[test]
fn a_token_cancelled_before_the_job_runs_skips_the_edit() {
    let (thread, edit) = handle();
    let (release, gate) = std::sync::mpsc::channel::<()>();
    let (parked_tx, parked) = std::sync::mpsc::channel::<()>();
    let blocker = thread.call(move |_| {
        parked_tx.send(()).unwrap();
        gate.recv().unwrap();
    });
    parked.recv().unwrap();
    let cancel = token();
    let rotate = edit.set_rotation(vec![P10], PageEditRotation::Clockwise180, cancel.clone());
    cancel.cancel();
    release.send(()).unwrap();
    block_on(blocker).unwrap();
    let error = block_on(rotate).unwrap_err();
    assert_eq!(code(&error), Some("operation-cancelled"), "{error:?}");
    let layout =
        block_on(edit.set_rotation(vec![P11], PageEditRotation::None, token())).unwrap();
    assert_eq!(layout.pages[0].rotation, 0, "the cancelled rotation never ran");
}

#[test]
fn a_closed_documents_capability_refuses_with_instance_closed() {
    let (thread, edit) = handle();
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    for error in [
        block_on(edit.can_edit()).map(|_| ()).unwrap_err(),
        block_on(edit.delete_pages(vec![P10], token())).map(|_| ()).unwrap_err(),
        block_on(edit.extract(vec![P10], token())).map(|_| ()).unwrap_err(),
    ] {
        assert_eq!(code(&error), Some("instance-closed"), "{error:?}");
    }
}
```

- [x] **Step 3: Run them to see them fail**

Run: `cargo test -p epdf-plugin-page-edit --features uniffi --test ffi`
Expected: FAIL to compile, `could not find ffi in epdf_plugin_page_edit`.

- [x] **Step 4: Write `ffi.rs`**

Create `crates/plugins/page-edit/src/ffi.rs`:

```rust
//! The page-edit plugin through uniffi. The plugin answers every verb with
//! the engine's JSON (`PageMoveResult`, `PageInsertResult`, ...), so every
//! record here is new, modelled on `packages/plugin/page-edit`'s contract
//! and `packages/engine/core/src/mutation/Page*Result.ts`. A result that
//! does not decode is an `invalid-state` error.
//!
//! The plugin's promises take no abort. A token cancelled before the job
//! runs skips the edit with `operation-cancelled`; once the job has run,
//! the edit is on its way to the engine and lands.

use std::sync::Arc;

use epdf_ffi::page::decode_pages;
use epdf_ffi::{CancelToken, KernelBound, KernelPost, PageInfo, PageRef, Size, ViewerError};
use epdf_kernel::Error;
use epdf_kernel::bytes::Bytes;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::geometry::PageRef as KernelPageRef;
use serde_json::Value;

use crate::contract::{InsertBlankOptions, InsertFromBytesOptions, PagePlacement, PdfSize};
use crate::controller::PageEditCapability;

/// `PagePlacement`: where pages land. The ref anchors stay correct across a
/// concurrent reorder; `Index` is an absolute position and `End` appends.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Enum)]
pub enum PageEditPlacement {
    After { page: PageRef },
    Before { page: PageRef },
    Index { index: u32 },
    End,
}

impl From<PageEditPlacement> for PagePlacement {
    fn from(placement: PageEditPlacement) -> Self {
        match placement {
            PageEditPlacement::After { page } => PagePlacement::After(page.into()),
            PageEditPlacement::Before { page } => PagePlacement::Before(page.into()),
            PageEditPlacement::Index { index } => PagePlacement::Index(f64::from(index)),
            PageEditPlacement::End => PagePlacement::End,
        }
    }
}

/// `rotateBy`'s delta: a quarter turn either way, or a half turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum PageEditTurn {
    Clockwise,
    CounterClockwise,
    Half,
}

impl PageEditTurn {
    fn degrees(self) -> f64 {
        match self {
            PageEditTurn::Clockwise => 90.0,
            PageEditTurn::CounterClockwise => -90.0,
            PageEditTurn::Half => 180.0,
        }
    }
}

/// `PageRotation`: an absolute `/Rotate`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum PageEditRotation {
    None,
    Clockwise90,
    Clockwise180,
    Clockwise270,
}

impl PageEditRotation {
    fn degrees(self) -> f64 {
        match self {
            PageEditRotation::None => 0.0,
            PageEditRotation::Clockwise90 => 90.0,
            PageEditRotation::Clockwise180 => 180.0,
            PageEditRotation::Clockwise270 => 270.0,
        }
    }
}

/// `insertBlank(options)`. `size` defaults to the page the new pages sit
/// beside, `count` to 1, `placement` to `End`.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct PageEditBlankOptions {
    #[uniffi(default = None)]
    pub count: Option<u32>,
    #[uniffi(default = None)]
    pub size: Option<Size>,
    #[uniffi(default = None)]
    pub placement: Option<PageEditPlacement>,
}

impl From<PageEditBlankOptions> for InsertBlankOptions {
    fn from(options: PageEditBlankOptions) -> Self {
        InsertBlankOptions {
            size: options.size.map(|s| PdfSize { width: s.width, height: s.height }),
            count: options.count.map(f64::from),
            placement: options.placement.map(Into::into),
        }
    }
}

/// `insertFromBytes(bytes, options)`: a subset of the PDF's pages by index,
/// every page when `page_indexes` is `None`.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct PageEditBytesOptions {
    #[uniffi(default = None)]
    pub page_indexes: Option<Vec<u32>>,
    #[uniffi(default = None)]
    pub placement: Option<PageEditPlacement>,
}

impl From<PageEditBytesOptions> for InsertFromBytesOptions {
    fn from(options: PageEditBytesOptions) -> Self {
        InsertFromBytesOptions {
            page_indexes: options.page_indexes.map(|i| i.into_iter().map(f64::from).collect()),
            placement: options.placement.map(Into::into),
        }
    }
}

/// `PageMoveResult`, `PageRotateResult` and `PageDeleteResult`: the new
/// layout, every page in display order. The engine's `cache` is `null` for
/// a local engine and is dropped.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PageEditLayout {
    pub pages: Vec<PageInfo>,
}

/// `PageInsertResult`: the new pages in insertion order, and the new layout.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PageEditInsertion {
    pub inserted: Vec<PageRef>,
    pub pages: Vec<PageInfo>,
}

fn invalid_state(what: String) -> Error {
    Error::named("InvalidState", format!("[page-edit] {what}")).with("code", "invalid-state")
}

fn layout(result: &Value) -> Result<Vec<PageInfo>, Error> {
    let pages = result
        .get("layout")
        .and_then(|l| l.get("pages"))
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_state(format!("the engine answered no layout: {result}")))?;
    decode_pages(pages)
}

fn decode_layout(result: Value) -> Result<PageEditLayout, ViewerError> {
    Ok(PageEditLayout { pages: layout(&result)? })
}

fn decode_insertion(result: Value) -> Result<PageEditInsertion, ViewerError> {
    let inserted = result
        .get("insertedPages")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_state(format!("an insert answered no insertedPages: {result}")))?
        .iter()
        .map(|r| {
            KernelPageRef::from_json(r)
                .map(PageRef::from)
                .ok_or_else(|| invalid_state(format!("an inserted page that is not a ref: {r}")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PageEditInsertion { inserted, pages: layout(&result)? })
}

fn refs(pages: Vec<PageRef>) -> Vec<KernelPageRef> {
    pages.into_iter().map(Into::into).collect()
}

/// The capability, if its document is still open.
fn open(cap: &PageEditCapability) -> Result<&PageEditCapability, Error> {
    if cap.lifetime().aborted() {
        return Err(plugin_error(
            code::INSTANCE_CLOSED,
            "page-edit",
            "the document this capability belongs to is closed",
            None,
            None,
        ));
    }
    Ok(cap)
}

#[derive(uniffi::Object)]
pub struct PageEditHandle {
    cap: KernelBound<PageEditCapability>,
}

impl PageEditHandle {
    /// On the kernel thread.
    pub fn new(cap: PageEditCapability, post: KernelPost) -> Arc<Self> {
        Arc::new(PageEditHandle { cap: KernelBound::new(cap, post) })
    }
}

#[uniffi::export]
impl PageEditHandle {
    /// Whether this caller may make structural page edits
    /// (`doc.pages.assemble`). Every verb shares that one grant.
    pub async fn can_edit(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| open(c)?.can_edit()).await
    }

    /// Turn each page from its own rotation. Cancelling skips an edit that
    /// has not started.
    pub async fn rotate_by(
        &self,
        pages: Vec<PageRef>,
        turn: PageEditTurn,
        cancel: Arc<CancelToken>,
    ) -> Result<PageEditLayout, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.rotate_by(refs(pages), turn.degrees())),
                decode_layout,
            )
            .await?
    }

    /// One absolute rotation for every page given.
    pub async fn set_rotation(
        &self,
        pages: Vec<PageRef>,
        rotation: PageEditRotation,
        cancel: Arc<CancelToken>,
    ) -> Result<PageEditLayout, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.set_rotation(refs(pages), rotation.degrees())),
                decode_layout,
            )
            .await?
    }

    /// Reorder pages as a contiguous block at `placement` (`move`).
    pub async fn move_pages(
        &self,
        pages: Vec<PageRef>,
        placement: PageEditPlacement,
        cancel: Arc<CancelToken>,
    ) -> Result<PageEditLayout, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.move_pages(refs(pages), Some(placement.into()))),
                decode_layout,
            )
            .await?
    }

    /// Delete pages (`delete`). The engine refuses deleting every page.
    pub async fn delete_pages(
        &self,
        pages: Vec<PageRef>,
        cancel: Arc<CancelToken>,
    ) -> Result<PageEditLayout, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.delete(refs(pages))),
                decode_layout,
            )
            .await?
    }

    pub async fn insert_blank(
        &self,
        options: PageEditBlankOptions,
        cancel: Arc<CancelToken>,
    ) -> Result<PageEditInsertion, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.insert_blank(options.into())),
                decode_insertion,
            )
            .await?
    }

    /// Pages of another PDF, given as its bytes.
    pub async fn insert_from_bytes(
        &self,
        bytes: Vec<u8>,
        options: PageEditBytesOptions,
        cancel: Arc<CancelToken>,
    ) -> Result<PageEditInsertion, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.insert_from_bytes(Bytes(bytes), options.into())),
                decode_insertion,
            )
            .await?
    }

    /// Pages of another open document, by its id.
    pub async fn insert_from_document(
        &self,
        document_id: String,
        pages: Vec<PageRef>,
        placement: Option<PageEditPlacement>,
        cancel: Arc<CancelToken>,
    ) -> Result<PageEditInsertion, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    Ok(open(c)?.insert_from_document(
                        document_id,
                        refs(pages),
                        placement.map(Into::into),
                    ))
                },
                decode_insertion,
            )
            .await?
    }

    /// Copies of pages; by default right after the last of them.
    pub async fn duplicate(
        &self,
        pages: Vec<PageRef>,
        placement: Option<PageEditPlacement>,
        cancel: Arc<CancelToken>,
    ) -> Result<PageEditInsertion, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.duplicate(refs(pages), placement.map(Into::into))),
                decode_insertion,
            )
            .await?
    }

    /// A new PDF from a page subset, as its bytes.
    pub async fn extract(
        &self,
        pages: Vec<PageRef>,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<u8>, ViewerError> {
        self.cap
            .call_promise_cancellable(cancel, move |c| Ok(open(c)?.extract(refs(pages))), |b| b.0)
            .await
    }
}
```

`call_promise_cancellable`'s `map` returns `T`; the five methods whose map can fail return `Result<_, ViewerError>` from it and flatten with the trailing `?`, as `MetadataHandle::update` does.

- [x] **Step 5: Run the tests**

Run: `cargo test -p epdf-plugin-page-edit --features uniffi --test ffi`
Expected: PASS, 11 tests.

Run: `cargo test -p epdf-plugin-page-edit`
Expected: PASS. The conformance vectors don't see the feature.

- [x] **Step 6: Commit**

```bash
git add crates/plugins/page-edit
git commit -m "Export the page-edit plugin through uniffi"
```

### Task 3: The six `pages.*` engine paths

**Files:**
- Modify: `crates/epdf-viewer-host/src/translate.rs`, `crates/epdf-viewer-host/src/host_table.rs`, `crates/epdf-viewer-host/src/native_engine.rs`
- Test: `crates/epdf-viewer-host/tests/translate.rs`

**Interfaces:**
- Consumes: nothing from Task 2. These are engine paths the plugin calls: `pages.move`, `pages.rotate`, `pages.delete`, `pages.insert`, `pages.insertBlank`, `pages.extract` (spec Appendix C, row "B3").
- Produces:
  - `translate::Decode::PageMutation`: a mutation whose `result.layout.pages[]` and `event.layout.pages[]` each gain `ref`, and whose `insertedPageObjectNumbers` becomes `insertedPages: [PageRef]`.
  - `translate::Decode::HostBytes`: a byte payload parked in the `HostTable`, answered as `{"$hostBytes": id}`.
  - `HostTable::install_bytes_resolver()`: makes `epdf_kernel::bytes::Bytes::from_json` resolve `$hostBytes` by taking the entry out of the table. Called on the kernel thread.
  - `NativeEngine::open` accepts `{"kind": "bytes", "bytes": <Bytes JSON>}` as well as `{"$host": id}`.
  - `NativeDocument::page_index` answers from the layout of the last page mutation.

The host reads `pageObjectNumbers: [u32]` (not refs), `destIndex: u32`, `rotation: u16` and `count: u32`, and takes `pages.insert`'s PDF as payload 0 (`crates/epdf-engine/src/host.rs:807-920`). The plugin sends refs, JavaScript numbers (floats, or `{"$number": "NaN"}`), `undefined` for an absent destination, and the PDF as `Bytes` JSON. `translate` converts each, and refuses a number that isn't a whole, non-negative one with `invalid-input` rather than letting serde fail on the host with a message nobody can act on.

`$hostBytes` exists so an extracted PDF doesn't cross base64 twice. `extract` and `duplicate` hand the extract's value straight to `pages.insert` or `Bytes::from_json`, which takes it out of the table. The price: a `$hostBytes` entry whose promise is aborted between the extract and the insert stays in the table until the session drops. `insert_from_bytes` still pays one base64 round trip, because the plugin calls `Bytes::to_json` on the caller's bytes (`crates/plugins/page-edit/src/controller.rs`, `insert_at`). Removing it means changing the plugin, which this plan doesn't do; B5's size and speed work can.

- [x] **Step 1: Write the failing tests**

Append to `crates/epdf-viewer-host/tests/translate.rs`:

```rust
// ── page edits ─────────────────────────────────────────────────────────────

fn page_ref(pon: u32) -> Value {
    json!({ "kind": "objectNumber", "pageObjectNumber": pon })
}

#[test]
fn a_rotation_travels_as_a_whole_number() {
    let args = [json!([page_ref(3), page_ref(4)]), json!(90.0)];
    let call = translate("d1", "pages.rotate", None, &args, 11).unwrap();
    assert_eq!(
        call.request,
        json!({
            "kind": "pages.rotate", "jobId": 11, "docId": "d1",
            "pageObjectNumbers": [3, 4], "rotation": 90
        })
    );
    assert!(call.request["rotation"].is_u64());
    assert!(matches!(call.decode, Decode::PageMutation));
}

#[test]
fn a_move_carries_its_destination() {
    let args = [json!([page_ref(3)]), json!(0.0)];
    let call = translate("d1", "pages.move", None, &args, 12).unwrap();
    assert_eq!(call.request["pageObjectNumbers"], json!([3]));
    assert_eq!(call.request["destIndex"], json!(0));
}

#[test]
fn a_fractional_index_is_invalid_input() {
    for bad in [json!(1.5), json!(-1.0), json!({ "$number": "NaN" })] {
        let args = [json!([page_ref(3)]), bad.clone()];
        let error = translate("d1", "pages.move", None, &args, 13).err().unwrap();
        assert_eq!(error.code(), Some("invalid-input"), "{bad}");
    }
}

#[test]
fn an_absent_destination_is_left_out() {
    let spec = json!({ "size": { "width": 612.0, "height": 792.0 }, "count": 2.0 });
    let call =
        translate("d1", "pages.insertBlank", None, &[spec, epdf_kernel::undefined()], 14).unwrap();
    assert_eq!(
        call.request,
        json!({
            "kind": "pages.insertBlank", "jobId": 14, "docId": "d1",
            "size": { "width": 612.0, "height": 792.0 }, "count": 2
        })
    );
}

#[test]
fn an_insert_moves_its_pdf_into_the_payload() {
    let bytes = epdf_kernel::bytes::Bytes(b"%PDF-1.7".to_vec()).to_json();
    let call = translate("d1", "pages.insert", None, &[bytes, json!(1.0)], 15).unwrap();
    assert_eq!(call.payloads, vec![b"%PDF-1.7".to_vec()]);
    assert_eq!(call.request["destIndex"], json!(1));
    assert!(call.request.get("bytes").is_none());
}

#[test]
fn an_insert_without_bytes_is_invalid_input() {
    let error = translate("d1", "pages.insert", None, &[json!({})], 16).err().unwrap();
    assert_eq!(error.code(), Some("invalid-input"));
}

#[test]
fn an_extract_decodes_to_host_bytes() {
    let response = HostResponse { json: r#"{"result":{"size":3}}"#.into(), payloads: vec![vec![1, 2, 3]] };
    let value = decode(&Decode::HostBytes, response).unwrap().value;
    let id = value["$hostBytes"].as_u64().unwrap();
    assert_eq!(HostTable::take(id), Some(vec![1, 2, 3]));
}

const PAGE: &str = r#"{"boxes":{},"index":0,"label":null,"pageObjectNumber":2,"rotation":90,"size":{"height":792.0,"width":612.0},"userUnit":1.0}"#;

#[test]
fn a_page_mutation_gives_every_layout_page_its_ref() {
    let json = format!(
        r#"{{"result":{{"event":{{"layout":{{"pageCount":1,"pages":[{PAGE}]}},"type":"pages.rotated"}},"mutationSequence":2,"result":{{"layout":{{"pageCount":1,"pages":[{PAGE}]}}}}}}}}"#
    );
    let decoded = decode(&Decode::PageMutation, answer(&json)).unwrap();
    assert_eq!(decoded.value["layout"]["pages"][0]["ref"], page_ref(2));
    let event = decoded.event.unwrap();
    assert_eq!(event["type"], "pages.rotated");
    assert_eq!(event["layout"]["pages"][0]["ref"], page_ref(2));
}

#[test]
fn an_insert_result_names_its_pages_as_refs() {
    let json = format!(
        r#"{{"result":{{"event":{{"insertedPageObjectNumbers":[2],"layout":{{"pageCount":1,"pages":[{PAGE}]}},"type":"pages.inserted"}},"mutationSequence":3,"result":{{"insertedPageObjectNumbers":[2],"layout":{{"pageCount":1,"pages":[{PAGE}]}}}}}}}}"#
    );
    let decoded = decode(&Decode::PageMutation, answer(&json)).unwrap();
    assert_eq!(decoded.value["insertedPages"], json!([page_ref(2)]));
}
```

And the native-engine half, below `a_metadata_update_answers_its_result_after_publishing_its_event`:

```rust
#[test]
fn an_inserted_page_is_known_to_the_engine_handle() {
    let (inserted, index, text) = on_kernel(EngineWorker::spawn(), |engine, tx| {
        engine.open(open_input(test_support::REPORT), None).on_settle(move |doc| {
            let doc = doc.as_ref().ok().cloned().unwrap();
            let spec = json!({ "size": { "width": 612.0, "height": 792.0 } });
            doc.clone().call("pages.insertBlank", None, vec![spec, epdf_kernel::undefined()]).on_settle(
                move |result| {
                    let result = result.as_ref().unwrap().clone();
                    let pon = result["insertedPages"][0]["pageObjectNumber"].as_u64().unwrap() as u32;
                    let index = doc.page_index(pon);
                    doc.call("page.text.read", Some(pon), vec![]).on_settle(move |text| {
                        let _ = tx.send((result, index, text.is_ok()));
                    });
                },
            );
        });
    });
    assert_eq!(inserted["layout"]["pages"].as_array().map_or(0, Vec::len), 15);
    assert_eq!(index.ok(), Some(14), "the new last page");
    assert!(text, "a page call reaches the inserted page");
}

#[test]
fn an_extract_round_trips_through_the_host_table() {
    let (bytes, left) = on_kernel(EngineWorker::spawn(), |engine, tx| {
        HostTable::install_bytes_resolver();
        engine.open(open_input(test_support::REPORT), None).on_settle(move |doc| {
            let doc = doc.as_ref().ok().cloned().unwrap();
            doc.clone().call("pages.list", None, vec![]).on_settle(move |list| {
                let first = list.as_ref().unwrap()["pages"][0]["ref"].clone();
                doc.call("pages.extract", None, vec![json!([first])]).on_settle(move |value| {
                    let bytes = epdf_kernel::bytes::Bytes::from_json(value.as_ref().unwrap());
                    let _ = tx.send((bytes, HostTable::len()));
                });
            });
        });
    });
    assert!(bytes.unwrap().0.starts_with(b"%PDF"));
    assert_eq!(left, 0, "resolving took the bytes out of the table");
}

#[test]
fn a_raw_engine_opens_bytes_given_as_bytes_json() {
    let count = on_kernel(EngineWorker::spawn(), |engine, tx| {
        let bytes = epdf_kernel::bytes::Bytes(test_support::fixture(test_support::COLORS));
        let input = json!({ "kind": "bytes", "id": "source", "bytes": bytes.to_json() });
        engine.open(input, None).on_settle(move |doc| {
            let doc = doc.as_ref().ok().cloned().unwrap();
            doc.call("pages.list", None, vec![]).on_settle(move |list| {
                let _ = tx.send(list.as_ref().unwrap()["pages"].as_array().map_or(0, Vec::len));
            });
        });
    });
    assert!(count > 0);
}
```

- [x] **Step 2: Run them to see them fail**

Run: `cargo test -p epdf-viewer-host --test translate`
Expected: FAIL to compile on `Decode::PageMutation`, `Decode::HostBytes` and `HostTable::install_bytes_resolver`.

- [x] **Step 3: Route and translate**

In `crates/epdf-viewer-host/src/translate.rs`, add two variants to `Decode`:

```rust
    /// A page mutation: `Mutation`, with `ref` added to every layout page of
    /// the result and the event (the kernel's page registry and the
    /// page-edit plugin read `ref`), and the host's
    /// `insertedPageObjectNumbers` given as `insertedPages` refs.
    PageMutation,
    /// A byte payload (an extracted PDF), parked in the `HostTable` and
    /// answered as `{"$hostBytes": id}` for `Bytes::from_json` to take.
    HostBytes,
```

Extend `route`:

```rust
        "pages.move" => ("pages.move", Decode::PageMutation),
        "pages.rotate" => ("pages.rotate", Decode::PageMutation),
        "pages.delete" => ("pages.delete", Decode::PageMutation),
        "pages.insert" => ("pages.insert", Decode::PageMutation),
        "pages.insertBlank" => ("pages.insertBlank", Decode::PageMutation),
        "pages.extract" => ("pages.extract", Decode::HostBytes),
```

Add arms to `translate`'s `extra` match (don't replace the match; other plans' arms are in it). If `translate` doesn't have `let mut payloads = Vec::new();` yet, add it above the match and return `Ok(WireCall { request: Value::Object(request), payloads, decode })`. `whole` and `optional_whole` are umbrella Task 0e's, and the request builder already drops `null` and `$undefined` values.

```rust
        "pages.move" => vec![
            ("pageObjectNumbers", object_numbers(path, &arg(0))?),
            ("destIndex", whole(path, "destIndex", &arg(1))?),
        ],
        "pages.rotate" => vec![
            ("pageObjectNumbers", object_numbers(path, &arg(0))?),
            ("rotation", whole(path, "rotation", &arg(1))?),
        ],
        "pages.delete" | "pages.extract" => {
            vec![("pageObjectNumbers", object_numbers(path, &arg(0))?)]
        }
        "pages.insertBlank" => {
            let spec = arg(0);
            vec![
                ("size", spec.get("size").cloned().unwrap_or(Value::Null)),
                ("count", optional_whole(path, "count", spec.get("count"))?),
                ("destIndex", optional_whole(path, "destIndex", args.get(1))?),
            ]
        }
        "pages.insert" => {
            payloads.push(pdf_bytes(path, &arg(0))?);
            vec![("destIndex", optional_whole(path, "destIndex", args.get(1))?)]
        }
```

Add to `PATHS`, one line each in plan order:

```rust
    "pages.move",
    "pages.rotate",
    "pages.delete",
    "pages.insert",
    "pages.insertBlank",
    "pages.extract",
```

Add the helpers below `translate`:

```rust
/// `[PageRef]` as the host's `[pageObjectNumber]`.
fn object_numbers(path: &str, refs: &Value) -> Result<Value, Error> {
    let refs = refs.as_array().ok_or_else(|| {
        coded("invalid-input", format!("[epdf-viewer-host] {path} takes a list of pages: {refs}"))
    })?;
    refs.iter()
        .map(|r| {
            PageRef::from_json(r).map(|p| Value::from(p.page_object_number)).ok_or_else(|| {
                coded("invalid-input", format!("[epdf-viewer-host] {path}: not a PageRef: {r}"))
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Value::Array)
}

/// A PDF given as `Bytes` JSON: `{"$bytes": base64}` from the caller, or
/// `{"$hostBytes": id}` from an extract, which this takes out of the table.
fn pdf_bytes(path: &str, value: &Value) -> Result<Vec<u8>, Error> {
    epdf_kernel::bytes::Bytes::from_json(value).map(|b| b.0).ok_or_else(|| {
        coded("invalid-input", format!("[epdf-viewer-host] {path} takes a PDF's bytes: {value}"))
    })
}
```

In `decode`, add two arms before `Decode::Raster`:

```rust
        Decode::PageMutation => {
            let mut value = take(&mut result, "result")?;
            add_layout_refs(&mut value, "a page mutation's result")?;
            if let Some(pons) = value.get("insertedPageObjectNumbers").and_then(Value::as_array) {
                let refs = pons
                    .iter()
                    .map(|n| {
                        n.as_u64()
                            .and_then(|n| u32::try_from(n).ok())
                            .map(|n| PageRef::new(n).to_json())
                            .ok_or_else(|| malformed(format!("an inserted page that is not a number: {n}")))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                value["insertedPages"] = Value::Array(refs);
            }
            let mut event = result.remove("event");
            if let Some(event) = event.as_mut() {
                add_layout_refs(event, "a page event")?;
            }
            return Ok(Decoded { value, event });
        }
        Decode::HostBytes => {
            let bytes = response
                .payloads
                .into_iter()
                .next()
                .ok_or_else(|| malformed("bytes without their payload".into()))?;
            serde_json::json!({ "$hostBytes": HostTable::put(bytes) })
        }
```

and the helper beside `add_page_ref`:

```rust
/// `ref` on every page of `object.layout.pages`, which must be there.
fn add_layout_refs(object: &mut Value, what: &str) -> Result<(), Error> {
    let Some(Value::Array(pages)) = object.get_mut("layout").and_then(|l| l.get_mut("pages"))
    else {
        return Err(malformed(format!("{what} without a layout")));
    };
    for page in pages {
        add_page_ref(page, "ref", "a layout page")?;
    }
    Ok(())
}
```

- [x] **Step 4: Resolve `$hostBytes` from the table**

In `crates/epdf-viewer-host/src/host_table.rs`:

```rust
    /// Make `Bytes::from_json` resolve `{"$hostBytes": id}` by taking that
    /// entry out of this thread's table, so an extracted PDF reaches its
    /// one reader without a base64 round trip. On the kernel thread, once
    /// per thread.
    pub fn install_bytes_resolver() {
        epdf_kernel::bytes::set_host_bytes_resolver(Some(std::rc::Rc::new(|id: u32| {
            HostTable::take(u64::from(id))
        })));
    }
```

`Bytes::from_json` reads the id as `u32` (`crates/epdf-kernel/src/bytes.rs:68`). The table counts from 1 and a session doesn't reach four billion entries; past that the cast truncates and the resolve misses, which surfaces as `invalid-input` from `pdf_bytes`, not as wrong bytes.

- [x] **Step 5: Keep the live layout**

In `crates/epdf-viewer-host/src/native_engine.rs`, add a thread-local beside `LISTENERS`:

```rust
    /// Each open document's current `PageListSnapshot`: the one its open
    /// loaded, then the `layout` of every page mutation's event.
    static LAYOUTS: RefCell<HashMap<String, Value>> = RefCell::new(HashMap::new());
```

In `publish`, before handing the event to listeners:

```rust
    if let Some(layout) = page_layout(event) {
        LAYOUTS.with(|l| l.borrow_mut().insert(doc.to_owned(), layout.clone()));
    }
```

with:

```rust
/// The layout a page mutation's event carries, as `kernel.rs` reads it.
fn page_layout(event: &Value) -> Option<&Value> {
    match event.get("type").and_then(Value::as_str)? {
        "pages.moved" | "pages.rotated" | "pages.deleted" | "pages.inserted" => event.get("layout"),
        _ => None,
    }
}
```

`Engine::open` already takes `{"bytes": <Bytes JSON>}` beside `{"$host": id}` (umbrella Task 0e), which page edit's subset insert sends.

Where the open resolves (`Ok(pages) => settled.resolve(...)`), record the layout first:

```rust
                Ok(pages) => {
                    LAYOUTS.with(|l| l.borrow_mut().insert(doc.clone(), pages.clone()));
                    settled.resolve(Rc::new(NativeDocument { id: doc, inner, pages, security }))
                }
```

In `NativeDocument::page_index`, read the live layout:

```rust
    fn page_index(&self, pon: u32) -> Result<u32, Error> {
        let page = PageRef::new(pon);
        let layout = LAYOUTS.with(|l| l.borrow().get(&self.id).cloned());
        layout
            .as_ref()
            .unwrap_or(&self.pages)
            .get("pages")
            .and_then(Value::as_array)
            .and_then(|pages| pages.iter().find(|p| page.names(p)))
            .and_then(|p| p.get("index").and_then(Value::as_u64))
            .and_then(|i| u32::try_from(i).ok())
            .ok_or_else(|| Error::engine("NotFound", format!("NotFound: no page {pon}")))
    }
```

`PageRef::names` matches on `ref.pageObjectNumber`, which `Decode::PageMutation` now adds to every layout page. Drop the entry when the document closes: in `NativeDocument::call`, when `path == "close"`, remove `LAYOUTS[self.id]` before sending.

```rust
    fn call(&self, path: &str, pon: Option<u32>, args: Vec<Value>) -> EngineCall<Value> {
        if path == "close" {
            LAYOUTS.with(|l| l.borrow_mut().remove(&self.id));
        }
        self.inner.send(&self.id, |job| translate::translate(&self.id, path, pon, &args, job))
    }
```

Update the module doc comment's last paragraph: the `pages.list` snapshot is no longer frozen.

- [x] **Step 6: Run the tests**

Run: `cargo test -p epdf-viewer-host --test translate`
Expected: PASS, including the three native-engine tests against real PDFium.

- [x] **Step 7: Commit**

```bash
git add crates/epdf-viewer-host/src crates/epdf-viewer-host/tests/translate.rs
git commit -m "Map the six page-edit engine paths and keep the native page list live"
```

### Task 4: Host wiring

**Files:**
- Modify: `crates/epdf-viewer-host/Cargo.toml`, `src/api.rs`, `src/session.rs`, `src/lib.rs`
- Test: `crates/epdf-viewer-host/tests/viewer_host.rs`

**Interfaces:**
- Consumes: `PageEditHandle::new` (Task 2), `HostTable::install_bytes_resolver` (Task 3).
- Produces: `ViewerConfig.page_edit: bool` (default `false`), `ViewerHost::page_edit(document_id: String) -> Result<Arc<PageEditHandle>, ViewerError>`.

- [ ] **Step 1: Write the failing tests**

Append to `crates/epdf-viewer-host/tests/viewer_host.rs`, adding `use epdf_plugin_page_edit::ffi::{PageEditBlankOptions, PageEditPlacement, PageEditTurn};` and `use epdf_ffi::PageRef;` at the top:

```rust
fn with_page_edit() -> Arc<ViewerHost> {
    let config = ViewerConfig {
        page_edit: true,
        render: Some(RenderConfig::default()),
        ..Default::default()
    };
    let host = ViewerHost::new(config, Arc::new(Delegate)).unwrap();
    block_on(host.start()).unwrap();
    host
}

#[test]
fn rotating_twice_reads_the_rotation_the_first_edit_left() {
    let host = with_page_edit();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let edit = block_on(host.page_edit(doc.id.clone())).unwrap();
    let first = block_on(host.pages(doc.id.clone())).unwrap()[0].page;
    block_on(edit.rotate_by(vec![first], PageEditTurn::Clockwise, CancelToken::new())).unwrap();
    let layout =
        block_on(edit.rotate_by(vec![first], PageEditTurn::Clockwise, CancelToken::new())).unwrap();
    assert_eq!(layout.pages[0].rotation, 180);
    assert_eq!(block_on(host.pages(doc.id)).unwrap()[0].rotation, 180);
}

#[test]
fn an_inserted_blank_page_renders() {
    let host = with_page_edit();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let edit = block_on(host.page_edit(doc.id.clone())).unwrap();
    let first = block_on(host.pages(doc.id.clone())).unwrap()[0].page;
    let options = PageEditBlankOptions {
        placement: Some(PageEditPlacement::After { page: first }),
        ..Default::default()
    };
    let insertion = block_on(edit.insert_blank(options, CancelToken::new())).unwrap();
    assert_eq!(insertion.pages.len(), 15);
    assert_eq!(insertion.pages[1].page, insertion.inserted[0]);
    let render = block_on(host.render(doc.id)).unwrap();
    let image = block_on(render.render_page(
        insertion.inserted[0],
        RenderPageOptions::default(),
        CancelToken::new(),
    ))
    .unwrap();
    assert!(image.width > 0 && image.height > 0);
}

#[test]
fn extracting_and_duplicating_leave_nothing_in_the_host_table() {
    let host = with_page_edit();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let edit = block_on(host.page_edit(doc.id.clone())).unwrap();
    let before = block_on(host.host_table_entries()).unwrap();
    let pages: Vec<PageRef> =
        block_on(host.pages(doc.id.clone())).unwrap().iter().take(2).map(|p| p.page).collect();
    let pdf = block_on(edit.extract(pages.clone(), CancelToken::new())).unwrap();
    assert!(pdf.starts_with(b"%PDF"));
    let insertion = block_on(edit.duplicate(pages, None, CancelToken::new())).unwrap();
    assert_eq!(insertion.inserted.len(), 2);
    assert_eq!(block_on(host.host_table_entries()).unwrap(), before);
}

#[test]
fn a_subset_of_another_pdf_inserts_through_a_second_open() {
    let host = with_page_edit();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let edit = block_on(host.page_edit(doc.id.clone())).unwrap();
    let options = epdf_plugin_page_edit::ffi::PageEditBytesOptions {
        page_indexes: Some(vec![0]),
        placement: Some(PageEditPlacement::Index { index: 0 }),
    };
    let insertion = block_on(edit.insert_from_bytes(
        test_support::fixture(test_support::REPORT),
        options,
        CancelToken::new(),
    ))
    .unwrap();
    assert_eq!(insertion.inserted.len(), 1);
    assert_eq!(insertion.pages[0].page, insertion.inserted[0]);
}

#[test]
fn bytes_that_are_not_a_pdf_are_refused() {
    let host = with_page_edit();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let edit = block_on(host.page_edit(doc.id.clone())).unwrap();
    let error = block_on(edit.insert_from_bytes(b"not a pdf".to_vec(), Default::default(), CancelToken::new()))
        .unwrap_err();
    assert!(format!("{error:?}").contains("DocOpenFailed"), "{error:?}");
    assert_eq!(block_on(host.pages(doc.id)).unwrap().len(), 14);
}

#[test]
fn without_its_flag_the_page_edit_plugin_is_not_installed() {
    let host = started();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    assert!(block_on(host.page_edit(doc.id)).is_err());
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p epdf-viewer-host --test viewer_host page_edit`
Expected: FAIL to compile, `no field page_edit on ViewerConfig`.

- [ ] **Step 3: Wire the plugin in**

`crates/epdf-viewer-host/Cargo.toml`, alphabetical among the plugins:

```toml
epdf-plugin-page-edit = { path = "../plugins/page-edit", default-features = false, features = ["uniffi"] }
```

`crates/epdf-viewer-host/src/api.rs`, in `ViewerConfig`, after the fields earlier plans added:

```rust
    /// Install `pageEditPlugin()`: rotate, move, delete, insert and extract
    /// pages, addressed by `PageRef`.
    #[uniffi(default = false)]
    pub page_edit: bool,
```

`crates/epdf-viewer-host/src/session.rs`: in `Session::build`, right after `let worker = EngineWorker::spawn();`:

```rust
        HostTable::install_bytes_resolver();
```

and in the plugin list, after the earlier plans' blocks:

```rust
        if config.page_edit {
            plugins.push(epdf_plugin_page_edit::page_edit_plugin());
        }
```

`crates/epdf-viewer-host/src/lib.rs`: `use epdf_plugin_page_edit::ffi::PageEditHandle;`, and in the exported `impl ViewerHost`:

```rust
    /// The page-edit capability of an open document. Needs
    /// `ViewerConfig.page_edit`; without it the kernel refuses the token.
    pub async fn page_edit(&self, document_id: String) -> Result<Arc<PageEditHandle>, ViewerError> {
        let post = self.post.clone();
        self.call(move |s| {
            let cap = s.kernel.capability(&epdf_plugin_page_edit::PAGE_EDIT, Some(&document_id))?;
            Ok(PageEditHandle::new(cap, post))
        })
        .await
    }
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p epdf-ffi -p epdf-viewer-host`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/epdf-viewer-host
git commit -m "Wire the page-edit plugin into the viewer host"
```

### Task 5: Swift `PageEditCapability`

**Files:**
- Create: `platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/PageEdit.swift`
- Test: `platforms/apple/EmbedPDF/Tests/EmbedPDFTests/PageEditTests.swift`

**Interfaces:**
- Consumes: the generated `PageEditHandle`, `ViewerHost.pageEdit(documentId:)`, and the records from Task 2.
- Produces: `PageEditCapability` and `PDFViewer.pageEdit(for:)`. Methods: `canEdit()`, `rotate(_:by:)`, `setRotation(_:of:)`, `movePages(_:to:)`, `deletePages(_:)`, `insertBlank(_:)`, `insert(contentsOf:options:)`, `insert(_:from:at:)`, `duplicate(_:at:)`, `extract(_:)`.

- [ ] **Step 1: Write the failing tests**

Create `platforms/apple/EmbedPDF/Tests/EmbedPDFTests/PageEditTests.swift`:

```swift
import CustomDump
import EmbedPDF
import Foundation
import Testing

extension BaseSuite {
  @Suite struct PageEditTests {
    func opened() async throws -> (PDFViewer, PDFDocument, PageEditCapability) {
      let viewer = try PDFViewer(ViewerConfig(pageEdit: true))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      return (viewer, document, try await viewer.pageEdit(for: document))
    }

    @Test func rotatingTwiceMakesAHalfTurn() async throws {
      let (viewer, document, edit) = try await opened()
      let first = try await viewer.pages(of: document)[0].page
      _ = try await edit.rotate([first], by: .clockwise)
      let layout = try await edit.rotate([first], by: .clockwise)
      expectNoDifference(layout.pages[0].rotation, 180)
      let pages = try await viewer.pages(of: document)
      expectNoDifference(pages[0].rotation, 180)
    }

    @Test func movingAPageToTheEndReordersTheList() async throws {
      let (viewer, document, edit) = try await opened()
      let first = try await viewer.pages(of: document)[0].page
      let layout = try await edit.movePages([first], to: .end)
      expectNoDifference(layout.pages.last?.page, first)
      expectNoDifference(layout.pages.map(\.index), Array(0..<14))
    }

    @Test func deletingEveryPageThrowsAndKeepsThePages() async throws {
      let (viewer, document, edit) = try await opened()
      let all = try await viewer.pages(of: document).map(\.page)
      await #expect(throws: PDFError.self) {
        _ = try await edit.deletePages(all)
      }
      let pages = try await viewer.pages(of: document)
      expectNoDifference(pages.count, 14)
    }

    @Test func anInsertedBlankPageTakesItsNeighboursSize() async throws {
      let (viewer, document, edit) = try await opened()
      let first = try await viewer.pages(of: document)[0]
      let insertion = try await edit.insertBlank(PageEditBlankOptions(count: 2, placement: .after(page: first.page)))
      expectNoDifference(insertion.inserted.count, 2)
      expectNoDifference(insertion.pages.count, 16)
      expectNoDifference(insertion.pages[1].width, first.width)
      expectNoDifference(insertion.pages[1].page, insertion.inserted[0])
    }

    @Test func anExtractOpensAsItsOwnDocument() async throws {
      let (viewer, document, edit) = try await opened()
      let pages = try await viewer.pages(of: document).prefix(3).map(\.page)
      let data = try await edit.extract(Array(pages))
      expectNoDifference(data.prefix(4), Data("%PDF".utf8))
      let extracted = try await viewer.open(data)
      expectNoDifference(extracted.pageCount, 3)
    }

    @Test func pagesFromAnotherOpenDocumentInsert() async throws {
      let (viewer, document, edit) = try await opened()
      let other = try await viewer.open(Fixture.data("colors"))
      let source = try await viewer.pages(of: other).map(\.page)
      let insertion = try await edit.insert(source, from: other, at: .index(index: 0))
      expectNoDifference(insertion.inserted.count, source.count)
      expectNoDifference(insertion.pages.count, 14 + source.count)
    }

    @Test func anEditIsObservedAsPagesChanged() async throws {
      let (viewer, document, edit) = try await opened()
      let events = viewer.documentEvents
      let first = try await viewer.pages(of: document)[0].page
      _ = try await edit.setRotation(.clockwise90, of: [first])
      for await event in events {
        guard case let .pagesChanged(documentId) = event else { continue }
        expectNoDifference(documentId, document.id)
        break
      }
    }

    @Test func rotateCancelsWhenTheTaskIsCancelled() async throws {
      let (viewer, document, edit) = try await opened()
      let first = try await viewer.pages(of: document)[0].page
      let task = Task { try await edit.rotate([first], by: .half) }
      task.cancel()
      do {
        _ = try await task.value
      } catch {
        // Cancelled before it started. Once started, the edit lands.
        expectNoDifference((error as? PDFError)?.code, "operation-cancelled")
      }
      // The capability still answers after a cancelled call.
      #expect(try await edit.canEdit())
    }

    @Test func aClosedDocumentsCapabilityThrows() async throws {
      let (viewer, document, edit) = try await opened()
      try await viewer.close(document)
      await #expect {
        _ = try await edit.canEdit()
      } throws: { error in
        let error = error as? PDFError
        return error?.code == "instance-closed" && error?.name == "PluginError"
      }
    }
  }
}
```

There's no `droppingTheStreamUnsubscribes`: page edit has no event stream of its own. `anEditIsObservedAsPagesChanged` covers the events an edit does cause.

- [ ] **Step 2: Run them to see them fail**

```bash
bash platforms/apple/build-xcframework.sh
(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17' -only-testing:EmbedPDFTests/BaseSuite/PageEditTests)
```

Expected: FAIL to compile, `value of type 'PDFViewer' has no member 'pageEdit'`.

- [ ] **Step 3: Write the capability**

Create `platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/PageEdit.swift`:

```swift
public import EmbedPDFBindings
public import Foundation

public typealias PageEditPlacement = EmbedPDFBindings.PageEditPlacement
public typealias PageEditTurn = EmbedPDFBindings.PageEditTurn
public typealias PageEditRotation = EmbedPDFBindings.PageEditRotation
public typealias PageEditBlankOptions = EmbedPDFBindings.PageEditBlankOptions
public typealias PageEditBytesOptions = EmbedPDFBindings.PageEditBytesOptions
public typealias PageEditLayout = EmbedPDFBindings.PageEditLayout
public typealias PageEditInsertion = EmbedPDFBindings.PageEditInsertion

/// Structural page edits, addressed by `PageRef`: rotate, move, delete,
/// insert and extract. Get one with `PDFViewer.pageEdit(for:)`; the viewer
/// needs `ViewerConfig(pageEdit: true)`. Page-list changes arrive on
/// `PDFViewer.documentEvents` as `.pagesChanged`; this capability has no
/// stream of its own. Cancelling a task skips an edit that hasn't started;
/// one that has started lands. After its document closes, every call
/// throws `instance-closed`.
public struct PageEditCapability: Sendable {
  let handle: PageEditHandle
  let viewer: PDFViewer

  /// Whether this caller may edit pages (`doc.pages.assemble`).
  public func canEdit() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canEdit() }
  }

  /// Turn each page from its own rotation.
  public func rotate(_ pages: [PageRef], by turn: PageEditTurn) async throws(PDFError) -> PageEditLayout {
    try await bridgedCancellable { token in try await handle.rotateBy(pages: pages, turn: turn, cancel: token) }
  }

  public func setRotation(_ rotation: PageEditRotation, of pages: [PageRef]) async throws(PDFError)
    -> PageEditLayout
  {
    try await bridgedCancellable { token in
      try await handle.setRotation(pages: pages, rotation: rotation, cancel: token)
    }
  }

  /// Reorder pages as a contiguous block at `placement`.
  public func movePages(_ pages: [PageRef], to placement: PageEditPlacement) async throws(PDFError)
    -> PageEditLayout
  {
    try await bridgedCancellable { token in
      try await handle.movePages(pages: pages, placement: placement, cancel: token)
    }
  }

  /// The engine refuses deleting every page.
  public func deletePages(_ pages: [PageRef]) async throws(PDFError) -> PageEditLayout {
    try await bridgedCancellable { token in try await handle.deletePages(pages: pages, cancel: token) }
  }

  public func insertBlank(_ options: PageEditBlankOptions = PageEditBlankOptions()) async throws(PDFError)
    -> PageEditInsertion
  {
    try await bridgedCancellable { token in try await handle.insertBlank(options: options, cancel: token) }
  }

  /// Pages of another PDF, given as its bytes.
  public func insert(contentsOf data: Data, options: PageEditBytesOptions = PageEditBytesOptions())
    async throws(PDFError) -> PageEditInsertion
  {
    try await bridgedCancellable { token in
      try await handle.insertFromBytes(bytes: data, options: options, cancel: token)
    }
  }

  /// Pages of another open document.
  public func insert(_ pages: [PageRef], from document: PDFDocument, at placement: PageEditPlacement? = nil)
    async throws(PDFError) -> PageEditInsertion
  {
    try await bridgedCancellable { token in
      try await handle.insertFromDocument(
        documentId: document.id, pages: pages, placement: placement, cancel: token)
    }
  }

  /// Copies of pages; by default right after the last of them.
  public func duplicate(_ pages: [PageRef], at placement: PageEditPlacement? = nil) async throws(PDFError)
    -> PageEditInsertion
  {
    try await bridgedCancellable { token in
      try await handle.duplicate(pages: pages, placement: placement, cancel: token)
    }
  }

  /// A new PDF from a page subset.
  public func extract(_ pages: [PageRef]) async throws(PDFError) -> Data {
    try await bridgedCancellable { token in try await handle.extract(pages: pages, cancel: token) }
  }
}

extension PDFViewer {
  public func pageEdit(for document: PDFDocument) async throws(PDFError) -> PageEditCapability {
    let handle = try await bridged { try await host.pageEdit(documentId: document.id) }
    return PageEditCapability(handle: handle, viewer: self)
  }
}
```

`ViewerConfig(pageEdit: true)` compiles because the field has `#[uniffi(default = false)]`, so every other argument defaults.

- [ ] **Step 4: Run the tests**

```bash
bash platforms/apple/build-xcframework.sh
(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17')
```

Expected: PASS, every suite, `PageEditTests` with 9 tests.

- [ ] **Step 5: Commit**

```bash
git add platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/PageEdit.swift platforms/apple/EmbedPDF/Tests/EmbedPDFTests/PageEditTests.swift
git commit -m "Add the Swift page-edit capability"
```

### Task 6: Docs, CI, merge and clean up

**Files:**
- Modify: `.github/workflows/plugin-rust.yml`, `platforms/apple/README.md`, `docs/superpowers/plans/2026-10-01-swift-package.md`

- [ ] **Step 1: CI**

Append `page-edit` to the `crates=( ... )` list in `.github/workflows/plugin-rust.yml` (umbrella Task 0 made it a list), keeping plan-number order with whatever earlier plans added.

- [ ] **Step 2: README row**

In `platforms/apple/README.md`'s `## Plugins` table, in plan-number order:

```markdown
| page-edit | `PageEditCapability` | `pageEdit: true` | `2026-10-06-swift-plugins/10-page-edit.md` |
```

- [ ] **Step 3: Mark the spec entry delivered**

In `docs/superpowers/plans/2026-10-01-swift-package.md` §A.4, change the start of the page-edit entry from `**page-edit** (D).` to `**page-edit** (D), delivered by 10-page-edit.md.`, and in Appendix C change the `pages.move` row's status from `B3` to `mapped (10-page-edit.md)`.

- [ ] **Step 4: Run the definition of done**

```bash
cargo test -p epdf-plugin-page-edit --features uniffi
cargo test -p epdf-ffi -p epdf-viewer-host
cargo clippy -p epdf-plugin-page-edit -p epdf-viewer-host --all-targets --features epdf-plugin-page-edit/uniffi -- -D warnings
cargo test -p epdf-plugin-page-edit
bash platforms/apple/build-xcframework.sh
(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17')
cargo build -p epdf-viewer-host -p uniffi-bindgen
./target/debug/uniffi-bindgen generate --library target/debug/libepdf_viewer_host.dylib --language kotlin --no-format --out-dir "$TMPDIR/epdf-kotlin-check"
```

Expected: every command passes.

- [ ] **Step 5: Commit, with the archive size**

```bash
before=$(cat "$TMPDIR/page-edit-size-before")
after=$(stat -f %z target/aarch64-apple-ios-sim/release/libepdf_viewer_host.a)
git add .github/workflows/plugin-rust.yml platforms/apple/README.md docs/superpowers/plans/2026-10-01-swift-package.md
git commit -m "List page-edit in CI and the Apple README" -m "libepdf_viewer_host.a (ios-sim, release): $before -> $after bytes"
```

- [ ] **Step 6: Merge into the integration branch**

In the integration worktree umbrella Task 0 created, which has `research/swift-plugins` checked out:

```bash
cd .claude/worktrees/swift-plugins-integration
git merge --no-ff research/swift-page-edit -m "Merge the page-edit Swift wrapper"
```

Resolve a conflict in a shared file by keeping both blocks in plan-number order (umbrella "Shared files").

- [ ] **Step 7: Rerun after the merge**

In the integration worktree:

```bash
bash platforms/apple/build-xcframework.sh
(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17')
cargo build -p epdf-viewer-host -p uniffi-bindgen
./target/debug/uniffi-bindgen generate --library target/debug/libepdf_viewer_host.dylib --language kotlin --no-format --out-dir "$TMPDIR/epdf-kotlin-check"
```

Expected: PASS.

- [ ] **Step 8: Remove the worktree and branch**

From the repository root. Neither command gets `--force`; if one refuses, find out what was left behind first.

```bash
git worktree remove .claude/worktrees/swift-page-edit
git branch -d research/swift-page-edit
git worktree prune
```

Then tick `10 page-edit` in the umbrella's Tasks 1 to 16 list.
