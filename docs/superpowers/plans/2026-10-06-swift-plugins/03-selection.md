# Selection Swift Wrapper Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Export the selection plugin (text selection in character space, its geometry, and text extraction) through UniFFI, wire it into `ViewerHost`, and give Swift a `SelectionCapability`.

**Architecture:** The recipe from `2026-10-01-swift-package.md` Appendix A §A.0, with metadata and search as models. Every selection record carries a kernel `PageRef` or an `epdf-geometry` quad, so each is mirrored in `crates/plugins/selection/src/ffi.rs` under a `Selection` prefix. The plugin's pointer handler registers with the interaction hub on its own (`selection_plugin`'s `connect`), so a pointer drag sent through plan 01's `InteractionCapability.dispatchPointer` selects text with no extra wiring; this plan proves that end to end.

**Tech Stack:** Rust 2024, `uniffi =0.32.0`, Swift 6.2 tools, Swift Testing, `swift-custom-dump` 1.3, Xcode 27.0, iPhone 17 simulator (iOS 26.0).

**Spec:** `docs/superpowers/plans/2026-10-06-swift-plugins/00-umbrella.md` and `docs/superpowers/plans/2026-10-01-swift-package.md` (Global Constraints, Decisions, Appendix A §A.0 and §A.2). Plan 01 (`01-interaction.md`) must be merged first: selection requires interaction, and this plan's cross-plugin tests drive the hub.

## Global Constraints

Everything in the umbrella's Global Constraints applies. The ones this plan meets first:

- `uniffi` is pinned at `=0.32.0`.
- UniFFI names are unique across the workspace. Every record here starts with `Selection`. `ChangeOrigin` and `ChangeTrigger` are generic names other plugins also have, so the trigger crosses as `SelectionChangeTrigger` and the rest of the origin (always local, no session or actor) is dropped.
- Character indices are `i64`, as the plugin keeps them: `select_all` parks its open end at `Number.MAX_SAFE_INTEGER` and `select` accepts a negative start. These are PDFium character indices, not UTF-16 offsets into extracted text; the doc comment on `SelectionTextPosition` says so, as search's `SearchHit` does.
- This plan runs in wave B, alongside plan 02. Its `From` impls are written by hand, which is fine: `epdf_ffi::mirror_enum!` and `to_u32` (umbrella Task 0c) are there to use, but nothing here needs rewriting to adopt them. No selection record has a `usize`.
- The kernel is `!Send`; selection's pointer handler runs on the kernel thread inside `dispatch_pointer`.

## Review Focus

1. A `select` span whose count runs past the page's last character (`count: 1_000_000`). Expected: the selection clamps to the page's text, and `read_text` answers the page's text, not an error. Pinned in Task 2 (`a_span_past_the_end_clamps_to_the_page`).
2. `read_text` across pages whose text hasn't loaded yet. Expected: it loads them and joins pages with `\n`, never answering an empty string for a non-empty selection. Pinned in Task 2 (`reading_text_loads_the_pages_it_needs`).
3. A `read_text` cancelled from Swift mid-read. Expected: it throws `operation-cancelled`, and a later `read_text` still works. Pinned in Task 5 (`readTextCancelsWhenTheTaskIsCancelled`).
4. A pointer drag that starts over empty space. Expected: the selection handler doesn't capture it (a lower-priority handler like stage's pan gets it), and nothing is selected. Pinned in Task 3 (`a_drag_that_starts_off_text_selects_nothing`).
5. A document without `doc.text.copy` (copy forbidden by its permissions). Expected: `can_copy` is false and `read_text` throws `permission-denied`, while selection itself still works. Pinned in Task 2 (`without_copy_permission_reading_text_is_denied`).

## Cross-plugin tests

- `interaction` (plan 01, required): this plan owns `a_pointer_drag_over_text_selects_it` (Task 3) and the umbrella's Review Focus item 2, `a_plugin_without_its_requirement_fails_to_build` (Task 3), which plan 01 handed here because interaction requires nothing.
- `feedback` (plan 01, optional): this plan owns `a_long_press_on_a_word_ticks_the_feedback` (Task 3).
- `redaction` (plan 07, optional dependency on selection): plan 07 owns a test that `mark_selection` marks the current selection.
- `annotation` (plan 05, optional dependency on selection): plan 05 owns a test that a text-markup tool creates a highlight from the selection.

## File Structure

```
crates/plugins/selection/Cargo.toml        uniffi feature
crates/plugins/selection/src/lib.rs        scaffolding, ffi module
crates/plugins/selection/src/controller.rs lifetime()
crates/plugins/selection/src/ffi.rs        SelectionHandle, records, events
crates/plugins/selection/tests/ffi.rs
crates/epdf-viewer-host/{Cargo.toml,src/api.rs,src/session.rs,src/lib.rs}
crates/epdf-viewer-host/tests/viewer_host.rs
platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Selection.swift
platforms/apple/EmbedPDF/Tests/EmbedPDFTests/SelectionTests.swift
```

---

### Task 1: Records and conversions

**Files:**
- Modify: `crates/plugins/selection/Cargo.toml`, `crates/plugins/selection/src/lib.rs`, `crates/plugins/selection/src/controller.rs`
- Create: `crates/plugins/selection/src/ffi.rs`, `crates/plugins/selection/tests/ffi.rs`

**Interfaces:**
- Consumes: `epdf_plugin_selection::{TextPosition, TextRange, SelectionRangeInput, SelectionSegment, SelectionEndpoint, SelectionAnchor, SelectionDirection, SnapshotPage, SelectionSnapshot, ChangeTrigger, ChangeOrigin, SelectionChangedEvent, SelectionCommittedEvent, SelectionClearedEvent}` (`crates/plugins/selection/src/contract.rs`, `geometry.rs`).
- Produces (in `epdf_plugin_selection::ffi`): records `SelectionTextPosition { page: PageRef, index: i64 }`, `SelectionTextRange { start, end }`, `SelectionSegment { quad: TextQuad, rect: Rect, advance: i32 }`, `SelectionEndpoint { page, glyph_quad, advance: i32, rect }`, `SelectionAnchor { page, bounds }`, `SelectionSnapshotPage { page, segments }`, `SelectionSnapshot { pages, start, end, direction, range }`; enums `SelectionInput { Range { range }, Span { page, start: i64, count: i64 } }`, `SelectionDirection { Forward, Backward }`, `SelectionChangeTrigger { User, Api, Script, System, Unknown }`; conversions both ways for positions, ranges and the input, out only for the rest.

- [x] **Step 1: Create the worktree**

```bash
git worktree add .claude/worktrees/swift-selection -b research/swift-selection research/swift-plugins
cd .claude/worktrees/swift-selection
```

Fetch PDFium for `darwin-arm64`, `ios-arm64` and `ios-sim-arm64` as `platforms/apple/README.md` describes.

- [x] **Step 2: Add the feature and `lifetime()`**

In `crates/plugins/selection/Cargo.toml`:

```toml
# under [features]
# The uniffi exports (`ffi` module), for `epdf-viewer-host`.
uniffi = ["dep:uniffi", "dep:epdf-ffi"]

# under [dependencies]
epdf-ffi = { path = "../../epdf-ffi", optional = true }
uniffi = { version = "=0.32.0", optional = true }

[dev-dependencies]
futures = { version = "0.3", default-features = false, features = ["std", "executor"] }
plugin-conformance = { path = "../../plugin-conformance" }
```

In `crates/plugins/selection/src/lib.rs`:

```rust
#[cfg(feature = "uniffi")]
pub mod ffi;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();
```

In `crates/plugins/selection/src/controller.rs`, inside `impl SelectionCapability`:

