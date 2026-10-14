# Redaction Plugin Swift Wrapper Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Export `epdf-plugin-redaction` through UniFFI as `RedactionHandle`, wire it into `ViewerHost` over the native engine's `redaction.apply`, and wrap it in Swift as `RedactionCapability`, so a Swift app can mark regions, review the pending marks and apply them.

**Architecture:** The recipe in `2026-10-01-swift-package.md` Appendix A §A.0, one handle, all 20 members (`crates/plugins/redaction/src/harness.rs:29` `MEMBERS`). A mark is a `redact` annotation made through the annotation plugin, so records reuse plan 05's `epdf_ffi::AnnotationRef`. The apply result stays JSON inside the plugin (`contract.rs`: "the plugin never reads inside them"), so the FFI decodes it into a typed `RedactionApplyResult`. The native host spells the apply scope and result with page object numbers where the kernel uses `PageRef`s, so `translate.rs` rewrites both. The plugin decides whether apply is possible with `doc.has("redaction")`, a namespace the native engine's `maps` doesn't answer for today, which umbrella Task 0e's namespace rule in `maps` answers once `redaction.apply` is listed in `PATHS`.

**Tech Stack:** Rust 2024, `uniffi =0.32.0`, `serde_json`, Swift 6.2 tools, Swift Testing, `swift-custom-dump` 1.3, Xcode 27.0, iPhone 17 simulator.

**Spec:** `docs/superpowers/plans/2026-10-06-swift-plugins/00-umbrella.md` (this plan's row, cross-plan names, template, definition of done) and `docs/superpowers/plans/2026-10-01-swift-package.md` (Global Constraints, Appendix A §A.0 and §A.3, Appendix C). Read both.

## Global Constraints

Everything in the umbrella's Global Constraints applies. The ones this plugin hits hardest:

- `uniffi` is pinned at `=0.32.0`.
- UniFFI names are workspace-unique. Every record here starts with `Redaction`.
- No `usize`, `i64` sentinels or `serde_json::Value` across the boundary. `RedactionMark.page_index` (`i64`, `-1` for none) becomes `Option<u32>`. `RedactionCollateral.count` (`usize`) becomes `u32`. `overlay_text` (`Value`) becomes `Option<String>`.
- Requires plan 05 (`ViewerConfig.annotation`) and plan 04 (`epdf_ffi::AnnotationRef`, `flatten_refs`, `nest_refs`), which plan 05 waits for anyway, and therefore plan 01. The kernel rejects a redaction plugin without annotation.
- Applying is destructive and can't be undone. Every apply member's doc comment says "Irreversible", as the Rust does.

## Review Focus

1. `can_apply` on a native viewer. People expect `true` for an unencrypted document. The plugin asks `doc.has("redaction")` (`services/store.rs:75`), and `NativeDocument::has` answers `translate::maps(member)`. If `redaction.apply` is routed but missing from `PATHS`, the namespace check answers `false` and the apply buttons stay disabled. Pinned in Task 2 (`redaction_is_present_once_its_path_is_listed`) and Task 3 (`a_native_viewer_can_apply`).
2. Applying the marks on a page, then reading the page's text. People expect the marked text gone. Pinned in Task 3 (`applying_a_page_removes_its_marked_text`), which reads the text through the search plugin before and after.
3. An apply result whose scope is per page. People expect `RedactionApplyResult.scope` to name the pages they asked for. The host answers `pageObjectNumbers`, the kernel's shape is `pages: [PageRef]`, and a missing rewrite fails the decode with `invalid-state` after the irreversible apply has already happened. Pinned in Task 2 (`an_apply_result_answers_pages_and_nested_refs`).
4. Unmarking a ref that isn't a pending mark. People expect it in `skipped` with a reason, not a failure of the whole call. Pinned in Task 1 (`unmarking_a_non_mark_skips_it`).
5. A label edit that only changes the text. People expect the mark's colour and font to stay. The plugin carries the current `/DA` for that reason (`write/marks.rs` `update_label`); the FFI must not build its own patch that drops it. Pinned in Task 1 (`a_label_edit_goes_through_the_plugins_update_label`).

## Decisions

`mark_matches` takes the search plugin's own `SearchQuery` record (`crates/plugins/search/src/contract.rs:23`, already a `uniffi::Record` under search's `uniffi` feature), so the redaction crate's `uniffi` feature turns on `epdf-plugin-search/uniffi`. Swift passes the same `SearchQuery` it gives `SearchCapability`. The cost is a compile-time edge from redaction's FFI to search's; it already depends on the search crate.

`unmark` and `clear_pending` return the annotation plane's batch result, including the skipped entries the plugin adds. That's `RedactionUnmarkResult`, not plan 05's `AnnotationBatchResult`, because the plugin adds a `skipped` list with reasons that the annotation batch doesn't carry.

## File Structure

```
crates/plugins/redaction/Cargo.toml          + uniffi feature
crates/plugins/redaction/src/lib.rs          + scaffolding, pub mod ffi
crates/plugins/redaction/src/controller.rs   + lifetime()
crates/plugins/redaction/src/ffi.rs          RedactionHandle, records, RedactionEvent, listener
crates/plugins/redaction/tests/ffi.rs
crates/epdf-viewer-host/{Cargo.toml, src/api.rs, src/session.rs, src/lib.rs, src/translate.rs}
crates/epdf-viewer-host/tests/{translate.rs, viewer_host.rs}
platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Redaction.swift
platforms/apple/EmbedPDF/Tests/EmbedPDFTests/RedactionTests.swift
.github/workflows/plugin-rust.yml, platforms/apple/README.md
```

## Cross-plugin tests

| Optional integration | Behaviour | Test owner |
|---|---|---|
| selection (plan 03) | `mark_selection` marks the selected text, one mark per page | this plan if 03 merged first (`markingTheSelectionCreatesOneMarkPerPage` in Task 4), otherwise plan 03 |
| search (in the base package) | `mark_matches` marks every hit of a query | this plan, Task 1 (`marking_matches_creates_a_mark_per_hit`) and Task 3 |

---

### Task 1: Worktree and `RedactionHandle`

**Files:**
- Modify: `crates/plugins/redaction/Cargo.toml`
- Modify: `crates/plugins/redaction/src/lib.rs`
- Modify: `crates/plugins/redaction/src/controller.rs`
- Create: `crates/plugins/redaction/src/ffi.rs`
- Test: `crates/plugins/redaction/tests/ffi.rs`

**Interfaces:**
- Consumes: `epdf_ffi::{AnnotationRef, PageRef, Rect, KernelBound, KernelPost, CancelToken, Subscription, ViewerError}`; `epdf_plugin_search::SearchQuery` (a `uniffi::Record` under `epdf-plugin-search/uniffi`).
- Produces: `epdf_plugin_redaction::ffi::{RedactionHandle, RedactionConfig, RedactionOverlayText, RedactionMark, RedactionMarkKind, RedactionCollateral, RedactionLabelPatch, RedactionUnmarkResult, RedactionSkipped, RedactionApplyResult, RedactionApplyScope, RedactionPageResult, RedactionPageStatus, RedactionEvent, RedactionEventListener}`. `RedactionHandle::new(cap: RedactionCapability, post: KernelPost) -> Arc<RedactionHandle>`.

Members, under their FFI names (TypeScript name in parentheses where it differs): `pending` (`listPending`), `pending_item` (`getPending`), `pending_count` (`getPendingCount`), `estimate_collateral`, `mark_selection`, `mark_area`, `mark_page`, `mark_matches`, `unmark`, `clear_pending`, `update_label`, `apply`, `apply_pages`, `apply_all`, `can_mark`, `can_apply`, `is_applying`, `last_result` (`getLastResult`), `subscribe` (`onApplied`, `onPendingChanged`).

- [x] **Step 1: Create the worktree**

```bash
git worktree add .claude/worktrees/swift-redaction -b research/swift-redaction research/swift-plugins
cd .claude/worktrees/swift-redaction
```

Fetch PDFium for `darwin-arm64`, `ios-arm64` and `ios-sim-arm64` as `platforms/apple/README.md` describes. Confirm plan 05 has merged: `rg -n 'pub fn nest_refs' crates/epdf-viewer-host/src/translate.rs` prints a line.

- [x] **Step 2: Add the feature and the lifetime**

`crates/plugins/redaction/Cargo.toml`, in `[features]`:

```toml
# The uniffi exports (`ffi` module), for `epdf-viewer-host`. `mark_matches`
# takes search's `SearchQuery` record, so search's exports come along.
uniffi = ["dep:uniffi", "dep:epdf-ffi", "epdf-plugin-search/uniffi"]
```

`[dependencies]`:

