# View Manager Swift Wrapper Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Export `epdf-plugin-view-manager` through UniFFI as `ViewManagerHandle`, reach it from `ViewerHost::view_manager()`, and wrap it in Swift as `ViewManagerCapability`, so a native app can lay its open documents out in panes (split view, tab strips) with the same rules the web viewer uses.

**Architecture:** The recipe in `2026-10-01-swift-package.md` Appendix A §A.0, for the first workspace-scoped plugin. The capability comes from `kernel.capability(&VIEW_MANAGER, None)`, lives as long as the kernel, and its lifetime aborts at `destroy`, not at a document close. Every record (`PaneInfo` and the three event payloads) is plain strings and lists, so each derives `uniffi::Record` in place under `cfg_attr`. Every member is synchronous, so every method is `cap.call` and none takes a `CancelToken`. JavaScript-number indexes cross as `u32`.

**Tech Stack:** Rust 2024, `uniffi =0.32.0`, Swift 6.2 tools, Swift Testing, `swift-custom-dump` 1.3, Xcode 27.0, iPhone 17 simulator (iOS 26.0).

**Spec:** `docs/superpowers/plans/2026-10-06-swift-plugins/00-umbrella.md` and the plan it argues from, `docs/superpowers/plans/2026-10-01-swift-package.md` (Global Constraints, Appendix A §A.0 and §A.4). The plugin's contract is `packages/plugin/view-manager/src/contract.ts`, ported in `crates/plugins/view-manager/src/contract.rs`.

## Global Constraints

Every constraint in the umbrella's Global Constraints applies. The ones this plan meets:

- `uniffi` is pinned at `=0.32.0`.
- UniFFI names are unique across the workspace. The records keep the plugin's names (`PaneInfo`, `PaneEvent`, `PaneFocusChangedEvent`, `DocumentMovedEvent`); before deriving, grep for each name in `crates/epdf-ffi/src` and every `crates/plugins/*/src/ffi.rs` and prefix with `ViewManager` if any is taken.
- No `usize`, `isize`, `Rc` or `serde_json::Value` across the boundary. Index arguments (`f64` in the plugin) cross as `u32`.
- Facades don't decide semantics. An unknown pane or document id is a no-op in the plugin, and it stays a no-op in Swift.
- The view manager requires `DOCUMENTS`, which the kernel always provides, so `view_manager: true` needs no other `ViewerConfig` field.
- Swift value assertions use `expectNoDifference`.
- One shared-file block per plugin, in plan-number order.

## Review Focus

1. An index past the end. Swift passes `UInt32.max` to `movePane` or `addDocument`. People expect the plugin's clamp (append), not a crash or a wrap. Pinned in Task 1 (`an_index_past_the_end_appends`).
2. An unknown pane or document id. People expect nothing to happen and no error, exactly as on the web, since the facade adds no semantics. Pinned in Task 1 (`unknown_ids_are_ignored_not_errors`).
3. A document closed while it sits in a pane. People expect it to leave the pane, the pane's active document to fall to a neighbour, and a `documentMoved` event with no destination. Pinned in Task 1 (`closing_a_document_takes_it_out_of_its_pane`) and Task 3 (`closingADocumentTakesItOutOfItsPane`).
4. The first document a viewer opens. People expect a default pane, `pane-1`, focused and holding that document, and no pane before it. Pinned in Task 2 (`the_first_open_creates_the_default_pane`).
5. A viewer destroyed while Swift holds the capability. People expect `instance-closed` from the next call and every event stream to finish, the workspace version of a closed document. Pinned in Task 1 (`a_destroyed_kernels_capability_refuses_with_instance_closed`) and Task 3 (`destroyingTheViewerFinishesTheEvents`).

## Cross-plugin tests

View manager has no optional plugin, and no plugin lists it as optional. None to hand off. A later plan that adds something pane-aware (a split reader sample in `EmbedPDFUI`, say) owns its own tests.

## File Structure

```
crates/plugins/view-manager/
  Cargo.toml                  + uniffi feature, epdf-ffi, futures dev-dep
  src/lib.rs                  + setup_scaffolding, pub mod ffi
  src/contract.rs             + cfg_attr(uniffi::Record) on the four records
  src/controller.rs           + lifetime() under cfg(feature = "uniffi")
  src/ffi.rs                  new: ViewManagerEvent, listener, ViewManagerHandle
  tests/ffi.rs                new
crates/epdf-viewer-host/
  Cargo.toml, src/api.rs, src/session.rs, src/lib.rs
  tests/viewer_host.rs        + view manager through the host
platforms/apple/EmbedPDF/
  Sources/EmbedPDF/Plugins/ViewManager.swift   new
  Tests/EmbedPDFTests/ViewManagerTests.swift   new
.github/workflows/plugin-rust.yml, platforms/apple/README.md
docs/superpowers/plans/2026-10-01-swift-package.md   A.4 view-manager marked delivered
```

---

### Task 1: Worktree, and `ffi.rs` for the view manager

**Files:**
- Create: `crates/plugins/view-manager/src/ffi.rs`, `crates/plugins/view-manager/tests/ffi.rs`
- Modify: `crates/plugins/view-manager/Cargo.toml`, `src/lib.rs`, `src/contract.rs`, `src/controller.rs`