```rust
    /// The instance lifetime, aborted when the document closes. A foreign
    /// handle outlives the close, checks this before every call, and ends
    /// its event streams when it aborts.
    #[cfg(feature = "uniffi")]
    #[doc(hidden)]
    pub fn lifetime(&self) -> &AbortSignal {
        self.0.cx.lifetime()
    }
```

`AbortSignal` is already imported in `controller.rs`.

- [x] **Step 3: Write the failing conversion test**

`crates/plugins/selection/tests/ffi.rs`:

```rust
#![cfg(feature = "uniffi")]

use epdf_ffi::PageRef;
use epdf_plugin_selection::ffi::{SelectionInput, SelectionTextPosition, SelectionTextRange};
use epdf_plugin_selection::{SelectionRangeInput, TextRange};

const PAGE_1: PageRef = PageRef { page_object_number: 1 };

#[test]
fn a_range_and_a_span_round_trip() {
    let range = SelectionTextRange {
        start: SelectionTextPosition { page: PAGE_1, index: -3 },
        end: SelectionTextPosition { page: PAGE_1, index: 9_007_199_254_740_991 },
    };
    let core = TextRange::from(range);
    assert_eq!(core.start.index, -3);
    assert_eq!(SelectionTextRange::from(core), range);
    let span = SelectionInput::Span { page: PAGE_1, start: 2, count: 5 };
    assert!(matches!(SelectionRangeInput::from(span),
        SelectionRangeInput::Span { start: 2, count: 5, .. }));
}
```

- [x] **Step 4: Run it to verify it fails**

Run: `cargo test -p epdf-plugin-selection --features uniffi --test ffi`
Expected: FAIL to compile, "unresolved import `epdf_plugin_selection::ffi`".

- [x] **Step 5: Write the records half of `ffi.rs`**

```rust
//! The selection plugin through uniffi: one handle over the selection, its
//! geometry and text extraction, and one event enum.
//!
//! Every record carries a kernel `PageRef` or an `epdf-geometry` quad, so
//! each is a mirror here. Character indices stay `i64`: they are PDFium
//! character indices (the space search hits address), not UTF-16 offsets
//! into extracted text. The selection's `ChangeOrigin` is always local,
//! with no session or actor, so only its trigger crosses.
//!
//! Every method hops to the kernel thread through `KernelBound`.

use std::sync::Arc;

use epdf_ffi::{
    CancelToken, KernelBound, KernelPost, PageRef, Point, Rect, Subscription, TextQuad,
    ViewerError,
};
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{Error, Unsubscribe};

use crate::{OperationOptions, SelectionCapability};

/// A position in a page's character space.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SelectionTextPosition {
    pub page: PageRef,
    /// A PDFium character index, not a UTF-16 offset.
    pub index: i64,
}

impl From<crate::TextPosition> for SelectionTextPosition {
    fn from(p: crate::TextPosition) -> Self {
        SelectionTextPosition { page: p.page.into(), index: p.index }
    }
}

impl From<SelectionTextPosition> for crate::TextPosition {
    fn from(p: SelectionTextPosition) -> Self {
        crate::TextPosition { page: p.page.into(), index: p.index }
    }
}

/// A half-open range in character space, in document order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SelectionTextRange {
    pub start: SelectionTextPosition,
    pub end: SelectionTextPosition,
}

impl From<crate::TextRange> for SelectionTextRange {
    fn from(r: crate::TextRange) -> Self {
        SelectionTextRange { start: r.start.into(), end: r.end.into() }
    }
}

impl From<SelectionTextRange> for crate::TextRange {
    fn from(r: SelectionTextRange) -> Self {
        crate::TextRange { start: r.start.into(), end: r.end.into() }
    }
}

/// What `select` takes: a cross-page range, or a span on one page (a
/// search hit's shape: `char_start` and `char_count`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SelectionInput {
    Range { range: SelectionTextRange },
    Span { page: PageRef, start: i64, count: i64 },
}

impl From<SelectionInput> for crate::SelectionRangeInput {
    fn from(i: SelectionInput) -> Self {
        match i {
            SelectionInput::Range { range } => crate::SelectionRangeInput::Range(range.into()),
            SelectionInput::Span { page, start, count } => {
                crate::SelectionRangeInput::Span { page: page.into(), start, count }
            }
        }
    }
}

/// One merged visual line of a selection, in page space.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct SelectionSegment {
    pub quad: TextQuad,
    pub rect: Rect,
    /// Reading direction along the baseline: 1 or -1.
    pub advance: i32,
}

impl From<&crate::SelectionSegment> for SelectionSegment {
    fn from(s: &crate::SelectionSegment) -> Self {
        SelectionSegment { quad: s.quad.into(), rect: s.rect.into(), advance: s.advance }
    }
}

/// A selection boundary on its boundary glyph's own cell.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct SelectionEndpoint {
    pub page: PageRef,
    pub glyph_quad: TextQuad,
    pub advance: i32,
    pub rect: Rect,
}

impl From<&crate::SelectionEndpoint> for SelectionEndpoint {
    fn from(e: &crate::SelectionEndpoint) -> Self {
        SelectionEndpoint {
            page: e.page.into(),
            glyph_quad: e.glyph_quad.into(),
            advance: e.advance,
            rect: e.rect.into(),
        }
    }
}

/// Where selection-scoped floating UI attaches.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct SelectionAnchor {
    pub page: PageRef,
    pub bounds: Rect,
}

impl From<crate::SelectionAnchor> for SelectionAnchor {
    fn from(a: crate::SelectionAnchor) -> Self {
        SelectionAnchor { page: a.page.into(), bounds: a.bounds.into() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SelectionDirection {
    Forward,
    Backward,
}

impl From<crate::SelectionDirection> for SelectionDirection {
    fn from(d: crate::SelectionDirection) -> Self {
        match d {
            crate::SelectionDirection::Forward => SelectionDirection::Forward,
            crate::SelectionDirection::Backward => SelectionDirection::Backward,
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SelectionSnapshotPage {
    pub page: PageRef,
    pub segments: Vec<SelectionSegment>,
}

/// Everything a selection overlay draws.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SelectionSnapshot {
    pub pages: Vec<SelectionSnapshotPage>,
    pub start: Option<SelectionEndpoint>,
    pub end: Option<SelectionEndpoint>,
    pub direction: SelectionDirection,
    /// A valid `select` input; `None` with nothing selected.
    pub range: Option<SelectionTextRange>,
}

impl From<&crate::SelectionSnapshot> for SelectionSnapshot {
    fn from(s: &crate::SelectionSnapshot) -> Self {
        SelectionSnapshot {
            pages: s
                .pages
                .iter()
                .map(|p| SelectionSnapshotPage {
                    page: p.page.into(),
                    segments: p.segments.iter().map(Into::into).collect(),
                })
                .collect(),
            start: s.start.as_ref().map(Into::into),
            end: s.end.as_ref().map(Into::into),
            direction: s.direction.into(),
            range: s.range.map(Into::into),
        }
    }
}

/// Who changed the selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SelectionChangeTrigger {
    User,
    Api,
    Script,
    System,
    Unknown,
}

impl From<crate::ChangeTrigger> for SelectionChangeTrigger {
    fn from(t: crate::ChangeTrigger) -> Self {
        match t {
            crate::ChangeTrigger::User => SelectionChangeTrigger::User,
            crate::ChangeTrigger::Api => SelectionChangeTrigger::Api,
            crate::ChangeTrigger::Script => SelectionChangeTrigger::Script,
            crate::ChangeTrigger::System => SelectionChangeTrigger::System,
            crate::ChangeTrigger::Unknown => SelectionChangeTrigger::Unknown,
        }
    }
}
```