```toml
epdf-ffi = { path = "../../epdf-ffi", optional = true }
uniffi = { version = "=0.32.0", optional = true }
```

`[dev-dependencies]` gets `futures = { version = "0.3", default-features = false, features = ["std", "executor"] }` and `plugin-conformance = { path = "../../plugin-conformance" }` if absent, plus `epdf-plugin-interaction = { path = "../interaction" }`, and:

```toml
[[test]]
name = "ffi"
required-features = ["uniffi"]
```

`src/lib.rs`, after `pub mod write;`:

```rust
#[cfg(feature = "uniffi")]
pub mod ffi;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();
```

`src/controller.rs`, inside `impl RedactionCapability`:

```rust
    /// The instance lifetime, aborted when the document closes. A foreign
    /// handle outlives the close, checks this before every call, and ends
    /// its event streams when it aborts.
    #[cfg(feature = "uniffi")]
    pub(crate) fn lifetime(&self) -> &epdf_kernel::signal::AbortSignal {
        self.0.cx.lifetime()
    }
```

- [x] **Step 3: Write the failing tests**

`crates/plugins/redaction/tests/ffi.rs`:

```rust
#![cfg(feature = "uniffi")]

use std::cell::RefCell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use epdf_ffi::{AnnotationRef, CancelToken, KernelThread, PageRef, Rect, ViewerError};
use epdf_plugin_annotation::{AnnotationConfig, annotation_plugin};
use epdf_plugin_interaction::{InteractionConfig, interaction_plugin};
use epdf_plugin_redaction::ffi::{
    RedactionApplyScope, RedactionEvent, RedactionEventListener, RedactionHandle,
    RedactionLabelPatch, RedactionMarkKind, RedactionPageStatus,
};
use epdf_plugin_redaction::{REDACTION, RedactionConfig, redaction_plugin};
use epdf_plugin_search::{SearchConfig, SearchQuery, search_plugin};
use futures::executor::block_on;
use plugin_conformance::test_kernel::{Reply, TestKernel, TestSpec};
use serde_json::{Value, json};

thread_local! {
    /// What the engine was sent, by path, on the kernel thread.
    static SENT: RefCell<Vec<(String, Value)>> = const { RefCell::new(Vec::new()) };
}

fn sent(thread: &KernelThread<TestKernel>, path: &str) -> Vec<Value> {
    let path = path.to_string();
    block_on(thread.call(move |_| {
        SENT.with(|s| s.borrow().iter().filter(|(p, _)| *p == path).map(|(_, v)| v.clone()).collect())
    }))
    .unwrap()
}

fn page(pon: u32) -> Value {
    json!({ "kind": "objectNumber", "pageObjectNumber": pon })
}

fn mark_dto(annot: u32) -> Value {
    json!({ "ref": { "kind": "objectNumber", "page": page(1), "annotObjectNumber": annot },
        "page": page(1), "index": annot, "identityQuality": "durable", "nm": null,
        "flags": { "invisible": false, "hidden": false, "print": true, "noZoom": false,
            "noRotate": false, "noView": false, "readOnly": false, "locked": false,
            "toggleNoView": false, "lockedContents": false },
        "rect": { "left": 100, "bottom": 600, "right": 200, "top": 700 },
        "contents": null, "subject": null, "author": null, "created": null, "modified": null,
        "blendMode": "normal", "inReplyTo": null, "replyType": null, "subtype": "redact",
        "overlayText": "CONFIDENTIAL", "quadPoints": [] })
}

fn spec(marks: Vec<Value>) -> TestSpec {
    let bx = json!({ "left": 0, "bottom": 0, "right": 600, "top": 800 });
    let snapshot = json!({ "pages": [{ "pageState": { "page": page(1) }, "annotations": marks }] });
    TestSpec::new(json!({ "documents": { "a": { "id": "a", "security": { "allows": "*" },
        "services": ["redaction"],
        "pages": [{ "index": 0, "ref": page(1), "label": null, "size": { "width": 600, "height": 800 },
            "rotation": 0, "userUnit": 1, "boxes": { "media": bx, "crop": bx } }] } } }))
        .respond("annotations.listRawAll", move |_| Ok(Reply::value(snapshot.clone())))
        .respond_page("page.annotations.create", |_, args| {
            SENT.with(|s| s.borrow_mut().push(("page.annotations.create".into(), args[0].clone())));
            Ok(Reply::value(json!({ "created": mark_dto(20) })))
        })
        .respond_page("page.annotations.update", |_, args| {
            SENT.with(|s| s.borrow_mut().push(("page.annotations.update".into(), json!(args))));
            Ok(Reply::value(json!({ "updated": mark_dto(5) })))
        })
        .respond_page("page.annotations.delete", |_, _| Ok(Reply::value(json!({}))))
        .respond("redaction.apply", |args| {
            SENT.with(|s| s.borrow_mut().push(("redaction.apply".into(), args[0].clone())));
            Ok(Reply::value(json!({
                "scope": args[0],
                "results": [{ "page": page(1), "status": "applied", "removedAnnotationCount": 1 }],
                "removedAnnotationCount": 1
            })))
        })
        .respond_page("search.query", |_, _| Ok(Reply::value(json!({ "matches": [], "done": true }))))
}

fn handle(marks: Vec<Value>) -> (KernelThread<TestKernel>, Arc<RedactionHandle>) {
    let thread = KernelThread::spawn(move |_| {
        TestKernel::open(
            vec![
                interaction_plugin(InteractionConfig::default()),
                annotation_plugin(AnnotationConfig::default()),
                search_plugin(SearchConfig::default()),
                redaction_plugin(RedactionConfig::default()),
            ],
            spec(marks),
        )
    })
    .unwrap();
    let post = thread.post();
    let handle =
        block_on(thread.call(move |k| RedactionHandle::new(k.capability(&REDACTION).unwrap(), post))).unwrap();
    // Hydrate the annotation plane the marks project from.
    block_on(thread.call(|k| {
        let annotation = k.capability(&epdf_plugin_annotation::ANNOTATION).unwrap();
        plugin_conformance::test_kernel::settle(&annotation.refresh())
    }))
    .unwrap()
    .unwrap();
    (thread, handle)
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

fn obj(annot: u32) -> AnnotationRef {
    AnnotationRef::ObjectNumber { page: PageRef { page_object_number: 1 }, annot_object_number: annot }
}

#[derive(Default)]
struct Events(Mutex<Vec<RedactionEvent>>, AtomicUsize);

impl RedactionEventListener for Events {
    fn on_event(&self, event: RedactionEvent) {
        self.0.lock().unwrap().push(event);
    }
    fn on_end(&self) {
        self.1.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn pending_marks_project_from_the_annotation_plane() {
    let (_thread, redaction) = handle(vec![mark_dto(5)]);
    let pending = block_on(redaction.pending(None)).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].annotation_ref, obj(5));
    assert_eq!(pending[0].page_index, Some(0));
    assert_eq!(pending[0].kind, RedactionMarkKind::Area);
    assert_eq!(pending[0].overlay_text.as_deref(), Some("CONFIDENTIAL"));
    assert_eq!(block_on(redaction.pending_count(None)).unwrap(), 1);
}

#[test]
fn marking_an_area_creates_a_redact_annotation() {
    let (thread, redaction) = handle(vec![]);
    let events = Arc::new(Events::default());
    let _sub = redaction.subscribe(events.clone());
    let mark = block_on(redaction.mark_area(
        PageRef { page_object_number: 1 },
        Rect { x: 10.0, y: 10.0, width: 100.0, height: 20.0 },
        token(),
    ))
    .unwrap();
    assert_eq!(mark, obj(20));
    let created = sent(&thread, "page.annotations.create");
    assert_eq!(created.last().unwrap()["subtype"], "redact");
    let seen = events.0.lock().unwrap();
    assert!(seen.iter().any(|e| matches!(e, RedactionEvent::PendingChanged { .. })), "{seen:?}");
}

#[test]
fn marking_matches_creates_a_mark_per_hit() {
    let (_thread, redaction) = handle(vec![]);
    // The fake's search answers no hits, so nothing is marked; the call
    // still reaches the search plugin and resolves.
    let marks = block_on(redaction.mark_matches(SearchQuery { text: "secret".into(), ..Default::default() }, None, token())).unwrap();
    assert!(marks.is_empty());
}

#[test]
fn unmarking_a_non_mark_skips_it() {
    let (_thread, redaction) = handle(vec![mark_dto(5)]);
    let result = block_on(redaction.unmark(vec![obj(5), obj(77)], token())).unwrap();
    assert_eq!(result.applied, vec![obj(5)]);
    assert_eq!(result.skipped.len(), 1);
    assert_eq!(result.skipped[0].annotation_ref, obj(77));
    assert_eq!(result.skipped[0].reason, "not a pending redaction mark");
}

#[test]
fn a_label_edit_goes_through_the_plugins_update_label() {
    let (thread, redaction) = handle(vec![mark_dto(5)]);
    block_on(redaction.update_label(
        obj(5),
        RedactionLabelPatch { overlay_text: Some("REDACTED".into()), clear_overlay_text: false, repeat: None },
        token(),
    ))
    .unwrap();
    let patch = sent(&thread, "page.annotations.update").last().unwrap()[1].clone();
    assert_eq!(patch["overlayText"], "REDACTED");
    // The plugin carries the mark's current styling with every label edit.
    assert!(patch.get("defaultAppearance").is_some() || patch.get("fontColor").is_some(), "{patch}");
}

#[test]
fn applying_a_page_answers_a_typed_result_and_is_observed() {
    let (thread, redaction) = handle(vec![mark_dto(5)]);
    let events = Arc::new(Events::default());
    let _sub = redaction.subscribe(events.clone());
    assert!(block_on(redaction.can_apply()).unwrap());
    let result = block_on(redaction.apply_pages(vec![PageRef { page_object_number: 1 }], token())).unwrap();
    assert_eq!(result.scope, RedactionApplyScope::Pages { pages: vec![PageRef { page_object_number: 1 }] });
    assert_eq!(result.results[0].status, RedactionPageStatus::Applied);
    assert_eq!(result.removed_annotation_count, 1);
    assert_eq!(sent(&thread, "redaction.apply").last().unwrap()["kind"], "pages");
    assert_eq!(block_on(redaction.last_result()).unwrap(), Some(result));
}

#[test]
fn applying_refs_that_are_not_pending_is_not_found() {
    let (_thread, redaction) = handle(vec![]);
    let error = block_on(redaction.apply(vec![obj(5)], token())).unwrap_err();
    assert_eq!(code(&error), Some("not-found"), "{error:?}");
}

#[test]
fn a_closed_documents_capability_refuses_with_instance_closed() {
    let (thread, redaction) = handle(vec![]);
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    for error in [
        block_on(redaction.pending(None)).map(|_| ()).unwrap_err(),
        block_on(redaction.can_apply()).map(|_| ()).unwrap_err(),
        block_on(redaction.apply_all(token())).map(|_| ()).unwrap_err(),
    ] {
        assert_eq!(code(&error), Some("instance-closed"), "{error:?}");
    }
}

#[test]
fn closing_the_document_ends_its_subscriptions() {
    let (thread, redaction) = handle(vec![]);
    let events = Arc::new(Events::default());
    let _sub = redaction.subscribe(events.clone());
    block_on(thread.call(|_| ())).unwrap();
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    assert_eq!(events.1.load(Ordering::SeqCst), 1);
}

#[test]
fn cancelling_an_apply_before_it_runs_applies_nothing() {
    let (thread, redaction) = handle(vec![mark_dto(5)]);
    let (release, gate) = std::sync::mpsc::channel::<()>();
    let (parked_tx, parked) = std::sync::mpsc::channel::<()>();
    let blocker = thread.call(move |_| {
        parked_tx.send(()).unwrap();
        gate.recv().unwrap();
    });
    parked.recv().unwrap();
    let cancel = token();
    let apply = redaction.apply_all(cancel.clone());
    cancel.cancel();
    release.send(()).unwrap();
    block_on(blocker).unwrap();
    let error = block_on(apply).unwrap_err();
    assert_eq!(code(&error), Some("operation-cancelled"), "{error:?}");
    assert!(sent(&thread, "redaction.apply").is_empty());
}
```