**Interfaces:**
- Consumes: `epdf_ffi::{KernelBound, KernelPost, Subscription, ViewerError}`.
- Produces, in `epdf_plugin_view_manager`:
  - Records, derived in place: `PaneInfo { id: String, document_ids: Vec<String>, active_document_id: Option<String> }`, `PaneEvent { pane_id: String }`, `PaneFocusChangedEvent { pane_id: Option<String> }`, `DocumentMovedEvent { document_id: String, from_pane_id: Option<String>, to_pane_id: Option<String> }`.
  - `ffi::ViewManagerEvent`: `PaneCreated { event: PaneEvent }`, `PaneRemoved { event: PaneEvent }`, `FocusChanged { event: PaneFocusChangedEvent }`, `DocumentMoved { event: DocumentMovedEvent }`.
  - `ffi::ViewManagerEventListener` (`on_event`, `on_end`).
  - `ffi::ViewManagerHandle::new(cap: ViewManagerCapability, post: KernelPost) -> Arc<Self>`, with async `panes`, `pane`, `pane_order`, `focused_pane_id`, `pane_of_document`, `create_pane`, `remove_pane`, `move_pane`, `set_focused_pane`, `split_pane`, `set_active_document`, `add_document`, `remove_document`, `move_document_within`, `move_document_between`, and sync `subscribe`.

The member list is `crates/plugins/view-manager/src/harness.rs` `MEMBERS`, checked against `impl ViewManagerCapability`: `listPanes`, `getPane`, `getPaneOrder`, `getFocusedPaneId`, `getPaneOfDocument`, `createPane`, `removePane`, `movePane`, `setFocusedPane`, `splitPane`, `setActiveDocument`, `addDocument`, `removeDocument`, `moveDocumentWithin`, `moveDocumentBetween`, and the four `on*` hooks, which become `subscribe`. FFI names drop `get_` and `list_` per §A.0.

- [x] **Step 1: Create the worktree**

From the repository root, once `research/swift-plugins` exists (umbrella Task 0):

```bash
git worktree add .claude/worktrees/swift-view-manager -b research/swift-view-manager research/swift-plugins
cd .claude/worktrees/swift-view-manager
```

Fetch PDFium for `darwin-arm64`, `ios-arm64` and `ios-sim-arm64` as `platforms/apple/README.md` describes, then record the archive size before any change:

```bash
bash platforms/apple/build-xcframework.sh
stat -f %z target/aarch64-apple-ios-sim/release/libepdf_viewer_host.a > "$TMPDIR/view-manager-size-before"
```

- [x] **Step 2: Turn on the feature**

`crates/plugins/view-manager/Cargo.toml`:

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

Keep any `[dev-dependencies]` lines already there.

`crates/plugins/view-manager/src/lib.rs`, after the `harness` module:

```rust
#[cfg(feature = "uniffi")]
pub mod ffi;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();
```

`crates/plugins/view-manager/src/contract.rs`: add `#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]` under the existing `#[derive(...)]` of `PaneInfo`, `PaneEvent`, `PaneFocusChangedEvent` and `DocumentMovedEvent`. `PaneId` is `type PaneId = String`, which UniFFI resolves through the alias.

`crates/plugins/view-manager/src/controller.rs`, inside `impl ViewManagerCapability`:

```rust
    /// Aborted when the kernel is destroyed: a workspace plugin outlives
    /// every document. The foreign handle checks it before every call.
    #[cfg(feature = "uniffi")]
    pub(crate) fn lifetime(&self) -> &epdf_kernel::signal::AbortSignal {
        self.0.cx.lifetime()
    }
```

- [x] **Step 3: Write the failing tests**

Create `crates/plugins/view-manager/tests/ffi.rs`:

```rust
#![cfg(feature = "uniffi")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use epdf_ffi::{KernelThread, ViewerError};
use epdf_kernel::{OpenOptions, OpenSource};
use epdf_plugin_view_manager::ffi::{ViewManagerEvent, ViewManagerEventListener, ViewManagerHandle};
use epdf_plugin_view_manager::{DocumentMovedEvent, VIEW_MANAGER, view_manager_plugin};
use futures::executor::block_on;
use plugin_conformance::test_kernel::{TestKernel, settle};
use serde_json::json;

#[derive(Default)]
struct Events(Mutex<Vec<ViewManagerEvent>>, AtomicUsize);

impl ViewManagerEventListener for Events {
    fn on_event(&self, event: ViewManagerEvent) {
        self.0.lock().unwrap().push(event);
    }
    fn on_end(&self) {
        self.1.fetch_add(1, Ordering::SeqCst);
    }
}

/// Two documents in the engine, `a` and `b`. `TestKernel` opens `a`; the
/// test opens `b` itself with `open_b`.
fn spec() -> serde_json::Value {
    json!({ "documents": {
        "a": { "id": "a", "security": { "allows": "*" }, "pages": [] },
        "b": { "id": "b", "security": { "allows": "*" }, "pages": [] }
    } })
}

fn handle() -> (KernelThread<TestKernel>, Arc<ViewManagerHandle>) {
    let thread = KernelThread::spawn(|_| TestKernel::open(vec![view_manager_plugin()], spec())).unwrap();
    let post = thread.post();
    let handle = block_on(thread.call(move |k| {
        ViewManagerHandle::new(k.kernel.capability(&VIEW_MANAGER, None).unwrap(), post)
    }))
    .unwrap();
    (thread, handle)
}

fn document_a(thread: &KernelThread<TestKernel>) -> String {
    block_on(thread.call(|k| k.document_id.clone())).unwrap()
}

fn open_b(thread: &KernelThread<TestKernel>) -> String {
    block_on(thread.call(|k| {
        let opened = k.kernel.documents().open(
            OpenSource::Input(json!({ "kind": "bytes", "id": "b", "bytes": { "$bytes": "" } })),
            OpenOptions::default(),
        );
        settle(&opened).unwrap().as_str().unwrap().to_string()
    }))
    .unwrap()
}

fn code(error: &ViewerError) -> Option<&str> {
    match error {
        ViewerError::Kernel { code, .. } => code.as_deref(),
        ViewerError::Closed => None,
    }
}

#[test]
fn the_open_document_lands_in_the_default_pane() {
    let (thread, vm) = handle();
    let a = document_a(&thread);
    let panes = block_on(vm.panes()).unwrap();
    assert_eq!(panes.len(), 1);
    assert_eq!(panes[0].id, "pane-1");
    assert_eq!(panes[0].document_ids, [a.clone()]);
    assert_eq!(panes[0].active_document_id.as_deref(), Some(a.as_str()));
    assert_eq!(block_on(vm.focused_pane_id()).unwrap().as_deref(), Some("pane-1"));
    assert_eq!(block_on(vm.pane_of_document(a)).unwrap().as_deref(), Some("pane-1"));
}

#[test]
fn splitting_moves_the_document_and_says_so_in_order() {
    let (thread, vm) = handle();
    let a = document_a(&thread);
    let b = open_b(&thread);
    let events = Arc::new(Events::default());
    let _sub = vm.subscribe(events.clone());
    let pane = block_on(vm.split_pane(b.clone())).unwrap();
    assert_eq!(pane, "pane-2");
    assert_eq!(block_on(vm.pane_order()).unwrap(), ["pane-1", "pane-2"]);
    assert_eq!(block_on(vm.pane(pane.clone())).unwrap().unwrap().document_ids, [b.clone()]);
    assert_eq!(block_on(vm.pane("pane-1".into())).unwrap().unwrap().document_ids, [a]);
    let seen = events.0.lock().unwrap();
    assert!(matches!(&seen[0], ViewManagerEvent::PaneCreated { event } if event.pane_id == "pane-2"));
    assert!(matches!(&seen[1], ViewManagerEvent::FocusChanged { event } if event.pane_id.as_deref() == Some("pane-2")));
    assert!(
        matches!(&seen[2], ViewManagerEvent::DocumentMoved { event } if *event == DocumentMovedEvent {
            document_id: b.clone(),
            from_pane_id: Some("pane-1".into()),
            to_pane_id: Some("pane-2".into()),
        }),
        "{seen:?}"
    );
}

#[test]
fn removing_a_pane_hands_its_documents_on() {
    let (thread, vm) = handle();
    let b = open_b(&thread);
    let pane = block_on(vm.split_pane(b.clone())).unwrap();
    block_on(vm.remove_pane(pane, Some("pane-1".into()))).unwrap();
    let panes = block_on(vm.panes()).unwrap();
    assert_eq!(panes.len(), 1);
    assert!(panes[0].document_ids.contains(&b));
}

#[test]
fn an_index_past_the_end_appends() {
    let (thread, vm) = handle();
    let a = document_a(&thread);
    let b = open_b(&thread);
    let second = block_on(vm.create_pane(vec![])).unwrap();
    block_on(vm.move_pane("pane-1".into(), u32::MAX)).unwrap();
    assert_eq!(block_on(vm.pane_order()).unwrap(), [second, "pane-1".to_string()]);
    block_on(vm.move_document_within("pane-1".into(), a.clone(), u32::MAX)).unwrap();
    assert_eq!(block_on(vm.pane("pane-1".into())).unwrap().unwrap().document_ids, [b, a]);
}

#[test]
fn unknown_ids_are_ignored_not_errors() {
    let (thread, vm) = handle();
    let a = document_a(&thread);
    let before = block_on(vm.panes()).unwrap();
    block_on(vm.remove_pane("pane-9".into(), None)).unwrap();
    block_on(vm.move_pane("pane-9".into(), 0)).unwrap();
    block_on(vm.add_document("pane-9".into(), a.clone(), None)).unwrap();
    block_on(vm.set_active_document("pane-1".into(), Some("nope".into()))).unwrap();
    block_on(vm.remove_document("pane-1".into(), "nope".into())).unwrap();
    assert_eq!(block_on(vm.pane("pane-9".into())).unwrap(), None);
    assert_eq!(block_on(vm.pane_of_document("nope".into())).unwrap(), None);
    assert_eq!(block_on(vm.panes()).unwrap(), before);
}

#[test]
fn a_tab_dragged_between_panes_becomes_active_there() {
    let (thread, vm) = handle();
    let a = document_a(&thread);
    let second = block_on(vm.create_pane(vec![])).unwrap();
    block_on(vm.move_document_between("pane-1".into(), second.clone(), a.clone(), Some(0))).unwrap();
    let pane = block_on(vm.pane(second.clone())).unwrap().unwrap();
    assert_eq!(pane.document_ids, [a.clone()]);
    assert_eq!(pane.active_document_id, Some(a));
    assert_eq!(block_on(vm.focused_pane_id()).unwrap(), Some(second));
}

#[test]
fn closing_a_document_takes_it_out_of_its_pane() {
    let (thread, vm) = handle();
    let a = document_a(&thread);
    let b = open_b(&thread);
    block_on(vm.set_active_document("pane-1".into(), Some(a.clone()))).unwrap();
    let events = Arc::new(Events::default());
    let _sub = vm.subscribe(events.clone());
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    let pane = block_on(vm.pane("pane-1".into())).unwrap().unwrap();
    assert_eq!(pane.document_ids, [b.clone()]);
    assert_eq!(pane.active_document_id, Some(b));
    let seen = events.0.lock().unwrap();
    assert!(
        seen.iter().any(|e| matches!(e, ViewManagerEvent::DocumentMoved { event }
            if event.document_id == a && event.to_pane_id.is_none())),
        "{seen:?}"
    );
    assert_eq!(events.1.load(Ordering::SeqCst), 0, "a document close doesn't end a workspace stream");
}

#[test]
fn a_destroyed_kernels_capability_refuses_with_instance_closed() {
    let (thread, vm) = handle();
    let events = Arc::new(Events::default());
    let _sub = vm.subscribe(events.clone());
    block_on(thread.call(|k| settle(&k.kernel.destroy()))).unwrap().unwrap();
    assert_eq!(events.1.load(Ordering::SeqCst), 1);
    for error in [
        block_on(vm.panes()).map(|_| ()).unwrap_err(),
        block_on(vm.create_pane(vec![])).map(|_| ()).unwrap_err(),
    ] {
        assert_eq!(code(&error), Some("instance-closed"), "{error:?}");
    }
    // A stream made after the destroy ends at once.
    let late = Arc::new(Events::default());
    let _late = vm.subscribe(late.clone());
    block_on(thread.call(|_| ())).unwrap();
    assert_eq!(late.1.load(Ordering::SeqCst), 1);
}

#[test]
fn a_cancelled_subscription_hears_nothing_more() {
    let (thread, vm) = handle();
    let events = Arc::new(Events::default());
    let subscription = vm.subscribe(events.clone());
    subscription.cancel();
    block_on(thread.call(|_| ())).unwrap();
    block_on(vm.create_pane(vec![])).unwrap();
    assert!(events.0.lock().unwrap().is_empty());
}
```