`crate::SelectionSegment` and the FFI `SelectionSegment` share a Rust name; inside `ffi.rs` the plugin's is always spelled `crate::SelectionSegment`, as written. `TextPosition.page` is the kernel `PageRef` (`contract.rs` imports `epdf_kernel::geometry::PageRef`), so `.into()` uses `epdf-ffi`'s existing impls.

- [x] **Step 6: Run the test to verify it passes**

Run: `cargo test -p epdf-plugin-selection --features uniffi --test ffi`
Expected: PASS, 1 test.

- [x] **Step 7: Commit**

```bash
git add crates/plugins/selection
git commit -m "Mirror the selection plugin's records for uniffi"
```

### Task 2: `SelectionHandle` and its events

**Files:**
- Modify: `crates/plugins/selection/src/ffi.rs`, `crates/plugins/selection/tests/ffi.rs`

**Interfaces:**
- Consumes: Task 1's records; `SelectionCapability`'s members (`crates/plugins/selection/src/controller.rs:626-925`).
- Produces: object `SelectionHandle` with `SelectionHandle::new(cap: SelectionCapability, post: KernelPost) -> Arc<Self>`. Methods: `can_select`, `can_copy`, `select(input)`, `select_all`, `select_page(page)`, `select_word_at(page, point) -> bool`, `select_line_at(page, point) -> bool`, `extend_to(page, point)`, `clear`, `has_selection`, `snapshot`, `range`, `selected_pages`, `segments(page)`, `rects(page)`, `anchor`, `read_text(cancel)`, `read_text_in_range(range, cancel)`, `ensure_loaded(page)`, `is_loaded(page)`, `is_over_text(page, point)`, `begin_gesture`, `begin_gesture_at(page, point) -> bool`, `end_gesture`, `is_gesture_active`, `set_highlight_visible(visible)`, `is_highlight_visible`, `subscribe(listener)`. Records `SelectionChangedEvent { range, pages, trigger }`, `SelectionCommittedEvent { range }`, `SelectionClearedEvent { trigger }`; enum `SelectionEvent { Changed, Committed, Cleared }`; trait `SelectionEventListener { on_event, on_end }`.

That's the 30 entries of `crates/plugins/selection/src/harness.rs` `MEMBERS`, with the three `on*` hooks folded into `subscribe`.

- [x] **Step 1: Write the failing tests**

Append to `crates/plugins/selection/tests/ffi.rs`. The fake engine needs text geometry for a page to select anything; build it from the vectors the conformance tests replay. `crates/plugins/selection/tests/conformance.rs` loads `tooling/engine-conformance/vectors/plugins/selection*.json`; open one, find the engine spec it starts from (its `engine` member), and copy that spec into `fixtures/selection-engine.json` under `crates/plugins/selection/tests/`. The spec has a document with at least two pages and `page.geometry.read` and `page.text.read` responses for both.

```rust
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use epdf_ffi::{CancelToken, KernelThread, Point, ViewerError};
use epdf_plugin_interaction::{InteractionConfig, interaction_plugin};
use epdf_plugin_selection::ffi::{
    SelectionChangeTrigger, SelectionEvent, SelectionEventListener, SelectionHandle,
};
use epdf_plugin_selection::{SELECTION, SelectionConfig, selection_plugin};
use futures::executor::block_on;
use plugin_conformance::test_kernel::{TestKernel, TestSpec};
use serde_json::Value;

fn spec() -> Value {
    serde_json::from_str(include_str!("fixtures/selection-engine.json")).unwrap()
}

fn engine(allows: &str) -> TestSpec {
    let mut spec = spec();
    for doc in spec["documents"].as_object_mut().unwrap().values_mut() {
        doc["security"] = serde_json::json!({ "allows": allows });
    }
    TestSpec::new(spec)
}

/// The fixture document's pages, in order.
fn page_refs() -> Vec<PageRef> {
    let spec = spec();
    let doc = spec["documents"].as_object().unwrap().values().next().unwrap().clone();
    doc["pages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| PageRef { page_object_number: p["ref"]["pageObjectNumber"].as_u64().unwrap() as u32 })
        .collect()
}

#[derive(Default)]
struct Events(Mutex<Vec<SelectionEvent>>, AtomicUsize);

impl SelectionEventListener for Events {
    fn on_event(&self, event: SelectionEvent) {
        self.0.lock().unwrap().push(event);
    }
    fn on_end(&self) {
        self.1.fetch_add(1, Ordering::SeqCst);
    }
}

fn handle_with(spec: impl FnOnce() -> TestSpec + Send + 'static)
    -> (KernelThread<TestKernel>, Arc<SelectionHandle>, Vec<PageRef>)
{
    let thread = KernelThread::spawn(move |_| {
        TestKernel::open(
            vec![
                interaction_plugin(InteractionConfig::default()),
                selection_plugin(SelectionConfig::default()),
            ],
            spec(),
        )
    })
    .unwrap();
    let post = thread.post();
    let handle =
        block_on(thread.call(move |k| SelectionHandle::new(k.capability(&SELECTION).unwrap(), post)))
            .unwrap();
    (thread, handle, page_refs())
}

fn handle() -> (KernelThread<TestKernel>, Arc<SelectionHandle>, Vec<PageRef>) {
    handle_with(|| engine("*"))
}

fn code(error: &ViewerError) -> Option<&str> {
    match error {
        ViewerError::Kernel { code, .. } => code.as_deref(),
        ViewerError::Closed => None,
    }
}

#[test]
fn selecting_a_span_reads_its_text_and_geometry() {
    let (_thread, selection, pages) = handle();
    assert!(block_on(selection.can_select()).unwrap());
    block_on(selection.ensure_loaded(pages[0])).unwrap();
    block_on(selection.select(SelectionInput::Span { page: pages[0], start: 0, count: 4 })).unwrap();
    assert!(block_on(selection.has_selection()).unwrap());
    let text = block_on(selection.read_text(CancelToken::new())).unwrap();
    assert_eq!(text.encode_utf16().count(), 4, "{text:?}");
    assert!(!block_on(selection.rects(pages[0])).unwrap().is_empty());
    let snapshot = block_on(selection.snapshot()).unwrap();
    assert_eq!(snapshot.pages.len(), 1);
    assert!(snapshot.start.is_some() && snapshot.end.is_some());
    assert_eq!(block_on(selection.selected_pages()).unwrap(), [pages[0]]);
}

#[test]
fn a_span_past_the_end_clamps_to_the_page() {
    let (_thread, selection, pages) = handle();
    block_on(selection.ensure_loaded(pages[0])).unwrap();
    block_on(selection.select_page(pages[0])).unwrap();
    let page_text = block_on(selection.read_text(CancelToken::new())).unwrap();
    block_on(selection.select(SelectionInput::Span { page: pages[0], start: 0, count: 1_000_000 }))
        .unwrap();
    assert_eq!(block_on(selection.read_text(CancelToken::new())).unwrap(), page_text);
}

#[test]
fn reading_text_loads_the_pages_it_needs() {
    let (_thread, selection, pages) = handle();
    assert!(!block_on(selection.is_loaded(pages[1])).unwrap());
    block_on(selection.select_all()).unwrap();
    let text = block_on(selection.read_text(CancelToken::new())).unwrap();
    assert!(text.contains('\n'), "pages join with a newline: {text:?}");
    assert!(!text.trim().is_empty());
}

#[test]
fn reading_a_range_leaves_the_selection_alone() {
    let (_thread, selection, pages) = handle();
    block_on(selection.ensure_loaded(pages[0])).unwrap();
    block_on(selection.select(SelectionInput::Span { page: pages[0], start: 0, count: 2 })).unwrap();
    let before = block_on(selection.range()).unwrap();
    let range = SelectionTextRange {
        start: SelectionTextPosition { page: pages[0], index: 0 },
        end: SelectionTextPosition { page: pages[0], index: 6 },
    };
    let text = block_on(selection.read_text_in_range(range, CancelToken::new())).unwrap();
    assert_eq!(text.encode_utf16().count(), 6);
    assert_eq!(block_on(selection.range()).unwrap(), before);
}

#[test]
fn without_copy_permission_reading_text_is_denied() {
    let (_thread, selection, pages) = handle_with(|| engine("doc.text.select"));
    assert!(block_on(selection.can_select()).unwrap());
    assert!(!block_on(selection.can_copy()).unwrap());
    block_on(selection.ensure_loaded(pages[0])).unwrap();
    block_on(selection.select_page(pages[0])).unwrap();
    let error = block_on(selection.read_text(CancelToken::new())).unwrap_err();
    assert_eq!(code(&error), Some("permission-denied"), "{error:?}");
}

#[test]
fn a_cancelled_read_rejects_operation_cancelled() {
    let (_thread, selection, _) = handle();
    block_on(selection.select_all()).unwrap();
    let cancel = CancelToken::new();
    cancel.cancel();
    let error = block_on(selection.read_text(cancel)).unwrap_err();
    assert_eq!(code(&error), Some("operation-cancelled"), "{error:?}");
}

#[test]
fn select_and_clear_are_observed() {
    let (thread, selection, pages) = handle();
    let events = Arc::new(Events::default());
    let _sub = selection.subscribe(events.clone());
    block_on(selection.ensure_loaded(pages[0])).unwrap();
    block_on(selection.select(SelectionInput::Span { page: pages[0], start: 0, count: 3 })).unwrap();
    block_on(selection.clear()).unwrap();
    block_on(thread.call(|_| ())).unwrap();
    let seen = events.0.lock().unwrap();
    assert!(matches!(seen.first(), Some(SelectionEvent::Changed { event })
        if event.range.is_some() && event.trigger == SelectionChangeTrigger::Api), "{seen:?}");
    assert!(seen.iter().any(|e| matches!(e, SelectionEvent::Cleared { .. })), "{seen:?}");
}

#[test]
fn a_gesture_commits_on_end() {
    let (thread, selection, pages) = handle();
    let events = Arc::new(Events::default());
    let _sub = selection.subscribe(events.clone());
    block_on(selection.ensure_loaded(pages[0])).unwrap();
    block_on(selection.select_page(pages[0])).unwrap();
    let rect = block_on(selection.rects(pages[0])).unwrap()[0];
    block_on(selection.clear()).unwrap();
    let left = Point { x: rect.x + 0.5, y: rect.y + rect.height / 2.0 };
    let right = Point { x: rect.x + rect.width - 0.5, y: left.y };
    assert!(block_on(selection.begin_gesture_at(pages[0], left)).unwrap());
    assert!(block_on(selection.is_gesture_active()).unwrap());
    block_on(selection.extend_to(pages[0], right)).unwrap();
    block_on(selection.end_gesture()).unwrap();
    block_on(thread.call(|_| ())).unwrap();
    assert!(events.0.lock().unwrap().iter().any(|e| matches!(e, SelectionEvent::Committed { .. })));
}

#[test]
fn a_closed_documents_capability_refuses_with_instance_closed() {
    let (thread, selection, _) = handle();
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    for error in [
        block_on(selection.has_selection()).map(|_| ()).unwrap_err(),
        block_on(selection.select_all()).unwrap_err(),
        block_on(selection.read_text(CancelToken::new())).map(|_| ()).unwrap_err(),
    ] {
        assert_eq!(code(&error), Some("instance-closed"), "{error:?}");
    }
}

#[test]
fn closing_the_document_ends_selection_subscriptions() {
    let (thread, selection, _) = handle();
    let events = Arc::new(Events::default());
    let _sub = selection.subscribe(events.clone());
    block_on(thread.call(|_| ())).unwrap();
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    assert_eq!(events.1.load(Ordering::SeqCst), 1);
}
```