`pending_marks_project_from_the_annotation_plane` asserts `kind: Area` for a DTO with empty `quadPoints`. Read `crates/plugins/redaction/src/read/pending.rs` `regions_of` and the code that picks the kind before running: if the kind comes from another member, set that member in `mark_dto` instead of changing the assertion.

- [x] **Step 4: Run them to see them fail**

Run: `cargo test -p epdf-plugin-redaction --features uniffi --test ffi`
Expected: compile errors, `could not find ffi in epdf_plugin_redaction`.

- [x] **Step 5: Write `ffi.rs`**

```rust
//! The redaction plugin through uniffi: one handle over the two-stage
//! model (mark, then apply), typed records, one event enum. Every method
//! hops to the kernel thread through `KernelBound`.

use std::sync::Arc;

use epdf_annotation_core as core;
use epdf_ffi::{AnnotationRef, CancelToken, KernelBound, KernelPost, PageRef, Rect, Subscription, ViewerError};
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::{Error, Unsubscribe};
use epdf_plugin_search::SearchQuery;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::RedactionCapability;

fn decode<T: DeserializeOwned>(value: Value) -> Result<T, Error> {
    serde_json::from_value(value).map_err(|e| {
        Error::named("InvalidState", format!("[redaction] a value does not decode: {e}"))
            .with("code", "invalid-state")
    })
}

fn open(cap: &RedactionCapability) -> Result<&RedactionCapability, Error> {
    if cap.lifetime().aborted() {
        return Err(plugin_error(
            code::INSTANCE_CLOSED,
            "redaction",
            "the document this capability belongs to is closed",
            None,
            None,
        ));
    }
    Ok(cap)
}

fn ref_out(r: core::AnnotationRef) -> Result<AnnotationRef, Error> {
    AnnotationRef::try_from(r)
}

fn refs_out(refs: Vec<core::AnnotationRef>) -> Result<Vec<AnnotationRef>, Error> {
    refs.into_iter().map(ref_out).collect()
}

fn refs_in(refs: Vec<AnnotationRef>) -> Vec<core::AnnotationRef> {
    refs.into_iter().map(Into::into).collect()
}

fn json_ref(value: &Value) -> Result<AnnotationRef, Error> {
    ref_out(decode(value.clone())?)
}

// ── config ───────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct RedactionOverlayText {
    /// CSS hex colour of the label. White when `None`.
    #[uniffi(default = None)]
    pub color: Option<String>,
    #[uniffi(default = None)]
    pub font_family: Option<String>,
    /// Points; 0 fits the label to the region.
    #[uniffi(default = None)]
    pub font_size: Option<f64>,
}

/// Defaults for marks this plugin creates.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct RedactionConfig {
    /// CSS hex colour painted over the region on apply. Black when `None`.
    #[uniffi(default = None)]
    pub fill: Option<String>,
    #[uniffi(default = None)]
    pub overlay_text: Option<RedactionOverlayText>,
}

impl From<RedactionConfig> for crate::RedactionConfig {
    fn from(c: RedactionConfig) -> Self {
        let overlay = (c.fill.is_some() || c.overlay_text.is_some()).then(|| crate::RedactionOverlay {
            fill: c.fill,
            text: c.overlay_text.map(|t| crate::RedactionOverlayText {
                color: t.color,
                font_family: t.font_family,
                font_size: t.font_size,
            }),
        });
        crate::RedactionConfig { overlay, ..Default::default() }
    }
}

// ── marks ────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RedactionMarkKind {
    /// A rectangle (a marquee, a whole page).
    Area,
    /// Per-line quads over text (a selection, a search hit).
    Text,
}

/// One pending mark: a live projection of a `redact` annotation. Delete the
/// annotation and the mark is gone.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RedactionMark {
    pub annotation_ref: AnnotationRef,
    pub page: PageRef,
    /// Display index; `None` when the page registry lacks the page.
    pub page_index: Option<u32>,
    pub kind: RedactionMarkKind,
    /// Page space.
    pub bounds: Rect,
    /// `/OverlayText`, when set.
    pub overlay_text: Option<String>,
}

impl TryFrom<crate::RedactionMark> for RedactionMark {
    type Error = Error;

    fn try_from(m: crate::RedactionMark) -> Result<Self, Error> {
        Ok(RedactionMark {
            annotation_ref: ref_out(m.ref_)?,
            page: m.page.into(),
            page_index: u32::try_from(m.page_index).ok(),
            kind: match m.kind {
                crate::RedactionMarkKind::Area => RedactionMarkKind::Area,
                crate::RedactionMarkKind::Text => RedactionMarkKind::Text,
            },
            bounds: m.bounds.into(),
            overlay_text: m.overlay_text.as_ref().and_then(Value::as_str).map(str::to_string),
        })
    }
}

fn marks_out(list: Vec<crate::RedactionMark>) -> Result<Vec<RedactionMark>, Error> {
    list.into_iter().map(RedactionMark::try_from).collect()
}

/// What an apply would destroy beyond the marks: a client-side estimate.
/// The apply result is authoritative.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RedactionCollateral {
    pub count: u32,
    pub refs: Vec<AnnotationRef>,
}

/// A label edit. `overlay_text` sets the label, `clear_overlay_text` removes
/// it (setting both is refused), and `repeat` tiles it across the region.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct RedactionLabelPatch {
    #[uniffi(default = None)]
    pub overlay_text: Option<String>,
    #[uniffi(default = false)]
    pub clear_overlay_text: bool,
    #[uniffi(default = None)]
    pub repeat: Option<bool>,
}

impl TryFrom<RedactionLabelPatch> for crate::RedactionLabelPatch {
    type Error = Error;

    fn try_from(p: RedactionLabelPatch) -> Result<Self, Error> {
        use core::js::Nullable;
        let overlay_text = match (p.overlay_text, p.clear_overlay_text) {
            (Some(_), true) => {
                return Err(plugin_error(
                    code::INVALID_INPUT,
                    "redaction",
                    "a label patch both sets and clears the overlay text",
                    None,
                    None,
                ));
            }
            (Some(text), false) => Nullable::Value(text),
            (None, true) => Nullable::Null,
            (None, false) => Nullable::Undefined,
        };
        Ok(crate::RedactionLabelPatch { overlay_text, repeat: p.repeat })
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RedactionSkipped {
    pub annotation_ref: AnnotationRef,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RedactionUnmarkFailure {
    pub annotation_ref: AnnotationRef,
    pub message: String,
}

/// What `unmark` and `clear_pending` did: marks removed, refs that weren't
/// pending marks, and deletes that failed.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RedactionUnmarkResult {
    pub applied: Vec<AnnotationRef>,
    pub skipped: Vec<RedactionSkipped>,
    pub failed: Vec<RedactionUnmarkFailure>,
}

impl RedactionUnmarkResult {
    fn from_json(value: &Value) -> Result<Self, Error> {
        let list = |key: &str| value.get(key).and_then(Value::as_array).cloned().unwrap_or_default();
        Ok(RedactionUnmarkResult {
            applied: list("applied").iter().map(json_ref).collect::<Result<_, _>>()?,
            skipped: list("skipped")
                .iter()
                .map(|s| {
                    Ok(RedactionSkipped {
                        annotation_ref: json_ref(&s["ref"])?,
                        reason: s.get("reason").and_then(Value::as_str).unwrap_or_default().into(),
                    })
                })
                .collect::<Result<_, Error>>()?,
            failed: list("failed")
                .iter()
                .map(|f| {
                    Ok(RedactionUnmarkFailure {
                        annotation_ref: json_ref(&f["ref"])?,
                        message: f["error"].get("message").and_then(Value::as_str).unwrap_or_default().into(),
                    })
                })
                .collect::<Result<_, Error>>()?,
        })
    }
}

// ── apply ────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum RedactionApplyScope {
    /// Every mark on each page.
    Pages { pages: Vec<PageRef> },
    /// Exactly these marks.
    Annotations { refs: Vec<AnnotationRef> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, uniffi::Enum)]
#[serde(rename_all = "camelCase")]
pub enum RedactionPageStatus {
    Applied,
    /// The page had no marks, which is not a failure.
    Unchanged,
    Failed,
    /// Not attempted: an earlier page failed.
    Skipped,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RedactionPageResult {
    pub page: PageRef,
    pub status: RedactionPageStatus,
    /// Annotations removed alongside the marks (popups, detached widgets),
    /// not the marks themselves.
    pub removed_annotation_count: u32,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RedactionApplyResult {
    pub scope: RedactionApplyScope,
    pub results: Vec<RedactionPageResult>,
    pub removed_annotation_count: u32,
}

impl RedactionApplyResult {
    /// The kernel's spelling (`engine-core`'s `RedactionApplyResult`), which
    /// `translate.rs` turns the host's answer into.
    pub(crate) fn from_json(value: &Value) -> Result<Self, Error> {
        #[derive(Deserialize)]
        #[serde(tag = "kind", rename_all = "camelCase")]
        enum Scope {
            Pages { pages: Vec<core::PageRef> },
            Annotations { refs: Vec<core::AnnotationRef> },
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Item {
            page: core::PageRef,
            status: RedactionPageStatus,
            removed_annotation_count: u32,
            #[serde(default)]
            error: Option<String>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Dto {
            scope: Scope,
            results: Vec<Item>,
            removed_annotation_count: u32,
        }
        let dto: Dto = decode(value.clone())?;
        Ok(RedactionApplyResult {
            scope: match dto.scope {
                Scope::Pages { pages } => RedactionApplyScope::Pages { pages: pages.into_iter().map(Into::into).collect() },
                Scope::Annotations { refs } => RedactionApplyScope::Annotations { refs: refs_out(refs)? },
            },
            results: dto
                .results
                .into_iter()
                .map(|i| RedactionPageResult {
                    page: i.page.into(),
                    status: i.status,
                    removed_annotation_count: i.removed_annotation_count,
                    error: i.error,
                })
                .collect(),
            removed_annotation_count: dto.removed_annotation_count,
        })
    }
}

fn applied(value: Value) -> Result<RedactionApplyResult, ViewerError> {
    RedactionApplyResult::from_json(&value).map_err(Into::into)
}

// ── events ───────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum RedactionEvent {
    /// A confirmed apply, this session's or a remote one.
    Applied { result: RedactionApplyResult },
    /// Marks were created, changed or removed on these pages.
    PendingChanged { pages: Vec<PageRef> },
}

#[uniffi::export(with_foreign)]
pub trait RedactionEventListener: Send + Sync {
    fn on_event(&self, event: RedactionEvent);
    /// The document closed: no event follows. Called once, and at once for
    /// a subscription made after the close.
    fn on_end(&self);
}

// ── the handle ───────────────────────────────────────────────────────────────

#[derive(uniffi::Object)]
pub struct RedactionHandle {
    cap: KernelBound<RedactionCapability>,
    post: KernelPost,
}

impl RedactionHandle {
    /// On the kernel thread.
    pub fn new(cap: RedactionCapability, post: KernelPost) -> Arc<Self> {
        Arc::new(RedactionHandle { cap: KernelBound::new(cap, post.clone()), post })
    }
}

#[uniffi::export]
impl RedactionHandle {
    /// Pending marks, or those on one page.
    pub async fn pending(&self, page: Option<PageRef>) -> Result<Vec<RedactionMark>, ViewerError> {
        self.cap
            .call(move |c| {
                let filter = crate::RedactionMarkFilter { page: page.map(Into::into) };
                marks_out(open(c)?.list_pending(Some(&filter)))
            })
            .await
    }

    pub async fn pending_item(&self, annotation_ref: AnnotationRef) -> Result<Option<RedactionMark>, ViewerError> {
        self.cap
            .call(move |c| open(c)?.get_pending(&annotation_ref.into()).map(RedactionMark::try_from).transpose())
            .await
    }

    pub async fn pending_count(&self, page: Option<PageRef>) -> Result<u32, ViewerError> {
        self.cap
            .call(move |c| {
                let page = page.map(core::PageRef::from);
                Ok(u32::try_from(open(c)?.get_pending_count(page.as_ref())).unwrap_or(u32::MAX))
            })
            .await
    }

    /// What applying `refs` (every pending mark when `None`) would also
    /// remove.
    pub async fn estimate_collateral(&self, refs: Option<Vec<AnnotationRef>>) -> Result<RedactionCollateral, ViewerError> {
        self.cap
            .call(move |c| {
                let refs = refs.map(refs_in);
                let estimate = open(c)?.estimate_collateral(refs.as_deref());
                Ok(RedactionCollateral {
                    count: u32::try_from(estimate.count).unwrap_or(u32::MAX),
                    refs: estimate.refs.iter().map(json_ref).collect::<Result<_, _>>()?,
                })
            })
            .await
    }

    /// Mark the current text selection, one mark per page, and clear it.
    /// Empty without a selection; `unsupported` without the selection plugin.
    pub async fn mark_selection(&self, cancel: Arc<CancelToken>) -> Result<Vec<AnnotationRef>, ViewerError> {
        self.cap
            .call_promise_cancellable(cancel, |c| Ok(open(c)?.mark_selection()), |refs| refs_out(refs).map_err(ViewerError::from))
            .await?
    }

    /// Mark a page-space rectangle.
    pub async fn mark_area(&self, page: PageRef, bounds: Rect, cancel: Arc<CancelToken>) -> Result<AnnotationRef, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let bounds = json!({ "x": bounds.x, "y": bounds.y, "width": bounds.width, "height": bounds.height });
                    open(c)?.mark_area(&page.into(), bounds)
                },
                |r| ref_out(r).map_err(ViewerError::from),
            )
            .await?
    }

    pub async fn mark_page(&self, page: PageRef, cancel: Arc<CancelToken>) -> Result<AnnotationRef, ViewerError> {
        self.cap
            .call_promise_cancellable(cancel, move |c| open(c)?.mark_page(&page.into()), |r| ref_out(r).map_err(ViewerError::from))
            .await?
    }

    /// Mark every hit of `query`, or only those on `pages`.
    pub async fn mark_matches(&self, query: SearchQuery, pages: Option<Vec<PageRef>>, cancel: Arc<CancelToken>) -> Result<Vec<AnnotationRef>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.mark_matches(query, pages.map(|p| p.into_iter().map(Into::into).collect()))),
                |refs| refs_out(refs).map_err(ViewerError::from),
            )
            .await?
    }

    /// Remove marks. A ref that isn't a pending mark is skipped, not failed.
    pub async fn unmark(&self, refs: Vec<AnnotationRef>, cancel: Arc<CancelToken>) -> Result<RedactionUnmarkResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.unmark(refs_in(refs))),
                |v| RedactionUnmarkResult::from_json(&v).map_err(ViewerError::from),
            )
            .await?
    }

    pub async fn clear_pending(&self, cancel: Arc<CancelToken>) -> Result<RedactionUnmarkResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                |c| Ok(open(c)?.clear_pending()),
                |v| RedactionUnmarkResult::from_json(&v).map_err(ViewerError::from),
            )
            .await?
    }

    /// Edit a mark's label. The mark keeps its current styling.
    pub async fn update_label(&self, annotation_ref: AnnotationRef, patch: RedactionLabelPatch, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(cancel, move |c| Ok(open(c)?.update_label(annotation_ref.into(), patch.try_into()?)), |()| ())
            .await
    }

    /// Apply these pending marks. Irreversible.
    pub async fn apply(&self, refs: Vec<AnnotationRef>, cancel: Arc<CancelToken>) -> Result<RedactionApplyResult, ViewerError> {
        self.cap
            .call_promise_cancellable(cancel, move |c| Ok(open(c)?.apply(&refs_in(refs))), applied)
            .await?
    }

    /// Apply every mark on these pages. Irreversible.
    pub async fn apply_pages(&self, pages: Vec<PageRef>, cancel: Arc<CancelToken>) -> Result<RedactionApplyResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let pages: Vec<core::PageRef> = pages.into_iter().map(Into::into).collect();
                    Ok(open(c)?.apply_pages(&pages))
                },
                applied,
            )
            .await?
    }

    /// Apply every redaction in the document, including marks on pages this
    /// session never loaded. Irreversible.
    pub async fn apply_all(&self, cancel: Arc<CancelToken>) -> Result<RedactionApplyResult, ViewerError> {
        self.cap.call_promise_cancellable(cancel, |c| Ok(open(c)?.apply_all()), applied).await?
    }

    pub async fn can_mark(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.can_mark())).await
    }

    /// Whether the engine can apply and the session holds every capability
    /// the apply asserts.
    pub async fn can_apply(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.can_apply())).await
    }

    pub async fn is_applying(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.is_applying())).await
    }

    pub async fn last_result(&self) -> Result<Option<RedactionApplyResult>, ViewerError> {
        self.cap
            .call(|c| open(c)?.get_last_result().as_ref().map(RedactionApplyResult::from_json).transpose())
            .await
    }

    pub fn subscribe(&self, listener: Arc<dyn RedactionEventListener>) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.cap.fire(move |c| {
            let ending = listener.clone();
            slot.fill_until_end(
                c.lifetime(),
                || {
                    let l = listener.clone();
                    let applied = c.on_applied(move |e| {
                        l.on_event(RedactionEvent::Applied { result: RedactionApplyResult::from_json(&e.result)? });
                        Ok(())
                    });
                    let l = listener.clone();
                    let pending = c.on_pending_changed(move |e| {
                        l.on_event(RedactionEvent::PendingChanged { pages: e.pages.iter().map(|p| (*p).into()).collect() });
                        Ok(())
                    });
                    Unsubscribe::new(move || {
                        applied.call();
                        pending.call();
                    })
                },
                move || ending.on_end(),
            );
        });
        subscription
    }
}
```