- [x] **Step 4: Run them to see them fail**

Run: `cargo test -p epdf-plugin-view-manager --features uniffi --test ffi`
Expected: FAIL to compile, `could not find ffi in epdf_plugin_view_manager`.

- [x] **Step 5: Write `ffi.rs`**

Create `crates/plugins/view-manager/src/ffi.rs`:

```rust
//! The view-manager plugin through uniffi: one workspace handle over the
//! panes, one event enum. Every member is synchronous, so every method is
//! one hop to the kernel thread.
//!
//! The capability is workspace-scoped. Its lifetime aborts when the kernel
//! is destroyed, never at a document close, so a closed document only
//! leaves its pane (a `DocumentMoved` event with no destination).

use std::sync::Arc;

use epdf_ffi::{KernelBound, KernelPost, Subscription, ViewerError};
use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::store::Unsubscribe;

use crate::contract::{DocumentMovedEvent, PaneEvent, PaneFocusChangedEvent, PaneInfo};
use crate::controller::ViewManagerCapability;

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum ViewManagerEvent {
    PaneCreated { event: PaneEvent },
    PaneRemoved { event: PaneEvent },
    FocusChanged { event: PaneFocusChangedEvent },
    DocumentMoved { event: DocumentMovedEvent },
}

#[uniffi::export(with_foreign)]
pub trait ViewManagerEventListener: Send + Sync {
    fn on_event(&self, event: ViewManagerEvent);
    /// The viewer was destroyed: no event follows. Called once, and at once
    /// for a subscription made after the destroy.
    fn on_end(&self);
}

/// The capability, while the kernel lives.
fn open(cap: &ViewManagerCapability) -> Result<&ViewManagerCapability, Error> {
    if cap.lifetime().aborted() {
        return Err(plugin_error(
            code::INSTANCE_CLOSED,
            "view-manager",
            "the viewer this capability belongs to is destroyed",
            None,
            None,
        ));
    }
    Ok(cap)
}

#[derive(uniffi::Object)]
pub struct ViewManagerHandle {
    cap: KernelBound<ViewManagerCapability>,
    post: KernelPost,
}

impl ViewManagerHandle {
    /// On the kernel thread.
    pub fn new(cap: ViewManagerCapability, post: KernelPost) -> Arc<Self> {
        Arc::new(ViewManagerHandle { cap: KernelBound::new(cap, post.clone()), post })
    }
}

#[uniffi::export]
impl ViewManagerHandle {
    /// Panes in display order (`listPanes`).
    pub async fn panes(&self) -> Result<Vec<PaneInfo>, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.list_panes())).await
    }

    pub async fn pane(&self, id: String) -> Result<Option<PaneInfo>, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.get_pane(&id))).await
    }

    pub async fn pane_order(&self) -> Result<Vec<String>, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.get_pane_order())).await
    }

    pub async fn focused_pane_id(&self) -> Result<Option<String>, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.get_focused_pane_id())).await
    }

    /// The first pane in display order that holds the document.
    pub async fn pane_of_document(&self, document_id: String) -> Result<Option<String>, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.get_pane_of_document(&document_id))).await
    }

    /// A new pane, focused; each document moves in from wherever it was.
    /// Answers the new pane's id.
    pub async fn create_pane(&self, document_ids: Vec<String>) -> Result<String, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.create_pane(&document_ids))).await
    }

    /// Its documents go to `move_documents_to`, else the focused pane when
    /// that is another one, else the last remaining pane. An unknown id is
    /// ignored.
    pub async fn remove_pane(
        &self,
        id: String,
        move_documents_to: Option<String>,
    ) -> Result<(), ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.remove_pane(&id, move_documents_to.as_deref()))).await
    }

    /// Clamped into the order; an unknown id is ignored.
    pub async fn move_pane(&self, id: String, to_index: u32) -> Result<(), ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.move_pane(&id, f64::from(to_index)))).await
    }

    pub async fn set_focused_pane(&self, id: Option<String>) -> Result<(), ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.set_focused_pane(id.as_deref()))).await
    }

    /// A new pane showing the document. Answers the new pane's id.
    pub async fn split_pane(&self, document_id: String) -> Result<String, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.split_pane(&document_id))).await
    }

    /// A document the pane doesn't hold is ignored.
    pub async fn set_active_document(
        &self,
        pane_id: String,
        document_id: Option<String>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| Ok(open(c)?.set_active_document(&pane_id, document_id.as_deref())))
            .await
    }

    /// A document another pane holds moves. `index` is clamped; `None`
    /// appends.
    pub async fn add_document(
        &self,
        pane_id: String,
        document_id: String,
        index: Option<u32>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                Ok(open(c)?.add_document(&pane_id, &document_id, index.map(f64::from)))
            })
            .await
    }

    pub async fn remove_document(
        &self,
        pane_id: String,
        document_id: String,
    ) -> Result<(), ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.remove_document(&pane_id, &document_id))).await
    }

    /// Reorder a tab inside its pane; `to_index` is clamped.
    pub async fn move_document_within(
        &self,
        pane_id: String,
        document_id: String,
        to_index: u32,
    ) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                Ok(open(c)?.move_document_within(&pane_id, &document_id, f64::from(to_index)))
            })
            .await
    }

    /// Drag a tab into another pane, where it becomes active and the pane
    /// takes the focus.
    pub async fn move_document_between(
        &self,
        from_pane_id: String,
        to_pane_id: String,
        document_id: String,
        to_index: Option<u32>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                Ok(open(c)?.move_document_between(
                    &from_pane_id,
                    &to_pane_id,
                    &document_id,
                    to_index.map(f64::from),
                ))
            })
            .await
    }

    /// Registered on the kernel thread after this returns; a cancel before
    /// then still takes effect. `on_end` comes when the viewer is destroyed
    /// (at once if it already has been).
    pub fn subscribe(&self, listener: Arc<dyn ViewManagerEventListener>) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.cap.fire(move |c| {
            let ending = listener.clone();
            slot.fill_until_end(
                c.lifetime(),
                || {
                    let (created, removed, focus, moved) =
                        (listener.clone(), listener.clone(), listener.clone(), listener);
                    let offs = [
                        c.on_pane_created(move |e| {
                            created.on_event(ViewManagerEvent::PaneCreated { event: e.clone() });
                            Ok(())
                        }),
                        c.on_pane_removed(move |e| {
                            removed.on_event(ViewManagerEvent::PaneRemoved { event: e.clone() });
                            Ok(())
                        }),
                        c.on_focus_changed(move |e| {
                            focus.on_event(ViewManagerEvent::FocusChanged { event: e.clone() });
                            Ok(())
                        }),
                        c.on_document_moved(move |e| {
                            moved.on_event(ViewManagerEvent::DocumentMoved { event: e.clone() });
                            Ok(())
                        }),
                    ];
                    Unsubscribe::new(move || offs.iter().for_each(Unsubscribe::call))
                },
                move || ending.on_end(),
            );
        });
        subscription
    }
}
```

