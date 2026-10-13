# Interaction and Feedback Swift Wrappers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Export the interaction plugin (the pointer hub) and the feedback plugin (haptics) through UniFFI, wire both into `ViewerHost`, and give Swift an `InteractionCapability`, a `FeedbackCapability`, an `InteractionHandler` protocol it can implement, and a measured `dispatchPointer` round trip at 120 Hz.

**Architecture:** The recipe from `2026-10-01-swift-package.md` Appendix A §A.0, with metadata (`crates/plugins/metadata/src/ffi.rs`) as the model. Interaction is the first plugin with ports Swift must supply, so this plan adds the pointer records later plans reuse to `epdf-ffi` (`src/input.rs`). It consumes three shared pieces from umbrella Task 0: `RegistrationHandle` for anything a plugin hands back an unregister closure for, `HostError`, the error a Swift implementation of a foreign trait throws, and `PDFViewer.reportedErrors`, the stream of errors the kernel reports, so a Swift test can see a throwing handler's error arrive.

**Tech Stack:** Rust 2024, `uniffi =0.32.0`, Swift 6.2 tools, Swift Testing, `swift-custom-dump` 1.3, Xcode 27.0, iPhone 17 simulator (iOS 26.0).

**Spec:** `docs/superpowers/plans/2026-10-06-swift-plugins/00-umbrella.md` (order, names, shared files, definition of done) and `docs/superpowers/plans/2026-10-01-swift-package.md` (Global Constraints, Decisions, Appendix A §A.0 and §A.2).

## Global Constraints

Everything in the umbrella's Global Constraints applies. The ones this plan meets first:

- `uniffi` is pinned at `=0.32.0`.
- UniFFI names are unique across the workspace. The crate already has Rust types named `Tool`, `InteractionHandler` and `PlatformFeedback`. The FFI trait names `InteractionHandler` and `PlatformFeedback` are taken by the `ffi` module's foreign traits, as the umbrella's table promises, so Rust code in `ffi.rs` refers to the plugin's own traits as `crate::InteractionHandler` and `crate::PlatformFeedback`. Records get the `Interaction` or `Feedback` prefix (`InteractionTool`, `FeedbackImpactWeight`).
- Pointer input records live in `epdf-ffi` (`crates/epdf-ffi/src/input.rs`), because selection, link, annotation, form and measurement all take them.
- The kernel is `!Send`. A foreign handler is called on the kernel thread, synchronously, inside `dispatch_pointer`.
- No `serde_json::Value` crosses. `ActivateToolOptions.payload` and `ToolChangedEvent.payload` are dropped: a payload is a hand-off between Rust plugins, and nothing Swift could put there means anything to them.
- Facades don't decide PDF semantics. The Swift `InteractionHandler` defaults (empty `onMove`, `onUp`, `onHover`; `onCancel` calling `onUp`; no touch claim) are the hub's own defaults, copied from `crates/plugins/interaction/src/contract.rs` `InteractionHandler`.

## Decisions

`PointerSample.project` doesn't cross. It's a Rust closure that projects one sample onto another page's frame (`sample_point_on`, `contract.rs`). A native source has no such closure to send, so a sample from Swift arrives with `project: None` and handlers fall back to the page hit. The cost: a selection drag that leaves the page it started on stops extending at the page edge until it enters another page. Plan 02 (stage) can add a projecting source later, because the stage knows every page's frame.

`ViewerConfig.interaction` is a `bool`, as the umbrella says. `InteractionConfig` has two fields, `default_tool` and `tools`. Both can be had at run time (`register_tool`, then `activate_tool`), so a config record doesn't carry its weight yet.

`ViewerConfig.feedback` is `Option<FeedbackConfig>`, not the `Option<Arc<dyn PlatformFeedback>>` the umbrella's table shows. `ViewerConfig` derives `Debug`, and a foreign trait object isn't `Debug`. Wrapping the provider in a record with a hand-written `Debug` keeps the derive and keeps the field additive. This plan's Task 4 updates the umbrella row in the same commit.

A `RegistrationHandle` that Swift drops without calling `unregister()` leaves the registration in place. Subscriptions cancel on drop because an event stream nobody reads is a leak. A registered tool or handler is the opposite: people register a tool and discard the result all the time, and having it vanish at the end of the statement would be a nasty surprise.

## Review Focus

1. A Swift handler that calls back into the viewer from `onDown` (say, `try await interaction.cursor()` in a `Task`). Expected: no deadlock; the call queues behind the dispatch and answers once it returns. Pinned in Task 6 (`aHandlerThatCallsBackIntoTheViewerDoesNotDeadlock`).
2. Pointer samples sent faster than the kernel thread drains them. Expected: every sample arrives, in order. Pinned in Task 2 (`samples_arrive_in_the_order_they_were_sent`).
3. The capturing handler unregistered mid-gesture. Expected: the hub forgets it, the next move hovers instead of reaching it, and nothing panics. Pinned in Task 2 (`unregistering_the_capturing_handler_mid_gesture_releases_it`).
4. A Swift handler whose `onDown` throws (umbrella Review Focus item 4). Expected: the error reaches `ViewerDelegate.report` named `HostError`, and the gesture isn't captured. Pinned in Task 2 (`a_throwing_handler_is_reported_and_does_not_capture`) and Task 6 (`aThrowingHandlerReportsAndFails`).
5. A `Registration` dropped without `unregister()`. Expected: the tool stays registered. Pinned in Task 6 (`droppingARegistrationKeepsTheTool`).

The umbrella pins its Review Focus item 2 (a plugin enabled without its requirement) here. Interaction requires nothing, so there's no config that can show it yet. Plan 03 (selection, which requires interaction) owns that test instead; see Cross-plugin tests.

## Cross-plugin tests

Interaction has no optional dependencies. Later plans test their use of it:

- `selection` (plan 03) owns `a_plugin_without_its_requirement_fails_to_build` (umbrella Review Focus item 2) and a test that a pointer drag dispatched through `InteractionCapability.dispatchPointer` selects text.
- `link` (plan 04) owns a test that a tap dispatched through `dispatchPointer` activates a link.
- `selection` (plan 03) owns a test that a selection gesture calls `PlatformFeedback.selection()`.

## File Structure

```
crates/epdf-ffi/src/input.rs            PointerSample, PointerPhase, PointerKind, PointerGesture, Modifiers, PointerPageHit
crates/epdf-ffi/src/lib.rs              module and re-exports
crates/epdf-ffi/tests/input.rs
crates/plugins/interaction/Cargo.toml   uniffi feature
crates/plugins/interaction/src/lib.rs   scaffolding, ffi module
crates/plugins/interaction/src/controller.rs   lifetime()
crates/plugins/interaction/src/ffi.rs   InteractionHandle, FeedbackHandle, records, foreign traits, adapters
crates/plugins/interaction/tests/ffi.rs
crates/epdf-viewer-host/{Cargo.toml,src/api.rs,src/session.rs,src/lib.rs}
crates/epdf-viewer-host/tests/viewer_host.rs
platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Interaction.swift
platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Feedback.swift
platforms/apple/EmbedPDF/Sources/EmbedPDFUI/HapticFeedback.swift
platforms/apple/EmbedPDF/Tests/EmbedPDFTests/InteractionTests.swift
platforms/apple/EmbedPDF/Tests/EmbedPDFTests/FeedbackTests.swift
platforms/apple/EmbedPDF/Tests/EmbedPDFTests/PointerLatencyTests.swift
docs/research/swift-pointer-latency.md
```

---

### Task 1: The shared input records in `epdf-ffi`

**Files:**
- Create: `crates/epdf-ffi/src/input.rs`, `crates/epdf-ffi/tests/input.rs`
- Modify: `crates/epdf-ffi/src/lib.rs`

**Interfaces:**
- Consumes: `Point`, `PageRef` (`geometry.rs`); umbrella Task 0c's `epdf_ffi::RegistrationHandle` (`RegistrationHandle::new(post: KernelPost, unregister: impl Fn() + 'static) -> Arc<RegistrationHandle>`, export `unregister(&self)`) and `epdf_ffi::HostError` (`Failed { detail }`, with `From<uniffi::UnexpectedUniFFICallbackError>` and `From<HostError> for epdf_kernel::Error`, name `HostError`, code `operation-failed`).
- Produces: `epdf_ffi::{PointerSample, PointerPhase, PointerKind, PointerGesture, Modifiers, PointerPageHit}` (records).

- [x] **Step 1: Create the worktree**

```bash
git worktree add .claude/worktrees/swift-interaction -b research/swift-interaction research/swift-plugins
cd .claude/worktrees/swift-interaction
```

Fetch PDFium for `darwin-arm64`, `ios-arm64` and `ios-sim-arm64` as `platforms/apple/README.md` describes. Every later command in this plan runs from this directory. Confirm umbrella Task 0 has landed: `grep -n 'pub use registration::RegistrationHandle\|pub use foreign' crates/epdf-ffi/src/lib.rs` prints both lines.

- [x] **Step 2: Write the failing test**

`crates/epdf-ffi/tests/input.rs`:

```rust
use epdf_ffi::{Modifiers, PageRef, Point, PointerKind, PointerPageHit, PointerPhase, PointerSample};

#[test]
fn modifiers_default_to_none_held() {
    assert_eq!(Modifiers::default(), Modifiers { shift: false, alt: false, ctrl: false, meta: false });
}

#[test]
fn a_sample_over_a_page_keeps_its_page_point() {
    let sample = PointerSample {
        phase: PointerPhase::Down,
        viewport: Point { x: 10.0, y: 20.0 },
        page: Some(PointerPageHit {
            page: PageRef { page_object_number: 4 },
            point: Point { x: 1.0, y: 2.0 },
            scale: Some(2.0),
            rotation: None,
            zoom: None,
        }),
        modifiers: Modifiers::default(),
        click_count: Some(2),
        pointer_type: Some(PointerKind::Touch),
        gesture: None,
        source: None,
    };
    assert_eq!(sample.clone(), sample);
    assert_eq!(sample.page.unwrap().point, Point { x: 1.0, y: 2.0 });
}
```

- [x] **Step 3: Run it to verify it fails**

Run: `cargo test -p epdf-ffi --test input`
Expected: FAIL to compile, "unresolved imports `epdf_ffi::Modifiers`, ...".

- [x] **Step 4: Write `input.rs`**

```rust
//! A normalized pointer event, as a native source reports it to the
//! interaction hub. `viewport` is the source's own points; `page` is the
//! page under the pointer and its page-space point, absent over a gap.
//! Conversions to the interaction plugin's types live in that crate.