`RedactionAppliedEvent.result` may be the whole `redaction.applied` event rather than the result inside it (`contract.rs`: "`RedactionApplyResult` from `engine-core`, or the `redaction.applied` event that carries one"). In the `applied` hook, decode `e.result.get("result").unwrap_or(&e.result)` if the event shape is the wrapper; `rg -n 'RedactionAppliedEvent \{' crates/plugins/redaction/src` shows which value the plugin passes.

Add `epdf-annotation-core = { path = "../annotation-core", default-features = false }` is already in the crate's `[dependencies]`; nothing to add there.

- [x] **Step 6: Run the tests**

Run: `cargo test -p epdf-plugin-redaction --features uniffi --test ffi`
Expected: 10 passed.

If `a_label_edit_goes_through_the_plugins_update_label` fails on the styling assertion, read the patch the plugin built (print it) and assert on the key that carries the current `/DA` there. The point of the test is that the FFI calls `update_label` and not `annotation().update`.

Run: `cargo test -p epdf-plugin-redaction`
Expected: the conformance vectors still pass with the feature off.

- [x] **Step 7: Commit**

```bash
git add crates/plugins/redaction
git commit -m "Export the redaction plugin through uniffi"
```

---

### Task 2: Engine path: `redaction.apply`

**Files:**
- Modify: `crates/epdf-viewer-host/src/translate.rs`
- Test: `crates/epdf-viewer-host/tests/translate.rs`