- [x] **Step 6: Run the tests**

Run: `cargo test -p epdf-plugin-view-manager --features uniffi --test ffi`
Expected: PASS, 9 tests.

Run: `cargo test -p epdf-plugin-view-manager`
Expected: PASS. The vectors and `tests/record.rs` don't see the feature.

- [x] **Step 7: Commit**

```bash
git add crates/plugins/view-manager
git commit -m "Export the view-manager plugin through uniffi"
```

### Task 2: Host wiring

**Files:**
- Modify: `crates/epdf-viewer-host/Cargo.toml`, `src/api.rs`, `src/session.rs`, `src/lib.rs`
- Test: `crates/epdf-viewer-host/tests/viewer_host.rs`

**Interfaces:**
- Consumes: `ViewManagerHandle::new` (Task 1).
- Produces: `ViewerConfig.view_manager: bool` (default `false`), `ViewerHost::view_manager() -> Result<Arc<ViewManagerHandle>, ViewerError>`, which takes no document id.

- [x] **Step 1: Write the failing tests**

Append to `crates/epdf-viewer-host/tests/viewer_host.rs`:

```rust
fn with_view_manager() -> Arc<ViewerHost> {
    let config = ViewerConfig { view_manager: true, ..Default::default() };
    let host = ViewerHost::new(config, Arc::new(Delegate)).unwrap();
    block_on(host.start()).unwrap();
    host
}

#[test]
fn the_first_open_creates_the_default_pane() {
    let host = with_view_manager();
    let vm = block_on(host.view_manager()).unwrap();
    assert!(block_on(vm.panes()).unwrap().is_empty(), "no pane before a document opens");
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let panes = block_on(vm.panes()).unwrap();
    assert_eq!(panes.len(), 1);
    assert_eq!(panes[0].id, "pane-1");
    assert_eq!(panes[0].document_ids, [doc.id.clone()]);
    assert_eq!(block_on(vm.focused_pane_id()).unwrap().as_deref(), Some("pane-1"));
}

#[test]
fn a_second_document_splits_into_its_own_pane() {
    let host = with_view_manager();
    let vm = block_on(host.view_manager()).unwrap();
    let report = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let colors = block_on(host.open_document(test_support::fixture(test_support::COLORS), options()))
        .unwrap();
    let pane = block_on(vm.split_pane(colors.id.clone())).unwrap();
    assert_eq!(block_on(vm.pane_of_document(colors.id)).unwrap(), Some(pane));
    assert_eq!(block_on(vm.pane_of_document(report.id)).unwrap().as_deref(), Some("pane-1"));
}

#[test]
fn without_its_flag_the_view_manager_is_not_installed() {
    let host = started();
    assert!(block_on(host.view_manager()).is_err());
}
```

