# Shell Swift Wrapper Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Decide whether `epdf-plugin-shell` ships to Swift, and since the answer is yes, export it through UniFFI as `ShellHandle`, reach it from `ViewerHost::shell(document_id)`, and wrap it in Swift as `ShellCapability`.

**Architecture:** The recipe in `2026-10-01-swift-package.md` Appendix A §A.0. Shell is pure state: which named surfaces (panels, sheets, inspectors) and menus are open, per document, with exclusivity tags. Every member is synchronous, so every method is `cap.call` with no `CancelToken`. One field resists typing: a surface's `props` is an opaque JSON object by contract (`SurfaceProps = serde_json::Map<String, Value>`), so it crosses as a JSON string, `props_json`, validated on the way in. Swift adds `Encodable`/`Decodable` conveniences over that string and nothing else.

**Tech Stack:** Rust 2024, `uniffi =0.32.0`, Swift 6.2 tools, Swift Testing, `swift-custom-dump` 1.3, Xcode 27.0, iPhone 17 simulator (iOS 26.0).

**Spec:** `docs/superpowers/plans/2026-10-06-swift-plugins/00-umbrella.md` and the plan it argues from, `docs/superpowers/plans/2026-10-01-swift-package.md` (Global Constraints, Appendix A §A.0 and §A.4, which says "Swift exposure is optional ... B3 decides whether a native reader wants it"). The plugin's contract is `packages/plugin/shell/src/contract.ts`, ported in `crates/plugins/shell/src/contract.rs`.

## Global Constraints

Every constraint in the umbrella's Global Constraints applies. The ones this plan meets:

- `uniffi` is pinned at `=0.32.0`.
- UniFFI names are unique across the workspace. Every record is prefixed `Shell`.
- No `serde_json::Value` across the boundary. `props` is the one opaque field the plugin's contract defines; it crosses as `Option<String>` holding a JSON object, and anything else is `invalid-input`. That's the decision in Task 1, with its cost.
- Kotlin method names: the TypeScript's `open`, `close`, `toggle` and `closeAll` become `open_surface`, `close_surface`, `toggle_surface` and `close_all_surfaces`. Every generated Kotlin object already has `close()` (it's `AutoCloseable`), and the suffix keeps the surface verbs distinct from the menu verbs.
- Facades don't decide semantics. The exclusivity rule, the JavaScript property order of surfaces, and the inherited-name quirk all stay in Rust.
- Swift value assertions use `expectNoDifference`.
- One shared-file block per plugin, in plan-number order.

## Review Focus

1. `props_json` that isn't a JSON object (an array, a number, malformed text). People expect `invalid-input` and the surface untouched, not a silently dropped prop set. Pinned in Task 2 (`props_that_are_not_an_object_are_invalid_input`).
2. Opening a surface with an exclusivity tag. People expect every other open surface with that tag to close, and a `surfaceClosed` event for each, before the `surfaceOpened`. Pinned in Task 2 (`an_exclusive_open_closes_its_siblings_and_says_so`).
3. A snapshot saved and restored. People expect surfaces back in the same order, which is JavaScript property order (integer-like ids first), with their tags and props. Pinned in Task 2 (`a_snapshot_round_trips_in_property_order`).
4. A surface id that names an `Object.prototype` member, like `toString`. The plugin finds it as a JavaScript lookup does and reports no open flag (`SurfaceState::open` is `None`). People expect a closed surface, not a crash or a missing record. Pinned in Task 2 (`an_inherited_name_reads_as_closed`).
5. Two documents open at once. People expect each to keep its own panels and menus, and a document's close to end only its own event stream. Pinned in Task 4 (`eachDocumentKeepsItsOwnSurfaces`, `closingTheDocumentFinishesTheEvents`).

## Cross-plugin tests

Shell has no optional plugin of its own. One plugin lists shell as optional:

- commands (plan 14): a command whose target is a panel, modal or menu toggles it through `ShellCapability::toggle`/`toggle_menu`, and `is_active` reads it back (`crates/plugins/commands/src/controller.rs:99-107`, `:285-296`). Plan 14 owns that test, in its host task and in `CommandsTests.swift`, using `ShellCapability` from this plan to observe the toggle.

## File Structure

```
crates/plugins/shell/
  README.md                   + "Native exposure" section (the decision)
  Cargo.toml                  + uniffi feature, epdf-ffi, futures dev-dep
  src/lib.rs                  + setup_scaffolding, pub mod ffi
  src/controller.rs           + lifetime() under cfg(feature = "uniffi")
  src/ffi.rs                  new: Shell* records, ShellEvent, ShellHandle
  tests/ffi.rs                new
crates/epdf-viewer-host/
  Cargo.toml, src/api.rs, src/session.rs, src/lib.rs
  tests/viewer_host.rs        + shell through the host
platforms/apple/EmbedPDF/
  Sources/EmbedPDF/Plugins/Shell.swift   new
  Tests/EmbedPDFTests/ShellTests.swift   new
.github/workflows/plugin-rust.yml, platforms/apple/README.md
docs/superpowers/plans/2026-10-01-swift-package.md   A.4 shell marked delivered
```

---

### Task 1: Worktree, and the decision to ship shell

**Files:**
- Modify: `crates/plugins/shell/README.md`

**Interfaces:**
- Consumes: nothing.
- Produces: the decision every later task builds on: shell ships, with `props` as `props_json: Option<String>`.

We're shipping shell to Swift. Appendix A §A.4 left it open because shell models web chrome, and a native app has its own: SwiftUI decides where an inspector or a sheet lives, and nothing here should try to. But shell doesn't render chrome. It stores which named surfaces are open, per document, and that's a job a native reader has too. Three reasons tip it:

1. Commands (plan 14) needs it. A command whose target is a panel, modal or menu toggles shell state and reports itself active from it (`crates/plugins/commands/src/controller.rs:99-107`, `:285-296`). Without shell in the kernel those commands do nothing, and without a Swift handle the app can't see what they did. A native toolbar built on commands needs both.
2. Per-document panel state comes free. Shell is document-scoped (`crates/plugins/shell/src/lib.rs:28-35`), so switching tabs restores each document's open panels, and `snapshot`/`apply_snapshot` persist them. A Swift app would otherwise write that itself, and its rules would drift from the web's exclusivity rules.
3. It's small. Twenty members, all synchronous, no engine paths, no ports.