Add the extra `use` items (`SelectionInput`, `SelectionTextPosition`, `SelectionTextRange`) to the existing `use epdf_plugin_selection::ffi::{...}` line from Task 1. `interaction_plugin` needs `epdf-plugin-interaction` as a dev-dependency only if it isn't already a normal one; it is (`Cargo.toml` `[dependencies]`), so it's in scope. If the vector's engine spec gives a page no text, pick a vector that does; `select_page` followed by `rects` returning empty is the sign.

- [x] **Step 2: Run them to verify they fail**

Run: `cargo test -p epdf-plugin-selection --features uniffi --test ffi`
Expected: FAIL to compile, "cannot find `SelectionHandle` in `ffi`".

- [x] **Step 3: Write the handle and events**

Append to `ffi.rs`:

```rust
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SelectionChangedEvent {
    pub range: Option<SelectionTextRange>,
    /// The pages that carry a segment now.
    pub pages: Vec<PageRef>,
    pub trigger: SelectionChangeTrigger,
}

/// A selection gesture ended with a selection in place.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SelectionCommittedEvent {
    pub range: SelectionTextRange,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SelectionClearedEvent {
    pub trigger: SelectionChangeTrigger,
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum SelectionEvent {
    Changed { event: SelectionChangedEvent },
    Committed { event: SelectionCommittedEvent },
    Cleared { event: SelectionClearedEvent },
}

#[uniffi::export(with_foreign)]
pub trait SelectionEventListener: Send + Sync {
    fn on_event(&self, event: SelectionEvent);
    /// The document closed: no event follows.
    fn on_end(&self);
}

fn open(cap: &SelectionCapability) -> Result<&SelectionCapability, Error> {
    if cap.lifetime().aborted() {
        return Err(plugin_error(
            code::INSTANCE_CLOSED,
            "selection",
            "the document this capability belongs to is closed",
            None,
            None,
        ));
    }
    Ok(cap)
}

/// A signal for the plugin's options, and the abort to hang on the promise
/// it returns, so a cancelled `CancelToken` reaches the plugin.
fn abortable() -> (AbortSignal, impl Fn(Option<serde_json::Value>) + 'static) {
    let signal = AbortSignal::new();
    let aborter = signal.clone();
    (signal, move |reason| aborter.abort(Error::abort(reason)))
}

#[derive(uniffi::Object)]
pub struct SelectionHandle {
    cap: KernelBound<SelectionCapability>,
    post: KernelPost,
}

impl SelectionHandle {
    /// On the kernel thread.
    pub fn new(cap: SelectionCapability, post: KernelPost) -> Arc<Self> {
        Arc::new(SelectionHandle { cap: KernelBound::new(cap, post.clone()), post })
    }

    async fn on<T: Send + 'static>(
        &self,
        f: impl FnOnce(&SelectionCapability) -> Result<T, Error> + Send + 'static,
    ) -> Result<T, ViewerError> {
        self.cap.call(move |c| f(open(c)?)).await
    }
}

#[uniffi::export]
impl SelectionHandle {
    /// Whether selection geometry is served (`doc.text.select`).
    pub async fn can_select(&self) -> Result<bool, ViewerError> {
        self.on(|c| Ok(c.can_select())).await
    }

    /// Whether text extraction is served (`doc.text.copy`).
    pub async fn can_copy(&self) -> Result<bool, ViewerError> {
        self.on(|c| Ok(c.can_copy())).await
    }

    pub async fn select(&self, input: SelectionInput) -> Result<(), ViewerError> {
        self.on(move |c| c.select(input.into())).await
    }

    pub async fn select_all(&self) -> Result<(), ViewerError> {
        self.on(|c| c.select_all()).await
    }

    pub async fn select_page(&self, page: PageRef) -> Result<(), ViewerError> {
        self.on(move |c| c.select_page(page.into())).await
    }

    /// False when the point has no selectable text.
    pub async fn select_word_at(&self, page: PageRef, point: Point) -> Result<bool, ViewerError> {
        self.on(move |c| c.select_word_at(page.into(), point.into())).await
    }

    pub async fn select_line_at(&self, page: PageRef, point: Point) -> Result<bool, ViewerError> {
        self.on(move |c| c.select_line_at(page.into(), point.into())).await
    }

    pub async fn extend_to(&self, page: PageRef, point: Point) -> Result<(), ViewerError> {
        self.on(move |c| c.extend_to(page.into(), point.into())).await
    }

    pub async fn clear(&self) -> Result<(), ViewerError> {
        self.on(|c| {
            c.clear();
            Ok(())
        })
        .await
    }

    pub async fn has_selection(&self) -> Result<bool, ViewerError> {
        self.on(|c| Ok(c.has_selection())).await
    }

    pub async fn snapshot(&self) -> Result<SelectionSnapshot, ViewerError> {
        self.on(|c| Ok((&c.get_snapshot()).into())).await
    }

    pub async fn range(&self) -> Result<Option<SelectionTextRange>, ViewerError> {
        self.on(|c| Ok(c.get_range().map(Into::into))).await
    }

    pub async fn selected_pages(&self) -> Result<Vec<PageRef>, ViewerError> {
        self.on(|c| Ok(c.list_selected_pages().into_iter().map(Into::into).collect())).await
    }

    pub async fn segments(&self, page: PageRef) -> Result<Vec<SelectionSegment>, ViewerError> {
        self.on(move |c| Ok(c.list_segments(page.into()).iter().map(Into::into).collect())).await
    }

    pub async fn rects(&self, page: PageRef) -> Result<Vec<Rect>, ViewerError> {
        self.on(move |c| Ok(c.list_rects(page.into()).into_iter().map(Into::into).collect())).await
    }

    pub async fn anchor(&self) -> Result<Option<SelectionAnchor>, ViewerError> {
        self.on(|c| Ok(c.get_anchor().map(Into::into))).await
    }

    /// The selected text, pages joined with `\n`; `""` with nothing
    /// selected. Rejects `permission-denied` without `doc.text.copy`.
    pub async fn read_text(&self, cancel: Arc<CancelToken>) -> Result<String, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                |c| {
                    let (signal, abort) = abortable();
                    Ok(open(c)?.read_text(OperationOptions { signal: Some(signal) }).with_abort(abort))
                },
                |text| text,
            )
            .await
    }

    /// Text for any range, without touching the selection.
    pub async fn read_text_in_range(
        &self,
        range: SelectionTextRange,
        cancel: Arc<CancelToken>,
    ) -> Result<String, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let (signal, abort) = abortable();
                    Ok(open(c)?
                        .read_text_in_range(range.into(), OperationOptions { signal: Some(signal) })
                        .with_abort(abort))
                },
                |text| text,
            )
            .await
    }

    /// Warm a page's text geometry.
    pub async fn ensure_loaded(&self, page: PageRef) -> Result<(), ViewerError> {
        self.cap.call_promise(move |c| Ok(open(c)?.ensure_loaded(page.into())), |()| ()).await
    }

    pub async fn is_loaded(&self, page: PageRef) -> Result<bool, ViewerError> {
        self.on(move |c| Ok(c.is_loaded(page.into()))).await
    }

    pub async fn is_over_text(&self, page: PageRef, point: Point) -> Result<bool, ViewerError> {
        self.on(move |c| Ok(c.is_over_text(page.into(), point.into()))).await
    }

    /// Mark the following changes as the user's until `end_gesture`.
    pub async fn begin_gesture(&self) -> Result<(), ViewerError> {
        self.on(|c| {
            c.begin_gesture();
            Ok(())
        })
        .await
    }

    /// Start a drag-select anchored at a point; false off text.
    pub async fn begin_gesture_at(&self, page: PageRef, point: Point) -> Result<bool, ViewerError> {
        self.on(move |c| Ok(c.begin_gesture_at(page.into(), point.into()))).await
    }

    pub async fn end_gesture(&self) -> Result<(), ViewerError> {
        self.on(|c| {
            c.end_gesture();
            Ok(())
        })
        .await
    }

    pub async fn is_gesture_active(&self) -> Result<bool, ViewerError> {
        self.on(|c| Ok(c.is_gesture_active())).await
    }

    pub async fn set_highlight_visible(&self, visible: bool) -> Result<(), ViewerError> {
        self.on(move |c| {
            c.set_highlight_visible(visible);
            Ok(())
        })
        .await
    }

    pub async fn is_highlight_visible(&self) -> Result<bool, ViewerError> {
        self.on(|c| Ok(c.is_highlight_visible())).await
    }

    /// The listener is registered on the kernel thread after this returns.
    /// It gets `on_end` when the document closes (at once if it already has).
    pub fn subscribe(&self, listener: Arc<dyn SelectionEventListener>) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.cap.fire(move |c| {
            let ending = listener.clone();
            slot.fill_until_end(
                c.lifetime(),
                || {
                    let changed = {
                        let listener = listener.clone();
                        c.on_changed(move |e| {
                            listener.on_event(SelectionEvent::Changed {
                                event: SelectionChangedEvent {
                                    range: e.range.map(Into::into),
                                    pages: e.pages.iter().map(|p| (*p).into()).collect(),
                                    trigger: e.origin.trigger.into(),
                                },
                            });
                            Ok(())
                        })
                    };
                    let committed = {
                        let listener = listener.clone();
                        c.on_committed(move |e| {
                            listener.on_event(SelectionEvent::Committed {
                                event: SelectionCommittedEvent { range: e.range.into() },
                            });
                            Ok(())
                        })
                    };
                    let cleared = {
                        let listener = listener.clone();
                        c.on_cleared(move |e| {
                            listener.on_event(SelectionEvent::Cleared {
                                event: SelectionClearedEvent { trigger: e.origin.trigger.into() },
                            });
                            Ok(())
                        })
                    };
                    let offs: Vec<Unsubscribe> = vec![changed, committed, cleared];
                    Unsubscribe::new(move || offs.iter().for_each(Unsubscribe::call))
                },
                move || ending.on_end(),
            );
        });
        subscription
    }
}
```