- [x] **Step 2: Run them to see them fail**

Run: `cargo test -p epdf-viewer-host --test viewer_host view_manager`
Expected: FAIL to compile, `no field view_manager on ViewerConfig`.

- [x] **Step 3: Wire the plugin in**

`crates/epdf-viewer-host/Cargo.toml`, alphabetical among the plugins:

```toml
epdf-plugin-view-manager = { path = "../plugins/view-manager", default-features = false, features = ["uniffi"] }
```

`crates/epdf-viewer-host/src/api.rs`, in `ViewerConfig`, after the fields earlier plans added:

```rust
    /// Install `viewManagerPlugin()`: workspace panes that partition the
    /// open documents (split views and tab strips).
    #[uniffi(default = false)]
    pub view_manager: bool,
```

`crates/epdf-viewer-host/src/session.rs`, in the plugin list after the earlier plans' blocks:

```rust
        if config.view_manager {
            plugins.push(epdf_plugin_view_manager::view_manager_plugin());
        }
```

`crates/epdf-viewer-host/src/lib.rs`: `use epdf_plugin_view_manager::ffi::ViewManagerHandle;`, and in the exported `impl ViewerHost`:

```rust
    /// The workspace's panes. Needs `ViewerConfig.view_manager`; without it
    /// the kernel refuses the token. One capability for the whole viewer:
    /// it takes no document id and lives until `destroy`.
    pub async fn view_manager(&self) -> Result<Arc<ViewManagerHandle>, ViewerError> {
        let post = self.post.clone();
        self.call(move |s| {
            let cap = s.kernel.capability(&epdf_plugin_view_manager::VIEW_MANAGER, None)?;
            Ok(ViewManagerHandle::new(cap, post))
        })
        .await
    }
```

- [x] **Step 4: Run the tests**

Run: `cargo test -p epdf-ffi -p epdf-viewer-host`
Expected: PASS.

- [x] **Step 5: Commit**

```bash
git add crates/epdf-viewer-host
git commit -m "Wire the view-manager plugin into the viewer host"
```

### Task 3: Swift `ViewManagerCapability`

**Files:**
- Create: `platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/ViewManager.swift`
- Test: `platforms/apple/EmbedPDF/Tests/EmbedPDFTests/ViewManagerTests.swift`

**Interfaces:**
- Consumes: the generated `ViewManagerHandle`, `ViewerHost.viewManager()`, `PaneInfo` and the event records.
- Produces: `ViewManagerCapability` and `PDFViewer.viewManager()`. Methods: `panes()`, `pane(_:)`, `paneOrder()`, `focusedPaneId()`, `pane(holding:)`, `createPane(documentIds:)`, `removePane(_:movingDocumentsTo:)`, `movePane(_:to:)`, `setFocusedPane(_:)`, `splitPane(showing:)`, `setActiveDocument(_:in:)`, `addDocument(_:to:at:)`, `removeDocument(_:from:)`, `moveDocument(_:within:to:)`, `moveDocument(_:from:to:at:)`, and `events`. Document arguments are ids (`PDFDocument.id`), because `PaneInfo.documentIds` is.

- [x] **Step 1: Write the failing tests**

Create `platforms/apple/EmbedPDF/Tests/EmbedPDFTests/ViewManagerTests.swift`:

```swift
import CustomDump
import EmbedPDF
import Testing

extension BaseSuite {
  @Suite struct ViewManagerTests {
    func opened() async throws -> (PDFViewer, PDFDocument, ViewManagerCapability) {
      let viewer = try PDFViewer(ViewerConfig(viewManager: true))
      try await viewer.start()
      let manager = try await viewer.viewManager()
      let document = try await viewer.open(Fixture.data("report"))
      return (viewer, document, manager)
    }

    @Test func theFirstDocumentLandsInTheDefaultPane() async throws {
      let (_, document, manager) = try await opened()
      let panes = try await manager.panes()
      expectNoDifference(panes, [PaneInfo(id: "pane-1", documentIds: [document.id], activeDocumentId: document.id)])
      let focused = try await manager.focusedPaneId()
      expectNoDifference(focused, "pane-1")
    }

    @Test func splittingIsObserved() async throws {
      let (viewer, _, manager) = try await opened()
      let colors = try await viewer.open(Fixture.data("colors"))
      let events = manager.events
      let pane = try await manager.splitPane(showing: colors.id)
      for await event in events {
        guard case let .documentMoved(event) = event else { continue }
        expectNoDifference(event, DocumentMovedEvent(documentId: colors.id, fromPaneId: "pane-1", toPaneId: pane))
        break
      }
      let holder = try await manager.pane(holding: colors.id)
      expectNoDifference(holder, pane)
    }

    @Test func closingADocumentTakesItOutOfItsPane() async throws {
      let (viewer, document, manager) = try await opened()
      let colors = try await viewer.open(Fixture.data("colors"))
      try await manager.setActiveDocument(document.id, in: "pane-1")
      try await viewer.close(document)
      let pane = try await manager.pane("pane-1")
      expectNoDifference(pane?.documentIds, [colors.id])
      expectNoDifference(pane?.activeDocumentId, colors.id)
    }

    @Test func anIndexPastTheEndAppends() async throws {
      let (_, _, manager) = try await opened()
      let second = try await manager.createPane()
      try await manager.movePane("pane-1", to: .max)
      let order = try await manager.paneOrder()
      expectNoDifference(order, [second, "pane-1"])
    }

    @Test func droppingTheStreamUnsubscribes() async throws {
      let (_, _, manager) = try await opened()
      let task = Task { for await _ in manager.events {} }
      try await Task.sleep(for: .milliseconds(20))
      task.cancel()
      _ = await task.value
      _ = try await manager.createPane()
    }

    @Test func destroyingTheViewerFinishesTheEvents() async throws {
      let (viewer, _, manager) = try await opened()
      let events = Task { for await _ in manager.events {} }
      try await Task.sleep(for: .milliseconds(50))
      try await viewer.destroy()
      await events.value
    }

    @Test func aDestroyedViewersCapabilityThrows() async throws {
      let viewer = try PDFViewer(ViewerConfig(viewManager: true))
      try await viewer.start()
      let manager = try await viewer.viewManager()
      try await viewer.destroy()
      await #expect {
        _ = try await manager.panes()
      } throws: { error in
        // `destroy` stops the kernel and then the thread, so the call
        // answers either the plugin's `instance-closed` or the viewer's.
        (error as? PDFError)?.code == "instance-closed"
      }
    }
  }
}
```

There's no `aClosedDocumentsCapabilityThrows` (the capability belongs to no document) and no cancellation test (no member takes a token), as the umbrella allows for a workspace plugin. `aDestroyedViewersCapabilityThrows` stands in for the first.

- [x] **Step 2: Run them to see them fail**

```bash
bash platforms/apple/build-xcframework.sh
(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17' -only-testing:EmbedPDFTests/BaseSuite/ViewManagerTests)
```

Expected: FAIL to compile, `value of type 'PDFViewer' has no member 'viewManager'`.

- [x] **Step 3: Write the capability**

Create `platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/ViewManager.swift`:

```swift
public import EmbedPDFBindings

public typealias PaneInfo = EmbedPDFBindings.PaneInfo
public typealias PaneEvent = EmbedPDFBindings.PaneEvent
public typealias PaneFocusChangedEvent = EmbedPDFBindings.PaneFocusChangedEvent
public typealias DocumentMovedEvent = EmbedPDFBindings.DocumentMovedEvent
public typealias ViewManagerEvent = EmbedPDFBindings.ViewManagerEvent

/// Panes over the open documents: each pane is a tab strip with one active
/// document, and the panes partition the documents. Get it with
/// `PDFViewer.viewManager()`; the viewer needs
/// `ViewerConfig(viewManager: true)`. One capability for the whole viewer.
/// It keeps the viewer alive, and after `destroy` every call throws
/// `instance-closed`. An unknown pane or document id is ignored, not an
/// error, as on the web.
public struct ViewManagerCapability: Sendable {
  let handle: ViewManagerHandle
  let viewer: PDFViewer

  /// In display order.
  public func panes() async throws(PDFError) -> [PaneInfo] {
    try await bridged { try await handle.panes() }
  }

  public func pane(_ id: String) async throws(PDFError) -> PaneInfo? {
    try await bridged { try await handle.pane(id: id) }
  }

  public func paneOrder() async throws(PDFError) -> [String] {
    try await bridged { try await handle.paneOrder() }
  }

  public func focusedPaneId() async throws(PDFError) -> String? {
    try await bridged { try await handle.focusedPaneId() }
  }

  /// The pane that holds the document, if any.
  public func pane(holding documentId: String) async throws(PDFError) -> String? {
    try await bridged { try await handle.paneOfDocument(documentId: documentId) }
  }

  /// A new pane, focused. Each document moves in from wherever it was.
  public func createPane(documentIds: [String] = []) async throws(PDFError) -> String {
    try await bridged { try await handle.createPane(documentIds: documentIds) }
  }

  /// Its documents go to `target`, else the focused pane, else the last one.
  public func removePane(_ id: String, movingDocumentsTo target: String? = nil) async throws(PDFError) {
    try await bridged { try await handle.removePane(id: id, moveDocumentsTo: target) }
  }

  /// Clamped into the order.
  public func movePane(_ id: String, to index: UInt32) async throws(PDFError) {
    try await bridged { try await handle.movePane(id: id, toIndex: index) }
  }

  public func setFocusedPane(_ id: String?) async throws(PDFError) {
    try await bridged { try await handle.setFocusedPane(id: id) }
  }

  /// A new pane showing the document.
  public func splitPane(showing documentId: String) async throws(PDFError) -> String {
    try await bridged { try await handle.splitPane(documentId: documentId) }
  }

  public func setActiveDocument(_ documentId: String?, in paneId: String) async throws(PDFError) {
    try await bridged { try await handle.setActiveDocument(paneId: paneId, documentId: documentId) }
  }

  /// A document another pane holds moves. `nil` appends.
  public func addDocument(_ documentId: String, to paneId: String, at index: UInt32? = nil)
    async throws(PDFError)
  {
    try await bridged { try await handle.addDocument(paneId: paneId, documentId: documentId, index: index) }
  }

  public func removeDocument(_ documentId: String, from paneId: String) async throws(PDFError) {
    try await bridged { try await handle.removeDocument(paneId: paneId, documentId: documentId) }
  }

  /// Reorder a tab inside its pane.
  public func moveDocument(_ documentId: String, within paneId: String, to index: UInt32)
    async throws(PDFError)
  {
    try await bridged {
      try await handle.moveDocumentWithin(paneId: paneId, documentId: documentId, toIndex: index)
    }
  }

  /// Drag a tab into another pane, where it becomes active.
  public func moveDocument(
    _ documentId: String, from source: String, to destination: String, at index: UInt32? = nil
  ) async throws(PDFError) {
    try await bridged {
      try await handle.moveDocumentBetween(
        fromPaneId: source, toPaneId: destination, documentId: documentId, toIndex: index)
    }
  }

  /// Pane events. Finishes when the viewer is destroyed (at once if it
  /// already has been).
  public var events: AsyncStream<ViewManagerEvent> {
    viewer.registry.stream { handle.subscribe(listener: $0) }
  }
}

extension EventRelay: ViewManagerEventListener where Event == ViewManagerEvent {
  func onEvent(event: ViewManagerEvent) {
    continuation.yield(event)
  }

  func onEnd() {
    continuation.finish()
  }
}

extension PDFViewer {
  public func viewManager() async throws(PDFError) -> ViewManagerCapability {
    let handle = try await bridged { try await host.viewManager() }
    return ViewManagerCapability(handle: handle, viewer: self)
  }
}
```