**Interfaces:**
- Consumes: plan 04's `flatten_refs`, `nest_refs`; the host kind at `crates/epdf-engine/src/host.rs:923` (`redaction.apply`, reading `scope` as `RedactionScope`, `crates/epdf-engine/src/features/redaction.rs:40`).
- Consumes: umbrella Task 0e's `PATHS` and the namespace rule in `maps`.
- Produces: the `redaction.apply` route; `Decode::Redaction`.

The kernel sends `{kind: "pages", pages: [PageRef]}` or `{kind: "annotations", refs: [<nested ref>]}` (`crates/plugins/redaction/src/write/apply.rs:65-96`). The host reads `{kind: "pages", pageObjectNumbers: [n]}` or `{kind: "annotations", refs: [<flat ref>]}`. Its answer (`RedactionApplyResult`, `redaction.rs:75`) has the scope in the host's spelling and `results[].pageObjectNumber`; the kernel's spelling, which the vectors pin (`tooling/plugin-conformance/vectors/plugins/redaction.json`), is the scope as sent and `results[].page: PageRef`.

- [x] **Step 1: Write the failing tests**

Append to `tests/translate.rs`:

```rust
#[test]
fn redaction_is_present_once_its_path_is_listed() {
    assert!(epdf_viewer_host::translate::maps("redaction"));
    assert!(epdf_viewer_host::translate::maps("redaction.apply"));
    assert!(!epdf_viewer_host::translate::maps("redact"));
    assert!(!epdf_viewer_host::translate::maps("signatures"));
}

#[test]
fn a_pages_scope_goes_out_as_object_numbers() {
    let scope = json!({ "kind": "pages", "pages": [
        { "kind": "objectNumber", "pageObjectNumber": 3 },
        { "kind": "objectNumber", "pageObjectNumber": 7 } ] });
    let call = translate("d1", "redaction.apply", None, &[scope], 30).unwrap();
    assert_eq!(
        call.request,
        json!({ "kind": "redaction.apply", "jobId": 30, "docId": "d1",
            "scope": { "kind": "pages", "pageObjectNumbers": [3, 7] } })
    );
}

#[test]
fn an_annotations_scope_goes_out_with_flat_refs() {
    let scope = json!({ "kind": "annotations", "refs": [{ "kind": "objectNumber",
        "page": { "kind": "objectNumber", "pageObjectNumber": 3 }, "annotObjectNumber": 17 }] });
    let call = translate("d1", "redaction.apply", None, &[scope], 31).unwrap();
    assert_eq!(
        call.request["scope"],
        json!({ "kind": "annotations", "refs": [{ "kind": "objectNumber", "pageObjectNumber": 3, "annotObjectNumber": 17 }] })
    );
}

#[test]
fn an_apply_result_answers_pages_and_nested_refs() {
    let json = r#"{"result":{"result":{"scope":{"kind":"pages","pageObjectNumbers":[3]},"results":[{"pageObjectNumber":3,"status":"applied","removedAnnotationCount":2}],"removedAnnotationCount":2},"mutationSequence":4,"event":{"type":"redaction.applied","result":{"scope":{"kind":"pages","pageObjectNumbers":[3]},"results":[{"pageObjectNumber":3,"status":"applied","removedAnnotationCount":2}],"removedAnnotationCount":2}}}}"#;
    let call = translate("d1", "redaction.apply", None, &[json!({ "kind": "pages", "pages": [] })], 32).unwrap();
    let decoded = decode(&call.decode, answer(json)).unwrap();
    let page3 = json!({ "kind": "objectNumber", "pageObjectNumber": 3 });
    assert_eq!(decoded.value["scope"], json!({ "kind": "pages", "pages": [page3.clone()] }));
    assert_eq!(decoded.value["results"][0]["page"], page3);
    assert!(decoded.value["results"][0].get("pageObjectNumber").is_none());
    assert_eq!(decoded.event.unwrap()["result"]["results"][0]["page"], page3);
}
```