The cost is the props field. `SurfaceProps` is an opaque JSON object in the TypeScript contract ("which annotation a style panel edits, say"), with no schema to mirror. Crossing it as a JSON string keeps it faithful and keeps Rust in charge of validating it, but Swift sees a `String` where it would like a type, and every prop read pays a JSON parse. The Swift layer narrows the gap with `Codable` helpers (Task 4). We'd rather pay that than invent a schema the web doesn't have, or drop props and break commands that pass them.

If a native app never wants shell, `ViewerConfig.shell` stays `false` and none of this is linked into its behaviour, though the exports still add their scaffolding to the archive. Task 5 records that size.

- [x] **Step 1: Create the worktree**

From the repository root, once `research/swift-plugins` exists (umbrella Task 0):

```bash
git worktree add .claude/worktrees/swift-shell -b research/swift-shell research/swift-plugins
cd .claude/worktrees/swift-shell
```

Fetch PDFium for `darwin-arm64`, `ios-arm64` and `ios-sim-arm64` as `platforms/apple/README.md` describes, then record the archive size before any change:

```bash
bash platforms/apple/build-xcframework.sh
stat -f %z target/aarch64-apple-ios-sim/release/libepdf_viewer_host.a > "$TMPDIR/shell-size-before"
```

- [x] **Step 2: Record the decision in the crate's README**

Append to `crates/plugins/shell/README.md`:

```markdown
## Native exposure

Shell ships to Swift (`ShellCapability`, `2026-10-06-swift-plugins/12-shell.md`). It stores which
named surfaces and menus are open per document; the app still owns their UI. The commands plugin
toggles panel, modal and menu targets through it, and per-document panel state and snapshots come
with it, so a native reader wants it for the same reasons the web viewer does.

`props` is opaque by contract, so it crosses UniFFI as `props_json`, a JSON object in a string.
Anything that isn't an object is refused with `invalid-input`. The Swift layer adds `Codable`
helpers over the string and no schema of its own.
```

- [x] **Step 3: Commit**

```bash
git add crates/plugins/shell/README.md
git commit -m "Record why the shell plugin ships to Swift"
```

### Task 2: `ffi.rs` for shell

**Files:**
- Create: `crates/plugins/shell/src/ffi.rs`, `crates/plugins/shell/tests/ffi.rs`
- Modify: `crates/plugins/shell/Cargo.toml`, `src/lib.rs`, `src/controller.rs`

**Interfaces:**
- Consumes: `epdf_ffi::{KernelBound, KernelPost, Subscription, ViewerError}`.
- Produces, in `epdf_plugin_shell::ffi`:
  - `ShellSurface { id: String, open: bool, exclusive: Option<String>, props_json: Option<String> }`: a surface's state, and one entry of a snapshot.
  - `ShellOpenOptions { exclusive: Option<String>, props_json: Option<String> }`, both defaulting to `None`.
  - `ShellSnapshot { surfaces: Vec<ShellSurface>, open_menus: Vec<String> }`: surfaces in JavaScript property order, menus in opening order.
  - `ShellSurfaceEvent { id: String, props_json: Option<String> }`, `ShellMenuEvent { id: String }`.
  - `ShellEvent`: `SurfaceOpened { event: ShellSurfaceEvent }`, `SurfaceClosed { event: ShellSurfaceEvent }`, `MenuOpened { event: ShellMenuEvent }`, `MenuClosed { event: ShellMenuEvent }`.
  - `ShellEventListener` (`on_event`, `on_end`).
  - `ShellHandle::new(cap: ShellCapability, post: KernelPost) -> Arc<Self>`, with async `is_open`, `surface`, `open_surfaces`, `open_surface`, `close_surface`, `toggle_surface`, `update_surface_props`, `close_all_surfaces`, `is_menu_open`, `open_menus`, `open_menu`, `close_menu`, `toggle_menu`, `close_all_menus`, `snapshot`, `apply_snapshot`, and sync `subscribe`.

The member list is `crates/plugins/shell/src/harness.rs` `MEMBERS`, checked against `impl ShellCapability`: `isOpen`, `getSurface`, `listOpenSurfaces`, `open`, `close`, `toggle`, `updateSurfaceProps`, `closeAll`, `isMenuOpen`, `listOpenMenus`, `openMenu`, `closeMenu`, `toggleMenu`, `closeAllMenus`, `getSnapshot`, `applySnapshot`, and four `on*` hooks that become `subscribe`.

- [x] **Step 1: Turn on the feature**

`crates/plugins/shell/Cargo.toml`:

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

`crates/plugins/shell/src/lib.rs`, after the `harness` module:

```rust
#[cfg(feature = "uniffi")]
pub mod ffi;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();
```

`crates/plugins/shell/src/controller.rs`, inside `impl ShellCapability`:

```rust
    /// Aborted when the document closes. The foreign handle, which can
    /// outlive the close, checks it before every call and ends its event
    /// streams when it aborts.
    #[cfg(feature = "uniffi")]
    pub(crate) fn lifetime(&self) -> &epdf_kernel::signal::AbortSignal {
        self.0.cx.lifetime()
    }
```

- [x] **Step 2: Write the failing tests**

Create `crates/plugins/shell/tests/ffi.rs`:

```rust
#![cfg(feature = "uniffi")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use epdf_ffi::{KernelThread, ViewerError};
use epdf_plugin_shell::ffi::{
    ShellEvent, ShellEventListener, ShellHandle, ShellMenuEvent, ShellOpenOptions, ShellSnapshot,
    ShellSurface, ShellSurfaceEvent,
};
use epdf_plugin_shell::{SHELL, shell_plugin};
use futures::executor::block_on;
use plugin_conformance::test_kernel::{TestKernel, one_document};

#[derive(Default)]
struct Events(Mutex<Vec<ShellEvent>>, AtomicUsize);

impl ShellEventListener for Events {
    fn on_event(&self, event: ShellEvent) {
        self.0.lock().unwrap().push(event);
    }
    fn on_end(&self) {
        self.1.fetch_add(1, Ordering::SeqCst);
    }
}

fn handle() -> (KernelThread<TestKernel>, Arc<ShellHandle>) {
    let thread = KernelThread::spawn(|_| TestKernel::open(vec![shell_plugin()], one_document())).unwrap();
    let post = thread.post();
    let handle =
        block_on(thread.call(move |k| ShellHandle::new(k.capability(&SHELL).unwrap(), post))).unwrap();
    (thread, handle)
}

fn code(error: &ViewerError) -> Option<&str> {
    match error {
        ViewerError::Kernel { code, .. } => code.as_deref(),
        ViewerError::Closed => None,
    }
}

fn tagged(tag: &str) -> Option<ShellOpenOptions> {
    Some(ShellOpenOptions { exclusive: Some(tag.into()), props_json: None })
}

#[test]
fn an_opened_surface_reads_back_with_its_props() {
    let (_thread, shell) = handle();
    let options = ShellOpenOptions {
        exclusive: None,
        props_json: Some(r#"{"annotationId":"a1","page":3}"#.into()),
    };
    block_on(shell.open_surface("style".into(), Some(options))).unwrap();
    assert!(block_on(shell.is_open("style".into())).unwrap());
    let surface = block_on(shell.surface("style".into())).unwrap().unwrap();
    assert_eq!(surface.id, "style");
    assert!(surface.open);
    let props: serde_json::Value = serde_json::from_str(surface.props_json.as_deref().unwrap()).unwrap();
    assert_eq!(props, serde_json::json!({ "annotationId": "a1", "page": 3 }));
    assert_eq!(block_on(shell.surface("never".into())).unwrap(), None);
}

#[test]
fn props_that_are_not_an_object_are_invalid_input() {
    let (_thread, shell) = handle();
    for bad in ["[1,2]", "3", "{not json"] {
        let options = ShellOpenOptions { exclusive: None, props_json: Some(bad.into()) };
        let error = block_on(shell.open_surface("style".into(), Some(options))).unwrap_err();
        assert_eq!(code(&error), Some("invalid-input"), "{bad}: {error:?}");
        let error = block_on(shell.update_surface_props("style".into(), bad.into())).unwrap_err();
        assert_eq!(code(&error), Some("invalid-input"), "{bad}: {error:?}");
    }
    assert!(!block_on(shell.is_open("style".into())).unwrap());
}

#[test]
fn an_exclusive_open_closes_its_siblings_and_says_so() {
    let (_thread, shell) = handle();
    block_on(shell.open_surface("thumbnails".into(), tagged("left"))).unwrap();
    block_on(shell.open_surface("outline".into(), None)).unwrap();
    let events = Arc::new(Events::default());
    let _sub = shell.subscribe(events.clone());
    block_on(shell.open_surface("search".into(), tagged("left"))).unwrap();
    assert!(!block_on(shell.is_open("thumbnails".into())).unwrap());
    assert!(block_on(shell.is_open("outline".into())).unwrap(), "an untagged surface stays open");
    let seen = events.0.lock().unwrap().clone();
    assert_eq!(
        seen,
        [
            ShellEvent::SurfaceClosed {
                event: ShellSurfaceEvent { id: "thumbnails".into(), props_json: None }
            },
            ShellEvent::SurfaceOpened {
                event: ShellSurfaceEvent { id: "search".into(), props_json: None }
            },
        ]
    );
}

#[test]
fn menus_stack_in_opening_order() {
    let (_thread, shell) = handle();
    let events = Arc::new(Events::default());
    let _sub = shell.subscribe(events.clone());
    block_on(shell.open_menu("file".into())).unwrap();
    block_on(shell.toggle_menu("zoom".into())).unwrap();
    assert_eq!(block_on(shell.open_menus()).unwrap(), ["file", "zoom"]);
    block_on(shell.toggle_menu("zoom".into())).unwrap();
    assert!(!block_on(shell.is_menu_open("zoom".into())).unwrap());
    block_on(shell.close_all_menus()).unwrap();
    assert!(block_on(shell.open_menus()).unwrap().is_empty());
    let seen = events.0.lock().unwrap();
    assert_eq!(seen[0], ShellEvent::MenuOpened { event: ShellMenuEvent { id: "file".into() } });
    assert_eq!(seen[2], ShellEvent::MenuClosed { event: ShellMenuEvent { id: "zoom".into() } });
}

#[test]
fn a_snapshot_round_trips_in_property_order() {
    let (_thread, shell) = handle();
    for id in ["b", "2", "a", "1"] {
        block_on(shell.open_surface(id.into(), None)).unwrap();
    }
    block_on(shell.close_surface("a".into())).unwrap();
    block_on(shell.open_menu("file".into())).unwrap();
    let snapshot = block_on(shell.snapshot()).unwrap();
    let ids: Vec<&str> = snapshot.surfaces.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, ["1", "2", "b", "a"], "integer-like ids first, then insertion order");
    assert!(!snapshot.surfaces[3].open, "a closed surface keeps its record");
    let open: Vec<String> =
        block_on(shell.open_surfaces()).unwrap().into_iter().map(|s| s.id).collect();
    assert_eq!(open, ["1", "2", "b"]);

    block_on(shell.close_all_surfaces()).unwrap();
    block_on(shell.close_all_menus()).unwrap();
    block_on(shell.apply_snapshot(snapshot.clone())).unwrap();
    assert_eq!(block_on(shell.snapshot()).unwrap(), snapshot);
}

#[test]
fn a_snapshot_with_bad_props_is_refused_whole() {
    let (_thread, shell) = handle();
    let snapshot = ShellSnapshot {
        surfaces: vec![
            ShellSurface { id: "ok".into(), open: true, exclusive: None, props_json: None },
            ShellSurface { id: "bad".into(), open: true, exclusive: None, props_json: Some("[]".into()) },
        ],
        open_menus: vec![],
    };
    let error = block_on(shell.apply_snapshot(snapshot)).unwrap_err();
    assert_eq!(code(&error), Some("invalid-input"), "{error:?}");
    assert!(!block_on(shell.is_open("ok".into())).unwrap());
}

#[test]
fn an_inherited_name_reads_as_closed() {
    let (_thread, shell) = handle();
    let surface = block_on(shell.surface("toString".into())).unwrap().unwrap();
    assert_eq!(
        surface,
        ShellSurface { id: "toString".into(), open: false, exclusive: None, props_json: None }
    );
    assert!(!block_on(shell.is_open("toString".into())).unwrap());
}

#[test]
fn a_closed_documents_capability_refuses_with_instance_closed() {
    let (thread, shell) = handle();
    let events = Arc::new(Events::default());
    let _sub = shell.subscribe(events.clone());
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    assert_eq!(events.1.load(Ordering::SeqCst), 1, "the close ends the stream");
    for error in [
        block_on(shell.is_open("x".into())).map(|_| ()).unwrap_err(),
        block_on(shell.open_surface("x".into(), None)).unwrap_err(),
        block_on(shell.snapshot()).map(|_| ()).unwrap_err(),
    ] {
        assert_eq!(code(&error), Some("instance-closed"), "{error:?}");
    }
}
```