If `list_rects` returns `epdf_geometry::Rect` or `epdf_kernel::geometry::Rect`, `epdf-ffi` converts either. `read_text`'s promise is `Promise<String>`, and `call_promise_cancellable` needs `U: Clone`, which `String` is.

- [x] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p epdf-plugin-selection --features uniffi --test ffi`
Expected: PASS, 11 tests.

Run: `cargo test -p epdf-plugin-selection`
Expected: PASS, conformance unchanged.

- [x] **Step 5: Commit**

```bash
git add crates/plugins/selection
git commit -m "Export the selection plugin through uniffi"
```

### Task 3: Host wiring and cross-plugin tests

**Files:**
- Modify: `crates/epdf-viewer-host/Cargo.toml`, `crates/epdf-viewer-host/src/api.rs`, `crates/epdf-viewer-host/src/session.rs`, `crates/epdf-viewer-host/src/lib.rs`, `crates/epdf-viewer-host/tests/viewer_host.rs`

**Interfaces:**
- Consumes: `SelectionHandle::new`; plan 01's `ViewerConfig.interaction`, `ViewerConfig.feedback`, `InteractionHandle::dispatch_pointer`, `FeedbackConfig`, `PlatformFeedback`.
- Produces: `ViewerConfig.selection: bool`; `ViewerHost::selection(document_id: String) -> Result<Arc<SelectionHandle>, ViewerError>`.

- [x] **Step 1: Write the failing tests**

Append to `crates/epdf-viewer-host/tests/viewer_host.rs`:

```rust
use epdf_ffi::{Modifiers as SelModifiers, Point as SelPoint, PointerGesture, PointerKind as SelKind,
    PointerPageHit as SelHit, PointerPhase as SelPhase, PointerSample as SelSample, Rect as SelRect};