Check the event's shape against the host before trusting the last assertion: `crate::session::save::DocumentEventInit::RedactionApplied(value(&result)?)` builds it, and `rg -n 'RedactionApplied' -A8 crates/epdf-engine/src/session/save.rs` shows whether the result sits under `result` or is spread into the event. Match the test JSON to that.

- [x] **Step 2: Run them to see them fail**

Run: `cargo test -p epdf-viewer-host --test translate`
Expected: the new tests fail, `maps("redaction")` answering `false` and `redaction.apply` answering `unsupported`.

- [x] **Step 3: List the path**

Add to `PATHS` in `translate.rs` (umbrella Task 0e), one line in plan order:

```rust
    "redaction.apply",
```

`maps` derives namespaces from `PATHS`, so `maps("redaction")` answers `true` once this line and Step 4's route are in, and `every_routed_path_is_listed` checks the two agree.

- [x] **Step 4: Add the route and decoder**

`Decode`:

```rust
    /// A redaction apply: the inner result and event, the scope and per-page
    /// results in the kernel's spelling.
    Redaction,
```

`route`:

```rust
        "redaction.apply" => ("redaction.apply", Decode::Redaction),
```

`translate`'s `extra`:

```rust
        "redaction.apply" => vec![("scope", redaction_scope_out(arg(0))?)],
```

Helpers:

```rust
/// The kernel's apply scope as `RedactionScope` reads it.
fn redaction_scope_out(mut scope: Value) -> Result<Value, Error> {
    match scope.get("kind").and_then(Value::as_str) {
        Some("pages") => {
            let pages = scope.get("pages").and_then(Value::as_array).cloned().unwrap_or_default();
            let numbers = pages
                .iter()
                .map(|p| {
                    PageRef::from_json(p).map(|p| Value::from(p.page_object_number)).ok_or_else(|| {
                        coded("invalid-input", format!("[epdf-viewer-host] a redaction scope page that is not a PageRef: {p}"))
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(serde_json::json!({ "kind": "pages", "pageObjectNumbers": numbers }))
        }
        Some("annotations") => {
            flatten_refs(&mut scope);
            Ok(scope)
        }
        _ => Err(coded("invalid-input", format!("[epdf-viewer-host] not a redaction scope: {scope}"))),
    }
}

/// A host `RedactionApplyResult` in the kernel's spelling.
fn redaction_result_in(result: &mut Value) {
    if let Some(scope) = result.get_mut("scope") {
        if let Some(Value::Array(numbers)) = scope.as_object_mut().and_then(|m| m.remove("pageObjectNumbers")) {
            let pages: Vec<Value> = numbers
                .iter()
                .filter_map(Value::as_u64)
                .map(|n| PageRef::new(n as u32).to_json())
                .collect();
            scope["pages"] = Value::Array(pages);
        }
    }
    if let Some(Value::Array(items)) = result.get_mut("results") {
        for item in items {
            if let Some(pon) = item.as_object_mut().and_then(|m| m.remove("pageObjectNumber")).and_then(|p| p.as_u64()) {
                item["page"] = PageRef::new(pon as u32).to_json();
            }
        }
    }
    nest_refs(result);
}
```

`decode`:

```rust
        Decode::Redaction => {
            let mut value = take(&mut result, "result")?;
            redaction_result_in(&mut value);
            let mut event = result.remove("event");
            if let Some(event) = event.as_mut() {
                match event.get_mut("result") {
                    Some(inner) => redaction_result_in(inner),
                    None => redaction_result_in(event),
                }
            }
            return Ok(Decoded { value, event });
        }
```

- [x] **Step 5: Run the tests**

Run: `cargo test -p epdf-viewer-host --test translate`
Expected: all pass.

- [x] **Step 6: Commit**

```bash
git add crates/epdf-viewer-host/src/translate.rs crates/epdf-viewer-host/tests/translate.rs
git commit -m "Map redaction.apply onto the native host"
```

---

### Task 3: Host wiring

**Files:**
- Modify: `crates/epdf-viewer-host/Cargo.toml`
- Modify: `crates/epdf-viewer-host/src/api.rs`
- Modify: `crates/epdf-viewer-host/src/session.rs`
- Modify: `crates/epdf-viewer-host/src/lib.rs`
- Test: `crates/epdf-viewer-host/tests/viewer_host.rs`

**Interfaces:**
- Consumes: `epdf_plugin_redaction::ffi::{RedactionHandle, RedactionConfig}`.
- Produces: `ViewerConfig.redaction: Option<RedactionConfig>`; `ViewerHost::redaction(document_id: String) -> Result<Arc<RedactionHandle>, ViewerError>`.

The umbrella's cross-plan table lists `redaction: bool`. This plan uses `Option<RedactionConfig>` because the overlay fill colour and label font are the only things a native app sets, and a `bool` can't carry them. Change the umbrella's row in this task's commit, as the umbrella allows.

- [x] **Step 1: Write the failing tests**

Append to `tests/viewer_host.rs`:

```rust
use epdf_plugin_redaction::ffi::{RedactionConfig, RedactionPageStatus};
use epdf_plugin_search::ffi::SearchQuery as SearchQueryRecord;

fn with_redaction() -> Arc<ViewerHost> {
    let config = ViewerConfig {
        interaction: true,
        annotation: Some(epdf_plugin_annotation::ffi::AnnotationConfig::default()),
        search: Some(SearchConfig::default()),
        redaction: Some(RedactionConfig::default()),
        ..Default::default()
    };
    let host = ViewerHost::new(config, Arc::new(Delegate)).unwrap();
    block_on(host.start()).unwrap();
    host
}

#[test]
fn a_native_viewer_can_apply() {
    let host = with_redaction();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options())).unwrap();
    let redaction = block_on(host.redaction(doc.id)).unwrap();
    assert!(block_on(redaction.can_apply()).unwrap());
    assert!(block_on(redaction.can_mark()).unwrap());
}

#[test]
fn applying_a_page_removes_its_marked_text() {
    let host = with_redaction();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options())).unwrap();
    let search = block_on(host.search(doc.id.clone())).unwrap();
    let annotations = block_on(host.annotation(doc.id.clone())).unwrap();
    let redaction = block_on(host.redaction(doc.id)).unwrap();
    block_on(annotations.refresh(CancelToken::new())).unwrap();
    let query = || SearchQueryRecord { text: "Report".into(), ..Default::default() };
    let before = block_on(search.find_all(query(), None, CancelToken::new())).unwrap();
    assert!(!before.is_empty(), "report.pdf contains \"Report\"");
    let page = before[0].page;
    let marks = block_on(redaction.mark_matches(query(), Some(vec![page]), CancelToken::new())).unwrap();
    assert!(!marks.is_empty());
    let result = block_on(redaction.apply_pages(vec![page], CancelToken::new())).unwrap();
    assert_eq!(result.results[0].status, RedactionPageStatus::Applied);
    let after = block_on(search.find_all(query(), None, CancelToken::new())).unwrap();
    assert!(after.iter().all(|hit| hit.page != page), "{after:?}");
    assert!(block_on(redaction.pending(Some(page))).unwrap().is_empty());
}

#[test]
fn without_its_config_the_redaction_plugin_is_not_installed() {
    let host = started();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options())).unwrap();
    assert!(block_on(host.redaction(doc.id)).is_err());
}
```

`SearchHit.page` and `find_all`'s signature come from `crates/plugins/search/src/ffi.rs`; check with `rg -n 'pub async fn find_all' -A6 crates/plugins/search/src/ffi.rs` and adjust the argument list to it.

- [x] **Step 2: Run them to see them fail**

Run: `cargo test -p epdf-viewer-host --test viewer_host`
Expected: compile errors, `no field redaction on ViewerConfig`.