`ShellEvent` and the records need `Clone` and `PartialEq` for these assertions; `ffi.rs` derives both.

- [x] **Step 3: Run them to see them fail**

Run: `cargo test -p epdf-plugin-shell --features uniffi --test ffi`
Expected: FAIL to compile, `could not find ffi in epdf_plugin_shell`.

- [x] **Step 4: Write `ffi.rs`**

Create `crates/plugins/shell/src/ffi.rs`:

```rust
//! The shell plugin through uniffi: one handle per document over its
//! surfaces and menus, one event enum. Every member is synchronous.
//!
//! A surface's `props` is an opaque JSON object by contract
//! (`SurfaceProps`), with no schema to mirror. It crosses as `props_json`,
//! the object as JSON text, and anything that is not an object is refused
//! with `invalid-input` before the plugin sees it.

use std::sync::Arc;

use epdf_ffi::{KernelBound, KernelPost, Subscription, ViewerError};
use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::store::Unsubscribe;
use serde_json::Value;

use crate::contract::{MenuEvent, OpenSurfaceOptions, SurfaceEvent, SurfaceProps, SurfaceState};
use crate::controller::ShellCapability;
use crate::model::SurfaceRecord;
use crate::record::JsRecord;

/// A surface's open flag, exclusivity tag and props. Also one entry of a
/// [`ShellSnapshot`]. A name the plugin finds on `Object.prototype`
/// (`toString`) has no flag there; it reads as closed here.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ShellSurface {
    pub id: String,
    pub open: bool,
    pub exclusive: Option<String>,
    /// A JSON object, or `None` when the surface has no props.
    pub props_json: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct ShellOpenOptions {
    /// Opening closes every other open surface with the same tag.
    #[uniffi(default = None)]
    pub exclusive: Option<String>,
    /// A JSON object.
    #[uniffi(default = None)]
    pub props_json: Option<String>,
}

/// Everything the shell holds for one document, for persist and restore.
/// Surfaces are in JavaScript property order (integer-like ids first),
/// closed ones included; menus are in opening order, last is topmost.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ShellSnapshot {
    pub surfaces: Vec<ShellSurface>,
    pub open_menus: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ShellSurfaceEvent {
    pub id: String,
    pub props_json: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ShellMenuEvent {
    pub id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ShellEvent {
    SurfaceOpened { event: ShellSurfaceEvent },
    SurfaceClosed { event: ShellSurfaceEvent },
    MenuOpened { event: ShellMenuEvent },
    MenuClosed { event: ShellMenuEvent },
}

#[uniffi::export(with_foreign)]
pub trait ShellEventListener: Send + Sync {
    fn on_event(&self, event: ShellEvent);
    /// The document closed: no event follows. Called once, and at once for
    /// a subscription made after the close.
    fn on_end(&self);
}

fn props_to_json(props: &SurfaceProps) -> String {
    Value::Object(props.clone()).to_string()
}

fn props_from_json(json: &str) -> Result<SurfaceProps, Error> {
    match serde_json::from_str::<Value>(json) {
        Ok(Value::Object(props)) => Ok(props),
        Ok(other) => Err(invalid_props(format!("props must be a JSON object, not {other}"))),
        Err(e) => Err(invalid_props(format!("props are not JSON: {e}"))),
    }
}

fn invalid_props(message: String) -> Error {
    plugin_error(code::INVALID_INPUT, "shell", message, None, None)
}

fn optional_props(json: Option<&str>) -> Result<Option<SurfaceProps>, Error> {
    json.map(props_from_json).transpose()
}

impl From<SurfaceState> for ShellSurface {
    fn from(state: SurfaceState) -> Self {
        ShellSurface {
            id: state.id,
            open: state.open.unwrap_or(false),
            exclusive: state.exclusive,
            props_json: state.props.as_ref().map(props_to_json),
        }
    }
}

impl TryFrom<ShellOpenOptions> for OpenSurfaceOptions {
    type Error = Error;

    fn try_from(options: ShellOpenOptions) -> Result<Self, Error> {
        Ok(OpenSurfaceOptions {
            exclusive: options.exclusive,
            props: optional_props(options.props_json.as_deref())?,
        })
    }
}

impl From<crate::ShellSnapshot> for ShellSnapshot {
    fn from(snapshot: crate::ShellSnapshot) -> Self {
        ShellSnapshot {
            surfaces: snapshot
                .surfaces
                .iter()
                .map(|(id, record)| ShellSurface {
                    id: id.to_string(),
                    open: record.open,
                    exclusive: record.exclusive.clone(),
                    props_json: record.props.as_ref().map(props_to_json),
                })
                .collect(),
            open_menus: snapshot.open_menus,
        }
    }
}

impl TryFrom<ShellSnapshot> for crate::ShellSnapshot {
    type Error = Error;

    fn try_from(snapshot: ShellSnapshot) -> Result<Self, Error> {
        let surfaces = snapshot
            .surfaces
            .into_iter()
            .map(|s| {
                let props = optional_props(s.props_json.as_deref())?;
                Ok((s.id, SurfaceRecord { open: s.open, exclusive: s.exclusive, props }))
            })
            .collect::<Result<JsRecord<SurfaceRecord>, Error>>()?;
        Ok(crate::ShellSnapshot { surfaces, open_menus: snapshot.open_menus })
    }
}

impl From<&SurfaceEvent> for ShellSurfaceEvent {
    fn from(event: &SurfaceEvent) -> Self {
        ShellSurfaceEvent { id: event.id.clone(), props_json: event.props.as_ref().map(props_to_json) }
    }
}

impl From<&MenuEvent> for ShellMenuEvent {
    fn from(event: &MenuEvent) -> Self {
        ShellMenuEvent { id: event.id.clone() }
    }
}

/// The capability, if its document is still open.
fn open(cap: &ShellCapability) -> Result<&ShellCapability, Error> {
    if cap.lifetime().aborted() {
        return Err(plugin_error(
            code::INSTANCE_CLOSED,
            "shell",
            "the document this capability belongs to is closed",
            None,
            None,
        ));
    }
    Ok(cap)
}

#[derive(uniffi::Object)]
pub struct ShellHandle {
    cap: KernelBound<ShellCapability>,
    post: KernelPost,
}

impl ShellHandle {
    /// On the kernel thread.
    pub fn new(cap: ShellCapability, post: KernelPost) -> Arc<Self> {
        Arc::new(ShellHandle { cap: KernelBound::new(cap, post.clone()), post })
    }
}

#[uniffi::export]
impl ShellHandle {
    pub async fn is_open(&self, id: String) -> Result<bool, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.is_open(&id))).await
    }

    /// `None` for a surface never opened.
    pub async fn surface(&self, id: String) -> Result<Option<ShellSurface>, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.get_surface(&id).map(Into::into))).await
    }

    /// Open surfaces, in JavaScript property order (`listOpenSurfaces`).
    pub async fn open_surfaces(&self) -> Result<Vec<ShellSurface>, ViewerError> {
        self.cap
            .call(|c| Ok(open(c)?.list_open_surfaces().into_iter().map(Into::into).collect()))
            .await
    }

    /// `open`. A tagged open closes every other open surface with that tag.
    pub async fn open_surface(
        &self,
        id: String,
        options: Option<ShellOpenOptions>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                let options = options.map(OpenSurfaceOptions::try_from).transpose()?;
                open(c)?.open(&id, options);
                Ok(())
            })
            .await
    }

    /// `close`.
    pub async fn close_surface(&self, id: String) -> Result<(), ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.close(&id))).await
    }

    /// `toggle`.
    pub async fn toggle_surface(
        &self,
        id: String,
        options: Option<ShellOpenOptions>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                let options = options.map(OpenSurfaceOptions::try_from).transpose()?;
                open(c)?.toggle(&id, options);
                Ok(())
            })
            .await
    }

    /// Change a surface's props without reopening it. An unknown id is
    /// ignored.
    pub async fn update_surface_props(
        &self,
        id: String,
        props_json: String,
    ) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                let props = props_from_json(&props_json)?;
                open(c)?.update_surface_props(&id, props);
                Ok(())
            })
            .await
    }

    /// `closeAll`.
    pub async fn close_all_surfaces(&self) -> Result<(), ViewerError> {
        self.cap.call(|c| Ok(open(c)?.close_all())).await
    }

    pub async fn is_menu_open(&self, id: String) -> Result<bool, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.is_menu_open(&id))).await
    }

    /// In opening order; the last is topmost (`listOpenMenus`).
    pub async fn open_menus(&self) -> Result<Vec<String>, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.list_open_menus())).await
    }

    pub async fn open_menu(&self, id: String) -> Result<(), ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.open_menu(&id))).await
    }

    pub async fn close_menu(&self, id: String) -> Result<(), ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.close_menu(&id))).await
    }

    pub async fn toggle_menu(&self, id: String) -> Result<(), ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.toggle_menu(&id))).await
    }

    pub async fn close_all_menus(&self) -> Result<(), ViewerError> {
        self.cap.call(|c| Ok(open(c)?.close_all_menus())).await
    }

    /// `getSnapshot`.
    pub async fn snapshot(&self) -> Result<ShellSnapshot, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.get_snapshot().into())).await
    }

    /// Replace the state with `snapshot`. A surface whose props aren't a
    /// JSON object refuses the whole snapshot.
    pub async fn apply_snapshot(&self, snapshot: ShellSnapshot) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                let snapshot = crate::ShellSnapshot::try_from(snapshot)?;
                open(c)?.apply_snapshot(snapshot);
                Ok(())
            })
            .await
    }

    /// Registered on the kernel thread after this returns; a cancel before
    /// then still takes effect. `on_end` comes when the document closes (at
    /// once if it already has).
    pub fn subscribe(&self, listener: Arc<dyn ShellEventListener>) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.cap.fire(move |c| {
            let ending = listener.clone();
            slot.fill_until_end(
                c.lifetime(),
                || {
                    let (opened, closed, menu_opened, menu_closed) =
                        (listener.clone(), listener.clone(), listener.clone(), listener);
                    let offs = [
                        c.on_surface_opened(move |e| {
                            opened.on_event(ShellEvent::SurfaceOpened { event: e.into() });
                            Ok(())
                        }),
                        c.on_surface_closed(move |e| {
                            closed.on_event(ShellEvent::SurfaceClosed { event: e.into() });
                            Ok(())
                        }),
                        c.on_menu_opened(move |e| {
                            menu_opened.on_event(ShellEvent::MenuOpened { event: e.into() });
                            Ok(())
                        }),
                        c.on_menu_closed(move |e| {
                            menu_closed.on_event(ShellEvent::MenuClosed { event: e.into() });
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

`crate::record::JsRecord` is `pub` and re-exported from `lib.rs`, and implements `FromIterator<(String, V)>` (`crates/plugins/shell/src/record.rs:102`), so collecting a `Result` of pairs builds it in the record's own property order. `crate::model::SurfaceRecord` is `pub` and re-exported too.

- [x] **Step 5: Run the tests**

Run: `cargo test -p epdf-plugin-shell --features uniffi --test ffi`
Expected: PASS, 8 tests.

Run: `cargo test -p epdf-plugin-shell`
Expected: PASS. The 38 vector cases don't see the feature.

- [x] **Step 6: Commit**

```bash
git add crates/plugins/shell
git commit -m "Export the shell plugin through uniffi, with props as JSON text"
```

### Task 3: Host wiring

**Files:**
- Modify: `crates/epdf-viewer-host/Cargo.toml`, `src/api.rs`, `src/session.rs`, `src/lib.rs`
- Test: `crates/epdf-viewer-host/tests/viewer_host.rs`

**Interfaces:**
- Consumes: `ShellHandle::new` (Task 2).
- Produces: `ViewerConfig.shell: bool` (default `false`), `ViewerHost::shell(document_id: String) -> Result<Arc<ShellHandle>, ViewerError>`.

- [x] **Step 1: Write the failing tests**

Append to `crates/epdf-viewer-host/tests/viewer_host.rs`, with `use epdf_plugin_shell::ffi::ShellOpenOptions;` at the top:

```rust
fn with_shell() -> Arc<ViewerHost> {
    let config = ViewerConfig { shell: true, ..Default::default() };
    let host = ViewerHost::new(config, Arc::new(Delegate)).unwrap();
    block_on(host.start()).unwrap();
    host
}