use epdf_plugin_interaction::ffi::{FeedbackConfig, FeedbackImpactWeight, FeedbackNotifyKind, PlatformFeedback};
use epdf_plugin_selection::ffi::SelectionHandle;

fn with_selection(feedback: Option<FeedbackConfig>) -> Arc<ViewerHost> {
    let config = ViewerConfig { interaction: true, selection: true, feedback, ..Default::default() };
    let host = ViewerHost::new(config, Arc::new(Delegate)).unwrap();
    block_on(host.start()).unwrap();
    host
}

/// A text rect on page 0 of the report, found through selection itself.
fn text_rect(selection: &SelectionHandle, page: epdf_ffi::PageRef) -> SelRect {
    block_on(selection.ensure_loaded(page)).unwrap();
    block_on(selection.select_page(page)).unwrap();
    let rect = block_on(selection.rects(page)).unwrap()[0];
    block_on(selection.clear()).unwrap();
    rect
}

fn touch(phase: SelPhase, page: epdf_ffi::PageRef, point: SelPoint) -> SelSample {
    SelSample {
        phase,
        // Viewport points equal page points: a 1:1 view, enough to clear the drag threshold.
        viewport: point,
        page: Some(SelHit { page, point, scale: None, rotation: None, zoom: None }),
        modifiers: SelModifiers::default(),
        click_count: None,
        pointer_type: Some(SelKind::Touch),
        gesture: None,
        source: None,
    }
}

#[test]
fn a_plugin_without_its_requirement_fails_to_build() {
    let config = ViewerConfig { selection: true, ..Default::default() };
    let error = ViewerHost::new(config, Arc::new(Delegate)).err().unwrap();
    let ViewerError::Kernel { detail, .. } = error else { panic!("{error:?}") };
    assert!(detail.contains("requires capability \"interaction\""), "{detail}");
}

#[test]
fn the_selection_plugin_selects_and_reads_through_the_host() {
    let host = with_selection(None);
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let pages = block_on(host.pages(doc.id.clone())).unwrap();
    let selection = block_on(host.selection(doc.id)).unwrap();
    block_on(selection.select_page(pages[0].page)).unwrap();
    let text = block_on(selection.read_text(CancelToken::new())).unwrap();
    assert!(text.contains("Report"), "{text:?}");
}

#[test]
fn a_pointer_drag_over_text_selects_it() {
    let host = with_selection(None);
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let page = block_on(host.pages(doc.id.clone())).unwrap()[0].page;
    let selection = block_on(host.selection(doc.id.clone())).unwrap();
    let interaction = block_on(host.interaction(doc.id)).unwrap();
    let rect = text_rect(&selection, page);
    let y = rect.y + rect.height / 2.0;
    interaction.dispatch_pointer(touch(SelPhase::Down, page, SelPoint { x: rect.x + 1.0, y }));
    interaction.dispatch_pointer(touch(SelPhase::Move, page, SelPoint { x: rect.x + rect.width - 1.0, y }));
    interaction.dispatch_pointer(touch(SelPhase::Up, page, SelPoint { x: rect.x + rect.width - 1.0, y }));
    assert!(block_on(selection.has_selection()).unwrap());
    assert!(!block_on(selection.read_text(CancelToken::new())).unwrap().is_empty());
}

#[test]
fn a_drag_that_starts_off_text_selects_nothing() {
    let host = with_selection(None);
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let page = block_on(host.pages(doc.id.clone())).unwrap()[0].page;
    let selection = block_on(host.selection(doc.id.clone())).unwrap();
    let interaction = block_on(host.interaction(doc.id)).unwrap();
    block_on(selection.ensure_loaded(page)).unwrap();
    // The page's top-left corner: inside the page, outside any text.
    let corner = SelPoint { x: 1.0, y: 1.0 };
    assert!(!block_on(selection.is_over_text(page, corner)).unwrap());
    interaction.dispatch_pointer(touch(SelPhase::Down, page, corner));
    interaction.dispatch_pointer(touch(SelPhase::Move, page, SelPoint { x: 300.0, y: 300.0 }));
    interaction.dispatch_pointer(touch(SelPhase::Up, page, SelPoint { x: 300.0, y: 300.0 }));
    assert!(!block_on(selection.has_selection()).unwrap());
}

#[derive(Default)]
struct Ticks(Mutex<Vec<String>>);
impl PlatformFeedback for Ticks {
    fn selection(&self) {
        self.0.lock().unwrap().push("selection".into());
    }
    fn impact(&self, _weight: Option<FeedbackImpactWeight>) {}
    fn notify(&self, _kind: FeedbackNotifyKind) {}
}

#[test]
fn a_long_press_on_a_word_ticks_the_feedback() {
    let ticks = Arc::new(Ticks::default());
    let host = with_selection(Some(FeedbackConfig { provider: ticks.clone() }));
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let page = block_on(host.pages(doc.id.clone())).unwrap()[0].page;
    let selection = block_on(host.selection(doc.id.clone())).unwrap();
    let interaction = block_on(host.interaction(doc.id)).unwrap();
    let rect = text_rect(&selection, page);
    let mut press = touch(SelPhase::Down, page, SelPoint { x: rect.x + 2.0, y: rect.y + rect.height / 2.0 });
    press.click_count = Some(2);
    press.gesture = Some(PointerGesture::LongPress);
    interaction.dispatch_pointer(press.clone());
    press.phase = SelPhase::Up;
    interaction.dispatch_pointer(press);
    assert!(block_on(selection.has_selection()).unwrap());
    assert_eq!(*ticks.0.lock().unwrap(), ["selection"]);
}

#[test]
fn without_its_flag_the_selection_plugin_is_not_installed() {
    let host = with_interaction(Arc::new(Delegate));
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    assert!(block_on(host.selection(doc.id)).is_err());
}
```

`with_interaction` is plan 01's helper in this file. `has_selection` after the fires is a reply, so the dispatches have run by then. The aliased imports keep these names from clashing with plan 01's and plan 02's imports in the same file; if they don't clash on your branch, drop the aliases.

- [x] **Step 2: Run them to verify they fail**

Run: `cargo test -p epdf-viewer-host --test viewer_host selection`
Expected: FAIL to compile, "no field `selection` on type `ViewerConfig`".

- [x] **Step 3: Wire it**

`crates/epdf-viewer-host/Cargo.toml`, alphabetical:

```toml
epdf-plugin-selection = { path = "../plugins/selection", default-features = false, features = ["uniffi"] }
```

`crates/epdf-viewer-host/src/api.rs`, field at the end of `ViewerConfig`:

```rust
    /// Install `selectionPlugin()`: text selection and extraction, one per
    /// document. Requires `interaction`; without it the viewer fails to
    /// build.
    #[uniffi(default = false)]
    pub selection: bool,
```

`crates/epdf-viewer-host/src/session.rs`, after the previous plugin block:

```rust
        if config.selection {
            plugins.push(epdf_plugin_selection::selection_plugin(Default::default()));
        }