- [x] **Step 3: Wire it in**

`Cargo.toml`:

```toml
epdf-plugin-redaction = { path = "../plugins/redaction", default-features = false, features = ["uniffi"] }
```

`src/api.rs`, after plan 06's `form` field:

```rust
    /// Install `redactionPlugin(config)`: mark regions, then apply them.
    /// Needs `annotation` (and so `interaction`); marks from search hits
    /// need `search`, marks from the text selection need `selection`.
    #[uniffi(default = None)]
    pub redaction: Option<epdf_plugin_redaction::ffi::RedactionConfig>,
```

`src/session.rs`, after the form block:

```rust
        if let Some(redaction) = config.redaction {
            plugins.push(epdf_plugin_redaction::redaction_plugin(redaction.into()));
        }
```

`src/lib.rs`: `use epdf_plugin_redaction::ffi::RedactionHandle;` and, after `form`:

```rust
    /// The redaction capability of an open document. Needs
    /// `ViewerConfig.redaction` and `ViewerConfig.annotation`.
    pub async fn redaction(&self, document_id: String) -> Result<Arc<RedactionHandle>, ViewerError> {
        let post = self.post.clone();
        self.call(move |s| {
            let cap = s.kernel.capability(&epdf_plugin_redaction::REDACTION, Some(&document_id))?;
            Ok(RedactionHandle::new(cap, post))
        })
        .await
    }
```

In `docs/superpowers/plans/2026-10-06-swift-plugins/00-umbrella.md`, change row 07's `ViewerConfig` field from `redaction: bool` to `redaction: Option<RedactionConfig>`.

- [x] **Step 4: Run the tests**

Run: `cargo test -p epdf-viewer-host --test viewer_host`
Expected: all pass. If `applying_a_page_removes_its_marked_text` still finds the text, the engine applied nothing: check `result.results[0].status` first (an `Unchanged` means the marks never reached the page, so look at the `page.annotations.create` translation), then check that `find_all` doesn't serve a cached text layer (the search plugin re-reads on the `redaction.applied` event; if it doesn't, `search.refresh` before the second `find_all` and say so in the test).

Run: `cargo test -p epdf-ffi -p epdf-viewer-host`
Expected: all pass.

- [x] **Step 5: Commit**

```bash
git add crates/epdf-viewer-host docs/superpowers/plans/2026-10-06-swift-plugins/00-umbrella.md
git commit -m "Install the redaction plugin in the viewer host

The umbrella's row 07 said redaction: bool. The overlay fill and label
font are the plugin's only settings and a native app sets them, so the
field is Option<RedactionConfig>."
```

---

### Task 4: Swift capability

**Files:**
- Create: `platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Redaction.swift`
- Create: `platforms/apple/EmbedPDF/Tests/EmbedPDFTests/RedactionTests.swift`

**Interfaces:**
- Consumes: the generated `RedactionHandle`, records and `RedactionEventListener`; `SearchQuery` (already re-exported by `Search.swift`); `PDFViewer`, `bridged`, `bridgedCancellable`, `EventRelay`.
- Produces: `RedactionCapability`, `PDFViewer.redaction(for:)`.

- [x] **Step 1: Build the bindings**

Run: `bash platforms/apple/build-xcframework.sh`
Expected: `rg -n 'class RedactionHandle' platforms/apple/EmbedPDF/Sources/EmbedPDFBindings` finds it.

- [x] **Step 2: Write the failing tests**

`platforms/apple/EmbedPDF/Tests/EmbedPDFTests/RedactionTests.swift`:

```swift
import CustomDump
import EmbedPDF
import Foundation
import Testing

extension BaseSuite {
  @Suite struct RedactionTests {
    func opened() async throws -> (PDFViewer, PDFDocument, RedactionCapability, SearchCapability) {
      let viewer = try PDFViewer(
        ViewerConfig(
          interaction: true, search: SearchConfig(), annotation: AnnotationConfig(),
          redaction: RedactionConfig()))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      try await viewer.annotations(for: document).refresh()
      return (viewer, document, try await viewer.redaction(for: document), try await viewer.search(in: document))
    }

    @Test func markingAnAreaIsPendingAndObserved() async throws {
      let (viewer, document, redaction, _) = try await opened()
      let page = try await viewer.pages(of: document)[0].page
      let events = redaction.events
      let mark = try await redaction.markArea(Rect(x: 72, y: 72, width: 200, height: 20), on: page)
      let pending = try await redaction.pending(on: page)
      expectNoDifference(pending.map(\.annotationRef), [mark])
      for await event in events {
        if case .pendingChanged = event { break }
      }
      _ = try await redaction.unmark([mark])
      let after = try await redaction.pendingCount(on: page)
      expectNoDifference(after, 0)
    }

    @Test func applyingMatchesRemovesTheText() async throws {
      let (_, _, redaction, search) = try await opened()
      #expect(try await redaction.canApply())
      let before = try await search.findAll(SearchQuery(text: "Report"))
      let page = try #require(before.first?.page)
      let marks = try await redaction.markMatches(SearchQuery(text: "Report"), on: [page])
      #expect(!marks.isEmpty)
      let result = try await redaction.apply(pages: [page])
      expectNoDifference(result.results.first?.status, .applied)
      let after = try await search.findAll(SearchQuery(text: "Report"))
      #expect(after.allSatisfy { $0.page != page })
    }

    @Test func aClosedDocumentsCapabilityThrows() async throws {
      let (viewer, document, redaction, _) = try await opened()
      try await viewer.close(document)
      await #expect {
        _ = try await redaction.pending()
      } throws: { error in
        let error = error as? PDFError
        return error?.code == "instance-closed" && error?.name == "PluginError"
      }
    }

    @Test func droppingTheStreamUnsubscribes() async throws {
      let (_, _, redaction, _) = try await opened()
      let task = Task { for await _ in redaction.events {} }
      try await Task.sleep(for: .milliseconds(20))
      task.cancel()
      _ = await task.value
      _ = try await redaction.pending()
    }

    @Test func applyAllCancelsWhenTheTaskIsCancelled() async throws {
      let (_, _, redaction, _) = try await opened()
      let task = Task { try await redaction.applyAll() }
      task.cancel()
      do {
        _ = try await task.value
      } catch {
        expectNoDifference((error as? PDFError)?.code, "operation-cancelled")
      }
      _ = try await redaction.pending()
    }

    @Test func markMatchesCancelsWhenTheTaskIsCancelled() async throws {
      let (_, _, redaction, _) = try await opened()
      let task = Task { try await redaction.markMatches(SearchQuery(text: "e")) }
      task.cancel()
      do {
        _ = try await task.value
      } catch {
        expectNoDifference((error as? PDFError)?.code, "operation-cancelled")
      }
      _ = try await redaction.pending()
    }
  }
}
```

`applyAllCancelsWhenTheTaskIsCancelled` runs on a document with no marks, so a cancel that loses the race still applies nothing; the test's `catch` accepts the cancel, and a normal return is also fine. The Rust test in Task 1 pins the deterministic case.

If plan 03 (selection) has merged into `research/swift-plugins`, add:

```swift
    @Test func markingTheSelectionCreatesOneMarkPerPage() async throws {
      let viewer = try PDFViewer(
        ViewerConfig(interaction: true, selection: true, annotation: AnnotationConfig(), redaction: RedactionConfig()))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      try await viewer.annotations(for: document).refresh()
      let page = try await viewer.pages(of: document)[0].page
      try await viewer.selection(for: document).selectPage(page)
      let marks = try await viewer.redaction(for: document).markSelection()
      expectNoDifference(marks.count, 1)
    }
```

using plan 03's real select-a-page method name.

- [x] **Step 3: Run them to see them fail**

Run: `(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17' -only-testing:EmbedPDFTests/BaseSuite/RedactionTests)`
Expected: build failure, `value of type PDFViewer has no member redaction`.

- [x] **Step 4: Write `Redaction.swift`**