#[test]
fn each_document_keeps_its_own_shell() {
    let host = with_shell();
    let report = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let colors = block_on(host.open_document(test_support::fixture(test_support::COLORS), options()))
        .unwrap();
    let first = block_on(host.shell(report.id.clone())).unwrap();
    let second = block_on(host.shell(colors.id)).unwrap();
    let options = ShellOpenOptions { exclusive: None, props_json: Some(r#"{"tab":"outline"}"#.into()) };
    block_on(first.open_surface("sidebar".into(), Some(options))).unwrap();
    assert!(block_on(first.is_open("sidebar".into())).unwrap());
    assert!(!block_on(second.is_open("sidebar".into())).unwrap());
    block_on(host.close_document(report.id)).unwrap();
    assert!(block_on(second.open_surfaces()).unwrap().is_empty());
}

#[test]
fn without_its_flag_the_shell_plugin_is_not_installed() {
    let host = started();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    assert!(block_on(host.shell(doc.id)).is_err());
}
```

- [x] **Step 2: Run them to see them fail**

Run: `cargo test -p epdf-viewer-host --test viewer_host shell`
Expected: FAIL to compile, `no field shell on ViewerConfig`.

- [x] **Step 3: Wire the plugin in**

`crates/epdf-viewer-host/Cargo.toml`, alphabetical among the plugins:

```toml
epdf-plugin-shell = { path = "../plugins/shell", default-features = false, features = ["uniffi"] }
```

`crates/epdf-viewer-host/src/api.rs`, in `ViewerConfig`, after the fields earlier plans added:

```rust
    /// Install `shellPlugin()`: which named surfaces and menus are open, per
    /// document. The app still owns their UI.
    #[uniffi(default = false)]
    pub shell: bool,
```

`crates/epdf-viewer-host/src/session.rs`, in the plugin list after the earlier plans' blocks:

```rust
        if config.shell {
            plugins.push(epdf_plugin_shell::shell_plugin());
        }
```

`crates/epdf-viewer-host/src/lib.rs`: `use epdf_plugin_shell::ffi::ShellHandle;`, and in the exported `impl ViewerHost`:

```rust
    /// The shell of an open document: its open surfaces and menus. Needs
    /// `ViewerConfig.shell`; without it the kernel refuses the token.
    pub async fn shell(&self, document_id: String) -> Result<Arc<ShellHandle>, ViewerError> {
        let post = self.post.clone();
        self.call(move |s| {
            let cap = s.kernel.capability(&epdf_plugin_shell::SHELL, Some(&document_id))?;
            Ok(ShellHandle::new(cap, post))
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
git commit -m "Wire the shell plugin into the viewer host"
```

### Task 4: Swift `ShellCapability`

**Files:**
- Create: `platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Shell.swift`
- Test: `platforms/apple/EmbedPDF/Tests/EmbedPDFTests/ShellTests.swift`

**Interfaces:**
- Consumes: the generated `ShellHandle`, `ViewerHost.shell(documentId:)`, and the records from Task 2.
- Produces: `ShellCapability` and `PDFViewer.shell(for:)`. Methods: `isOpen(_:)`, `surface(_:)`, `openSurfaces()`, `open(_:exclusive:propsJSON:)`, `open(_:exclusive:props:)` (generic over `Encodable`), `close(_:)`, `toggle(_:exclusive:propsJSON:)`, `updateProps(of:propsJSON:)`, `updateProps(of:props:)`, `closeAll()`, `isMenuOpen(_:)`, `openMenus()`, `openMenu(_:)`, `closeMenu(_:)`, `toggleMenu(_:)`, `closeAllMenus()`, `snapshot()`, `applySnapshot(_:)`, `events`; plus `ShellSurface.props(as:)` and `ShellSurfaceEvent.props(as:)`.

The `Encodable` overloads encode with `JSONEncoder` and throw `PDFError` with code `invalid-input` if encoding fails. They decide nothing about the props: Rust still checks that the result is an object.

- [x] **Step 1: Write the failing tests**

Create `platforms/apple/EmbedPDF/Tests/EmbedPDFTests/ShellTests.swift`:

```swift
import CustomDump
import EmbedPDF
import Testing

extension BaseSuite {
  @Suite struct ShellTests {
    struct StyleProps: Codable, Equatable, Sendable {
      var annotationId: String
      var page: Int
    }

    func opened() async throws -> (PDFViewer, PDFDocument, ShellCapability) {
      let viewer = try PDFViewer(ViewerConfig(shell: true))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      return (viewer, document, try await viewer.shell(for: document))
    }

    @Test func typedPropsRoundTrip() async throws {
      let (_, _, shell) = try await opened()
      try await shell.open("style", props: StyleProps(annotationId: "a1", page: 3))
      let surface = try #require(try await shell.surface("style"))
      expectNoDifference(surface.open, true)
      expectNoDifference(try surface.props(as: StyleProps.self), StyleProps(annotationId: "a1", page: 3))
    }

    @Test func propsThatAreNotAnObjectThrow() async throws {
      let (_, _, shell) = try await opened()
      await #expect {
        try await shell.open("style", props: [1, 2, 3])
      } throws: { error in
        (error as? PDFError)?.code == "invalid-input"
      }
      let isOpen = try await shell.isOpen("style")
      expectNoDifference(isOpen, false)
    }

    @Test func anExclusiveOpenIsObserved() async throws {
      let (_, _, shell) = try await opened()
      try await shell.open("thumbnails", exclusive: "left")
      let events = shell.events
      try await shell.open("search", exclusive: "left")
      var seen: [ShellEvent] = []
      for await event in events {
        seen.append(event)
        if seen.count == 2 { break }
      }
      expectNoDifference(
        seen,
        [
          .surfaceClosed(event: ShellSurfaceEvent(id: "thumbnails", propsJson: nil)),
          .surfaceOpened(event: ShellSurfaceEvent(id: "search", propsJson: nil)),
        ])
    }

    @Test func aSnapshotRestores() async throws {
      let (_, _, shell) = try await opened()
      try await shell.open("sidebar", exclusive: "left")
      try await shell.openMenu("file")
      let snapshot = try await shell.snapshot()
      try await shell.closeAll()
      try await shell.closeAllMenus()
      try await shell.applySnapshot(snapshot)
      let restored = try await shell.snapshot()
      expectNoDifference(restored, snapshot)
    }

    @Test func eachDocumentKeepsItsOwnSurfaces() async throws {
      let (viewer, _, first) = try await opened()
      let colors = try await viewer.open(Fixture.data("colors"))
      let second = try await viewer.shell(for: colors)
      try await first.open("sidebar")
      let firstOpen = try await first.isOpen("sidebar")
      let secondOpen = try await second.isOpen("sidebar")
      expectNoDifference(firstOpen, true)
      expectNoDifference(secondOpen, false)
    }

    @Test func droppingTheStreamUnsubscribes() async throws {
      let (_, _, shell) = try await opened()
      let task = Task { for await _ in shell.events {} }
      try await Task.sleep(for: .milliseconds(20))
      task.cancel()
      _ = await task.value
      try await shell.open("sidebar")
    }

    @Test func closingTheDocumentFinishesTheEvents() async throws {
      let (viewer, document, shell) = try await opened()
      let events = Task { for await _ in shell.events {} }
      try await Task.sleep(for: .milliseconds(50))
      try await viewer.close(document)
      await events.value
    }

    @Test func aClosedDocumentsCapabilityThrows() async throws {
      let (viewer, document, shell) = try await opened()
      try await viewer.close(document)
      await #expect {
        _ = try await shell.isOpen("sidebar")
      } throws: { error in
        let error = error as? PDFError
        return error?.code == "instance-closed" && error?.name == "PluginError"
      }
    }
  }
}
```

No member takes a `CancelToken`, so there's no cancellation test.

- [x] **Step 2: Run them to see them fail**

```bash
bash platforms/apple/build-xcframework.sh
(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17' -only-testing:EmbedPDFTests/BaseSuite/ShellTests)
```

Expected: FAIL to compile, `value of type 'PDFViewer' has no member 'shell'`.

- [x] **Step 3: Write the capability**

Create `platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Shell.swift`:

```swift
public import EmbedPDFBindings
import Foundation

public typealias ShellSurface = EmbedPDFBindings.ShellSurface
public typealias ShellOpenOptions = EmbedPDFBindings.ShellOpenOptions
public typealias ShellSnapshot = EmbedPDFBindings.ShellSnapshot
public typealias ShellSurfaceEvent = EmbedPDFBindings.ShellSurfaceEvent
public typealias ShellMenuEvent = EmbedPDFBindings.ShellMenuEvent
public typealias ShellEvent = EmbedPDFBindings.ShellEvent

/// Which named surfaces (panels, sheets, inspectors) and menus are open in
/// one document. Pure state: your views decide what a surface looks like
/// and bind to this. Get one with `PDFViewer.shell(for:)`; the viewer needs
/// `ViewerConfig(shell: true)`. A surface's props are a JSON object, given
/// as text or as any `Encodable` that encodes to an object. After its
/// document closes, every call throws `instance-closed`.
public struct ShellCapability: Sendable {
  let handle: ShellHandle
  let viewer: PDFViewer

  public func isOpen(_ id: String) async throws(PDFError) -> Bool {
    try await bridged { try await handle.isOpen(id: id) }
  }

  /// `nil` for a surface never opened.
  public func surface(_ id: String) async throws(PDFError) -> ShellSurface? {
    try await bridged { try await handle.surface(id: id) }
  }

  public func openSurfaces() async throws(PDFError) -> [ShellSurface] {
    try await bridged { try await handle.openSurfaces() }
  }

  /// A tagged open closes every other open surface with the same tag.
  public func open(_ id: String, exclusive: String? = nil, propsJSON: String? = nil) async throws(PDFError) {
    let options = ShellOpenOptions(exclusive: exclusive, propsJson: propsJSON)
    try await bridged { try await handle.openSurface(id: id, options: options) }
  }

  public func open<Props: Encodable & Sendable>(_ id: String, exclusive: String? = nil, props: Props)
    async throws(PDFError)
  {
    try await open(id, exclusive: exclusive, propsJSON: try encoded(props))
  }

  public func close(_ id: String) async throws(PDFError) {
    try await bridged { try await handle.closeSurface(id: id) }
  }

  public func toggle(_ id: String, exclusive: String? = nil, propsJSON: String? = nil) async throws(PDFError) {
    let options = ShellOpenOptions(exclusive: exclusive, propsJson: propsJSON)
    try await bridged { try await handle.toggleSurface(id: id, options: options) }
  }

  /// Change a surface's props without reopening it.
  public func updateProps(of id: String, propsJSON: String) async throws(PDFError) {
    try await bridged { try await handle.updateSurfaceProps(id: id, propsJson: propsJSON) }
  }

  public func updateProps<Props: Encodable & Sendable>(of id: String, props: Props) async throws(PDFError) {
    try await updateProps(of: id, propsJSON: try encoded(props))
  }

  public func closeAll() async throws(PDFError) {
    try await bridged { try await handle.closeAllSurfaces() }
  }

  public func isMenuOpen(_ id: String) async throws(PDFError) -> Bool {
    try await bridged { try await handle.isMenuOpen(id: id) }
  }

  /// In opening order; the last is topmost.
  public func openMenus() async throws(PDFError) -> [String] {
    try await bridged { try await handle.openMenus() }
  }

  public func openMenu(_ id: String) async throws(PDFError) {
    try await bridged { try await handle.openMenu(id: id) }
  }

  public func closeMenu(_ id: String) async throws(PDFError) {
    try await bridged { try await handle.closeMenu(id: id) }
  }

  public func toggleMenu(_ id: String) async throws(PDFError) {
    try await bridged { try await handle.toggleMenu(id: id) }
  }

  public func closeAllMenus() async throws(PDFError) {
    try await bridged { try await handle.closeAllMenus() }
  }

  /// Everything this document's shell holds, for persist and restore.
  public func snapshot() async throws(PDFError) -> ShellSnapshot {
    try await bridged { try await handle.snapshot() }
  }

  public func applySnapshot(_ snapshot: ShellSnapshot) async throws(PDFError) {
    try await bridged { try await handle.applySnapshot(snapshot: snapshot) }
  }

  /// Surface and menu events. Finishes when the document closes (at once
  /// if it already has) or the viewer is destroyed.
  public var events: AsyncStream<ShellEvent> {
    viewer.registry.stream { handle.subscribe(listener: $0) }
  }

  private func encoded(_ props: some Encodable) throws(PDFError) -> String {
    do {
      return String(decoding: try JSONEncoder().encode(props), as: UTF8.self)
    } catch {
      throw PDFError(name: "EncodingError", code: "invalid-input", detail: "\(error)")
    }
  }
}

extension ShellSurface {
  /// The props decoded as `Props`, or `nil` when the surface has none.
  public func props<Props: Decodable>(as type: Props.Type) throws -> Props? {
    try propsJson.map { try JSONDecoder().decode(type, from: Data($0.utf8)) }
  }
}

extension ShellSurfaceEvent {
  public func props<Props: Decodable>(as type: Props.Type) throws -> Props? {
    try propsJson.map { try JSONDecoder().decode(type, from: Data($0.utf8)) }
  }
}

extension EventRelay: ShellEventListener where Event == ShellEvent {
  func onEvent(event: ShellEvent) {
    continuation.yield(event)
  }

  func onEnd() {
    continuation.finish()
  }
}

extension PDFViewer {
  public func shell(for document: PDFDocument) async throws(PDFError) -> ShellCapability {
    let handle = try await bridged { try await host.shell(documentId: document.id) }
    return ShellCapability(handle: handle, viewer: self)
  }
}
```

`PDFError.init(name:code:detail:)` is internal to `EmbedPDF`, which is where this file lives.

- [x] **Step 4: Run the tests**

```bash
bash platforms/apple/build-xcframework.sh
(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17')
```

Expected: PASS, every suite, `ShellTests` with 8 tests.

- [x] **Step 5: Commit**

```bash
git add platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Shell.swift platforms/apple/EmbedPDF/Tests/EmbedPDFTests/ShellTests.swift
git commit -m "Add the Swift shell capability"
```

### Task 5: Docs, CI, merge and clean up

**Files:**
- Modify: `.github/workflows/plugin-rust.yml`, `platforms/apple/README.md`, `docs/superpowers/plans/2026-10-01-swift-package.md`

- [ ] **Step 1: CI**

Append `shell` to the `crates=( ... )` list in `.github/workflows/plugin-rust.yml`, in plan-number order.

- [x] **Step 2: README row**

In `platforms/apple/README.md`'s `## Plugins` table, in plan-number order:

```markdown
| shell | `ShellCapability` | `shell: true` | `2026-10-06-swift-plugins/12-shell.md` |
```

- [x] **Step 3: Mark the spec entry delivered**

In `docs/superpowers/plans/2026-10-01-swift-package.md` §A.4, change `**shell** (D).` to `**shell** (D), delivered by 12-shell.md, which also records why it ships.`

- [x] **Step 4: Run the definition of done**

```bash
cargo test -p epdf-plugin-shell --features uniffi
cargo test -p epdf-ffi -p epdf-viewer-host
cargo clippy -p epdf-plugin-shell -p epdf-viewer-host --all-targets --features epdf-plugin-shell/uniffi -- -D warnings
cargo test -p epdf-plugin-shell
bash platforms/apple/build-xcframework.sh
(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17')
cargo build -p epdf-viewer-host -p uniffi-bindgen
./target/debug/uniffi-bindgen generate --library target/debug/libepdf_viewer_host.dylib --language kotlin --no-format --out-dir "$TMPDIR/epdf-kotlin-check"
```

Expected: every command passes.

- [x] **Step 5: Commit, with the archive size**

```bash
before=$(cat "$TMPDIR/shell-size-before")
after=$(stat -f %z target/aarch64-apple-ios-sim/release/libepdf_viewer_host.a)
git add .github/workflows/plugin-rust.yml platforms/apple/README.md docs/superpowers/plans/2026-10-01-swift-package.md
git commit -m "List shell in CI and the Apple README" -m "libepdf_viewer_host.a (ios-sim, release): $before -> $after bytes"
```

- [ ] **Step 6: Merge into the integration branch**

In the integration worktree umbrella Task 0 created, which has `research/swift-plugins` checked out:

```bash
cd .claude/worktrees/swift-plugins-integration
git merge --no-ff research/swift-shell -m "Merge the shell Swift wrapper"
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
git worktree remove .claude/worktrees/swift-shell
git branch -d research/swift-shell
git worktree prune
```

Then tick `12 shell` in the umbrella's Tasks 1 to 16 list.