```

`crates/epdf-viewer-host/src/lib.rs`, `use epdf_plugin_selection::ffi::SelectionHandle;` and in the exported impl:

```rust
    /// The text selection of an open document. Needs
    /// `ViewerConfig.selection` (and `interaction`).
    pub async fn selection(&self, document_id: String) -> Result<Arc<SelectionHandle>, ViewerError> {
        let post = self.post.clone();
        self.call(move |s| {
            let cap = s.kernel.capability(&epdf_plugin_selection::SELECTION, Some(&document_id))?;
            Ok(SelectionHandle::new(cap, post))
        })
        .await
    }
```

- [x] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p epdf-viewer-host`
Expected: PASS, the six new tests and every existing one.

- [x] **Step 5: Commit**

```bash
git add crates/epdf-viewer-host
git commit -m "Wire the selection plugin into the viewer host"
```

### Task 4: Swift `SelectionCapability`

**Files:**
- Create: `platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Selection.swift`

**Interfaces:**
- Consumes: generated `SelectionHandle` and records.
- Produces: `SelectionCapability` and `PDFViewer.selection(for:)`.

- [x] **Step 1: Write `Selection.swift`**

```swift
public import EmbedPDFBindings

public typealias SelectionTextPosition = EmbedPDFBindings.SelectionTextPosition
public typealias SelectionTextRange = EmbedPDFBindings.SelectionTextRange
public typealias SelectionInput = EmbedPDFBindings.SelectionInput
public typealias SelectionSegment = EmbedPDFBindings.SelectionSegment
public typealias SelectionEndpoint = EmbedPDFBindings.SelectionEndpoint
public typealias SelectionAnchor = EmbedPDFBindings.SelectionAnchor
public typealias SelectionDirection = EmbedPDFBindings.SelectionDirection
public typealias SelectionSnapshotPage = EmbedPDFBindings.SelectionSnapshotPage
public typealias SelectionSnapshot = EmbedPDFBindings.SelectionSnapshot
public typealias SelectionChangeTrigger = EmbedPDFBindings.SelectionChangeTrigger
public typealias SelectionEvent = EmbedPDFBindings.SelectionEvent
public typealias SelectionChangedEvent = EmbedPDFBindings.SelectionChangedEvent
public typealias SelectionCommittedEvent = EmbedPDFBindings.SelectionCommittedEvent
public typealias SelectionClearedEvent = EmbedPDFBindings.SelectionClearedEvent

/// A document's text selection: select by range, span, word, line or
/// page; read its geometry and its text. Get one with
/// `PDFViewer.selection(for:)`; the viewer needs
/// `ViewerConfig(interaction: true, selection: true)`. It keeps the viewer
/// alive. After its document closes, every call throws `instance-closed`.
///
/// Indices are PDFium character indices, the space search hits use, not
/// offsets into a Swift `String`.
public struct SelectionCapability: Sendable {
  let handle: SelectionHandle
  let viewer: PDFViewer

  /// Whether selection geometry is served (`doc.text.select`).
  public func canSelect() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canSelect() }
  }

  /// Whether text extraction is served (`doc.text.copy`).
  public func canCopy() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canCopy() }
  }

  public func select(_ input: SelectionInput) async throws(PDFError) {
    try await bridged { try await handle.select(input: input) }
  }

  /// Select a search hit's characters.
  public func select(_ hit: SearchHit) async throws(PDFError) {
    try await select(.span(page: hit.page, start: Int64(hit.charStart), count: Int64(hit.charCount)))
  }

  public func selectAll() async throws(PDFError) {
    try await bridged { try await handle.selectAll() }
  }

  public func selectPage(_ page: PageRef) async throws(PDFError) {
    try await bridged { try await handle.selectPage(page: page) }
  }

  /// False when the point has no selectable text.
  public func selectWord(at point: Point, on page: PageRef) async throws(PDFError) -> Bool {
    try await bridged { try await handle.selectWordAt(page: page, point: point) }
  }

  public func selectLine(at point: Point, on page: PageRef) async throws(PDFError) -> Bool {
    try await bridged { try await handle.selectLineAt(page: page, point: point) }
  }

  public func extend(to point: Point, on page: PageRef) async throws(PDFError) {
    try await bridged { try await handle.extendTo(page: page, point: point) }
  }

  public func clear() async throws(PDFError) {
    try await bridged { try await handle.clear() }
  }

  public func hasSelection() async throws(PDFError) -> Bool {
    try await bridged { try await handle.hasSelection() }
  }

  public func snapshot() async throws(PDFError) -> SelectionSnapshot {
    try await bridged { try await handle.snapshot() }
  }

  public func range() async throws(PDFError) -> SelectionTextRange? {
    try await bridged { try await handle.range() }
  }

  public func selectedPages() async throws(PDFError) -> [PageRef] {
    try await bridged { try await handle.selectedPages() }
  }

  public func segments(on page: PageRef) async throws(PDFError) -> [SelectionSegment] {
    try await bridged { try await handle.segments(page: page) }
  }

  public func rects(on page: PageRef) async throws(PDFError) -> [Rect] {
    try await bridged { try await handle.rects(page: page) }
  }

  public func anchor() async throws(PDFError) -> SelectionAnchor? {
    try await bridged { try await handle.anchor() }
  }

  /// The selected text, pages joined with a newline. Throws
  /// `permission-denied` without `doc.text.copy`, and
  /// `operation-cancelled` when the task is cancelled.
  public func readText() async throws(PDFError) -> String {
    try await bridgedCancellable { token in try await handle.readText(cancel: token) }
  }

  /// Text for any range, leaving the selection alone.
  public func readText(in range: SelectionTextRange) async throws(PDFError) -> String {
    try await bridgedCancellable { token in try await handle.readTextInRange(range: range, cancel: token) }
  }

  public func ensureLoaded(_ page: PageRef) async throws(PDFError) {
    try await bridged { try await handle.ensureLoaded(page: page) }
  }

  public func isLoaded(_ page: PageRef) async throws(PDFError) -> Bool {
    try await bridged { try await handle.isLoaded(page: page) }
  }

  public func isOverText(_ point: Point, on page: PageRef) async throws(PDFError) -> Bool {
    try await bridged { try await handle.isOverText(page: page, point: point) }
  }

  public func beginGesture() async throws(PDFError) {
    try await bridged { try await handle.beginGesture() }
  }

  /// False off text.
  public func beginGesture(at point: Point, on page: PageRef) async throws(PDFError) -> Bool {
    try await bridged { try await handle.beginGestureAt(page: page, point: point) }
  }

  public func endGesture() async throws(PDFError) {
    try await bridged { try await handle.endGesture() }
  }

  public func isGestureActive() async throws(PDFError) -> Bool {
    try await bridged { try await handle.isGestureActive() }
  }

  public func setHighlightVisible(_ visible: Bool) async throws(PDFError) {
    try await bridged { try await handle.setHighlightVisible(visible: visible) }
  }

  public func isHighlightVisible() async throws(PDFError) -> Bool {
    try await bridged { try await handle.isHighlightVisible() }
  }

  /// Finishes when the document closes (at once if it already has) or the
  /// viewer is destroyed.
  public var events: AsyncStream<SelectionEvent> {
    viewer.registry.stream { handle.subscribe(listener: $0) }
  }
}

extension EventRelay: SelectionEventListener where Event == SelectionEvent {
  func onEvent(event: SelectionEvent) {
    continuation.yield(event)
  }

  func onEnd() {
    continuation.finish()
  }
}

extension PDFViewer {
  public func selection(for document: PDFDocument) async throws(PDFError) -> SelectionCapability {
    let handle = try await bridged { try await host.selection(documentId: document.id) }
    return SelectionCapability(handle: handle, viewer: self)
  }
}
```