- [x] **Step 4: Run the tests**

```bash
bash platforms/apple/build-xcframework.sh
(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17')
```

Expected: PASS, every suite, `ViewManagerTests` with 7 tests.

- [x] **Step 5: Commit**

```bash
git add platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/ViewManager.swift platforms/apple/EmbedPDF/Tests/EmbedPDFTests/ViewManagerTests.swift
git commit -m "Add the Swift view-manager capability"
```

### Task 4: Docs, CI, merge and clean up

**Files:**
- Modify: `.github/workflows/plugin-rust.yml`, `platforms/apple/README.md`, `docs/superpowers/plans/2026-10-01-swift-package.md`

- [ ] **Step 1: CI**

Append `view-manager` to the `crates=( ... )` list in `.github/workflows/plugin-rust.yml`, in plan-number order.

- [x] **Step 2: README row**

In `platforms/apple/README.md`'s `## Plugins` table, in plan-number order:

```markdown
| view-manager | `ViewManagerCapability` | `viewManager: true` | `2026-10-06-swift-plugins/11-view-manager.md` |
```

- [x] **Step 3: Mark the spec entry delivered**

In `docs/superpowers/plans/2026-10-01-swift-package.md` §A.4, change `**view-manager** (W, requires documents).` to `**view-manager** (W, requires documents), delivered by 11-view-manager.md.`

- [x] **Step 4: Run the definition of done**

```bash
cargo test -p epdf-plugin-view-manager --features uniffi
cargo test -p epdf-ffi -p epdf-viewer-host
cargo clippy -p epdf-plugin-view-manager -p epdf-viewer-host --all-targets --features epdf-plugin-view-manager/uniffi -- -D warnings
cargo test -p epdf-plugin-view-manager
bash platforms/apple/build-xcframework.sh
(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17')
cargo build -p epdf-viewer-host -p uniffi-bindgen
./target/debug/uniffi-bindgen generate --library target/debug/libepdf_viewer_host.dylib --language kotlin --no-format --out-dir "$TMPDIR/epdf-kotlin-check"
```

Expected: every command passes.

- [x] **Step 5: Commit, with the archive size**

```bash
before=$(cat "$TMPDIR/view-manager-size-before")
after=$(stat -f %z target/aarch64-apple-ios-sim/release/libepdf_viewer_host.a)
git add .github/workflows/plugin-rust.yml platforms/apple/README.md docs/superpowers/plans/2026-10-01-swift-package.md
git commit -m "List view-manager in CI and the Apple README" -m "libepdf_viewer_host.a (ios-sim, release): $before -> $after bytes"
```

- [ ] **Step 6: Merge into the integration branch**

In the integration worktree umbrella Task 0 created, which has `research/swift-plugins` checked out:

```bash
cd .claude/worktrees/swift-plugins-integration
git merge --no-ff research/swift-view-manager -m "Merge the view-manager Swift wrapper"
```

Resolve a conflict in a shared file by keeping both blocks in plan-number order.

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

From the repository root, without `--force`:

```bash
git worktree remove .claude/worktrees/swift-view-manager
git branch -d research/swift-view-manager
git worktree prune
```

Then tick `11 view-manager` in the umbrella's Tasks 1 to 16 list.