use crate::{PageRef, Point};

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum PointerPhase {
    Down,
    Move,
    Up,
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum PointerKind {
    Mouse,
    Pen,
    Touch,
}

/// A sample synthesized from a recognized gesture rather than a raw press.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum PointerGesture {
    LongPress,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct Modifiers {
    #[uniffi(default = false)]
    pub shift: bool,
    #[uniffi(default = false)]
    pub alt: bool,
    #[uniffi(default = false)]
    pub ctrl: bool,
    #[uniffi(default = false)]
    pub meta: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct PointerPageHit {
    pub page: PageRef,
    /// Page space: y-down PDF units.
    pub point: Point,
    /// View points per page unit.
    #[uniffi(default = None)]
    pub scale: Option<f64>,
    /// The page's total display rotation in degrees.
    #[uniffi(default = None)]
    pub rotation: Option<f64>,
    /// The page's zoom relative to its 100% baseline.
    #[uniffi(default = None)]
    pub zoom: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PointerSample {
    pub phase: PointerPhase,
    pub viewport: Point,
    #[uniffi(default = None)]
    pub page: Option<PointerPageHit>,
    /// No default: a record field can't default to a record. Swift passes
    /// `Modifiers()`, whose fields all default to false.
    pub modifiers: Modifiers,
    /// For a down: 1 single, 2 double, 3 triple. `None` reads as 1.
    #[uniffi(default = None)]
    pub click_count: Option<u32>,
    /// `None` reads as mouse.
    #[uniffi(default = None)]
    pub pointer_type: Option<PointerKind>,
    #[uniffi(default = None)]
    pub gesture: Option<PointerGesture>,
    /// The lens (stage plugin id) the sample came from; `None` routes to
    /// every handler.
    #[uniffi(default = None)]
    pub source: Option<String>,
}
```

- [x] **Step 5: Export them from `lib.rs`**

In `crates/epdf-ffi/src/lib.rs`, add the module beside the existing ones and extend the re-exports:

```rust
pub mod input;

pub use input::{Modifiers, PointerGesture, PointerKind, PointerPageHit, PointerPhase, PointerSample};
```

- [x] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p epdf-ffi`
Expected: PASS, including the two `input` tests and every existing `epdf-ffi` test.

- [x] **Step 7: Commit**

```bash
git add crates/epdf-ffi
git commit -m "Add the pointer records to epdf-ffi"
```

### Task 2: `InteractionHandle`, the `InteractionHandler` foreign trait and its adapter

**Files:**
- Modify: `crates/plugins/interaction/Cargo.toml`, `crates/plugins/interaction/src/lib.rs`, `crates/plugins/interaction/src/controller.rs`
- Create: `crates/plugins/interaction/src/ffi.rs`, `crates/plugins/interaction/tests/ffi.rs`

**Interfaces:**
- Consumes: Task 1's `PointerSample` family; umbrella Task 0c's `RegistrationHandle` and `HostError`.
- Produces: `epdf_plugin_interaction::ffi::InteractionHandle` with `InteractionHandle::new(cap: InteractionCapability, post: KernelPost, report: Report) -> Arc<Self>` where `pub type Report = Rc<dyn Fn(&epdf_kernel::Error)>` is defined in `ffi.rs`; records `InteractionTool`, `InteractionToolChangedEvent`, `InteractionGestureEvent`, `InteractionCursorChangedEvent`; enum `InteractionEvent`; foreign traits `InteractionHandler` and `InteractionEventListener`. Methods (Swift names in parentheses where they differ): `active_tool`, `active_tool_id`, `default_tool_id`, `tools`, `tool(id)`, `has_tool(id)`, `active_tool_enables(behavior)`, `activate_tool(id)`, `activate_default_tool`, `push_tool(id)`, `pop_tool`, `set_tool_cursor(id, skin)`, `register_tool(tool, replace) -> Arc<RegistrationHandle>`, `register_handler(id, priority, source, handler) -> Arc<RegistrationHandle>`, `claim_cursor(token, cursor, priority)`, `cursor`, `would_claim_touch(sample)`, `dispatch_pointer(sample)` (a `fire` method, no reply), `subscribe(listener) -> Arc<Subscription>`.

The member list matches `crates/plugins/interaction/src/harness.rs` `MEMBERS` (23 entries): the five `on*` hooks fold into `subscribe`.

- [x] **Step 1: Add the feature**

In `crates/plugins/interaction/Cargo.toml`, under `[features]`:

```toml
# The uniffi exports (`ffi` module), for `epdf-viewer-host`.
uniffi = ["dep:uniffi", "dep:epdf-ffi"]
```

Under `[dependencies]`:

```toml
epdf-ffi = { path = "../../epdf-ffi", optional = true }
uniffi = { version = "=0.32.0", optional = true }
```

Add a `[dev-dependencies]` section:

```toml
[dev-dependencies]
futures = { version = "0.3", default-features = false, features = ["std", "executor"] }
plugin-conformance = { path = "../../plugin-conformance" }
```

If `tests/api.rs` already pulls `plugin-conformance` through a `[[test]] required-features`, keep that and add only `futures`.

In `crates/plugins/interaction/src/lib.rs`, beside the other `pub mod` lines:

```rust
#[cfg(feature = "uniffi")]
pub mod ffi;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();
```

In `crates/plugins/interaction/src/controller.rs`, at the end of the `impl InteractionCapability` block:

```rust
    /// The instance lifetime, aborted when the document closes. Not in the
    /// TypeScript, where a closed capability is unreachable; a foreign
    /// handle outlives the close, checks this before every call, and ends
    /// its event streams when it aborts.
    #[cfg(feature = "uniffi")]
    #[doc(hidden)]
    pub fn lifetime(&self) -> &epdf_kernel::signal::AbortSignal {
        self.0.cx.lifetime()
    }
```

- [x] **Step 2: Write the failing tests**

`crates/plugins/interaction/tests/ffi.rs`:

```rust
#![cfg(feature = "uniffi")]

use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use epdf_ffi::{
    HostError, KernelThread, Modifiers, PageRef, Point, PointerKind, PointerPageHit,
    PointerPhase, PointerSample, ViewerError,
};
use epdf_plugin_interaction::ffi::{
    InteractionEvent, InteractionEventListener, InteractionHandle, InteractionHandler,
    InteractionTool,
};
use epdf_plugin_interaction::{INTERACTION, InteractionConfig, interaction_plugin};
use futures::executor::block_on;
use plugin_conformance::test_kernel::{TestKernel, one_document};

#[derive(Default)]
struct Events(Mutex<Vec<InteractionEvent>>, AtomicUsize);

impl InteractionEventListener for Events {
    fn on_event(&self, event: InteractionEvent) {
        self.0.lock().unwrap().push(event);
    }
    fn on_end(&self) {
        self.1.fetch_add(1, Ordering::SeqCst);
    }
}

/// Logs every call; captures on down unless told to throw.
#[derive(Default)]
struct Recording {
    log: Mutex<Vec<String>>,
    throws: bool,
    claims_touch: bool,
}

impl InteractionHandler for Recording {
    fn enabled_for(&self, tool: InteractionTool) -> bool {
        tool.enables.iter().any(|t| t == "text-select")
    }
    fn on_down(&self, sample: PointerSample) -> Result<bool, HostError> {
        self.log.lock().unwrap().push(format!("down {}", sample.viewport.x));
        if self.throws {
            return Err(HostError::Failed { detail: "handler failed".into() });
        }
        Ok(true)
    }
    fn on_move(&self, sample: PointerSample) {
        self.log.lock().unwrap().push(format!("move {}", sample.viewport.x));
    }
    fn on_up(&self, sample: PointerSample) {
        self.log.lock().unwrap().push(format!("up {}", sample.viewport.x));
    }
    fn on_cancel(&self, sample: PointerSample) {
        self.log.lock().unwrap().push(format!("cancel {}", sample.viewport.x));
    }
    fn on_hover(&self, sample: PointerSample) {
        self.log.lock().unwrap().push(format!("hover {}", sample.viewport.x));
    }
    fn claims_touch(&self, _sample: PointerSample) -> bool {
        self.claims_touch
    }
}

type Reports = Arc<Mutex<Vec<String>>>;

fn handle() -> (KernelThread<TestKernel>, Arc<InteractionHandle>, Reports) {
    let thread = KernelThread::spawn(|_| {
        TestKernel::open(vec![interaction_plugin(InteractionConfig::default())], one_document())
    })
    .unwrap();
    let post = thread.post();
    let reports: Reports = Arc::default();
    let sink = reports.clone();
    let handle = block_on(thread.call(move |k| {
        let report = Rc::new(move |e: &epdf_kernel::Error| {
            sink.lock().unwrap().push(format!("{}: {}", e.name, e.message));
        });
        InteractionHandle::new(k.capability(&INTERACTION).unwrap(), post, report)
    }))
    .unwrap();
    (thread, handle, reports)
}

fn sample(phase: PointerPhase, x: f64) -> PointerSample {
    PointerSample {
        phase,
        viewport: Point { x, y: 10.0 },
        page: Some(PointerPageHit {
            page: PageRef { page_object_number: 1 },
            point: Point { x, y: 10.0 },
            scale: None,
            rotation: None,
            zoom: None,
        }),
        modifiers: Modifiers::default(),
        click_count: None,
        pointer_type: Some(PointerKind::Touch),
        gesture: None,
        source: None,
    }
}

fn code(error: &ViewerError) -> Option<&str> {
    match error {
        ViewerError::Kernel { code, .. } => code.as_deref(),
        ViewerError::Closed => None,
    }
}

/// Let every job posted so far run.
fn drain(thread: &KernelThread<TestKernel>) {
    block_on(thread.call(|_| ())).unwrap();
}

#[test]
fn the_built_in_tools_are_listed_and_pointer_is_active() {
    let (_thread, interaction, _) = handle();
    let ids: Vec<String> = block_on(interaction.tools()).unwrap().into_iter().map(|t| t.id).collect();
    assert_eq!(ids, ["pointer", "pan"]);
    assert_eq!(block_on(interaction.active_tool_id()).unwrap(), "pointer");
    assert_eq!(block_on(interaction.default_tool_id()).unwrap(), "pointer");
    assert!(block_on(interaction.active_tool_enables("text-select".into())).unwrap());
}

#[test]
fn activating_switches_the_tool_and_is_observed() {
    let (thread, interaction, _) = handle();
    let events = Arc::new(Events::default());
    let _sub = interaction.subscribe(events.clone());
    block_on(interaction.activate_tool("pan".into())).unwrap();
    drain(&thread);
    assert_eq!(block_on(interaction.active_tool()).unwrap().cursor, "grab");
    let seen = events.0.lock().unwrap();
    assert!(
        matches!(seen.first(), Some(InteractionEvent::ToolChanged { event })
            if event.tool_id == "pan" && event.previous_tool_id == "pointer"),
        "{seen:?}"
    );
}

#[test]
fn an_unknown_tool_is_not_found() {
    let (_thread, interaction, _) = handle();
    let error = block_on(interaction.activate_tool("laser".into())).unwrap_err();
    assert_eq!(code(&error), Some("not-found"), "{error:?}");
}

#[test]
fn push_and_pop_restore_the_previous_tool() {
    let (_thread, interaction, _) = handle();
    block_on(interaction.push_tool("pan".into())).unwrap();
    assert_eq!(block_on(interaction.active_tool_id()).unwrap(), "pan");
    block_on(interaction.pop_tool()).unwrap();
    assert_eq!(block_on(interaction.active_tool_id()).unwrap(), "pointer");
}

#[test]
fn a_registered_tool_conflicts_until_replaced_and_unregisters() {
    let (thread, interaction, _) = handle();
    let tool = InteractionTool {
        id: "ink".into(),
        cursor: "crosshair".into(),
        gap_cursor: None,
        enables: vec!["annotation-create".into()],
        touch_direct: Some(true),
    };
    let registration = block_on(interaction.register_tool(tool.clone(), false)).unwrap();
    let error = block_on(interaction.register_tool(tool.clone(), false)).err().unwrap();
    assert_eq!(code(&error), Some("conflict"), "{error:?}");
    let replaced = block_on(interaction.register_tool(tool, true)).unwrap();
    // The first registration no longer owns the id, so its unregister is a no-op.
    registration.unregister();
    drain(&thread);
    assert!(block_on(interaction.has_tool("ink".into())).unwrap());
    replaced.unregister();
    drain(&thread);
    assert!(!block_on(interaction.has_tool("ink".into())).unwrap());
}

#[test]
fn a_tool_cursor_skin_changes_the_resolved_cursor() {
    let (thread, interaction, _) = handle();
    let events = Arc::new(Events::default());
    let _sub = interaction.subscribe(events.clone());
    // A skin applies over a page; over a gap the tool's gap cursor wins.
    interaction.dispatch_pointer(sample(PointerPhase::Move, 1.0));
    block_on(interaction.set_tool_cursor(
        "pointer".into(),
        Some([("default".to_string(), "pointer".to_string())].into()),
    ))
    .unwrap();
    drain(&thread);
    assert_eq!(block_on(interaction.cursor()).unwrap(), "pointer");
    assert!(events.0.lock().unwrap().iter().any(|e| matches!(e,
        InteractionEvent::CursorChanged { event } if event.cursor == "pointer")));
}

#[test]
fn a_cursor_claim_wins_by_priority_and_clears() {
    let (_thread, interaction, _) = handle();
    block_on(interaction.claim_cursor("drag".into(), Some("grabbing".into()), 10.0)).unwrap();
    assert_eq!(block_on(interaction.cursor()).unwrap(), "grabbing");
    block_on(interaction.claim_cursor("drag".into(), None, 0.0)).unwrap();
    assert_eq!(block_on(interaction.cursor()).unwrap(), "default");
}

#[test]
fn a_foreign_handler_captures_a_gesture() {
    let (thread, interaction, _) = handle();
    let events = Arc::new(Events::default());
    let _sub = interaction.subscribe(events.clone());
    let handler = Arc::new(Recording::default());
    let _registration = block_on(interaction.register_handler(
        "recording".into(),
        5.0,
        None,
        handler.clone(),
    ))
    .unwrap();
    interaction.dispatch_pointer(sample(PointerPhase::Down, 1.0));
    interaction.dispatch_pointer(sample(PointerPhase::Move, 2.0));
    interaction.dispatch_pointer(sample(PointerPhase::Up, 3.0));
    drain(&thread);
    assert_eq!(*handler.log.lock().unwrap(), ["down 1", "move 2", "up 3"]);
    let seen = events.0.lock().unwrap();
    let kinds: Vec<&str> = seen
        .iter()
        .map(|e| match e {
            InteractionEvent::GestureStarted { .. } => "started",
            InteractionEvent::GestureEnded { .. } => "ended",
            InteractionEvent::GestureCancelled { .. } => "cancelled",
            InteractionEvent::ToolChanged { .. } => "tool",
            InteractionEvent::CursorChanged { .. } => "cursor",
        })
        .filter(|k| *k != "cursor")
        .collect();
    assert_eq!(kinds, ["started", "ended"]);
    assert!(matches!(&seen.iter().find(|e| matches!(e, InteractionEvent::GestureStarted { .. })),
        Some(InteractionEvent::GestureStarted { event })
            if event.handler_id == "recording"
                && event.page == Some(PageRef { page_object_number: 1 })
                && event.pointer_type == PointerKind::Touch));
}

#[test]
fn a_throwing_handler_is_reported_and_does_not_capture() {
    let (thread, interaction, reports) = handle();
    let events = Arc::new(Events::default());
    let _sub = interaction.subscribe(events.clone());
    let handler = Arc::new(Recording { throws: true, ..Default::default() });
    let _registration =
        block_on(interaction.register_handler("throws".into(), 5.0, None, handler.clone()))
            .unwrap();
    interaction.dispatch_pointer(sample(PointerPhase::Down, 1.0));
    interaction.dispatch_pointer(sample(PointerPhase::Move, 2.0));
    drain(&thread);
    let reports = reports.lock().unwrap();
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert!(reports[0].starts_with("HostError: "), "{reports:?}");
    assert!(reports[0].contains("handler failed"), "{reports:?}");
    // Not captured: the move hovers instead of reaching `on_move`.
    assert_eq!(*handler.log.lock().unwrap(), ["down 1", "hover 2"]);
    assert!(!events.0.lock().unwrap().iter().any(|e| matches!(e,
        InteractionEvent::GestureStarted { .. })));
}

#[test]
fn samples_arrive_in_the_order_they_were_sent() {
    let (thread, interaction, _) = handle();
    let handler = Arc::new(Recording::default());
    let _registration =
        block_on(interaction.register_handler("order".into(), 1.0, None, handler.clone()))
            .unwrap();
    interaction.dispatch_pointer(sample(PointerPhase::Down, 0.0));
    for x in 1..=1000 {
        interaction.dispatch_pointer(sample(PointerPhase::Move, f64::from(x)));
    }
    interaction.dispatch_pointer(sample(PointerPhase::Up, 1001.0));
    drain(&thread);
    let log = handler.log.lock().unwrap();
    assert_eq!(log.len(), 1002);
    for (i, entry) in log.iter().enumerate().skip(1).take(1000) {
        assert_eq!(*entry, format!("move {i}"));
    }
}

#[test]
fn unregistering_the_capturing_handler_mid_gesture_releases_it() {
    let (thread, interaction, _) = handle();
    let handler = Arc::new(Recording::default());
    let registration =
        block_on(interaction.register_handler("gone".into(), 1.0, None, handler.clone()))
            .unwrap();
    interaction.dispatch_pointer(sample(PointerPhase::Down, 1.0));
    drain(&thread);
    registration.unregister();
    interaction.dispatch_pointer(sample(PointerPhase::Move, 2.0));
    interaction.dispatch_pointer(sample(PointerPhase::Up, 3.0));
    drain(&thread);
    assert_eq!(*handler.log.lock().unwrap(), ["down 1"]);
}

#[test]
fn would_claim_touch_asks_the_eligible_handlers() {
    let (_thread, interaction, _) = handle();
    assert!(!block_on(interaction.would_claim_touch(sample(PointerPhase::Down, 1.0))).unwrap());
    let handler = Arc::new(Recording { claims_touch: true, ..Default::default() });
    let _registration =
        block_on(interaction.register_handler("claims".into(), 1.0, None, handler)).unwrap();
    assert!(block_on(interaction.would_claim_touch(sample(PointerPhase::Down, 1.0))).unwrap());
}

#[test]
fn a_handler_scoped_to_a_source_ignores_other_sources() {
    let (thread, interaction, _) = handle();
    let handler = Arc::new(Recording::default());
    let _registration = block_on(interaction.register_handler(
        "scoped".into(),
        1.0,
        Some("thumbnails".into()),
        handler.clone(),
    ))
    .unwrap();
    let mut down = sample(PointerPhase::Down, 1.0);
    down.source = Some("stage".into());
    interaction.dispatch_pointer(down);
    drain(&thread);
    assert!(handler.log.lock().unwrap().is_empty());
}

#[test]
fn a_closed_documents_capability_refuses_with_instance_closed() {
    let (thread, interaction, _) = handle();
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    for error in [
        block_on(interaction.active_tool_id()).map(|_| ()).unwrap_err(),
        block_on(interaction.activate_tool("pan".into())).unwrap_err(),
        block_on(interaction.cursor()).map(|_| ()).unwrap_err(),
    ] {
        assert_eq!(code(&error), Some("instance-closed"), "{error:?}");
    }
    // A sample after the close is dropped, not a panic.
    interaction.dispatch_pointer(sample(PointerPhase::Down, 1.0));
    drain(&thread);
}

#[test]
fn closing_the_document_ends_its_subscriptions() {
    let (thread, interaction, _) = handle();
    let events = Arc::new(Events::default());
    let _subscription = interaction.subscribe(events.clone());
    drain(&thread);
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    assert_eq!(events.1.load(Ordering::SeqCst), 1);
    let late = Arc::new(Events::default());
    let _late = interaction.subscribe(late.clone());
    drain(&thread);
    assert_eq!(late.1.load(Ordering::SeqCst), 1);
}
```

- [x] **Step 3: Run them to verify they fail**

Run: `cargo test -p epdf-plugin-interaction --features uniffi --test ffi`
Expected: FAIL to compile, "unresolved import `epdf_plugin_interaction::ffi`".

- [x] **Step 4: Write `ffi.rs`, the records and conversions**

```rust
//! The interaction plugin through uniffi: the pointer hub as one handle,
//! with a foreign handler trait a Swift gesture source can implement, and
//! the feedback plugin with its foreign provider.
//!
//! Every method hops to the kernel thread through `KernelBound`.
//! `dispatch_pointer` is the hot path: it fires and returns, so a 120 Hz
//! source never waits on the kernel thread. A foreign handler runs on the
//! kernel thread, synchronously, inside the dispatch.
//!
//! Payloads (`ActivateToolOptions.payload`, `ToolChangedEvent.payload`) do
//! not cross: they are hand-offs between Rust plugins. `PointerSample`'s
//! `project` does not cross either; a sample from a native source carries
//! only its page hit.

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use std::sync::Arc;

use epdf_ffi::{
    HostError, KernelBound, KernelPost, Modifiers, PageRef, PointerGesture, PointerKind,
    PointerPageHit, PointerPhase, PointerSample, RegistrationHandle, Subscription, ViewerError,
};
use epdf_geometry::PageRotation;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::{Error, Unsubscribe};

use crate::{InteractionCapability, RegisterHandlerOptions, RegisterToolOptions, Tool};

/// Where a foreign callback's failure goes: the kernel's report sink.
pub type Report = Rc<dyn Fn(&Error)>;

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct InteractionTool {
    pub id: String,
    pub cursor: String,
    /// The cursor over the viewport's page gaps; `None` is the arrow.
    #[uniffi(default = None)]
    pub gap_cursor: Option<String>,
    /// Behaviour tags (`text-select`, `scroll`, ...), each once, in order.
    pub enables: Vec<String>,
    /// Arming this tool is consent to create with a finger.
    #[uniffi(default = None)]
    pub touch_direct: Option<bool>,
}

impl From<Tool> for InteractionTool {
    fn from(tool: Tool) -> Self {
        InteractionTool {
            id: tool.id,
            cursor: tool.cursor,
            gap_cursor: tool.gap_cursor,
            enables: tool.enables,
            touch_direct: tool.touch_direct,
        }
    }
}

impl From<InteractionTool> for Tool {
    fn from(tool: InteractionTool) -> Self {
        let mut out = Tool::new(tool.id, tool.cursor, tool.enables.iter().map(String::as_str));
        out.gap_cursor = tool.gap_cursor;
        out.touch_direct = tool.touch_direct;
        out
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct InteractionToolChangedEvent {
    pub tool_id: String,
    pub previous_tool_id: String,
    pub tool: InteractionTool,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct InteractionGestureEvent {
    pub handler_id: String,
    pub page: Option<PageRef>,
    pub pointer_type: PointerKind,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct InteractionCursorChangedEvent {
    pub cursor: String,
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum InteractionEvent {
    ToolChanged { event: InteractionToolChangedEvent },
    GestureStarted { event: InteractionGestureEvent },
    GestureEnded { event: InteractionGestureEvent },
    GestureCancelled { event: InteractionGestureEvent },
    CursorChanged { event: InteractionCursorChangedEvent },
}

#[uniffi::export(with_foreign)]
pub trait InteractionEventListener: Send + Sync {
    fn on_event(&self, event: InteractionEvent);
    /// The document closed: no event follows. Called once, and at once for
    /// a subscription made after the close.
    fn on_end(&self);
}

impl From<PointerPhase> for crate::Phase {
    fn from(phase: PointerPhase) -> Self {
        match phase {
            PointerPhase::Down => crate::Phase::Down,
            PointerPhase::Move => crate::Phase::Move,
            PointerPhase::Up => crate::Phase::Up,
            PointerPhase::Cancel => crate::Phase::Cancel,
        }
    }
}

impl From<crate::Phase> for PointerPhase {
    fn from(phase: crate::Phase) -> Self {
        match phase {
            crate::Phase::Down => PointerPhase::Down,
            crate::Phase::Move => PointerPhase::Move,
            crate::Phase::Up => PointerPhase::Up,
            crate::Phase::Cancel => PointerPhase::Cancel,
        }
    }
}

impl From<PointerKind> for crate::PointerKind {
    fn from(kind: PointerKind) -> Self {
        match kind {
            PointerKind::Mouse => crate::PointerKind::Mouse,
            PointerKind::Pen => crate::PointerKind::Pen,
            PointerKind::Touch => crate::PointerKind::Touch,
        }
    }
}

impl From<crate::PointerKind> for PointerKind {
    fn from(kind: crate::PointerKind) -> Self {
        match kind {
            crate::PointerKind::Mouse => PointerKind::Mouse,
            crate::PointerKind::Pen => PointerKind::Pen,
            crate::PointerKind::Touch => PointerKind::Touch,
        }
    }
}

impl From<PointerGesture> for crate::Gesture {
    fn from(gesture: PointerGesture) -> Self {
        match gesture {
            PointerGesture::LongPress => crate::Gesture::LongPress,
        }
    }
}

impl From<crate::Gesture> for PointerGesture {
    fn from(gesture: crate::Gesture) -> Self {
        match gesture {
            crate::Gesture::LongPress => PointerGesture::LongPress,
        }
    }
}

impl From<Modifiers> for crate::Modifiers {
    fn from(m: Modifiers) -> Self {
        crate::Modifiers { shift: m.shift, alt: m.alt, ctrl: m.ctrl, meta: m.meta }
    }
}

impl From<crate::Modifiers> for Modifiers {
    fn from(m: crate::Modifiers) -> Self {
        Modifiers { shift: m.shift, alt: m.alt, ctrl: m.ctrl, meta: m.meta }
    }
}

impl From<PointerPageHit> for crate::PageHit {
    fn from(hit: PointerPageHit) -> Self {
        crate::PageHit {
            page: hit.page.into(),
            point: hit.point.into(),
            scale: hit.scale,
            rotation: hit.rotation.map(PageRotation),
            zoom: hit.zoom,
        }
    }
}

impl From<crate::PageHit> for PointerPageHit {
    fn from(hit: crate::PageHit) -> Self {
        PointerPageHit {
            page: hit.page.into(),
            point: hit.point.into(),
            scale: hit.scale,
            rotation: hit.rotation.map(|r| r.0),
            zoom: hit.zoom,
        }
    }
}

impl From<PointerSample> for crate::PointerSample {
    fn from(s: PointerSample) -> Self {
        crate::PointerSample {
            phase: s.phase.into(),
            viewport: s.viewport.into(),
            page: s.page.map(Into::into),
            modifiers: s.modifiers.into(),
            click_count: s.click_count.map(f64::from),
            pointer_type: s.pointer_type.map(Into::into),
            gesture: s.gesture.map(Into::into),
            source: s.source,
            project: None,
        }
    }
}

impl From<&crate::PointerSample> for PointerSample {
    fn from(s: &crate::PointerSample) -> Self {
        PointerSample {
            phase: s.phase.into(),
            viewport: s.viewport.into(),
            page: s.page.map(Into::into),
            modifiers: s.modifiers.into(),
            // A click count is 1, 2 or 3; anything else reads as a single.
            click_count: s.click_count.map(|n| if (1.0..=3.0).contains(&n) { n as u32 } else { 1 }),
            pointer_type: s.pointer_type.map(Into::into),
            gesture: s.gesture.map(Into::into),
            source: s.source.clone(),
        }
    }
}

fn gesture_event(event: &crate::GestureEvent) -> InteractionGestureEvent {
    InteractionGestureEvent {
        handler_id: event.handler_id.clone(),
        page: event.page.map(Into::into),
        pointer_type: event.pointer_type.into(),
    }
}
```

`epdf_ffi::Point` converts from `epdf_geometry::Point` and `epdf_kernel::geometry::PageRef` already (`crates/epdf-ffi/src/geometry.rs`). If `crate::PageHit.page` is the kernel `PageRef` (it is: `contract.rs` imports `epdf_kernel::geometry::PageRef`), the `.into()` calls above resolve to those impls.

- [x] **Step 5: Write `ffi.rs`, the foreign handler and its adapter**

```rust
/// A pointer handler a foreign gesture source implements. The hub routes
/// each gesture to the first eligible handler, by priority, whose `on_down`
/// returns true; move and up then go to it until the up.
///
/// Called on the kernel thread, synchronously, inside `dispatch_pointer`.
/// Return quickly and never block waiting for the viewer: a call into the
/// viewer from here queues behind this dispatch.
#[uniffi::export(with_foreign)]
pub trait InteractionHandler: Send + Sync {
    /// Usually "does the tool enable my tag".
    fn enabled_for(&self, tool: InteractionTool) -> bool;
    /// True captures the gesture. A thrown error is reported through the
    /// kernel's report sink and the gesture is not captured.
    fn on_down(&self, sample: PointerSample) -> Result<bool, HostError>;
    fn on_move(&self, sample: PointerSample);
    fn on_up(&self, sample: PointerSample);
    /// The gesture was aborted, not completed.
    fn on_cancel(&self, sample: PointerSample);
    /// The pointer moved with no gesture active: cursor feedback only.
    fn on_hover(&self, sample: PointerSample);
    /// Touch consent: claim this contact as a tool gesture. A pure read.
    fn claims_touch(&self, sample: PointerSample) -> bool;
}

/// The hub's handler, backed by a foreign one. The id and priority are
/// fixed at registration, because the hub reads them by reference.
struct ForeignHandler {
    id: String,
    priority: f64,
    inner: Arc<dyn InteractionHandler>,
    report: Report,
}

impl crate::InteractionHandler for ForeignHandler {
    fn id(&self) -> &str {
        &self.id
    }
    fn priority(&self) -> f64 {
        self.priority
    }
    fn enabled_for(&self, tool: &Tool) -> bool {
        self.inner.enabled_for(tool.clone().into())
    }
    fn on_down(&self, sample: &crate::PointerSample) -> bool {
        match self.inner.on_down(sample.into()) {
            Ok(captured) => captured,
            Err(error) => {
                (self.report)(&error.into());
                false
            }
        }
    }
    fn on_move(&self, sample: &crate::PointerSample) {
        self.inner.on_move(sample.into());
    }
    fn on_up(&self, sample: &crate::PointerSample) {
        self.inner.on_up(sample.into());
    }
    fn on_cancel(&self, sample: &crate::PointerSample) {
        self.inner.on_cancel(sample.into());
    }
    fn on_hover(&self, sample: &crate::PointerSample) {
        self.inner.on_hover(sample.into());
    }
    fn claims_touch(&self, sample: &crate::PointerSample) -> bool {
        self.inner.claims_touch(sample.into())
    }
}
```

- [x] **Step 6: Write `ffi.rs`, the handle**

```rust
/// The capability, if its document is still open.
fn open(cap: &InteractionCapability) -> Result<&InteractionCapability, Error> {
    if cap.lifetime().aborted() {
        return Err(plugin_error(
            code::INSTANCE_CLOSED,
            "interaction",
            "the document this capability belongs to is closed",
            None,
            None,
        ));
    }
    Ok(cap)
}

/// What lives in the kernel thread's slab for one handle.
struct Bound {
    cap: InteractionCapability,
    report: Report,
}

#[derive(uniffi::Object)]
pub struct InteractionHandle {
    bound: KernelBound<Bound>,
    post: KernelPost,
}

impl InteractionHandle {
    /// On the kernel thread. `report` receives a foreign handler's errors.
    pub fn new(cap: InteractionCapability, post: KernelPost, report: Report) -> Arc<Self> {
        Arc::new(InteractionHandle {
            bound: KernelBound::new(Bound { cap, report }, post.clone()),
            post,
        })
    }
}

#[uniffi::export]
impl InteractionHandle {
    /// The armed tool. An id nobody registered reads back as a neutral tool
    /// with no tags.
    pub async fn active_tool(&self) -> Result<InteractionTool, ViewerError> {
        self.bound.call(|b| Ok(open(&b.cap)?.get_active_tool().into())).await
    }

    pub async fn active_tool_id(&self) -> Result<String, ViewerError> {
        self.bound.call(|b| Ok(open(&b.cap)?.get_active_tool_id())).await
    }

    /// The tool a freshly opened document starts with.
    pub async fn default_tool_id(&self) -> Result<String, ViewerError> {
        self.bound.call(|b| Ok(open(&b.cap)?.get_default_tool_id())).await
    }

    pub async fn tools(&self) -> Result<Vec<InteractionTool>, ViewerError> {
        self.bound
            .call(|b| Ok(open(&b.cap)?.list_tools().into_iter().map(Into::into).collect()))
            .await
    }

    pub async fn tool(&self, id: String) -> Result<Option<InteractionTool>, ViewerError> {
        self.bound.call(move |b| Ok(open(&b.cap)?.get_tool(&id).map(Into::into))).await
    }

    pub async fn has_tool(&self, id: String) -> Result<bool, ViewerError> {
        self.bound.call(move |b| Ok(open(&b.cap)?.has_tool(&id))).await
    }

    /// Whether the active tool enables a behaviour tag such as `text-select`.
    pub async fn active_tool_enables(&self, behavior: String) -> Result<bool, ViewerError> {
        self.bound.call(move |b| Ok(open(&b.cap)?.active_tool_enables(&behavior))).await
    }

    /// Rejects `not-found` for an id nobody registered.
    pub async fn activate_tool(&self, id: String) -> Result<(), ViewerError> {
        self.bound.call(move |b| open(&b.cap)?.activate_tool(&id, None)).await
    }

    pub async fn activate_default_tool(&self) -> Result<(), ViewerError> {
        self.bound.call(|b| open(&b.cap)?.activate_default_tool()).await
    }

    /// Arm a tool for now (hold to pan); `pop_tool` restores the last one.
    pub async fn push_tool(&self, id: String) -> Result<(), ViewerError> {
        self.bound.call(move |b| open(&b.cap)?.push_tool(&id, None)).await
    }

    /// Nothing when the stack is empty.
    pub async fn pop_tool(&self) -> Result<(), ViewerError> {
        self.bound.call(|b| open(&b.cap)?.pop_tool()).await
    }

    /// Reskin a tool's cursor keywords; `None` removes the skin.
    pub async fn set_tool_cursor(
        &self,
        id: String,
        skin: Option<HashMap<String, String>>,
    ) -> Result<(), ViewerError> {
        self.bound
            .call(move |b| {
                let skin = skin.map(|s| s.into_iter().collect::<BTreeMap<_, _>>());
                open(&b.cap)?.set_tool_cursor(&id, skin);
                Ok(())
            })
            .await
    }

    /// Rejects `conflict` for an id already registered unless `replace`.
    /// The registration's `unregister` removes only this registration.
    pub async fn register_tool(
        &self,
        tool: InteractionTool,
        replace: bool,
    ) -> Result<Arc<RegistrationHandle>, ViewerError> {
        let post = self.post.clone();
        self.bound
            .call(move |b| {
                let off =
                    open(&b.cap)?.register_tool(tool.into(), RegisterToolOptions { replace })?;
                Ok(RegistrationHandle::new(post, move || off.call()))
            })
            .await
    }

    /// Add a pointer handler. `priority` is fixed here; higher wins a
    /// gesture. `source` scopes it to one lens (a stage id).
    pub async fn register_handler(
        &self,
        id: String,
        priority: f64,
        source: Option<String>,
        handler: Arc<dyn InteractionHandler>,
    ) -> Result<Arc<RegistrationHandle>, ViewerError> {
        let post = self.post.clone();
        self.bound
            .call(move |b| {
                let foreign =
                    Rc::new(ForeignHandler { id, priority, inner: handler, report: b.report.clone() });
                let off = open(&b.cap)?
                    .register_handler(foreign, RegisterHandlerOptions { source });
                Ok(RegistrationHandle::new(post, move || off.call()))
            })
            .await
    }

    /// Push a cursor claim, or clear it with `None`. Higher priority wins.
    pub async fn claim_cursor(
        &self,
        token: String,
        cursor: Option<String>,
        priority: f64,
    ) -> Result<(), ViewerError> {
        self.bound
            .call(move |b| {
                open(&b.cap)?.claim_cursor(&token, cursor.as_deref(), priority);
                Ok(())
            })
            .await
    }

    /// The resolved cursor keyword.
    pub async fn cursor(&self) -> Result<String, ViewerError> {
        self.bound.call(|b| Ok(open(&b.cap)?.get_cursor())).await
    }

    /// Touch pre-flight: would any eligible handler claim this contact.
    pub async fn would_claim_touch(&self, sample: PointerSample) -> Result<bool, ViewerError> {
        self.bound.call(move |b| Ok(open(&b.cap)?.would_claim_touch(&sample.into()))).await
    }

    /// Pointer ingress. Returns at once; the sample runs on the kernel
    /// thread in the order it was sent. Dropped after the document closes.
    pub fn dispatch_pointer(&self, sample: PointerSample) {
        self.bound.fire(move |b| {
            if let Ok(cap) = open(&b.cap) {
                cap.dispatch_pointer(&sample.into());
            }
        });
    }

    /// The listener is registered on the kernel thread after this returns;
    /// a cancel before then still takes effect. It gets `on_end` when the
    /// document closes (at once if it already has).
    pub fn subscribe(&self, listener: Arc<dyn InteractionEventListener>) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.bound.fire(move |b| {
            let c = &b.cap;
            let ending = listener.clone();
            slot.fill_until_end(
                c.lifetime(),
                || {
                    macro_rules! relay {
                        ($hook:ident, |$e:ident| $event:expr) => {{
                            let listener = listener.clone();
                            c.$hook(move |$e| {
                                listener.on_event($event);
                                Ok(())
                            })
                        }};
                    }
                    let offs: Vec<Unsubscribe> = vec![
                        relay!(on_tool_changed, |e| InteractionEvent::ToolChanged {
                            event: InteractionToolChangedEvent {
                                tool_id: e.tool_id.clone(),
                                previous_tool_id: e.previous_tool_id.clone(),
                                tool: e.tool.clone().into(),
                            },
                        }),
                        relay!(on_gesture_started, |e| InteractionEvent::GestureStarted {
                            event: gesture_event(e),
                        }),
                        relay!(on_gesture_ended, |e| InteractionEvent::GestureEnded {
                            event: gesture_event(e),
                        }),
                        relay!(on_gesture_cancelled, |e| InteractionEvent::GestureCancelled {
                            event: gesture_event(e),
                        }),
                        relay!(on_cursor_changed, |e| InteractionEvent::CursorChanged {
                            event: InteractionCursorChangedEvent { cursor: e.cursor.clone() },
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

- [x] **Step 7: Run the tests to verify they pass**

Run: `cargo test -p epdf-plugin-interaction --features uniffi --test ffi`
Expected: PASS, 15 tests.

Run: `cargo test -p epdf-plugin-interaction`
Expected: PASS, the conformance vectors and `tests/api.rs` unchanged.

- [x] **Step 8: Commit**

```bash
git add crates/plugins/interaction
git commit -m "Export the interaction plugin through uniffi with a foreign handler trait"
```

### Task 3: `FeedbackHandle` and the `PlatformFeedback` foreign trait

**Files:**
- Modify: `crates/plugins/interaction/src/ffi.rs`, `crates/plugins/interaction/tests/ffi.rs`

**Interfaces:**
- Consumes: `crate::{Feedback, FeedbackPluginOptions, ImpactWeight, NotifyKind, PlatformFeedback, FEEDBACK, feedback_plugin}` (`crates/plugins/interaction/src/feedback.rs`).
- Produces: foreign trait `epdf_plugin_interaction::ffi::PlatformFeedback { selection(), impact(weight: Option<FeedbackImpactWeight>), notify(kind: FeedbackNotifyKind) }`; enums `FeedbackImpactWeight`, `FeedbackNotifyKind`; record `FeedbackConfig { provider: Arc<dyn PlatformFeedback> }` with `FeedbackConfig::into_options(self) -> crate::FeedbackPluginOptions`; object `FeedbackHandle` with `FeedbackHandle::new(cap: Feedback, post: KernelPost) -> Arc<Self>` and async `selection()`, `impact(weight)`, `notify(kind)`.

- [x] **Step 1: Write the failing test**

Append to `crates/plugins/interaction/tests/ffi.rs`:

```rust
use epdf_plugin_interaction::ffi::{
    FeedbackConfig, FeedbackHandle, FeedbackImpactWeight, FeedbackNotifyKind, PlatformFeedback,
};
use epdf_plugin_interaction::{FEEDBACK, feedback_plugin};

#[derive(Default)]
struct Haptics(Mutex<Vec<String>>);

impl PlatformFeedback for Haptics {
    fn selection(&self) {
        self.0.lock().unwrap().push("selection".into());
    }
    fn impact(&self, weight: Option<FeedbackImpactWeight>) {
        self.0.lock().unwrap().push(format!("impact {weight:?}"));
    }
    fn notify(&self, kind: FeedbackNotifyKind) {
        self.0.lock().unwrap().push(format!("notify {kind:?}"));
    }
}

#[test]
fn feedback_reaches_the_foreign_provider() {
    let haptics = Arc::new(Haptics::default());
    let config = FeedbackConfig { provider: haptics.clone() };
    let thread = KernelThread::spawn(move |_| {
        TestKernel::open(vec![feedback_plugin(config.into_options())], one_document())
    })
    .unwrap();
    let post = thread.post();
    let feedback = block_on(thread.call(move |k| {
        FeedbackHandle::new(k.kernel.capability(&FEEDBACK, None).unwrap(), post)
    }))
    .unwrap();
    block_on(feedback.selection()).unwrap();
    block_on(feedback.impact(Some(FeedbackImpactWeight::Heavy))).unwrap();
    block_on(feedback.impact(None)).unwrap();
    block_on(feedback.notify(FeedbackNotifyKind::Warning)).unwrap();
    assert_eq!(
        *haptics.0.lock().unwrap(),
        ["selection", "impact Some(Heavy)", "impact None", "notify Warning"]
    );
}
```

`KernelThread::spawn` needs its closure to be `Send`; `FeedbackConfig` holds an `Arc<dyn PlatformFeedback>`, and the trait is `Send + Sync`, so the move compiles. `into_options` runs on the kernel thread, where the `Rc` it builds stays.

- [x] **Step 2: Run it to verify it fails**

Run: `cargo test -p epdf-plugin-interaction --features uniffi --test ffi feedback`
Expected: FAIL to compile, "cannot find `FeedbackConfig` in `ffi`".

- [x] **Step 3: Write the feedback half of `ffi.rs`**

Change the `use crate::{...}` line to `use crate::{Feedback, ImpactWeight, InteractionCapability, NotifyKind, RegisterHandlerOptions, RegisterToolOptions, Tool};`, then append:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum FeedbackImpactWeight {
    Light,
    Medium,
    Heavy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum FeedbackNotifyKind {
    Success,
    Warning,
    Error,
}

impl From<ImpactWeight> for FeedbackImpactWeight {
    fn from(weight: ImpactWeight) -> Self {
        match weight {
            ImpactWeight::Light => FeedbackImpactWeight::Light,
            ImpactWeight::Medium => FeedbackImpactWeight::Medium,
            ImpactWeight::Heavy => FeedbackImpactWeight::Heavy,
        }
    }
}

impl From<FeedbackImpactWeight> for ImpactWeight {
    fn from(weight: FeedbackImpactWeight) -> Self {
        match weight {
            FeedbackImpactWeight::Light => ImpactWeight::Light,
            FeedbackImpactWeight::Medium => ImpactWeight::Medium,
            FeedbackImpactWeight::Heavy => ImpactWeight::Heavy,
        }
    }
}

impl From<NotifyKind> for FeedbackNotifyKind {
    fn from(kind: NotifyKind) -> Self {
        match kind {
            NotifyKind::Success => FeedbackNotifyKind::Success,
            NotifyKind::Warning => FeedbackNotifyKind::Warning,
            NotifyKind::Error => FeedbackNotifyKind::Error,
        }
    }
}

impl From<FeedbackNotifyKind> for NotifyKind {
    fn from(kind: FeedbackNotifyKind) -> Self {
        match kind {
            FeedbackNotifyKind::Success => NotifyKind::Success,
            FeedbackNotifyKind::Warning => NotifyKind::Warning,
            FeedbackNotifyKind::Error => NotifyKind::Error,
        }
    }
}

/// Haptics, as the platform provides them. Called on the kernel thread:
/// an implementation that needs the main thread (UIKit's feedback
/// generators do) hops there itself.
#[uniffi::export(with_foreign)]
pub trait PlatformFeedback: Send + Sync {
    fn selection(&self);
    fn impact(&self, weight: Option<FeedbackImpactWeight>);
    fn notify(&self, kind: FeedbackNotifyKind);
}

struct ForeignFeedback(Arc<dyn PlatformFeedback>);

impl crate::PlatformFeedback for ForeignFeedback {
    fn selection(&self) {
        self.0.selection();
    }
    fn impact(&self, weight: Option<ImpactWeight>) {
        self.0.impact(weight.map(Into::into));
    }
    fn notify(&self, kind: NotifyKind) {
        self.0.notify(kind.into());
    }
}

/// `feedbackPlugin({ provider })`'s options, as `ViewerConfig` carries them.
#[derive(Clone, uniffi::Record)]
pub struct FeedbackConfig {
    pub provider: Arc<dyn PlatformFeedback>,
}

impl std::fmt::Debug for FeedbackConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FeedbackConfig").field("provider", &"foreign").finish()
    }
}

impl FeedbackConfig {
    /// On the kernel thread: the options hold an `Rc`.
    pub fn into_options(self) -> crate::FeedbackPluginOptions {
        crate::FeedbackPluginOptions { provider: Some(Rc::new(ForeignFeedback(self.provider))) }
    }
}

/// The workspace's feedback capability: what plugins call, callable from
/// Swift too (a custom gesture that wants the same haptic vocabulary).
#[derive(uniffi::Object)]
pub struct FeedbackHandle {
    cap: KernelBound<Feedback>,
}

impl FeedbackHandle {
    /// On the kernel thread.
    pub fn new(cap: Feedback, post: KernelPost) -> Arc<Self> {
        Arc::new(FeedbackHandle { cap: KernelBound::new(cap, post) })
    }
}

#[uniffi::export]
impl FeedbackHandle {
    pub async fn selection(&self) -> Result<(), ViewerError> {
        self.cap.call(|c| {
            c.selection();
            Ok(())
        })
        .await
    }

    pub async fn impact(&self, weight: Option<FeedbackImpactWeight>) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                c.impact(weight.map(Into::into));
                Ok(())
            })
            .await
    }

    pub async fn notify(&self, kind: FeedbackNotifyKind) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                c.notify(kind.into());
                Ok(())
            })
            .await
    }
}
```

UniFFI 0.32 carries a foreign trait object inside a record. If the generator rejects `Arc<dyn PlatformFeedback>` as a record field on your machine, make `FeedbackConfig` an object (`#[derive(uniffi::Object)]` with a `#[uniffi::constructor] fn new(provider: Arc<dyn PlatformFeedback>) -> Arc<Self>`), make the `ViewerConfig` field `Option<Arc<FeedbackConfig>>`, and have `into_options` take `&self`. The Swift call site in Task 5 then reads `FeedbackConfig(provider: haptics)` either way.

- [x] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p epdf-plugin-interaction --features uniffi --test ffi`
Expected: PASS, 16 tests.

- [x] **Step 5: Commit**

```bash
git add crates/plugins/interaction
git commit -m "Export the feedback plugin with a foreign PlatformFeedback provider"
```

### Task 4: Host wiring

**Files:**
- Modify: `crates/epdf-viewer-host/Cargo.toml`, `crates/epdf-viewer-host/src/api.rs`, `crates/epdf-viewer-host/src/session.rs`, `crates/epdf-viewer-host/src/lib.rs`, `crates/epdf-viewer-host/tests/viewer_host.rs`

**Interfaces:**
- Consumes: `InteractionHandle::new`, `FeedbackHandle::new`, `FeedbackConfig::into_options`.
- Produces: `ViewerConfig.interaction: bool`, `ViewerConfig.feedback: Option<FeedbackConfig>`; `ViewerHost::interaction(document_id: String) -> Result<Arc<InteractionHandle>, ViewerError>`; `ViewerHost::feedback() -> Result<Arc<FeedbackHandle>, ViewerError>`; a private `fn reporter(kernel: &Kernel) -> Rc<dyn Fn(&Error)>` in `lib.rs` that later plans reuse for their own foreign ports.

- [x] **Step 1: Write the failing tests**

Append to `crates/epdf-viewer-host/tests/viewer_host.rs`:

```rust
use epdf_ffi::{HostError, Modifiers, Point, PointerPageHit, PointerPhase, PointerSample};
use epdf_plugin_interaction::ffi::{InteractionHandler, InteractionTool};

#[derive(Default)]
struct Reports(Mutex<Vec<String>>);
impl ViewerDelegate for Reports {
    fn state_changed(&self, _revision: u64) {}
    fn report(&self, error: ViewerError) {
        self.0.lock().unwrap().push(format!("{error}"));
    }
}

struct Throws;
impl InteractionHandler for Throws {
    fn enabled_for(&self, _tool: InteractionTool) -> bool {
        true
    }
    fn on_down(&self, _sample: PointerSample) -> Result<bool, HostError> {
        Err(HostError::Failed { detail: "swift threw".into() })
    }
    fn on_move(&self, _sample: PointerSample) {}
    fn on_up(&self, _sample: PointerSample) {}
    fn on_cancel(&self, _sample: PointerSample) {}
    fn on_hover(&self, _sample: PointerSample) {}
    fn claims_touch(&self, _sample: PointerSample) -> bool {
        false
    }
}

fn with_interaction(delegate: Arc<dyn ViewerDelegate>) -> Arc<ViewerHost> {
    let config = ViewerConfig { interaction: true, ..Default::default() };
    let host = ViewerHost::new(config, delegate).unwrap();
    block_on(host.start()).unwrap();
    host
}

#[test]
fn the_interaction_plugin_switches_tools_through_the_host() {
    let host = with_interaction(Arc::new(Delegate));
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let interaction = block_on(host.interaction(doc.id)).unwrap();
    assert_eq!(block_on(interaction.active_tool_id()).unwrap(), "pointer");
    block_on(interaction.activate_tool("pan".into())).unwrap();
    assert_eq!(block_on(interaction.active_tool_id()).unwrap(), "pan");
}

#[test]
fn a_foreign_handlers_error_reaches_the_delegate() {
    let reports = Arc::new(Reports::default());
    let host = with_interaction(reports.clone());
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let pages = block_on(host.pages(doc.id.clone())).unwrap();
    let interaction = block_on(host.interaction(doc.id)).unwrap();
    let _registration =
        block_on(interaction.register_handler("throws".into(), 1.0, None, Arc::new(Throws)))
            .unwrap();
    interaction.dispatch_pointer(PointerSample {
        phase: PointerPhase::Down,
        viewport: Point { x: 10.0, y: 10.0 },
        page: Some(PointerPageHit {
            page: pages[0].page,
            point: Point { x: 10.0, y: 10.0 },
            scale: None,
            rotation: None,
            zoom: None,
        }),
        modifiers: Modifiers::default(),
        click_count: None,
        pointer_type: None,
        gesture: None,
        source: None,
    });
    // Any reply after the fire means the dispatch has run.
    block_on(interaction.cursor()).unwrap();
    let reports = reports.0.lock().unwrap();
    assert!(reports.iter().any(|r| r.contains("HostError") && r.contains("swift threw")),
        "{reports:?}");
}

#[test]
fn without_its_flag_the_interaction_plugin_is_not_installed() {
    let host = started();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    assert!(block_on(host.interaction(doc.id)).is_err());
}

#[test]
fn without_its_config_the_feedback_plugin_is_not_installed() {
    let host = started();
    assert!(block_on(host.feedback()).is_err());
}
```

- [x] **Step 2: Run them to verify they fail**

Run: `cargo test -p epdf-viewer-host --test viewer_host interaction`
Expected: FAIL to compile, "no field `interaction` on type `ViewerConfig`".

- [x] **Step 3: Add the dependency**

In `crates/epdf-viewer-host/Cargo.toml`, in alphabetical order among the plugin lines:

```toml
epdf-plugin-interaction = { path = "../plugins/interaction", default-features = false, features = ["uniffi"] }
```

- [x] **Step 4: Add the config fields**

In `crates/epdf-viewer-host/src/api.rs`, add the import and the two fields at the end of `ViewerConfig`:

```rust
use epdf_plugin_interaction::ffi::FeedbackConfig;
```

```rust
    /// Install `interactionPlugin()`: the pointer hub, one per document.
    /// Selection, link, annotation, form and measurement require it.
    #[uniffi(default = false)]
    pub interaction: bool,
    /// Install `feedbackPlugin({ provider })`: haptics for the plugins that
    /// use them. `None` leaves it out, and plugins skip feedback.
    #[uniffi(default = None)]
    pub feedback: Option<FeedbackConfig>,
```

- [x] **Step 5: Install the plugins**

In `crates/epdf-viewer-host/src/session.rs`, after the render block in `Session::build`:

```rust
        if config.interaction {
            plugins.push(epdf_plugin_interaction::interaction_plugin(Default::default()));
        }
        if let Some(feedback) = config.feedback {
            plugins.push(epdf_plugin_interaction::feedback_plugin(feedback.into_options()));
        }
```

- [x] **Step 6: Add the accessors**

In `crates/epdf-viewer-host/src/lib.rs`, add to the `use` lines:

```rust
use epdf_kernel::Kernel;
use epdf_plugin_interaction::ffi::{FeedbackHandle, InteractionHandle};
```

Add this free function above `#[derive(uniffi::Object)] pub struct ViewerHost`:

```rust
/// The kernel's report sink, for an adapter that calls a foreign port and
/// has nowhere else to send its failure. It reaches `ViewerDelegate.report`.
/// Weak: the session owns the store.
fn reporter(kernel: &Kernel) -> Rc<dyn Fn(&Error)> {
    let store = Rc::downgrade(&kernel.store());
    Rc::new(move |error: &Error| {
        if let Some(store) = store.upgrade() {
            store.report(error);
        }
    })
}
```

Add to the `#[uniffi::export] impl ViewerHost` block, after `render`:

```rust
    /// The pointer hub of an open document. Needs `ViewerConfig.interaction`;
    /// without it the kernel refuses the token.
    pub async fn interaction(
        &self,
        document_id: String,
    ) -> Result<Arc<InteractionHandle>, ViewerError> {
        let post = self.post.clone();
        self.call(move |s| {
            let cap = s
                .kernel
                .capability(&epdf_plugin_interaction::INTERACTION, Some(&document_id))?;
            Ok(InteractionHandle::new(cap, post, reporter(&s.kernel)))
        })
        .await
    }

    /// The workspace's haptics. Needs `ViewerConfig.feedback`.
    pub async fn feedback(&self) -> Result<Arc<FeedbackHandle>, ViewerError> {
        let post = self.post.clone();
        self.call(move |s| {
            let cap = s.kernel.capability(&epdf_plugin_interaction::FEEDBACK, None)?;
            Ok(FeedbackHandle::new(cap, post))
        })
        .await
    }
```

If `Kernel::capability` with `None` refuses a workspace token on this branch, pass the active document's id instead (`s.kernel.documents().list().first()`), and say so in the commit message. The test in Step 1 that expects an error without the config still holds either way.

- [x] **Step 7: Run the tests to verify they pass**

Run: `cargo test -p epdf-viewer-host`
Expected: PASS, the four new tests and every existing one.

- [x] **Step 8: Commit**

```bash
git add crates/epdf-viewer-host
git commit -m "Wire the interaction and feedback plugins into the viewer host

ViewerConfig.feedback is Option<FeedbackConfig>, not a bare trait
object: ViewerConfig derives Debug and a foreign trait object is not."
```

### Task 5: Swift capabilities and haptics

**Files:**
- Create: `platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Interaction.swift`, `platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Feedback.swift`, `platforms/apple/EmbedPDF/Sources/EmbedPDFUI/HapticFeedback.swift`

**Interfaces:**
- Consumes: generated `InteractionHandle`, `FeedbackHandle`, `RegistrationHandle`, `InteractionHandler` protocol, `PlatformFeedback` protocol, records from Tasks 1 to 3.
- Consumes also: umbrella Task 0f's `PDFViewer.reportedErrors: AsyncStream<PDFError>`.
- Consumes also: umbrella Task 0f's Swift `Registration` (wraps `RegistrationHandle`, `unregister()`).
- Produces: `InteractionCapability` and `PDFViewer.interaction(for:)`; `FeedbackCapability` and `PDFViewer.feedback()`; default implementations for the optional `InteractionHandler` members; `HapticFeedback` (EmbedPDFUI) conforming to `PlatformFeedback`.

- [ ] **Step 1: Write `Plugins/Interaction.swift`**

```swift
public import EmbedPDFBindings

public typealias InteractionTool = EmbedPDFBindings.InteractionTool
public typealias InteractionEvent = EmbedPDFBindings.InteractionEvent
public typealias InteractionToolChangedEvent = EmbedPDFBindings.InteractionToolChangedEvent
public typealias InteractionGestureEvent = EmbedPDFBindings.InteractionGestureEvent
public typealias InteractionCursorChangedEvent = EmbedPDFBindings.InteractionCursorChangedEvent
public typealias InteractionHandler = EmbedPDFBindings.InteractionHandler
public typealias PointerSample = EmbedPDFBindings.PointerSample
public typealias PointerPhase = EmbedPDFBindings.PointerPhase
public typealias PointerKind = EmbedPDFBindings.PointerKind
public typealias PointerGesture = EmbedPDFBindings.PointerGesture
public typealias PointerPageHit = EmbedPDFBindings.PointerPageHit
public typealias Modifiers = EmbedPDFBindings.Modifiers

/// What the hub does when a handler leaves a member out.
extension InteractionHandler {
  public func onMove(sample: PointerSample) {}
  public func onUp(sample: PointerSample) {}
  public func onCancel(sample: PointerSample) { onUp(sample: sample) }
  public func onHover(sample: PointerSample) {}
  public func claimsTouch(sample: PointerSample) -> Bool { false }
}

/// A document's pointer hub: the active tool, the cursor, and the handlers
/// gestures route to. Get one with `PDFViewer.interaction(for:)`; the
/// viewer needs `ViewerConfig(interaction: true)`. It keeps the viewer
/// alive. After its document closes, every call throws `instance-closed`.
public struct InteractionCapability: Sendable {
  let handle: InteractionHandle
  let viewer: PDFViewer

  public func activeTool() async throws(PDFError) -> InteractionTool {
    try await bridged { try await handle.activeTool() }
  }

  public func activeToolId() async throws(PDFError) -> String {
    try await bridged { try await handle.activeToolId() }
  }

  public func defaultToolId() async throws(PDFError) -> String {
    try await bridged { try await handle.defaultToolId() }
  }

  public func tools() async throws(PDFError) -> [InteractionTool] {
    try await bridged { try await handle.tools() }
  }

  public func tool(_ id: String) async throws(PDFError) -> InteractionTool? {
    try await bridged { try await handle.tool(id: id) }
  }

  public func hasTool(_ id: String) async throws(PDFError) -> Bool {
    try await bridged { try await handle.hasTool(id: id) }
  }

  public func activeToolEnables(_ behavior: String) async throws(PDFError) -> Bool {
    try await bridged { try await handle.activeToolEnables(behavior: behavior) }
  }

  /// Throws `not-found` for an id nobody registered.
  public func activateTool(_ id: String) async throws(PDFError) {
    try await bridged { try await handle.activateTool(id: id) }
  }

  public func activateDefaultTool() async throws(PDFError) {
    try await bridged { try await handle.activateDefaultTool() }
  }

  public func pushTool(_ id: String) async throws(PDFError) {
    try await bridged { try await handle.pushTool(id: id) }
  }

  public func popTool() async throws(PDFError) {
    try await bridged { try await handle.popTool() }
  }

  /// `nil` removes the skin.
  public func setToolCursor(_ id: String, skin: [String: String]?) async throws(PDFError) {
    try await bridged { try await handle.setToolCursor(id: id, skin: skin) }
  }

  /// Throws `conflict` for an id already registered unless `replace`.
  public func registerTool(_ tool: InteractionTool, replace: Bool = false) async throws(PDFError)
    -> Registration
  {
    Registration(handle: try await bridged { try await handle.registerTool(tool: tool, replace: replace) })
  }

  /// `handler` is called on the viewer's kernel thread, synchronously,
  /// for every sample. Return quickly, and don't block waiting on the
  /// viewer from inside it.
  public func registerHandler(
    _ handler: some InteractionHandler,
    id: String,
    priority: Double,
    source: String? = nil
  ) async throws(PDFError) -> Registration {
    Registration(
      handle: try await bridged {
        try await handle.registerHandler(id: id, priority: priority, source: source, handler: handler)
      })
  }

  /// `nil` clears the claim.
  public func claimCursor(_ token: String, cursor: String?, priority: Double = 0) async throws(PDFError) {
    try await bridged { try await handle.claimCursor(token: token, cursor: cursor, priority: priority) }
  }

  public func cursor() async throws(PDFError) -> String {
    try await bridged { try await handle.cursor() }
  }

  public func wouldClaimTouch(_ sample: PointerSample) async throws(PDFError) -> Bool {
    try await bridged { try await handle.wouldClaimTouch(sample: sample) }
  }

  /// Returns at once. Samples run in the order they were sent.
  public func dispatchPointer(_ sample: PointerSample) {
    handle.dispatchPointer(sample: sample)
  }

  /// Finishes when the document closes (at once if it already has) or the
  /// viewer is destroyed.
  public var events: AsyncStream<InteractionEvent> {
    viewer.registry.stream { handle.subscribe(listener: $0) }
  }
}

extension EventRelay: InteractionEventListener where Event == InteractionEvent {
  func onEvent(event: InteractionEvent) {
    continuation.yield(event)
  }

  func onEnd() {
    continuation.finish()
  }
}

extension PDFViewer {
  public func interaction(for document: PDFDocument) async throws(PDFError) -> InteractionCapability {
    let handle = try await bridged { try await host.interaction(documentId: document.id) }
    return InteractionCapability(handle: handle, viewer: self)
  }
}
```

The generated `registerHandler` takes `handler: InteractionHandler` (an existential). `some InteractionHandler` passes through as one. If the generated protocol isn't `Sendable`, add `& Sendable` to the parameter.

- [ ] **Step 2: Write `Plugins/Feedback.swift`**

```swift
public import EmbedPDFBindings

public typealias PlatformFeedback = EmbedPDFBindings.PlatformFeedback
public typealias FeedbackConfig = EmbedPDFBindings.FeedbackConfig
public typealias FeedbackImpactWeight = EmbedPDFBindings.FeedbackImpactWeight
public typealias FeedbackNotifyKind = EmbedPDFBindings.FeedbackNotifyKind

/// The workspace's haptics. Get it with `PDFViewer.feedback()`; the viewer
/// needs `ViewerConfig(feedback: FeedbackConfig(provider: ...))`.
public struct FeedbackCapability: Sendable {
  let handle: FeedbackHandle

  public func selection() async throws(PDFError) {
    try await bridged { try await handle.selection() }
  }

  public func impact(_ weight: FeedbackImpactWeight? = nil) async throws(PDFError) {
    try await bridged { try await handle.impact(weight: weight) }
  }

  public func notify(_ kind: FeedbackNotifyKind) async throws(PDFError) {
    try await bridged { try await handle.notify(kind: kind) }
  }
}

extension PDFViewer {
  public func feedback() async throws(PDFError) -> FeedbackCapability {
    FeedbackCapability(handle: try await bridged { try await host.feedback() })
  }
}
```

- [ ] **Step 3: Write `EmbedPDFUI/HapticFeedback.swift`**

```swift
import EmbedPDF
import UIKit

/// `PlatformFeedback` over UIKit's feedback generators. The viewer calls it
/// on its kernel thread; each call hops to the main actor, where UIKit
/// wants the generators.
public final class HapticFeedback: PlatformFeedback, Sendable {
  public init() {}

  public func selection() {
    Task { @MainActor in UISelectionFeedbackGenerator().selectionChanged() }
  }

  public func impact(weight: FeedbackImpactWeight?) {
    Task { @MainActor in
      let style: UIImpactFeedbackGenerator.FeedbackStyle =
        switch weight {
        case .light: .light
        case .heavy: .heavy
        case .medium, nil: .medium
        }
      UIImpactFeedbackGenerator(style: style).impactOccurred()
    }
  }

  public func notify(kind: FeedbackNotifyKind) {
    Task { @MainActor in
      let type: UINotificationFeedbackGenerator.FeedbackType =
        switch kind {
        case .success: .success
        case .warning: .warning
        case .error: .error
        }
      UINotificationFeedbackGenerator().notificationOccurred(type)
    }
  }
}
```

- [ ] **Step 4: Build the package**

Run: `bash platforms/apple/build-xcframework.sh && (cd platforms/apple/EmbedPDF && xcodebuild build -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17')`
Expected: BUILD SUCCEEDED. A name collision with an existing generated type shows up here as a duplicate declaration.

- [ ] **Step 5: Commit**

```bash
git add platforms/apple/EmbedPDF/Sources
git commit -m "Add Swift interaction and feedback capabilities"
```

### Task 6: Swift tests

**Files:**
- Create: `platforms/apple/EmbedPDF/Tests/EmbedPDFTests/InteractionTests.swift`, `platforms/apple/EmbedPDF/Tests/EmbedPDFTests/FeedbackTests.swift`

**Interfaces:**
- Consumes: everything Task 5 produces.
- Produces: nothing later tasks use.

- [ ] **Step 1: Write `InteractionTests.swift`**

```swift
import CustomDump
import EmbedPDF
import Foundation
import Testing

/// Records what reaches it. Captures on down unless `throwsOnDown`.
final class RecordingHandler: InteractionHandler, @unchecked Sendable {
  let lock = NSLock()
  var log: [String] = []
  let throwsOnDown: Bool
  let onDownHook: (@Sendable () -> Void)?

  init(throwsOnDown: Bool = false, onDownHook: (@Sendable () -> Void)? = nil) {
    self.throwsOnDown = throwsOnDown
    self.onDownHook = onDownHook
  }

  struct Refused: Error {}

  func enabledFor(tool: InteractionTool) -> Bool { true }

  func onDown(sample: PointerSample) throws -> Bool {
    lock.withLock { log.append("down") }
    onDownHook?()
    if throwsOnDown { throw Refused() }
    return true
  }

  func onMove(sample: PointerSample) { lock.withLock { log.append("move") } }
  func onUp(sample: PointerSample) { lock.withLock { log.append("up") } }
  func onHover(sample: PointerSample) { lock.withLock { log.append("hover") } }

  var entries: [String] { lock.withLock { log } }
}

func sample(_ phase: PointerPhase, page: PageRef, x: Double = 10) -> PointerSample {
  PointerSample(
    phase: phase,
    viewport: Point(x: x, y: 10),
    page: PointerPageHit(page: page, point: Point(x: x, y: 10)),
    modifiers: Modifiers(),
    pointerType: .touch
  )
}

extension BaseSuite {
  @Suite struct InteractionTests {
    func opened() async throws -> (PDFViewer, PDFDocument, InteractionCapability, PageRef) {
      let viewer = try PDFViewer(ViewerConfig(interaction: true))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      let page = try await viewer.pages(of: document)[0].page
      return (viewer, document, try await viewer.interaction(for: document), page)
    }

    @Test func activatingAToolIsObserved() async throws {
      let (_, _, interaction, _) = try await opened()
      expectNoDifference(try await interaction.tools().map(\.id), ["pointer", "pan"])
      let events = interaction.events
      try await interaction.activateTool("pan")
      for await event in events {
        guard case let .toolChanged(event) = event else { continue }
        expectNoDifference(event.toolId, "pan")
        expectNoDifference(event.previousToolId, "pointer")
        break
      }
      expectNoDifference(try await interaction.activeToolId(), "pan")
    }

    @Test func anUnknownToolThrowsNotFound() async throws {
      let (_, _, interaction, _) = try await opened()
      await #expect {
        try await interaction.activateTool("laser")
      } throws: { error in
        (error as? PDFError)?.code == "not-found"
      }
    }

    @Test func aSwiftHandlerCapturesAGesture() async throws {
      let (_, _, interaction, page) = try await opened()
      let handler = RecordingHandler()
      _ = try await interaction.registerHandler(handler, id: "recording", priority: 5)
      interaction.dispatchPointer(sample(.down, page: page))
      interaction.dispatchPointer(sample(.move, page: page, x: 20))
      interaction.dispatchPointer(sample(.up, page: page, x: 30))
      // A reply after the fires means they have run.
      _ = try await interaction.cursor()
      expectNoDifference(handler.entries, ["down", "move", "up"])
    }

    @Test func aThrowingHandlerReportsAndFails() async throws {
      let (viewer, _, interaction, page) = try await opened()
      let handler = RecordingHandler(throwsOnDown: true)
      _ = try await interaction.registerHandler(handler, id: "throws", priority: 5)
      interaction.dispatchPointer(sample(.down, page: page))
      interaction.dispatchPointer(sample(.move, page: page, x: 20))
      var reported: PDFError?
      for await error in viewer.reportedErrors {
        reported = error
        break
      }
      expectNoDifference(reported?.name, "HostError")
      _ = try await interaction.cursor()
      // Not captured: the move hovered.
      expectNoDifference(handler.entries, ["down", "hover"])
    }

    @Test func aHandlerThatCallsBackIntoTheViewerDoesNotDeadlock() async throws {
      let (_, _, interaction, page) = try await opened()
      let (stream, continuation) = AsyncStream<String>.makeStream()
      let handler = RecordingHandler {
        Task {
          let cursor = try? await interaction.cursor()
          continuation.yield(cursor ?? "failed")
          continuation.finish()
        }
      }
      _ = try await interaction.registerHandler(handler, id: "reenters", priority: 5)
      interaction.dispatchPointer(sample(.down, page: page))
      var answer: String?
      for await cursor in stream {
        answer = cursor
      }
      expectNoDifference(answer, "default")
    }

    @Test func droppingARegistrationKeepsTheTool() async throws {
      let (_, _, interaction, _) = try await opened()
      do {
        _ = try await interaction.registerTool(
          InteractionTool(id: "ink", cursor: "crosshair", enables: ["annotation-create"]))
      }
      expectNoDifference(try await interaction.hasTool("ink"), true)
      let registration = try await interaction.registerTool(
        InteractionTool(id: "ink", cursor: "crosshair", enables: []), replace: true)
      registration.unregister()
      _ = try await interaction.cursor()
      expectNoDifference(try await interaction.hasTool("ink"), false)
    }

    @Test func aClosedDocumentsCapabilityThrows() async throws {
      let (viewer, document, interaction, _) = try await opened()
      try await viewer.close(document)
      await #expect {
        _ = try await interaction.activeToolId()
      } throws: { error in
        (error as? PDFError)?.code == "instance-closed"
      }
    }

    @Test func droppingTheStreamUnsubscribes() async throws {
      let (_, _, interaction, _) = try await opened()
      let task = Task { for await _ in interaction.events {} }
      try await Task.sleep(for: .milliseconds(20))
      task.cancel()
      _ = await task.value
      try await interaction.activateTool("pan")
    }
  }
}
```

The generated memberwise initializers take every field without a `#[uniffi(default)]`. `PointerSample`'s `modifiers` has no default (Task 1, Step 6), so the tests pass `Modifiers()`. If the generated `InteractionTool` initializer still requires `gapCursor` or `touchDirect`, the `#[uniffi(default = None)]` attributes in Task 2 didn't take; fix them there, not here.

No member of `InteractionCapability` takes a `CancelToken`, so there's no `CancelsWhenTheTaskIsCancelled` test.

- [ ] **Step 2: Write `FeedbackTests.swift`**

```swift
import CustomDump
import EmbedPDF
import Foundation
import Testing

final class RecordingFeedback: PlatformFeedback, @unchecked Sendable {
  let lock = NSLock()
  var calls: [String] = []

  func selection() { lock.withLock { calls.append("selection") } }
  func impact(weight: FeedbackImpactWeight?) { lock.withLock { calls.append("impact \(String(describing: weight))") } }
  func notify(kind: FeedbackNotifyKind) { lock.withLock { calls.append("notify \(kind)") } }
}

extension BaseSuite {
  @Suite struct FeedbackTests {
    @Test func feedbackReachesTheSwiftProvider() async throws {
      let provider = RecordingFeedback()
      let viewer = try PDFViewer(ViewerConfig(feedback: FeedbackConfig(provider: provider)))
      try await viewer.start()
      let feedback = try await viewer.feedback()
      try await feedback.selection()
      try await feedback.impact(.heavy)
      try await feedback.notify(.success)
      expectNoDifference(
        provider.lock.withLock { provider.calls },
        ["selection", "impact Optional(EmbedPDFBindings.FeedbackImpactWeight.heavy)", "notify success"])
    }

    @Test func withoutAProviderFeedbackIsNotInstalled() async throws {
      let viewer = try PDFViewer(ViewerConfig())
      try await viewer.start()
      await #expect(throws: PDFError.self) { _ = try await viewer.feedback() }
    }
  }
}
```

If the `impact` description string differs on your toolchain, compare `weight == .heavy` in the provider instead of matching the string. Feedback is workspace-scoped, so it skips `aClosedDocumentsCapabilityThrows` and has no events.

- [ ] **Step 3: Run the Swift tests**

Run: `bash platforms/apple/build-xcframework.sh && (cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17' -only-testing:EmbedPDFTests)`
Expected: TEST SUCCEEDED, including 8 `InteractionTests` and 2 `FeedbackTests`.

- [ ] **Step 4: Commit**

```bash
git add platforms/apple/EmbedPDF/Tests
git commit -m "Test the Swift interaction and feedback capabilities"
```

### Task 7: `dispatchPointer` round trip at 120 Hz

**Files:**
- Create: `platforms/apple/EmbedPDF/Tests/EmbedPDFTests/PointerLatencyTests.swift`, `docs/research/swift-pointer-latency.md`

**Interfaces:**
- Consumes: `InteractionCapability.dispatchPointer`, `registerHandler`.
- Produces: the latency numbers stage (plan 02) and the gesture work after it build on.

`2026-10-01-swift-package.md` Appendix A §A.2 asks for this before gestures are built on `dispatch_pointer`. At 120 Hz a frame is 8.33 ms. A sample that takes longer than one frame to reach its handler makes a drag lag visibly, so p95 under 8.33 ms is the bar.

- [ ] **Step 1: Write the test**

```swift
import EmbedPDF
import Foundation
import Testing

/// Stamps each move's arrival on the kernel thread.
final class StampingHandler: InteractionHandler, @unchecked Sendable {
  let lock = NSLock()
  var arrivals: [Int: ContinuousClock.Instant] = [:]

  func enabledFor(tool: InteractionTool) -> Bool { true }
  func onDown(sample: PointerSample) throws -> Bool { true }
  func onMove(sample: PointerSample) {
    let now = ContinuousClock.now
    lock.withLock { arrivals[Int(sample.viewport.x)] = now }
  }
}

extension BaseSuite {
  /// Opt in with EMBEDPDF_LATENCY=1: it takes ten seconds and its numbers
  /// mean something only on an idle machine.
  @Suite(.enabled(if: ProcessInfo.processInfo.environment["EMBEDPDF_LATENCY"] == "1"))
  struct PointerLatencyTests {
    @Test func movesReachTheirHandlerWithinAFrameAt120Hz() async throws {
      let viewer = try PDFViewer(ViewerConfig(interaction: true))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      let page = try await viewer.pages(of: document)[0].page
      let interaction = try await viewer.interaction(for: document)
      let handler = StampingHandler()
      _ = try await interaction.registerHandler(handler, id: "stamp", priority: 1)

      let count = 1200
      let period = Duration.nanoseconds(8_333_333)
      var sent: [Int: ContinuousClock.Instant] = [:]
      interaction.dispatchPointer(sample(.down, page: page, x: -1))
      var next = ContinuousClock.now
      for i in 0..<count {
        sent[i] = ContinuousClock.now
        interaction.dispatchPointer(sample(.move, page: page, x: Double(i)))
        next += period
        try await Task.sleep(until: next, clock: .continuous)
      }
      interaction.dispatchPointer(sample(.up, page: page, x: Double(count)))
      _ = try await interaction.cursor()

      let arrivals = handler.lock.withLock { handler.arrivals }
      let latencies = (0..<count).compactMap { i in arrivals[i].map { $0 - sent[i]! } }.sorted()
      #expect(latencies.count == count)
      let p50 = latencies[count / 2]
      let p95 = latencies[count * 95 / 100]
      let max = latencies[count - 1]
      print("pointer latency p50=\(p50) p95=\(p95) max=\(max)")
      #expect(p95 < period)
    }
  }
}
```

`sample(_:page:x:)` is the helper in `InteractionTests.swift`; both files are in the same test target.

- [ ] **Step 2: Run it**

Run: `(cd platforms/apple/EmbedPDF && EMBEDPDF_LATENCY=1 xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17' -only-testing:EmbedPDFTests/BaseSuite/PointerLatencyTests)`

`xcodebuild` passes the environment through to the test runner only when it's prefixed `TEST_RUNNER_`. If the suite reports as skipped, rerun with `TEST_RUNNER_EMBEDPDF_LATENCY=1` in place of `EMBEDPDF_LATENCY=1`.

Expected: TEST SUCCEEDED, with one `pointer latency p50=... p95=... max=...` line in the log. If p95 is over 8.33 ms, the test fails. Don't loosen the bar. Record the numbers, then profile the hop (Instruments' Time Profiler on the kernel thread), and stop the plan to raise it with the user: plan 02's gestures assume this budget holds.

- [ ] **Step 3: Write `docs/research/swift-pointer-latency.md`**

Record the machine, macOS, Xcode, simulator, git SHA, and the three numbers from the log, in a table with one row per run (three runs). Add one paragraph saying whether p95 is under one 120 Hz frame and what that means for plan 02's gestures. The simulator is a proxy for a device, as `docs/research/swift-bridge-benchmarks.md` says for the bridge numbers; B5's signed device run adds a device row.

- [ ] **Step 4: Commit**

```bash
git add platforms/apple/EmbedPDF/Tests/EmbedPDFTests/PointerLatencyTests.swift docs/research/swift-pointer-latency.md
git commit -m "Measure dispatchPointer's round trip at 120 Hz"
```

### Task 8: Docs, CI, merge and clean up

**Files:**
- Modify: `.github/workflows/plugin-rust.yml`, `platforms/apple/README.md`, `docs/superpowers/plans/2026-10-01-swift-package.md`

- [ ] **Step 1: Add the crate to CI**

In `.github/workflows/plugin-rust.yml`, append `interaction` to the `crates=( ... )` list Task 0 of the umbrella created, after `render`.

- [ ] **Step 2: Add the README rows**

In `platforms/apple/README.md`'s plugin table:

```markdown
| interaction | `InteractionCapability` | `interaction: true` | `2026-10-06-swift-plugins/01-interaction.md` |
| feedback | `FeedbackCapability` | `feedback: FeedbackConfig(provider: HapticFeedback())` | `2026-10-06-swift-plugins/01-interaction.md` |
```

- [ ] **Step 3: Mark the spec entry delivered**

In `2026-10-01-swift-package.md` Appendix A §A.2, change `**interaction** (D) plus **feedback** (W).` to `**interaction** (D) plus **feedback** (W), delivered by 01-interaction.md.`

- [ ] **Step 4: Run the definition of done**

```bash
cargo test -p epdf-plugin-interaction --features uniffi
cargo test -p epdf-ffi -p epdf-viewer-host
cargo clippy -p epdf-plugin-interaction -p epdf-viewer-host --all-targets --features epdf-plugin-interaction/uniffi -- -D warnings
cargo test -p epdf-plugin-interaction
bash platforms/apple/build-xcframework.sh
(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17')
cargo build -p epdf-viewer-host -p uniffi-bindgen
./target/debug/uniffi-bindgen generate --library target/debug/libepdf_viewer_host.dylib --language kotlin --no-format --out-dir "$TMPDIR/epdf-kotlin-check"
```

Expected: every command passes. Record `ls -l target/aarch64-apple-ios-sim/release/libepdf_viewer_host.a` before this plan's first commit (from `research/swift-plugins`) and now, for the commit message.

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/plugin-rust.yml platforms/apple/README.md docs/superpowers/plans/2026-10-01-swift-package.md
git commit -m "List interaction in CI and the Swift README

libepdf_viewer_host.a (simulator, release): <before> -> <after> bytes."
```

- [ ] **Step 6: Merge into the integration branch**

From the repository root's integration worktree (`.claude/worktrees/swift-plugins-integration`):

```bash
git merge --no-ff research/swift-interaction
```

Resolve any conflict in the shared files by keeping both blocks in plan-number order (umbrella, Shared files).

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
git worktree remove .claude/worktrees/swift-interaction
git branch -d research/swift-interaction
git worktree prune
```

Neither command gets `--force`. If one refuses, find out what was left behind first. Then tick 01 in the umbrella's Tasks 1 to 16 list.