`SearchHit.charStart` and `charCount` are `Double` (`crates/plugins/search/src/ffi.rs`); a hit's values are whole numbers, so `Int64(_)` doesn't lose anything.

- [x] **Step 2: Build**

Run: `bash platforms/apple/build-xcframework.sh && (cd platforms/apple/EmbedPDF && xcodebuild build -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17')`
Expected: BUILD SUCCEEDED.

- [x] **Step 3: Commit**

```bash
git add platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Selection.swift
git commit -m "Add the Swift selection capability"
```

### Task 5: Swift tests

**Files:**
- Create: `platforms/apple/EmbedPDF/Tests/EmbedPDFTests/SelectionTests.swift`

- [ ] **Step 1: Write the tests**

```swift
import CustomDump
import EmbedPDF
import Testing

extension BaseSuite {
  @Suite struct SelectionTests {
    func opened(search: Bool = false) async throws -> (PDFViewer, PDFDocument, SelectionCapability, PageRef) {
      let viewer = try PDFViewer(
        ViewerConfig(search: search ? SearchConfig() : nil, interaction: true, selection: true))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      let page = try await viewer.pages(of: document)[0].page
      return (viewer, document, try await viewer.selection(for: document), page)
    }

    @Test func selectingAPageReadsItsText() async throws {
      let (_, _, selection, page) = try await opened()
      try await selection.selectPage(page)
      let text = try await selection.readText()
      #expect(text.contains("Report"))
      #expect(try await selection.rects(on: page).isEmpty == false)
    }

    @Test func aSearchHitSelectsItsCharacters() async throws {
      let (viewer, document, selection, _) = try await opened(search: true)
      let search = try await viewer.search(in: document)
      let hit = try #require(try await search.findAll(SearchQuery(text: "Report")).first)
      try await selection.select(hit)
      // Search matches case-insensitively by default, so compare that way.
      expectNoDifference(try await selection.readText().lowercased(), "report")
    }

    @Test func selectAndClearAreObserved() async throws {
      let (_, _, selection, page) = try await opened()
      let events = selection.events
      try await selection.selectPage(page)
      try await selection.clear()
      var kinds: [String] = []
      for await event in events {
        switch event {
        case .changed: kinds.append("changed")
        case .committed: kinds.append("committed")
        case .cleared: kinds.append("cleared")
        }
        if kinds.contains("cleared") { break }
      }
      #expect(kinds.first == "changed")
    }

    @Test func readTextCancelsWhenTheTaskIsCancelled() async throws {
      let (_, _, selection, _) = try await opened()
      try await selection.selectAll()
      let task = Task { try await selection.readText() }
      task.cancel()
      do {
        _ = try await task.value
      } catch {
        expectNoDifference((error as? PDFError)?.code, "operation-cancelled")
      }
      #expect(try await selection.readText().isEmpty == false)
    }

    @Test func readTextInRangeCancelsWhenTheTaskIsCancelled() async throws {
      let (_, _, selection, page) = try await opened()
      let range = SelectionTextRange(
        start: SelectionTextPosition(page: page, index: 0),
        end: SelectionTextPosition(page: page, index: 20))
      let task = Task { try await selection.readText(in: range) }
      task.cancel()
      do {
        _ = try await task.value
      } catch {
        expectNoDifference((error as? PDFError)?.code, "operation-cancelled")
      }
      expectNoDifference(try await selection.readText(in: range).utf16.count, 20)
    }

    @Test func aClosedDocumentsCapabilityThrows() async throws {
      let (viewer, document, selection, _) = try await opened()
      try await viewer.close(document)
      await #expect {
        _ = try await selection.hasSelection()
      } throws: { error in
        (error as? PDFError)?.code == "instance-closed"
      }
    }

    @Test func droppingTheStreamUnsubscribes() async throws {
      let (_, _, selection, page) = try await opened()
      let task = Task { for await _ in selection.events {} }
      try await Task.sleep(for: .milliseconds(20))
      task.cancel()
      _ = await task.value
      try await selection.selectPage(page)
    }
  }
}
```

The first page of `report.pdf` contains "Report" (the search tests in `crates/epdf-viewer-host/tests/viewer_host.rs` find it). If the 20-character range on page 0 includes a character PDFium reports as two UTF-16 units, compare with `>= 20` instead; the point of the test is that cancellation doesn't wedge the next read.

- [ ] **Step 2: Run them**

Run: `bash platforms/apple/build-xcframework.sh && (cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17' -only-testing:EmbedPDFTests)`
Expected: TEST SUCCEEDED, including 7 `SelectionTests`.

- [ ] **Step 3: Commit**

```bash
git add platforms/apple/EmbedPDF/Tests/EmbedPDFTests/SelectionTests.swift
git commit -m "Test the Swift selection capability"
```

### Task 6: Docs, CI, merge and clean up

**Files:**
- Modify: `.github/workflows/plugin-rust.yml`, `platforms/apple/README.md`, `docs/superpowers/plans/2026-10-01-swift-package.md`

- [ ] **Step 1: Add the crate to CI**

Append `selection` to the `crates=( ... )` list in `.github/workflows/plugin-rust.yml`, keeping plan-number order (after `stage` if plan 02 has merged, after `interaction` otherwise).

- [ ] **Step 2: Add the README row**

```markdown
| selection | `SelectionCapability` | `interaction: true, selection: true` | `2026-10-06-swift-plugins/03-selection.md` |
```

- [ ] **Step 3: Mark the spec entry delivered**

In `2026-10-01-swift-package.md` Appendix A §A.2, change `**selection** (D, requires `interaction`, optional `feedback`).` to `**selection** (D, requires `interaction`, optional `feedback`), delivered by 03-selection.md.`

- [ ] **Step 4: Run the definition of done**

```bash
cargo test -p epdf-plugin-selection --features uniffi
cargo test -p epdf-ffi -p epdf-viewer-host
cargo clippy -p epdf-plugin-selection -p epdf-viewer-host --all-targets --features epdf-plugin-selection/uniffi -- -D warnings
cargo test -p epdf-plugin-selection
bash platforms/apple/build-xcframework.sh
(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17')
cargo build -p epdf-viewer-host -p uniffi-bindgen
./target/debug/uniffi-bindgen generate --library target/debug/libepdf_viewer_host.dylib --language kotlin --no-format --out-dir "$TMPDIR/epdf-kotlin-check"
```

Expected: all pass. Note the simulator `libepdf_viewer_host.a` size before and after.

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/plugin-rust.yml platforms/apple/README.md docs/superpowers/plans/2026-10-01-swift-package.md
git commit -m "List selection in CI and the Swift README

libepdf_viewer_host.a (simulator, release): <before> -> <after> bytes."
```

- [ ] **Step 6: Merge into the integration branch**

In `.claude/worktrees/swift-plugins-integration`:

```bash
git merge --no-ff research/swift-selection
```

Resolve shared-file conflicts by keeping both blocks in plan-number order.

- [ ] **Step 7: Rerun on the integration branch**

```bash
bash platforms/apple/build-xcframework.sh
(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17')
cargo build -p epdf-viewer-host -p uniffi-bindgen
./target/debug/uniffi-bindgen generate --library target/debug/libepdf_viewer_host.dylib --language kotlin --no-format --out-dir "$TMPDIR/epdf-kotlin-check"
```

Expected: all pass.

- [ ] **Step 8: Remove the worktree and branch**

```bash
git worktree remove .claude/worktrees/swift-selection
git branch -d research/swift-selection
git worktree prune
```

No `--force`. Tick 03 in the umbrella's list.