```swift
public import EmbedPDFBindings
import Foundation

public typealias RedactionConfig = EmbedPDFBindings.RedactionConfig
public typealias RedactionOverlayText = EmbedPDFBindings.RedactionOverlayText
public typealias RedactionMark = EmbedPDFBindings.RedactionMark
public typealias RedactionMarkKind = EmbedPDFBindings.RedactionMarkKind
public typealias RedactionCollateral = EmbedPDFBindings.RedactionCollateral
public typealias RedactionLabelPatch = EmbedPDFBindings.RedactionLabelPatch
public typealias RedactionUnmarkResult = EmbedPDFBindings.RedactionUnmarkResult
public typealias RedactionSkipped = EmbedPDFBindings.RedactionSkipped
public typealias RedactionUnmarkFailure = EmbedPDFBindings.RedactionUnmarkFailure
public typealias RedactionApplyResult = EmbedPDFBindings.RedactionApplyResult
public typealias RedactionApplyScope = EmbedPDFBindings.RedactionApplyScope
public typealias RedactionPageResult = EmbedPDFBindings.RedactionPageResult
public typealias RedactionPageStatus = EmbedPDFBindings.RedactionPageStatus
public typealias RedactionEvent = EmbedPDFBindings.RedactionEvent

/// A document's redactions: mark regions, review them, apply them. Get one
/// with `PDFViewer.redaction(for:)`; the viewer needs `ViewerConfig(
/// interaction: true, annotation: AnnotationConfig(), redaction:
/// RedactionConfig())`. It keeps the viewer alive. After its document
/// closes, every call throws `instance-closed`.
public struct RedactionCapability: Sendable {
  let handle: RedactionHandle
  let viewer: PDFViewer

  public func pending(on page: PageRef? = nil) async throws(PDFError) -> [RedactionMark] {
    try await bridged { try await handle.pending(page: page) }
  }

  public func pendingItem(_ ref: AnnotationRef) async throws(PDFError) -> RedactionMark? {
    try await bridged { try await handle.pendingItem(annotationRef: ref) }
  }

  public func pendingCount(on page: PageRef? = nil) async throws(PDFError) -> UInt32 {
    try await bridged { try await handle.pendingCount(page: page) }
  }

  /// What applying `refs` (every pending mark when `nil`) would also remove.
  public func estimateCollateral(_ refs: [AnnotationRef]? = nil) async throws(PDFError) -> RedactionCollateral {
    try await bridged { try await handle.estimateCollateral(refs: refs) }
  }

  public func markSelection() async throws(PDFError) -> [AnnotationRef] {
    try await bridgedCancellable { token in try await handle.markSelection(cancel: token) }
  }

  public func markArea(_ bounds: Rect, on page: PageRef) async throws(PDFError) -> AnnotationRef {
    try await bridgedCancellable { token in try await handle.markArea(page: page, bounds: bounds, cancel: token) }
  }

  public func markPage(_ page: PageRef) async throws(PDFError) -> AnnotationRef {
    try await bridgedCancellable { token in try await handle.markPage(page: page, cancel: token) }
  }

  public func markMatches(_ query: SearchQuery, on pages: [PageRef]? = nil) async throws(PDFError) -> [AnnotationRef] {
    try await bridgedCancellable { token in try await handle.markMatches(query: query, pages: pages, cancel: token) }
  }

  public func unmark(_ refs: [AnnotationRef]) async throws(PDFError) -> RedactionUnmarkResult {
    try await bridgedCancellable { token in try await handle.unmark(refs: refs, cancel: token) }
  }

  public func clearPending() async throws(PDFError) -> RedactionUnmarkResult {
    try await bridgedCancellable { token in try await handle.clearPending(cancel: token) }
  }

  public func updateLabel(_ ref: AnnotationRef, _ patch: RedactionLabelPatch) async throws(PDFError) {
    try await bridgedCancellable { token in
      try await handle.updateLabel(annotationRef: ref, patch: patch, cancel: token)
    }
  }

  /// Apply these pending marks. Irreversible.
  public func apply(_ refs: [AnnotationRef]) async throws(PDFError) -> RedactionApplyResult {
    try await bridgedCancellable { token in try await handle.apply(refs: refs, cancel: token) }
  }

  /// Apply every mark on these pages. Irreversible.
  public func apply(pages: [PageRef]) async throws(PDFError) -> RedactionApplyResult {
    try await bridgedCancellable { token in try await handle.applyPages(pages: pages, cancel: token) }
  }

  /// Apply every redaction in the document. Irreversible.
  public func applyAll() async throws(PDFError) -> RedactionApplyResult {
    try await bridgedCancellable { token in try await handle.applyAll(cancel: token) }
  }

  public func canMark() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canMark() }
  }

  public func canApply() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canApply() }
  }

  public func isApplying() async throws(PDFError) -> Bool {
    try await bridged { try await handle.isApplying() }
  }

  public func lastResult() async throws(PDFError) -> RedactionApplyResult? {
    try await bridged { try await handle.lastResult() }
  }

  /// Applies and pending-mark changes. Finishes when the document closes
  /// (at once if it already has) or the viewer is destroyed.
  public var events: AsyncStream<RedactionEvent> {
    viewer.registry.stream { handle.subscribe(listener: $0) }
  }
}

extension EventRelay: RedactionEventListener where Event == RedactionEvent {
  func onEvent(event: RedactionEvent) {
    continuation.yield(event)
  }

  func onEnd() {
    continuation.finish()
  }
}

extension PDFViewer {
  public func redaction(for document: PDFDocument) async throws(PDFError) -> RedactionCapability {
    let handle = try await bridged { try await host.redaction(documentId: document.id) }
    return RedactionCapability(handle: handle, viewer: self)
  }
}
```

The `ViewerConfig` memberwise initializer UniFFI generates orders arguments as the Rust struct declares its fields. The test passes `interaction:`, `search:`, `annotation:`, `redaction:` in the order the fields sit in `api.rs` after plans 01 to 07; if the compiler asks for another order, follow it.

- [x] **Step 5: Run the tests**

Run: `(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17' -only-testing:EmbedPDFTests/BaseSuite/RedactionTests)`
Expected: 6 tests pass (7 with the selection test).

Run: `(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17')`
Expected: every suite passes.

- [x] **Step 6: Commit**

```bash
git add platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Redaction.swift platforms/apple/EmbedPDF/Tests/EmbedPDFTests/RedactionTests.swift
git commit -m "Add EmbedPDF's RedactionCapability"
```

---

### Task 5: Docs, CI, merge and cleanup

**Files:**
- Modify: `.github/workflows/plugin-rust.yml`
- Modify: `platforms/apple/README.md`
- Modify: `docs/superpowers/plans/2026-10-01-swift-package.md` (Appendix A §A.3, the redaction entry)

- [ ] **Step 1: CI and README**

Append `redaction` to `plugin-rust.yml`'s `crates=( … )`, after `form`. Add the README row:

```markdown
| redaction | `RedactionCapability` | `interaction: true, annotation: AnnotationConfig(), redaction: RedactionConfig()` | `2026-10-06-swift-plugins/07-redaction.md` |
```

In the spec's §A.3, change `**redaction** (D, requires …)` to begin `**redaction** (D), delivered by 07-redaction.md, requires …`.

- [ ] **Step 2: Record the binary size**

```bash
ls -l target/aarch64-apple-ios-sim/release/libepdf_viewer_host.a
ls -l ../swift-plugins-integration/target/aarch64-apple-ios-sim/release/libepdf_viewer_host.a
```

- [ ] **Step 3: Run the definition of done**

```bash
cargo test -p epdf-plugin-redaction --features uniffi
cargo test -p epdf-ffi -p epdf-viewer-host
cargo clippy -p epdf-plugin-redaction -p epdf-viewer-host --all-targets --features epdf-plugin-redaction/uniffi -- -D warnings
cargo test -p epdf-plugin-redaction
bash platforms/apple/build-xcframework.sh
(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17')
cargo build -p epdf-viewer-host -p uniffi-bindgen
./target/debug/uniffi-bindgen generate --library target/debug/libepdf_viewer_host.dylib --language kotlin --no-format --out-dir "$TMPDIR/epdf-kotlin-check"
```

Expected: every command exits 0.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/plugin-rust.yml platforms/apple/README.md docs/superpowers/plans/2026-10-01-swift-package.md
git commit -m "List the redaction slice in CI and the README

libepdf_viewer_host.a (aarch64-apple-ios-sim, release): <before> -> <after> bytes."
```

- [ ] **Step 5: Merge**

From `.claude/worktrees/swift-plugins-integration`:

```bash
git merge --no-ff research/swift-redaction
```

Resolve shared-file conflicts by keeping both blocks in plan-number order.

- [ ] **Step 6: Rerun after the merge**

```bash
bash platforms/apple/build-xcframework.sh
(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17')
cargo build -p epdf-viewer-host -p uniffi-bindgen
./target/debug/uniffi-bindgen generate --library target/debug/libepdf_viewer_host.dylib --language kotlin --no-format --out-dir "$TMPDIR/epdf-kotlin-check"
```

Expected: all pass.

- [ ] **Step 7: Remove the worktree and branch**

```bash
git worktree remove .claude/worktrees/swift-redaction
git branch -d research/swift-redaction
git worktree prune
```

No `--force`. If either refuses, find out why first. Then tick 07 in the umbrella.
