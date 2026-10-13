# Stage Swift Wrapper Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Export the stage plugin (the viewport lens: camera, zoom, scroll, layout settings, navigation) and render's six viewport members through UniFFI, wire stage into `ViewerHost`, and ship a Swift `StageCapability` plus a SwiftUI scrolling reader in `EmbedPDFUI` that pans, flings and pinches through the stage.

**Architecture:** The recipe from `2026-10-01-swift-package.md` Appendix A §A.0, with metadata and search as models. Stage's vocabulary is mostly `epdf-stage-core` and `epdf-geometry` types, which no plugin crate owns, so the ones more than one plugin will use (camera, zoom, anchor, alignment) go into a new `crates/epdf-ffi/src/view.rs`, with their `From` impls there because Rust's orphan rule allows them nowhere else. Stage's own types are mirrored in `crates/plugins/stage/src/ffi.rs` under a `Stage` prefix. A frame scheduler on the kernel's 16 ms timer gives the stage motion (tweens, flings) without a Swift port. Render's viewport members land in `crates/plugins/render/src/ffi.rs`, including a `RenderViewDemandHandle` for the tile plane. The reader in `EmbedPDFUI` is a `@MainActor @Observable` model over both capabilities and a SwiftUI view that positions each visible page where the stage says.

**Tech Stack:** Rust 2024, `uniffi =0.32.0`, Swift 6.2 tools, Swift Testing, `swift-custom-dump` 1.3, SwiftUI (iOS 17), Xcode 27.0, iPhone 17 simulator (iOS 26.0).

**Spec:** `docs/superpowers/plans/2026-10-06-swift-plugins/00-umbrella.md` and `docs/superpowers/plans/2026-10-01-swift-package.md` (Global Constraints, Decisions, Appendix A §A.0, §A.1 render, §A.2 stage). Plan 01 (`01-interaction.md`) must be merged first: this plan reuses its `RegistrationHandle`, `HostError`, `reporter` helper in `epdf-viewer-host`, and `PDFViewer.reportedErrors`.

## Global Constraints

Everything in the umbrella's Global Constraints applies. The ones this plan meets first:

- `uniffi` is pinned at `=0.32.0`.
- UniFFI names are unique across the workspace. Every record this plan writes in `crates/plugins/stage/src/ffi.rs` starts with `Stage` or `Render`; the shared ones in `epdf-ffi/src/view.rs` are `Camera`, `ZoomMode`, `ZoomSpec`, `ViewAnchor`, `PageFrameInsets`, and `Stage`-prefixed alignment and layout enums.
- No `usize` crosses. Page indices become `u32`; a count that doesn't fit is an `invalid-state` error, not a clamp.
- No `serde_json::Value` crosses. The stage hands page registry rows back as JSON (the stage's `PageInfo` is a `Value`); the FFI decodes the members Swift needs into `epdf_ffi::PageInfo`, the record `ViewerHost::pages` returns, and errors on a row that doesn't decode.
- `epdf-ffi` can't depend on a plugin crate. Stage depends on interaction and actions, which depend on `epdf-ffi` behind their `uniffi` features, so a dependency the other way is a package cycle. `view.rs` therefore depends only on `epdf-stage-core` and `epdf-geometry`.
- Facades don't decide PDF semantics. The SwiftUI reader positions what the stage lays out and forwards gestures; it computes no layout of its own.

## Decisions

The stage's `Scheduler` port gets a Rust implementation on the kernel's timer (`epdf_kernel::rt::set_timeout(16.0, ...)`), the same fallback render's `RenderHost::default` uses for `request_frame`. A Swift `CADisplayLink` port would tick in step with the display, but every frame would then cross the boundary twice (the request and the callback), and plan 01's latency numbers are for one crossing. The kernel timer is good enough for motion that ends in a still frame. `StageConfig.frames: false` turns motion off (navigation jumps), which tests use.

`set_camera`, `pan_by`, `scroll_by` and `zoom_around` are `fire` methods: no reply, errors to the kernel's report sink. A gesture sends one per touch update, and waiting on each would cost a round trip per frame. `begin_gesture` and `end_gesture` reply, so a gesture's bracket is ordered with the camera reads around it.

Responsive rules cross as `StageBoxQuery` only. `RuleWhen::Predicate` is a Rust closure; a Swift predicate would be called on the kernel thread for every viewport change, and a query covers what a native reader needs (width and height bounds, orientation).

`provide_initial_view` takes a foreign `StageInitialViewProvider`. A reader that restores a saved position needs it, and it's called once per placement, so the crossing doesn't matter.

Render's `create_view_demand` returns a `RenderViewDemandHandle`, and its paint plan's tile sources cross without their image handles. Swift fetches a tile's pixels by key with `tile_image(page, key)`, which reads the resolved handle on the kernel thread and copies the pixels out, as `render_page` does.

## Review Focus

1. A viewport size of zero (a SwiftUI layout pass before the view has a frame). Expected: nothing is placed and nothing throws; placement happens on the first non-zero size. Pinned in Task 3 (`a_zero_viewport_places_nothing_until_it_has_a_size`).
2. A fling or tween in flight when the document closes. Expected: the frames stop, no camera write lands after the close, and nothing panics. Pinned in Task 3 (`motion_in_flight_at_close_stops`).
3. A page index past the last page (`go_to_page_index(99)` on a 3-page document). Expected: the stage clamps to the last page as the TypeScript does, and `current_page_index` reads 2. Pinned in Task 3 (`an_index_past_the_end_lands_on_the_last_page`).
4. Pans sent between `begin_gesture` and `end_gesture` from many threads at once. Expected: none is dropped or reported as an error, so the camera ends where the same pans sent from one thread put it. Pinned in Task 3 (`concurrent_pans_all_land`).
5. The reader's view resized mid-scroll (rotation). Expected: the stage keeps the page that was in view, and the model's visible pages update. Pinned in Task 9 (`resizingKeepsTheCurrentPage`).

## Cross-plugin tests

- `actions` (plan 15, optional): stage registers `goto` and `named` executors with actions when both are installed (`crates/plugins/stage/src/lib.rs` `register_executors`). Plan 15 owns a test that executing a `named` `NextPage` action moves the stage.
- `link` (plan 04): owns a test that activating a `goto` link reveals its page through the stage.
- `search` (already delivered, optional dependency on stage): this plan owns `revealing_the_active_hit_moves_the_stage` in Task 6, because search merged first.
- `interaction` (plan 01): stage's pan handler (`crates/plugins/stage/src/scroll_handler.rs`) registers with the hub when both are installed. This plan owns `a_pan_tool_drag_scrolls_the_stage` in Task 6.

## File Structure

```
crates/epdf-ffi/Cargo.toml              epdf-stage-core dependency
crates/epdf-ffi/src/view.rs             Camera, ZoomMode, ZoomSpec, ViewAnchor, PageFrameInsets, Stage* alignment and layout enums, StageScrollMetrics
crates/epdf-ffi/tests/view.rs
crates/plugins/stage/Cargo.toml         uniffi feature
crates/plugins/stage/src/lib.rs         scaffolding, ffi module
crates/plugins/stage/src/controller.rs  lifetime()
crates/plugins/stage/src/ffi.rs         StageHandle, records, frame scheduler, initial-view port
crates/plugins/stage/tests/ffi.rs
crates/plugins/render/src/ffi.rs        render_policy, render_source, source_key, conform_viewport, paint_settings, RenderViewDemandHandle
crates/plugins/render/tests/ffi.rs
crates/epdf-viewer-host/{Cargo.toml,src/api.rs,src/session.rs,src/lib.rs}
crates/epdf-viewer-host/tests/viewer_host.rs
platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Stage.swift
platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Render.swift          viewport members, RenderViewDemand
platforms/apple/EmbedPDF/Sources/EmbedPDFUI/StageReaderModel.swift
platforms/apple/EmbedPDF/Sources/EmbedPDFUI/StageReaderView.swift
platforms/apple/EmbedPDF/Tests/EmbedPDFTests/StageTests.swift
platforms/apple/EmbedPDF/Tests/EmbedPDFTests/RenderViewportTests.swift
platforms/apple/EmbedPDF/Tests/EmbedPDFUITests/StageReaderModelTests.swift
```

---

### Task 1: Shared view records in `epdf-ffi`

**Files:**
- Modify: `crates/epdf-ffi/Cargo.toml`, `crates/epdf-ffi/src/lib.rs`
- Create: `crates/epdf-ffi/src/view.rs`, `crates/epdf-ffi/tests/view.rs`

**Interfaces:**
- Consumes: `epdf_stage_core::{Camera, ZoomMode, ZoomSpec, Anchor, Align, AlignValue, Alignment, AlignmentValue, Direction, SpreadMode, SizingMode, ScrollMetrics}`, `epdf_geometry::PageFrame`.
- Consumes also: umbrella Task 0c's `epdf_ffi::mirror_enum!` (`mirror_enum!(Ffi <=> path::Rust { A, B, C })` writes `From` both ways for two fieldless enums with the same variant names) and `epdf_ffi::to_u32(n: usize, what: &str) -> Result<u32, epdf_kernel::Error>` (code `invalid-state` when it doesn't fit).
- Produces: records `Camera { x, y, zoom }`, `ViewAnchor { page_index: u32, fx, fy }`, `PageFrameInsets { top, right, bottom, left }`, `StageScrollMetrics { scroll_left, scroll_top, scroll_width, scroll_height, client_width, client_height, scrollable_x, scrollable_y }`, `StageAlignment { x: StageAlign, y: StageAlign }`, `StageAlignmentValue { x: StageAlignValue, y: StageAlignValue }`; enums `ZoomMode { Automatic, FitPage, FitWidth, FitAll }`, `ZoomSpec { Level { level }, PageWidth { page_width }, PageHeight { page_height }, Mode { mode } }`, `StageAlign { Start, Center, End }`, `StageAlignValue { Named { align }, Fraction { value } }`, `StageDirection { Ltr, Rtl }`, `StageSpreadMode { None, Odd, Even }`, `StageSizingMode { Intrinsic, Uniform }`. Conversions both ways, except `StageScrollMetrics` (out only) and `ViewAnchor` (`TryFrom<sc::Anchor>`, `From<ViewAnchor> for sc::Anchor`).

- [x] **Step 1: Create the worktree**

```bash
git worktree add .claude/worktrees/swift-stage -b research/swift-stage research/swift-plugins
cd .claude/worktrees/swift-stage
```

Fetch PDFium for `darwin-arm64`, `ios-arm64` and `ios-sim-arm64` as `platforms/apple/README.md` describes. Every later command runs from this directory.

- [x] **Step 2: Write the failing test**

`crates/epdf-ffi/tests/view.rs`:

```rust
use epdf_ffi::{
    Camera, PageFrameInsets, StageAlign, StageAlignValue, StageAlignmentValue, ViewAnchor,
    ZoomMode, ZoomSpec, to_u32,
};
use epdf_stage_core as sc;

#[test]
fn a_zoom_spec_round_trips_every_variant() {
    for spec in [
        ZoomSpec::Level { level: 1.5 },
        ZoomSpec::PageWidth { page_width: 0.9 },
        ZoomSpec::PageHeight { page_height: 1.1 },
        ZoomSpec::Mode { mode: ZoomMode::FitWidth },
    ] {
        let core: sc::ZoomSpec = spec.into();
        assert_eq!(ZoomSpec::from(core), spec);
    }
}

#[test]
fn a_camera_and_insets_round_trip() {
    let camera = Camera { x: 10.0, y: -4.0, zoom: 2.0 };
    assert_eq!(Camera::from(sc::Camera::from(camera)), camera);
    let insets = PageFrameInsets { top: 1.0, right: 2.0, bottom: 3.0, left: 4.0 };
    assert_eq!(PageFrameInsets::from(epdf_geometry::PageFrame::from(insets)), insets);
}

#[test]
fn an_alignment_value_keeps_named_stops_and_fractions() {
    let value = StageAlignmentValue {
        x: StageAlignValue::Named { align: StageAlign::Center },
        y: StageAlignValue::Fraction { value: 0.25 },
    };
    assert_eq!(StageAlignmentValue::from(sc::AlignmentValue::from(value)), value);
}

#[test]
fn an_anchor_index_that_does_not_fit_is_invalid_state() {
    let core = sc::Anchor { page_index: usize::MAX, fx: 0.0, fy: 0.0 };
    let error = ViewAnchor::try_from(core).unwrap_err();
    assert_eq!(error.code(), Some("invalid-state"));
    let ok = ViewAnchor::try_from(sc::Anchor { page_index: 3, fx: 0.5, fy: 0.25 }).unwrap();
    assert_eq!(ok, ViewAnchor { page_index: 3, fx: 0.5, fy: 0.25 });
    assert_eq!(to_u32(7, "a page index").unwrap(), 7);
}
```

Add `epdf-stage-core = { path = "../plugins/stage-core", default-features = false }` and `epdf-geometry` (already present) to `[dependencies]` in `crates/epdf-ffi/Cargo.toml` before running.

- [x] **Step 3: Run it to verify it fails**

Run: `cargo test -p epdf-ffi --test view`
Expected: FAIL to compile, "unresolved imports `epdf_ffi::Camera`, ...".

- [x] **Step 4: Write `view.rs`**

```rust
//! The view vocabulary from `epdf-stage-core` and `epdf-geometry` that more
//! than one plugin's exports use: the camera, the zoom intent, a
//! page-relative anchor, alignment, and the layout enums. The `From` impls
//! live here because neither side is a plugin type.

use epdf_stage_core as sc;

use crate::mirror_enum;

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct Camera {
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
}

impl From<sc::Camera> for Camera {
    fn from(c: sc::Camera) -> Self {
        Camera { x: c.x, y: c.y, zoom: c.zoom }
    }
}

impl From<Camera> for sc::Camera {
    fn from(c: Camera) -> Self {
        sc::Camera { x: c.x, y: c.y, zoom: c.zoom }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ZoomMode {
    Automatic,
    FitPage,
    FitWidth,
    /// Fit the whole scene.
    FitAll,
}

mirror_enum!(ZoomMode <=> sc::ZoomMode { Automatic, FitPage, FitWidth, FitAll });

/// The zoom intent: a fixed level, a fraction of the page's width or
/// height, or a fit mode.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Enum)]
pub enum ZoomSpec {
    Level { level: f64 },
    PageWidth { page_width: f64 },
    PageHeight { page_height: f64 },
    Mode { mode: ZoomMode },
}

impl From<sc::ZoomSpec> for ZoomSpec {
    fn from(z: sc::ZoomSpec) -> Self {
        match z {
            sc::ZoomSpec::Level { level } => ZoomSpec::Level { level },
            sc::ZoomSpec::PageWidth { page_width } => ZoomSpec::PageWidth { page_width },
            sc::ZoomSpec::PageHeight { page_height } => ZoomSpec::PageHeight { page_height },
            sc::ZoomSpec::Mode { mode } => ZoomSpec::Mode { mode: mode.into() },
        }
    }
}

impl From<ZoomSpec> for sc::ZoomSpec {
    fn from(z: ZoomSpec) -> Self {
        match z {
            ZoomSpec::Level { level } => sc::ZoomSpec::Level { level },
            ZoomSpec::PageWidth { page_width } => sc::ZoomSpec::PageWidth { page_width },
            ZoomSpec::PageHeight { page_height } => sc::ZoomSpec::PageHeight { page_height },
            ZoomSpec::Mode { mode } => sc::ZoomSpec::Mode { mode: mode.into() },
        }
    }
}

/// What I'm looking at: a page and a fraction across and down it.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct ViewAnchor {
    pub page_index: u32,
    pub fx: f64,
    pub fy: f64,
}

impl TryFrom<sc::Anchor> for ViewAnchor {
    type Error = epdf_kernel::Error;

    fn try_from(a: sc::Anchor) -> Result<Self, Self::Error> {
        Ok(ViewAnchor { page_index: crate::to_u32(a.page_index, "an anchor's page index")?, fx: a.fx, fy: a.fy })
    }
}

impl From<ViewAnchor> for sc::Anchor {
    fn from(a: ViewAnchor) -> Self {
        sc::Anchor { page_index: a.page_index as usize, fx: a.fx, fy: a.fy }
    }
}

/// Reserved chrome around each page, one thickness per side.
#[derive(Clone, Copy, Debug, Default, PartialEq, uniffi::Record)]
pub struct PageFrameInsets {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

impl From<epdf_geometry::PageFrame> for PageFrameInsets {
    fn from(f: epdf_geometry::PageFrame) -> Self {
        PageFrameInsets { top: f.top, right: f.right, bottom: f.bottom, left: f.left }
    }
}

impl From<PageFrameInsets> for epdf_geometry::PageFrame {
    fn from(f: PageFrameInsets) -> Self {
        epdf_geometry::PageFrame { top: f.top, right: f.right, bottom: f.bottom, left: f.left }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum StageAlign {
    Start,
    Center,
    End,
}

mirror_enum!(StageAlign <=> sc::Align { Start, Center, End });

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct StageAlignment {
    pub x: StageAlign,
    pub y: StageAlign,
}

impl From<sc::Alignment> for StageAlignment {
    fn from(a: sc::Alignment) -> Self {
        StageAlignment { x: a.x.into(), y: a.y.into() }
    }
}

impl From<StageAlignment> for sc::Alignment {
    fn from(a: StageAlignment) -> Self {
        sc::Alignment { x: a.x.into(), y: a.y.into() }
    }
}

/// A named stop, or a viewport fraction for the subject's center.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Enum)]
pub enum StageAlignValue {
    Named { align: StageAlign },
    Fraction { value: f64 },
}

impl From<sc::AlignValue> for StageAlignValue {
    fn from(v: sc::AlignValue) -> Self {
        match v {
            sc::AlignValue::Named(align) => StageAlignValue::Named { align: align.into() },
            sc::AlignValue::Fraction(value) => StageAlignValue::Fraction { value },
        }
    }
}

impl From<StageAlignValue> for sc::AlignValue {
    fn from(v: StageAlignValue) -> Self {
        match v {
            StageAlignValue::Named { align } => sc::AlignValue::Named(align.into()),
            StageAlignValue::Fraction { value } => sc::AlignValue::Fraction(value),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct StageAlignmentValue {
    pub x: StageAlignValue,
    pub y: StageAlignValue,
}

impl From<sc::AlignmentValue> for StageAlignmentValue {
    fn from(a: sc::AlignmentValue) -> Self {
        StageAlignmentValue { x: a.x.into(), y: a.y.into() }
    }
}

impl From<StageAlignmentValue> for sc::AlignmentValue {
    fn from(a: StageAlignmentValue) -> Self {
        sc::AlignmentValue { x: a.x.into(), y: a.y.into() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum StageDirection {
    Ltr,
    Rtl,
}

mirror_enum!(StageDirection <=> sc::Direction { Ltr, Rtl });

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum StageSpreadMode {
    None,
    Odd,
    Even,
}

mirror_enum!(StageSpreadMode <=> sc::SpreadMode { None, Odd, Even });

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum StageSizingMode {
    Intrinsic,
    Uniform,
}

mirror_enum!(StageSizingMode <=> sc::SizingMode { Intrinsic, Uniform });

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct StageScrollMetrics {
    pub scroll_left: f64,
    pub scroll_top: f64,
    pub scroll_width: f64,
    pub scroll_height: f64,
    pub client_width: f64,
    pub client_height: f64,
    pub scrollable_x: bool,
    pub scrollable_y: bool,
}

impl From<sc::ScrollMetrics> for StageScrollMetrics {
    fn from(m: sc::ScrollMetrics) -> Self {
        StageScrollMetrics {
            scroll_left: m.scroll_left,
            scroll_top: m.scroll_top,
            scroll_width: m.scroll_width,
            scroll_height: m.scroll_height,
            client_width: m.client_width,
            client_height: m.client_height,
            scrollable_x: m.scrollable_x,
            scrollable_y: m.scrollable_y,
        }
    }
}
```

- [x] **Step 5: Export them from `lib.rs`**

```rust
pub mod view;

pub use view::{
    Camera, PageFrameInsets, StageAlign, StageAlignValue, StageAlignment, StageAlignmentValue,
    StageDirection, StageScrollMetrics, StageSizingMode, StageSpreadMode, ViewAnchor, ZoomMode,
    ZoomSpec,
};
```

- [x] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p epdf-ffi`
Expected: PASS, the four `view` tests and every existing one.

- [x] **Step 7: Commit**

```bash
git add crates/epdf-ffi
git commit -m "Add the shared view records to epdf-ffi"
```

### Task 2: Stage records and conversions

**Files:**
- Modify: `crates/plugins/stage/Cargo.toml`, `crates/plugins/stage/src/lib.rs`, `crates/plugins/stage/src/controller.rs`
- Create: `crates/plugins/stage/src/ffi.rs`, `crates/plugins/stage/tests/ffi.rs`

**Interfaces:**
- Consumes: Task 1's records and helpers; `epdf_plugin_stage::{StageSettings, PartialSettings, FlowMode, LayoutKind, ScrollBehaviorKind, GridColumns, ColumnsPolicy, Gap, ArrivalAlignValue, ArrivalAlignment, PartialArrivalAlignment, VisiblePage, Viewpoint, StageViewState, StageScrollToOptions, GoToOptions, RevealWord, RevealAnchorValue, RevealAnchor, RevealZoom, RevealZoomWord, RevealOptions, PageHit, StageZoomMode, ZoomTarget, ZoomToOptions, BoxQuery, Orientation, ResponsiveRule, RuleWhen}` and `settings::SettingKey` (`crates/plugins/stage/src/contract.rs`, `settings.rs`).
- Produces (all in `epdf_plugin_stage::ffi`): enums `StageFlow`, `StageLayout`, `StageScrollBehavior`, `StageGridColumns { Square, Auto, Count { count: u32 } }`, `StageGap { World { units }, Px { px } }`, `StageArrivalAlignValue { Keep, Align { value: StageAlignValue } }`, `StageSettingKey`, `StageOrientation`, `StageRevealWord`, `StageRevealAnchorValue { Word { word }, Fraction { value } }`, `StageRevealZoom { Keep, Fit, FitWidth, FitHeight, Level { level } }`, `StageZoomMode { Mode { mode: ZoomMode }, Custom }`, `StageZoomTarget { Level { level }, Spec { spec: ZoomSpec } }`; records `StageArrivalAlignment`, `StagePartialArrivalAlignment`, `StageSettings`, `StageSettingsPatch`, `StageBoxQuery`, `StageResponsiveRule { name, when: StageBoxQuery, settings: Option<StageSettingsPatch> }`, `StagePageTransform`, `StageVisiblePage`, `StageViewpoint`, `StageViewState`, `StageScrollToOptions`, `StageGoToOptions`, `StageRevealAnchor`, `StageRevealOptions`, `StagePageHit`, `StageConfig { settings: Option<StageSettingsPatch>, responsive: Option<Vec<StageResponsiveRule>>, frames: bool }`. Link (plan 04) consumes `StageRevealOptions`.

- [x] **Step 1: Add the feature and `lifetime()`**

In `crates/plugins/stage/Cargo.toml`:

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

Add `[[test]] name = "ffi" required-features = ["uniffi"]` beside the existing `[[test]]` entries, because this crate lists its tests explicitly.

In `crates/plugins/stage/src/lib.rs`, beside the other modules:

```rust
#[cfg(feature = "uniffi")]
pub mod ffi;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();
```

In `crates/plugins/stage/src/controller.rs`, in `impl StageCapability` (after `same`):

```rust
    /// The instance lifetime, aborted when the document closes. A foreign
    /// handle outlives the close, checks this before every call, and ends
    /// its event streams when it aborts.
    #[cfg(feature = "uniffi")]
    #[doc(hidden)]
    pub fn lifetime(&self) -> &epdf_kernel::signal::AbortSignal {
        self.ctx.lifetime()
    }
```

- [x] **Step 2: Write the failing conversion tests**

`crates/plugins/stage/tests/ffi.rs` (Task 3 adds the handle tests to this file):

```rust
#![cfg(feature = "uniffi")]

use epdf_ffi::{StageAlign, StageAlignValue, ZoomMode, ZoomSpec};
use epdf_plugin_stage::ffi::{
    StageArrivalAlignValue, StageGap, StageGridColumns, StageRevealOptions, StageRevealZoom,
    StageSettings, StageSettingsPatch, StageZoomTarget,
};
use epdf_plugin_stage::{PartialSettings, RevealOptions, ZoomTarget, default_settings};

#[test]
fn the_default_settings_round_trip() {
    let settings = default_settings();
    let ffi = StageSettings::try_from(settings.clone()).unwrap();
    assert_eq!(epdf_plugin_stage::StageSettings::try_from(ffi).unwrap(), settings);
}

#[test]
fn a_patch_sets_only_what_it_names() {
    let patch = StageSettingsPatch {
        padding: Some(8.0),
        gap: Some(StageGap::Px { px: 4.0 }),
        columns: Some(StageGridColumns::Count { count: 3 }),
        zoom: Some(ZoomSpec::Mode { mode: ZoomMode::FitPage }),
        arrival_align: None,
        ..Default::default()
    };
    let partial = PartialSettings::try_from(patch).unwrap();
    assert_eq!(partial.padding, Some(8.0));
    assert_eq!(partial.gap, Some(epdf_plugin_stage::Gap::Px { px: 4.0 }));
    assert_eq!(partial.columns, Some(epdf_plugin_stage::GridColumns::Count(3.0)));
    assert!(partial.flow.is_none() && partial.arrival_align.is_none());
}

#[test]
fn a_fractional_column_count_is_invalid_state_not_a_round() {
    let mut settings = default_settings();
    settings.columns = epdf_plugin_stage::GridColumns::Count(2.5);
    let error = StageSettings::try_from(settings).unwrap_err();
    assert_eq!(error.code(), Some("invalid-state"));
}

#[test]
fn keep_survives_the_arrival_alignment() {
    let keep = StageArrivalAlignValue::Keep;
    let core: epdf_plugin_stage::ArrivalAlignValue = keep.into();
    assert_eq!(core, epdf_plugin_stage::ArrivalAlignValue::KEEP);
    let named = StageArrivalAlignValue::Align { value: StageAlignValue::Named { align: StageAlign::End } };
    assert_eq!(StageArrivalAlignValue::from(epdf_plugin_stage::ArrivalAlignValue::from(named)), named);
}

#[test]
fn reveal_options_and_zoom_targets_convert() {
    let options = StageRevealOptions {
        behavior: None,
        rect: Some(epdf_ffi::Rect { x: 1.0, y: 2.0, width: 3.0, height: 4.0 }),
        zoom: Some(StageRevealZoom::Level { level: 2.0 }),
        anchor: None,
    };
    let core = RevealOptions::from(options.clone());
    assert_eq!(core.zoom, Some(epdf_plugin_stage::RevealZoom::Level { level: 2.0 }));
    assert_eq!(StageRevealOptions::from(core), options);
    assert_eq!(ZoomTarget::from(StageZoomTarget::Level { level: 1.25 }), ZoomTarget::Level(1.25));
}
```

- [x] **Step 3: Run them to verify they fail**

Run: `cargo test -p epdf-plugin-stage --features uniffi --test ffi`
Expected: FAIL to compile, "unresolved import `epdf_plugin_stage::ffi`".

- [x] **Step 4: Write the records half of `ffi.rs`**

```rust
//! The stage plugin through uniffi: the viewport lens as one handle.
//!
//! Stage-core types shared with other plugins (the camera, the zoom spec,
//! the anchor, alignment) are `epdf-ffi`'s. Stage's own types are mirrored
//! here with a `Stage` prefix: most carry an `epdf-geometry` or
//! `epdf-stage-core` field, a `usize` index, or a JSON page row, none of
//! which uniffi can carry. A value that does not fit its record (a column
//! count that is not whole, an index past `u32`, a page row without its
//! size) is an `invalid-state` error, never a guess.
//!
//! Every method hops to the kernel thread. `set_camera`, `pan_by`,
//! `scroll_by` and `zoom_around` fire without a reply; their errors go to
//! the kernel's report sink.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use epdf_ffi::{
    Camera, HostError, KernelBound, KernelPost, PageFrameInsets, PageInfo, PageRef,
    Point, Rect, RegistrationHandle, Size, StageAlignValue, StageAlignment, StageAlignmentValue,
    StageDirection, StageScrollMetrics, StageSizingMode, StageSpreadMode, Subscription,
    ViewAnchor, ViewerError, ZoomMode, ZoomSpec, mirror_enum, to_u32,
};
use epdf_geometry::PageRotation;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::{Error, Unsubscribe, rt};
use serde::Deserialize;
use serde_json::Value;

use crate::settings::SettingKey;
use crate::{
    ArrivalAlignValue, ArrivalAlignment, BoxQuery, ColumnsPolicy, FlowMode, Gap, GestureOptions,
    GoToOptions, GridColumns, Keep, LayoutKind, Orientation, PageHit, PartialArrivalAlignment,
    PartialSettings, ResponsiveRule, RevealAnchor, RevealAnchorValue, RevealOptions, RevealWord,
    RevealZoom, RevealZoomWord, RuleWhen, Scheduler, ScrollBehaviorKind, StageCapability,
    StageZoomMode as CoreZoomMode, Viewpoint, VisiblePage, ZoomTarget, ZoomToOptions,
};

fn invalid(what: String) -> Error {
    Error::named("InvalidState", format!("[stage] {what}")).with("code", "invalid-state")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum StageFlow {
    Continuous,
    Paged,
}
mirror_enum!(StageFlow <=> FlowMode { Continuous, Paged });

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum StageLayout {
    Vertical,
    Horizontal,
    Grid,
}
mirror_enum!(StageLayout <=> LayoutKind { Vertical, Horizontal, Grid });

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum StageScrollBehavior {
    Smooth,
    Instant,
}
mirror_enum!(StageScrollBehavior <=> ScrollBehaviorKind { Smooth, Instant });

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum StageOrientation {
    Portrait,
    Landscape,
}
mirror_enum!(StageOrientation <=> Orientation { Portrait, Landscape });

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum StageRevealWord {
    Keep,
    Start,
    Center,
    End,
}
mirror_enum!(StageRevealWord <=> RevealWord { Keep, Start, Center, End });

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum StageSettingKey {
    Flow,
    Layout,
    Spread,
    Sizing,
    Columns,
    Bounded,
    Padding,
    Gap,
    PageFrame,
    Direction,
    FitAlign,
    ArrivalAlign,
    ZoomAlign,
    AnchorAlign,
    ViewRotation,
    Zoom,
    ScrollBehavior,
    ViewUnitsPerPoint,
}
mirror_enum!(StageSettingKey <=> SettingKey {
    Flow, Layout, Spread, Sizing, Columns, Bounded, Padding, Gap, PageFrame, Direction,
    FitAlign, ArrivalAlign, ZoomAlign, AnchorAlign, ViewRotation, Zoom, ScrollBehavior,
    ViewUnitsPerPoint,
});

/// Grid columns: about the square root of the item count, as many as fit,
/// or a fixed count.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum StageGridColumns {
    Square,
    Auto,
    Count { count: u32 },
}

impl TryFrom<GridColumns> for StageGridColumns {
    type Error = Error;
    fn try_from(c: GridColumns) -> Result<Self, Error> {
        Ok(match c {
            GridColumns::Policy(ColumnsPolicy::Square) => StageGridColumns::Square,
            GridColumns::Policy(ColumnsPolicy::Auto) => StageGridColumns::Auto,
            GridColumns::Count(n) if n >= 0.0 && n.fract() == 0.0 && n <= f64::from(u32::MAX) => {
                StageGridColumns::Count { count: n as u32 }
            }
            GridColumns::Count(n) => return Err(invalid(format!("a column count of {n}"))),
        })
    }
}

impl From<StageGridColumns> for GridColumns {
    fn from(c: StageGridColumns) -> Self {
        match c {
            StageGridColumns::Square => GridColumns::Policy(ColumnsPolicy::Square),
            StageGridColumns::Auto => GridColumns::Policy(ColumnsPolicy::Auto),
            StageGridColumns::Count { count } => GridColumns::Count(f64::from(count)),
        }
    }
}

/// Space between items: world units (scale with zoom) or screen points.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Enum)]
pub enum StageGap {
    World { units: f64 },
    Px { px: f64 },
}

impl From<Gap> for StageGap {
    fn from(g: Gap) -> Self {
        match g {
            Gap::World(units) => StageGap::World { units },
            Gap::Px { px } => StageGap::Px { px },
        }
    }
}

impl From<StageGap> for Gap {
    fn from(g: StageGap) -> Self {
        match g {
            StageGap::World { units } => Gap::World(units),
            StageGap::Px { px } => Gap::Px { px },
        }
    }
}

/// One axis of the arrival policy: an alignment, or keep this axis still.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Enum)]
pub enum StageArrivalAlignValue {
    Keep,
    Align { value: StageAlignValue },
}

impl From<ArrivalAlignValue> for StageArrivalAlignValue {
    fn from(v: ArrivalAlignValue) -> Self {
        match v {
            ArrivalAlignValue::Keep(Keep::Keep) => StageArrivalAlignValue::Keep,
            ArrivalAlignValue::Align(value) => StageArrivalAlignValue::Align { value: value.into() },
        }
    }
}

impl From<StageArrivalAlignValue> for ArrivalAlignValue {
    fn from(v: StageArrivalAlignValue) -> Self {
        match v {
            StageArrivalAlignValue::Keep => ArrivalAlignValue::KEEP,
            StageArrivalAlignValue::Align { value } => ArrivalAlignValue::Align(value.into()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct StageArrivalAlignment {
    pub x: StageArrivalAlignValue,
    pub y: StageArrivalAlignValue,
}

impl From<ArrivalAlignment> for StageArrivalAlignment {
    fn from(a: ArrivalAlignment) -> Self {
        StageArrivalAlignment { x: a.x.into(), y: a.y.into() }
    }
}

impl From<StageArrivalAlignment> for ArrivalAlignment {
    fn from(a: StageArrivalAlignment) -> Self {
        ArrivalAlignment { x: a.x.into(), y: a.y.into() }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, uniffi::Record)]
pub struct StagePartialArrivalAlignment {
    #[uniffi(default = None)]
    pub x: Option<StageArrivalAlignValue>,
    #[uniffi(default = None)]
    pub y: Option<StageArrivalAlignValue>,
}

impl From<StagePartialArrivalAlignment> for PartialArrivalAlignment {
    fn from(a: StagePartialArrivalAlignment) -> Self {
        PartialArrivalAlignment { x: a.x.map(Into::into), y: a.y.map(Into::into) }
    }
}

/// Every stage setting. `view_rotation` is degrees clockwise.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct StageSettings {
    pub flow: StageFlow,
    pub layout: StageLayout,
    pub spread: StageSpreadMode,
    pub sizing: StageSizingMode,
    pub columns: StageGridColumns,
    pub bounded: bool,
    pub padding: f64,
    pub gap: StageGap,
    pub page_frame: PageFrameInsets,
    pub direction: StageDirection,
    pub fit_align: StageAlignment,
    pub arrival_align: StageArrivalAlignment,
    pub zoom_align: StageAlignmentValue,
    pub anchor_align: StageAlignmentValue,
    pub view_rotation: f64,
    pub zoom: ZoomSpec,
    pub scroll_behavior: StageScrollBehavior,
    pub view_units_per_point: f64,
}

impl TryFrom<crate::StageSettings> for StageSettings {
    type Error = Error;
    fn try_from(s: crate::StageSettings) -> Result<Self, Error> {
        Ok(StageSettings {
            flow: s.flow.into(),
            layout: s.layout.into(),
            spread: s.spread.into(),
            sizing: s.sizing.into(),
            columns: s.columns.try_into()?,
            bounded: s.bounded,
            padding: s.padding,
            gap: s.gap.into(),
            page_frame: s.page_frame.into(),
            direction: s.direction.into(),
            fit_align: s.fit_align.into(),
            arrival_align: s.arrival_align.into(),
            zoom_align: s.zoom_align.into(),
            anchor_align: s.anchor_align.into(),
            view_rotation: s.view_rotation.0,
            zoom: s.zoom.into(),
            scroll_behavior: s.scroll_behavior.into(),
            view_units_per_point: s.view_units_per_point,
        })
    }
}

impl TryFrom<StageSettings> for crate::StageSettings {
    type Error = Error;
    fn try_from(s: StageSettings) -> Result<Self, Error> {
        Ok(crate::StageSettings {
            flow: s.flow.into(),
            layout: s.layout.into(),
            spread: s.spread.into(),
            sizing: s.sizing.into(),
            columns: s.columns.into(),
            bounded: s.bounded,
            padding: s.padding,
            gap: s.gap.into(),
            page_frame: s.page_frame.into(),
            direction: s.direction.into(),
            fit_align: s.fit_align.into(),
            arrival_align: s.arrival_align.into(),
            zoom_align: s.zoom_align.into(),
            anchor_align: s.anchor_align.into(),
            view_rotation: PageRotation(s.view_rotation),
            zoom: s.zoom.into(),
            scroll_behavior: s.scroll_behavior.into(),
            view_units_per_point: s.view_units_per_point,
        })
    }
}

/// Some settings: `None` leaves a key alone.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct StageSettingsPatch {
    #[uniffi(default = None)]
    pub flow: Option<StageFlow>,
    #[uniffi(default = None)]
    pub layout: Option<StageLayout>,
    #[uniffi(default = None)]
    pub spread: Option<StageSpreadMode>,
    #[uniffi(default = None)]
    pub sizing: Option<StageSizingMode>,
    #[uniffi(default = None)]
    pub columns: Option<StageGridColumns>,
    #[uniffi(default = None)]
    pub bounded: Option<bool>,
    #[uniffi(default = None)]
    pub padding: Option<f64>,
    #[uniffi(default = None)]
    pub gap: Option<StageGap>,
    #[uniffi(default = None)]
    pub page_frame: Option<PageFrameInsets>,
    #[uniffi(default = None)]
    pub direction: Option<StageDirection>,
    #[uniffi(default = None)]
    pub fit_align: Option<StageAlignment>,
    #[uniffi(default = None)]
    pub arrival_align: Option<StageArrivalAlignment>,
    #[uniffi(default = None)]
    pub zoom_align: Option<StageAlignmentValue>,
    #[uniffi(default = None)]
    pub anchor_align: Option<StageAlignmentValue>,
    #[uniffi(default = None)]
    pub view_rotation: Option<f64>,
    #[uniffi(default = None)]
    pub zoom: Option<ZoomSpec>,
    #[uniffi(default = None)]
    pub scroll_behavior: Option<StageScrollBehavior>,
    #[uniffi(default = None)]
    pub view_units_per_point: Option<f64>,
}

impl TryFrom<StageSettingsPatch> for PartialSettings {
    type Error = Error;
    fn try_from(p: StageSettingsPatch) -> Result<Self, Error> {
        Ok(PartialSettings {
            flow: p.flow.map(Into::into),
            layout: p.layout.map(Into::into),
            spread: p.spread.map(Into::into),
            sizing: p.sizing.map(Into::into),
            columns: p.columns.map(Into::into),
            bounded: p.bounded,
            padding: p.padding,
            gap: p.gap.map(Into::into),
            page_frame: p.page_frame.map(Into::into),
            direction: p.direction.map(Into::into),
            fit_align: p.fit_align.map(Into::into),
            arrival_align: p.arrival_align.map(Into::into),
            zoom_align: p.zoom_align.map(Into::into),
            anchor_align: p.anchor_align.map(Into::into),
            view_rotation: p.view_rotation.map(PageRotation),
            zoom: p.zoom.map(Into::into),
            scroll_behavior: p.scroll_behavior.map(Into::into),
            view_units_per_point: p.view_units_per_point,
        })
    }
}

/// A container query: bounds inclusive, every field optional, all AND-ed.
#[derive(Clone, Copy, Debug, Default, PartialEq, uniffi::Record)]
pub struct StageBoxQuery {
    #[uniffi(default = None)]
    pub min_width: Option<f64>,
    #[uniffi(default = None)]
    pub max_width: Option<f64>,
    #[uniffi(default = None)]
    pub min_height: Option<f64>,
    #[uniffi(default = None)]
    pub max_height: Option<f64>,
    #[uniffi(default = None)]
    pub orientation: Option<StageOrientation>,
}

impl From<StageBoxQuery> for BoxQuery {
    fn from(q: StageBoxQuery) -> Self {
        BoxQuery {
            min_width: q.min_width,
            max_width: q.max_width,
            min_height: q.min_height,
            max_height: q.max_height,
            orientation: q.orientation.map(Into::into),
        }
    }
}

/// When the viewport matches `when`, assert `settings`. Later rules win
/// per key. A rule without settings is a named query (`matches_rule`).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct StageResponsiveRule {
    #[uniffi(default = None)]
    pub name: Option<String>,
    pub when: StageBoxQuery,
    #[uniffi(default = None)]
    pub settings: Option<StageSettingsPatch>,
}

impl TryFrom<StageResponsiveRule> for ResponsiveRule {
    type Error = Error;
    fn try_from(r: StageResponsiveRule) -> Result<Self, Error> {
        Ok(ResponsiveRule {
            name: r.name,
            when: RuleWhen::Query(r.when.into()),
            settings: r.settings.map(PartialSettings::try_from).transpose()?,
        })
    }
}

/// A page's content, view and device sizes and scales, in points.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct StagePageTransform {
    pub rotation: f64,
    pub view_width: f64,
    pub view_height: f64,
    pub content_width: f64,
    pub content_height: f64,
    pub device_width: f64,
    pub device_height: f64,
    /// Device pixels per PDF point: the scale to render this page at.
    pub render_scale: f64,
    pub view_scale: f64,
    pub base_scale: f64,
    pub zoom: f64,
}

impl From<&epdf_geometry::PageTransform> for StagePageTransform {
    fn from(t: &epdf_geometry::PageTransform) -> Self {
        StagePageTransform {
            rotation: t.rotation.0,
            view_width: t.view_width,
            view_height: t.view_height,
            content_width: t.content_width,
            content_height: t.content_height,
            device_width: t.device_width,
            device_height: t.device_height,
            render_scale: t.render_scale,
            view_scale: t.view_scale,
            base_scale: t.base_scale,
            zoom: t.zoom,
        }
    }
}

/// A laid-out page: where it sits on screen and how it is scaled.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct StageVisiblePage {
    pub page: PageRef,
    pub page_index: u32,
    /// World-space layout box.
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub rotation: f64,
    pub content_scale: f64,
    /// Device-snapped screen origin, in viewport points.
    pub screen_x: f64,
    pub screen_y: f64,
    pub transform: StagePageTransform,
    /// The on-screen region in un-rotated page points.
    pub visible_rect: Rect,
}

impl TryFrom<&VisiblePage> for StageVisiblePage {
    type Error = Error;
    fn try_from(v: &VisiblePage) -> Result<Self, Error> {
        Ok(StageVisiblePage {
            page: v.page_ref.into(),
            page_index: to_u32(v.page.page_index, "a visible page's index")?,
            x: v.page.x,
            y: v.page.y,
            width: v.page.width,
            height: v.page.height,
            rotation: v.page.rotation.0,
            content_scale: v.page.content_scale,
            screen_x: v.screen_x,
            screen_y: v.screen_y,
            transform: (&v.transform).into(),
            visible_rect: v.visible_rect.into(),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct StageViewpoint {
    pub anchor: ViewAnchor,
    pub zoom: ZoomSpec,
}

impl TryFrom<Viewpoint> for StageViewpoint {
    type Error = Error;
    fn try_from(v: Viewpoint) -> Result<Self, Error> {
        Ok(StageViewpoint { anchor: v.anchor.try_into()?, zoom: v.zoom.into() })
    }
}

impl From<StageViewpoint> for Viewpoint {
    fn from(v: StageViewpoint) -> Self {
        Viewpoint { anchor: v.anchor.into(), zoom: v.zoom.into() }
    }
}

/// Durable view state: settings plus where you are. Save it, then
/// `apply_view_state` or an initial-view provider restores it.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct StageViewState {
    pub settings: StageSettings,
    /// `None` restores page 0.
    pub cursor: Option<u32>,
    pub anchor: ViewAnchor,
}

impl TryFrom<StageViewState> for crate::StageViewState {
    type Error = Error;
    fn try_from(v: StageViewState) -> Result<Self, Error> {
        Ok(crate::StageViewState {
            settings: v.settings.try_into()?,
            cursor: v.cursor.map(|c| c as usize),
            anchor: v.anchor.into(),
        })
    }
}

impl TryFrom<crate::StageViewState> for StageViewState {
    type Error = Error;
    fn try_from(v: crate::StageViewState) -> Result<Self, Error> {
        Ok(StageViewState {
            settings: v.settings.try_into()?,
            cursor: v.cursor.map(|c| to_u32(c, "a view state's cursor")).transpose()?,
            anchor: v.anchor.try_into()?,
        })
    }
}

/// Absolute offsets into the scroll range; an omitted axis stays.
#[derive(Clone, Copy, Debug, Default, PartialEq, uniffi::Record)]
pub struct StageScrollToOptions {
    #[uniffi(default = None)]
    pub left: Option<f64>,
    #[uniffi(default = None)]
    pub top: Option<f64>,
    #[uniffi(default = None)]
    pub behavior: Option<StageScrollBehavior>,
}

impl From<StageScrollToOptions> for crate::StageScrollToOptions {
    fn from(o: StageScrollToOptions) -> Self {
        crate::StageScrollToOptions { left: o.left, top: o.top, behavior: o.behavior.map(Into::into) }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, uniffi::Record)]
pub struct StageGoToOptions {
    #[uniffi(default = None)]
    pub behavior: Option<StageScrollBehavior>,
    /// Restore this viewpoint instead of a fresh placement.
    #[uniffi(default = None)]
    pub viewpoint: Option<StageViewpoint>,
    #[uniffi(default = None)]
    pub arrival_align: Option<StagePartialArrivalAlignment>,
}

impl From<StageGoToOptions> for GoToOptions {
    fn from(o: StageGoToOptions) -> Self {
        GoToOptions {
            behavior: o.behavior.map(Into::into),
            viewpoint: o.viewpoint.map(Into::into),
            arrival_align: o.arrival_align.map(Into::into),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Enum)]
pub enum StageRevealAnchorValue {
    Word { word: StageRevealWord },
    Fraction { value: f64 },
}

impl From<RevealAnchorValue> for StageRevealAnchorValue {
    fn from(v: RevealAnchorValue) -> Self {
        match v {
            RevealAnchorValue::Word(word) => StageRevealAnchorValue::Word { word: word.into() },
            RevealAnchorValue::Fraction(value) => StageRevealAnchorValue::Fraction { value },
        }
    }
}

impl From<StageRevealAnchorValue> for RevealAnchorValue {
    fn from(v: StageRevealAnchorValue) -> Self {
        match v {
            StageRevealAnchorValue::Word { word } => RevealAnchorValue::Word(word.into()),
            StageRevealAnchorValue::Fraction { value } => RevealAnchorValue::Fraction(value),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, uniffi::Record)]
pub struct StageRevealAnchor {
    #[uniffi(default = None)]
    pub x: Option<StageRevealAnchorValue>,
    #[uniffi(default = None)]
    pub y: Option<StageRevealAnchorValue>,
}

impl From<RevealAnchor> for StageRevealAnchor {
    fn from(a: RevealAnchor) -> Self {
        StageRevealAnchor { x: a.x.map(Into::into), y: a.y.map(Into::into) }
    }
}

impl From<StageRevealAnchor> for RevealAnchor {
    fn from(a: StageRevealAnchor) -> Self {
        RevealAnchor { x: a.x.map(Into::into), y: a.y.map(Into::into) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Enum)]
pub enum StageRevealZoom {
    Keep,
    Fit,
    FitWidth,
    FitHeight,
    Level { level: f64 },
}

impl From<RevealZoom> for StageRevealZoom {
    fn from(z: RevealZoom) -> Self {
        match z {
            RevealZoom::Word(RevealZoomWord::Keep) => StageRevealZoom::Keep,
            RevealZoom::Word(RevealZoomWord::Fit) => StageRevealZoom::Fit,
            RevealZoom::Word(RevealZoomWord::FitWidth) => StageRevealZoom::FitWidth,
            RevealZoom::Word(RevealZoomWord::FitHeight) => StageRevealZoom::FitHeight,
            RevealZoom::Level { level } => StageRevealZoom::Level { level },
        }
    }
}

impl From<StageRevealZoom> for RevealZoom {
    fn from(z: StageRevealZoom) -> Self {
        match z {
            StageRevealZoom::Keep => RevealZoom::Word(RevealZoomWord::Keep),
            StageRevealZoom::Fit => RevealZoom::Word(RevealZoomWord::Fit),
            StageRevealZoom::FitWidth => RevealZoom::Word(RevealZoomWord::FitWidth),
            StageRevealZoom::FitHeight => RevealZoom::Word(RevealZoomWord::FitHeight),
            StageRevealZoom::Level { level } => RevealZoom::Level { level },
        }
    }
}

/// A reveal. With none of `rect`, `zoom` and `anchor` set it is the
/// minimal movement that brings the page into view.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct StageRevealOptions {
    #[uniffi(default = None)]
    pub behavior: Option<StageScrollBehavior>,
    /// Page space; `None` is the whole page.
    #[uniffi(default = None)]
    pub rect: Option<Rect>,
    #[uniffi(default = None)]
    pub zoom: Option<StageRevealZoom>,
    #[uniffi(default = None)]
    pub anchor: Option<StageRevealAnchor>,
}

impl From<RevealOptions> for StageRevealOptions {
    fn from(o: RevealOptions) -> Self {
        StageRevealOptions {
            behavior: o.behavior.map(Into::into),
            rect: o.rect.map(Into::into),
            zoom: o.zoom.map(Into::into),
            anchor: o.anchor.map(Into::into),
        }
    }
}

impl From<StageRevealOptions> for RevealOptions {
    fn from(o: StageRevealOptions) -> Self {
        RevealOptions {
            behavior: o.behavior.map(Into::into),
            rect: o.rect.map(Into::into),
            zoom: o.zoom.map(Into::into),
            anchor: o.anchor.map(Into::into),
        }
    }
}

/// The page under a viewport point and its page-space point.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct StagePageHit {
    pub page: PageRef,
    pub point: Point,
    pub scale: f64,
    pub rotation: f64,
    pub zoom: f64,
}

impl From<PageHit> for StagePageHit {
    fn from(h: PageHit) -> Self {
        StagePageHit {
            page: h.page_ref.into(),
            point: h.point.into(),
            scale: h.scale,
            rotation: h.rotation.0,
            zoom: h.zoom,
        }
    }
}

/// The active zoom intent's mode; `Custom` for a level or a page fraction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum StageZoomMode {
    Mode { mode: ZoomMode },
    Custom,
}

impl From<CoreZoomMode> for StageZoomMode {
    fn from(m: CoreZoomMode) -> Self {
        match m {
            CoreZoomMode::Mode(mode) => StageZoomMode::Mode { mode: mode.into() },
            CoreZoomMode::Custom => StageZoomMode::Custom,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Enum)]
pub enum StageZoomTarget {
    Level { level: f64 },
    Spec { spec: ZoomSpec },
}

impl From<StageZoomTarget> for ZoomTarget {
    fn from(t: StageZoomTarget) -> Self {
        match t {
            StageZoomTarget::Level { level } => ZoomTarget::Level(level),
            StageZoomTarget::Spec { spec } => ZoomTarget::Spec(spec.into()),
        }
    }
}

// One row of the page registry crosses as `epdf_ffi::PageInfo` (umbrella
// Task 0d), the record `ViewerHost::pages` already returns, so Swift has one
// page type.

#[derive(Deserialize)]
struct Row {
    index: u32,
    #[serde(rename = "ref")]
    page: RowRef,
    size: RowSize,
    rotation: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RowRef {
    page_object_number: u32,
}

#[derive(Deserialize)]
struct RowSize {
    width: f64,
    height: f64,
}

fn page_info(row: &Value) -> Result<PageInfo, Error> {
    let decoded: Row = serde_json::from_value(row.clone())
        .map_err(|e| invalid(format!("a page row that does not decode ({e}): {row}")))?;
    let rotation = decoded.rotation;
    if !(rotation.fract() == 0.0 && (0.0..360.0).contains(&rotation)) {
        return Err(invalid(format!("a page row whose rotation is not 0, 90, 180 or 270: {row}")));
    }
    Ok(PageInfo {
        page: PageRef { page_object_number: decoded.page.page_object_number },
        index: decoded.index,
        width: decoded.size.width,
        height: decoded.size.height,
        rotation: rotation as u32,
    })
}

/// `stagePlugin({ config })`'s options, as `ViewerConfig` carries them.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct StageConfig {
    #[uniffi(default = None)]
    pub settings: Option<StageSettingsPatch>,
    /// `None` keeps the stage's default rules; an empty list opts out.
    #[uniffi(default = None)]
    pub responsive: Option<Vec<StageResponsiveRule>>,
    /// Tweens and flings on the kernel's 16 ms timer. `false` makes every
    /// navigation jump.
    #[uniffi(default = true)]
    pub frames: bool,
}

impl Default for StageConfig {
    fn default() -> Self {
        StageConfig { settings: None, responsive: None, frames: true }
    }
}
```

`StageSettings`, `StageViewState`, `StageScrollToOptions` and `StageConfig` are both plugin types and FFI records here. The FFI records own the bare names in this module, so the plugin's are never imported and are always spelled `crate::StageSettings` and so on. Importing one of them as well is a name clash (E0255).

- [x] **Step 5: Run the conversion tests**

Run: `cargo test -p epdf-plugin-stage --features uniffi --test ffi`
Expected: PASS, 5 tests.

- [x] **Step 6: Commit**

```bash
git add crates/plugins/stage
git commit -m "Mirror the stage plugin's records for uniffi"
```

### Task 3: `StageHandle`, the frame scheduler and the initial-view port

**Files:**
- Modify: `crates/plugins/stage/src/ffi.rs`, `crates/plugins/stage/tests/ffi.rs`

**Interfaces:**
- Consumes: Task 2's records; umbrella Task 0c's `RegistrationHandle` and `HostError`.
- Produces: `StageConfig::into_config(self) -> Result<crate::StageConfig, Error>` (on the kernel thread); `KernelFrames` (a `Scheduler` on `rt::set_timeout`); foreign trait `StageInitialViewProvider { fn initial_view(&self) -> Result<Option<StageViewState>, HostError> }`; object `StageHandle` with `StageHandle::new(cap: StageCapability, post: KernelPost, report: Rc<dyn Fn(&Error)>) -> Arc<Self>`. Methods, by area (all `async` and returning `Result<_, ViewerError>` unless marked fire):
  - Read: `camera`, `viewport_size`, `scroll_metrics`, `visible_pages`, `is_page_visible(page)`, `current_page_index`, `current_page`, `current_item_pages`, `page_frame(page)`, `page_at(point)`, `viewport_to_page(page, point)`, `page_to_world(page, point)`, `page_rect_to_viewport(page, rect)`, `world_to_viewport(point)`, `viewport_to_world(point)`, `page_to_viewport(page, point)`, `view_rotation`, `zoom_level`, `zoom_mode`, `viewpoint`.
  - Motion: `fling(velocity_x, velocity_y)`, `is_moving`, `stop_motion`, `set_camera(camera)` fire, `pan_by(dx, dy)` fire, `scroll_to(options)`, `scroll_by(options)` fire, `zoom_around(point, factor)` fire, `begin_gesture(elastic)`, `end_gesture`.
  - Navigation: `go_to_page_index(index, options)`, `go_to_page(page, options)`, `go_to_first_page(options)`, `go_to_last_page(options)`, `can_go_next`, `can_go_previous`, `next_page(options)`, `previous_page(options)`, `reset_view`, `reveal(page, options)`, `reveal_rect(page, rect, options)`, `reveal_index(index, options)`.
  - Settings: `settings`, `reset_settings`, `update_settings(patch)`, `set_responsive_rules(rules)`, `matches_rule(name)`, `active_rules`, `set_flow`, `set_layout`, `set_spread`, `set_sizing`, `apply_view_state(view)`.
  - Zoom: `double_tap_zoom(point)`, `zoom_in`, `zoom_out`, `zoom_to(target, around)`, `zoom_by(factor, around)`, `fit_width`, `fit_page`, `fit_all`, `fit_automatic`, `set_view_rotation(degrees)`, `rotate_view_by(delta)`.
  - Lifecycle: `set_viewport_size(size)`, `set_device_pixel_ratio(ratio)`, `refit`, `view_state`, `lens_id`, `provide_initial_view(priority, provider) -> Arc<RegistrationHandle>`, `place_initial`.

That's the 70 non-hook entries of `crates/plugins/stage/src/harness.rs` `MEMBERS`; Task 4 folds the six `on*` hooks into `subscribe`.

- [x] **Step 1: Write the failing handle tests**

Append to `crates/plugins/stage/tests/ffi.rs`:

```rust
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use epdf_ffi::{HostError, KernelThread, PageRef, Point, Size, ViewerError};
use epdf_plugin_stage::ffi::{
    StageConfig, StageGoToOptions, StageHandle, StageInitialViewProvider, StageLayout,
    StageScrollBehavior, StageViewState, StageZoomMode,
};
use epdf_plugin_stage::{STAGE, StagePluginOptions, stage_plugin};
use futures::executor::block_on;
use plugin_conformance::test_kernel::{DOCUMENT, TestKernel, TestSpec};
use serde_json::json;

const PAGE_2: PageRef = PageRef { page_object_number: 2 };

fn three_pages() -> TestSpec {
    let box_ = json!({ "left": 0, "bottom": 0, "right": 600, "top": 800 });
    let page = |pon: u32, index: u32| {
        json!({ "index": index, "ref": { "kind": "objectNumber", "pageObjectNumber": pon },
            "label": null, "size": { "width": 600, "height": 800 }, "rotation": 0,
            "userUnit": 1, "boxes": { "media": box_, "crop": box_ } })
    };
    TestSpec::new(json!({ "documents": { DOCUMENT: {
        "id": DOCUMENT, "security": { "allows": "*" },
        "pages": [page(1, 0), page(2, 1), page(3, 2)]
    } } }))
}

type Reports = Arc<Mutex<Vec<String>>>;

fn handle_with(config: StageConfig) -> (KernelThread<TestKernel>, Arc<StageHandle>, Reports) {
    let thread = KernelThread::spawn(move |_| {
        let config = config.into_config()?;
        TestKernel::open(vec![stage_plugin(StagePluginOptions { config, ..Default::default() })], three_pages())
    })
    .unwrap();
    let post = thread.post();
    let reports: Reports = Arc::default();
    let sink = reports.clone();
    let handle = block_on(thread.call(move |k| {
        let report = Rc::new(move |e: &epdf_kernel::Error| sink.lock().unwrap().push(e.message.clone()));
        StageHandle::new(k.capability(&STAGE).unwrap(), post, report)
    }))
    .unwrap();
    (thread, handle, reports)
}

fn frameless() -> (KernelThread<TestKernel>, Arc<StageHandle>, Reports) {
    handle_with(StageConfig { frames: false, ..Default::default() })
}

fn sized(stage: &StageHandle) {
    block_on(stage.set_viewport_size(Size { width: 400.0, height: 600.0 })).unwrap();
}

fn instant() -> Option<StageGoToOptions> {
    Some(StageGoToOptions { behavior: Some(StageScrollBehavior::Instant), ..Default::default() })
}

fn drain(thread: &KernelThread<TestKernel>) {
    block_on(thread.call(|_| ())).unwrap();
}

fn code(error: &ViewerError) -> Option<&str> {
    match error {
        ViewerError::Kernel { code, .. } => code.as_deref(),
        ViewerError::Closed => None,
    }
}

#[test]
fn a_zero_viewport_places_nothing_until_it_has_a_size() {
    let (_thread, stage, _) = frameless();
    block_on(stage.set_viewport_size(Size { width: 0.0, height: 0.0 })).unwrap();
    assert!(block_on(stage.visible_pages()).unwrap().is_empty());
    sized(&stage);
    let visible = block_on(stage.visible_pages()).unwrap();
    assert!(!visible.is_empty());
    assert_eq!(visible[0].page_index, 0);
    assert!(visible[0].transform.render_scale > 0.0);
}

#[test]
fn navigation_moves_the_current_page() {
    let (_thread, stage, _) = frameless();
    sized(&stage);
    assert!(block_on(stage.can_go_next()).unwrap());
    assert!(!block_on(stage.can_go_previous()).unwrap());
    block_on(stage.go_to_page(PAGE_2, instant())).unwrap();
    assert_eq!(block_on(stage.current_page_index()).unwrap(), 1);
    assert_eq!(block_on(stage.current_page()).unwrap().map(|p| p.page), Some(PAGE_2));
    block_on(stage.go_to_last_page(instant())).unwrap();
    assert_eq!(block_on(stage.current_page_index()).unwrap(), 2);
    assert!(!block_on(stage.can_go_next()).unwrap());
}

#[test]
fn an_index_past_the_end_lands_on_the_last_page() {
    let (_thread, stage, _) = frameless();
    sized(&stage);
    block_on(stage.go_to_page_index(99, instant())).unwrap();
    assert_eq!(block_on(stage.current_page_index()).unwrap(), 2);
}

#[test]
fn fit_width_sets_the_mode_and_zoom_in_leaves_it() {
    let (_thread, stage, _) = frameless();
    sized(&stage);
    block_on(stage.fit_width()).unwrap();
    assert_eq!(
        block_on(stage.zoom_mode()).unwrap(),
        StageZoomMode::Mode { mode: epdf_ffi::ZoomMode::FitWidth }
    );
    let fitted = block_on(stage.zoom_level()).unwrap();
    block_on(stage.zoom_in()).unwrap();
    assert!(block_on(stage.zoom_level()).unwrap() > fitted);
    assert_eq!(block_on(stage.zoom_mode()).unwrap(), StageZoomMode::Custom);
}

#[test]
fn page_and_viewport_points_round_trip() {
    let (_thread, stage, _) = frameless();
    sized(&stage);
    let page = PageRef { page_object_number: 1 };
    let point = Point { x: 100.0, y: 200.0 };
    let on_screen = block_on(stage.page_to_viewport(page, point)).unwrap().unwrap();
    let back = block_on(stage.viewport_to_page(page, on_screen)).unwrap().unwrap();
    assert!((back.x - point.x).abs() < 1e-6 && (back.y - point.y).abs() < 1e-6, "{back:?}");
    let hit = block_on(stage.page_at(on_screen)).unwrap().unwrap();
    assert_eq!(hit.page, page);
}

#[test]
fn settings_update_and_reset() {
    let (_thread, stage, _) = frameless();
    sized(&stage);
    block_on(stage.set_layout(StageLayout::Horizontal)).unwrap();
    assert_eq!(block_on(stage.settings()).unwrap().layout, StageLayout::Horizontal);
    block_on(stage.reset_settings()).unwrap();
    assert_eq!(block_on(stage.settings()).unwrap().layout, StageLayout::Vertical);
}

#[test]
fn a_view_state_restores_where_it_was_taken() {
    let (_thread, stage, _) = frameless();
    sized(&stage);
    block_on(stage.go_to_page_index(2, instant())).unwrap();
    let saved: StageViewState = block_on(stage.view_state()).unwrap();
    block_on(stage.go_to_first_page(instant())).unwrap();
    block_on(stage.apply_view_state(saved.clone())).unwrap();
    assert_eq!(block_on(stage.current_page_index()).unwrap(), 2);
}

struct Restore(StageViewState);
impl StageInitialViewProvider for Restore {
    fn initial_view(&self) -> Result<Option<StageViewState>, HostError> {
        Ok(Some(self.0.clone()))
    }
}

#[test]
fn an_initial_view_provider_places_the_first_view() {
    let (_thread, stage, _) = frameless();
    // A state taken from a sized stage, then handed to a fresh one.
    let (_t2, other, _) = frameless();
    sized(&other);
    block_on(other.go_to_page_index(1, instant())).unwrap();
    let saved = block_on(other.view_state()).unwrap();
    let _registration =
        block_on(stage.provide_initial_view(10.0, Arc::new(Restore(saved)))).unwrap();
    sized(&stage);
    assert_eq!(block_on(stage.current_page_index()).unwrap(), 1);
}

#[test]
fn concurrent_pans_all_land() {
    // The same 100 one-point pans, from one thread and from four.
    let pan = |stage: &Arc<StageHandle>, threads: usize| {
        block_on(stage.begin_gesture(None)).unwrap();
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                let stage = stage.clone();
                std::thread::spawn(move || {
                    for _ in 0..100 / threads {
                        stage.pan_by(0.0, 1.0);
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        block_on(stage.end_gesture()).unwrap();
    };
    let (serial_thread, serial, _) = frameless();
    sized(&serial);
    pan(&serial, 1);
    drain(&serial_thread);
    let (thread, stage, reports) = frameless();
    sized(&stage);
    let start = block_on(stage.camera()).unwrap();
    pan(&stage, 4);
    drain(&thread);
    let end = block_on(stage.camera()).unwrap();
    assert!(reports.lock().unwrap().is_empty(), "{:?}", reports.lock().unwrap());
    assert_ne!(end, start);
    assert_eq!(end, block_on(serial.camera()).unwrap());
}

#[test]
fn motion_in_flight_at_close_stops() {
    let (thread, stage, reports) = handle_with(StageConfig::default());
    sized(&stage);
    block_on(stage.fling(0.0, -4000.0)).unwrap();
    assert!(block_on(stage.is_moving()).unwrap());
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    // Let a few frames' worth of timers come due on the kernel thread.
    std::thread::sleep(std::time::Duration::from_millis(100));
    drain(&thread);
    assert!(reports.lock().unwrap().is_empty(), "{:?}", reports.lock().unwrap());
    let error = block_on(stage.camera()).unwrap_err();
    assert_eq!(code(&error), Some("instance-closed"));
}

#[test]
fn a_closed_documents_capability_refuses_with_instance_closed() {
    let (thread, stage, _) = frameless();
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    for error in [
        block_on(stage.zoom_level()).map(|_| ()).unwrap_err(),
        block_on(stage.fit_width()).unwrap_err(),
        block_on(stage.go_to_page_index(1, None)).unwrap_err(),
    ] {
        assert_eq!(code(&error), Some("instance-closed"), "{error:?}");
    }
    // A fire after the close is dropped, not a panic.
    stage.pan_by(1.0, 1.0);
    drain(&thread);
}
```

`concurrent_pans_all_land` compares against the same pans from one thread rather than a computed distance, because the stage's bounding and padding decide how far 100 points of pan move the camera. If `rt` timers don't fire on a `KernelThread` without a runtime pump on this branch, `motion_in_flight_at_close_stops` hangs on `is_moving`; check how `KernelThread` drives `rt::tick` in `crates/epdf-ffi/src/kernel_thread.rs` before assuming the scheduler is wrong.

- [x] **Step 2: Run them to verify they fail**

Run: `cargo test -p epdf-plugin-stage --features uniffi --test ffi`
Expected: FAIL to compile, "cannot find `StageHandle` in `ffi`".

- [x] **Step 3: Write the scheduler, config and port**

Append to `ffi.rs`:

```rust
/// The host frame seam on the kernel's clock: each frame is a 16 ms timer,
/// as render's `request_frame` falls back to. Lives on the kernel thread.
#[derive(Default)]
pub struct KernelFrames {
    next: std::cell::Cell<u64>,
    timers: RefCell<HashMap<u64, rt::TimerId>>,
}

impl Scheduler for KernelFrames {
    fn raf(&self, callback: Box<dyn FnOnce(f64)>) -> u64 {
        let handle = self.next.get() + 1;
        self.next.set(handle);
        let timer = rt::set_timeout(16.0, move || callback(rt::now()));
        self.timers.borrow_mut().insert(handle, timer);
        handle
    }

    fn caf(&self, handle: u64) {
        if let Some(timer) = self.timers.borrow_mut().remove(&handle) {
            rt::clear_timer(timer);
        }
    }
}

impl StageConfig {
    /// On the kernel thread: the scheduler is an `Rc`.
    pub fn into_config(self) -> Result<crate::StageConfig, Error> {
        Ok(crate::StageConfig {
            settings: self.settings.map(PartialSettings::try_from).transpose()?.unwrap_or_default(),
            scheduler: self.frames.then(|| Rc::new(KernelFrames::default()) as Rc<dyn Scheduler>),
            responsive: self
                .responsive
                .map(|rules| rules.into_iter().map(ResponsiveRule::try_from).collect())
                .transpose()?,
        })
    }
}

/// Supplies the first view of a document (a saved position, say). Called
/// on the kernel thread when the stage first places, highest priority
/// first; `None` passes to the next provider.
#[uniffi::export(with_foreign)]
pub trait StageInitialViewProvider: Send + Sync {
    fn initial_view(&self) -> Result<Option<StageViewState>, HostError>;
}
```

`KernelFrames.timers` keeps a handle until `caf`; a fired timer's entry stays until the stage cancels or the map drops with the stage, which is bounded by one entry per frame the stage requested and never cancelled. If `rt::TimerId` isn't `pub` from `epdf_kernel::rt`, export it there (`pub use` of the existing `pub struct TimerId`) in the same commit.

- [x] **Step 4: Write the handle**

```rust
fn open(cap: &StageCapability) -> Result<&StageCapability, Error> {
    if cap.lifetime().aborted() {
        return Err(plugin_error(
            code::INSTANCE_CLOSED,
            "stage",
            "the document this capability belongs to is closed",
            None,
            None,
        ));
    }
    Ok(cap)
}

struct Bound {
    cap: StageCapability,
    report: Rc<dyn Fn(&Error)>,
}

impl Bound {
    /// For a fire method: run `f` if the document is open, report its error.
    fn fire(&self, f: impl FnOnce(&StageCapability) -> Result<(), Error>) {
        if let Ok(cap) = open(&self.cap) {
            if let Err(error) = f(cap) {
                (self.report)(&error);
            }
        }
    }
}

#[derive(uniffi::Object)]
pub struct StageHandle {
    bound: KernelBound<Bound>,
    post: KernelPost,
}

impl StageHandle {
    /// On the kernel thread. `report` receives a fire method's errors.
    pub fn new(cap: StageCapability, post: KernelPost, report: Rc<dyn Fn(&Error)>) -> Arc<Self> {
        Arc::new(StageHandle { bound: KernelBound::new(Bound { cap, report }, post.clone()), post })
    }

    async fn read<T: Send + 'static>(
        &self,
        f: impl FnOnce(&StageCapability) -> Result<T, Error> + Send + 'static,
    ) -> Result<T, ViewerError> {
        self.bound.call(move |b| f(open(&b.cap)?)).await
    }
}

#[uniffi::export]
impl StageHandle {
    // ── read ──

    pub async fn camera(&self) -> Result<Camera, ViewerError> {
        self.read(|c| Ok(c.get_camera().into())).await
    }

    pub async fn viewport_size(&self) -> Result<Size, ViewerError> {
        self.read(|c| Ok(c.get_viewport_size().into())).await
    }

    pub async fn scroll_metrics(&self) -> Result<StageScrollMetrics, ViewerError> {
        self.read(|c| Ok(c.get_scroll_metrics().into())).await
    }

    pub async fn visible_pages(&self) -> Result<Vec<StageVisiblePage>, ViewerError> {
        self.read(|c| c.list_visible_pages().iter().map(StageVisiblePage::try_from).collect()).await
    }

    pub async fn is_page_visible(&self, page: PageRef) -> Result<bool, ViewerError> {
        self.read(move |c| Ok(c.is_page_visible(page.into()))).await
    }

    pub async fn current_page_index(&self) -> Result<u32, ViewerError> {
        self.read(|c| to_u32(c.get_current_page_index(), "the current page index")).await
    }

    pub async fn current_page(&self) -> Result<Option<PageInfo>, ViewerError> {
        self.read(|c| c.get_current_page().as_ref().map(page_info).transpose()).await
    }

    /// The pages of the current item (two in a spread).
    pub async fn current_item_pages(&self) -> Result<Vec<PageInfo>, ViewerError> {
        self.read(|c| c.list_current_item_pages().iter().map(page_info).collect()).await
    }

    pub async fn page_frame(&self, page: PageRef) -> Result<Option<StageVisiblePage>, ViewerError> {
        self.read(move |c| c.get_page_frame(page.into()).as_ref().map(StageVisiblePage::try_from).transpose())
            .await
    }

    pub async fn page_at(&self, point: Point) -> Result<Option<StagePageHit>, ViewerError> {
        self.read(move |c| Ok(c.get_page_at(point.into()).map(Into::into))).await
    }

    pub async fn viewport_to_page(&self, page: PageRef, point: Point) -> Result<Option<Point>, ViewerError> {
        self.read(move |c| Ok(c.viewport_to_page(page.into(), point.into()).map(Into::into))).await
    }

    pub async fn page_to_world(&self, page: PageRef, point: Point) -> Result<Option<Point>, ViewerError> {
        self.read(move |c| Ok(c.page_to_world(page.into(), point.into()).map(Into::into))).await
    }

    pub async fn page_rect_to_viewport(&self, page: PageRef, rect: Rect) -> Result<Option<Rect>, ViewerError> {
        self.read(move |c| Ok(c.page_rect_to_viewport(page.into(), rect.into()).map(Into::into))).await
    }

    pub async fn world_to_viewport(&self, point: Point) -> Result<Point, ViewerError> {
        self.read(move |c| Ok(c.world_to_viewport(point.into()).into())).await
    }

    pub async fn viewport_to_world(&self, point: Point) -> Result<Point, ViewerError> {
        self.read(move |c| Ok(c.viewport_to_world(point.into()).into())).await
    }

    pub async fn page_to_viewport(&self, page: PageRef, point: Point) -> Result<Option<Point>, ViewerError> {
        self.read(move |c| Ok(c.page_to_viewport(page.into(), point.into()).map(Into::into))).await
    }

    /// Degrees clockwise.
    pub async fn view_rotation(&self) -> Result<f64, ViewerError> {
        self.read(|c| Ok(c.get_view_rotation().0)).await
    }

    pub async fn zoom_level(&self) -> Result<f64, ViewerError> {
        self.read(|c| Ok(c.get_zoom_level())).await
    }

    pub async fn zoom_mode(&self) -> Result<StageZoomMode, ViewerError> {
        self.read(|c| Ok(c.get_zoom_mode().into())).await
    }

    pub async fn viewpoint(&self) -> Result<StageViewpoint, ViewerError> {
        self.read(|c| c.get_viewpoint().try_into()).await
    }

    // ── motion ──

    /// Momentum from a release velocity in viewport points per second.
    pub async fn fling(&self, velocity_x: f64, velocity_y: f64) -> Result<(), ViewerError> {
        self.read(move |c| c.fling(velocity_x, velocity_y)).await
    }

    pub async fn is_moving(&self) -> Result<bool, ViewerError> {
        self.read(|c| Ok(c.is_moving())).await
    }

    pub async fn stop_motion(&self) -> Result<(), ViewerError> {
        self.read(|c| {
            c.stop_motion();
            Ok(())
        })
        .await
    }

    /// Fire: no reply; an error goes to the report sink.
    pub fn set_camera(&self, camera: Camera) {
        self.bound.fire(move |b| b.fire(|c| c.set_camera(camera.into())));
    }

    /// Fire, in viewport points.
    pub fn pan_by(&self, dx: f64, dy: f64) {
        self.bound.fire(move |b| b.fire(|c| c.pan_by(dx, dy)));
    }

    pub async fn scroll_to(&self, options: StageScrollToOptions) -> Result<(), ViewerError> {
        self.read(move |c| {
            c.scroll_to(options.into());
            Ok(())
        })
        .await
    }

    /// Fire: relative offsets.
    pub fn scroll_by(&self, options: StageScrollToOptions) {
        self.bound.fire(move |b| {
            b.fire(|c| {
                c.scroll_by(options.into());
                Ok(())
            })
        });
    }

    /// Fire: hold `point` (viewport) fixed while zooming by `factor`.
    pub fn zoom_around(&self, point: Point, factor: f64) {
        self.bound.fire(move |b| b.fire(|c| c.zoom_around(point.into(), factor)));
    }

    /// Open a direct-manipulation bracket. `elastic` turns on rubber-band
    /// overscroll for it.
    pub async fn begin_gesture(&self, elastic: Option<bool>) -> Result<(), ViewerError> {
        self.read(move |c| {
            c.begin_gesture(Some(GestureOptions { elastic }));
            Ok(())
        })
        .await
    }

    pub async fn end_gesture(&self) -> Result<(), ViewerError> {
        self.read(|c| c.end_gesture()).await
    }

    // ── navigation ──

    /// A past-the-end index lands on the last page.
    pub async fn go_to_page_index(&self, index: u32, options: Option<StageGoToOptions>) -> Result<(), ViewerError> {
        self.read(move |c| {
            c.go_to_page_index(index as isize, options.map(Into::into));
            Ok(())
        })
        .await
    }

    pub async fn go_to_page(&self, page: PageRef, options: Option<StageGoToOptions>) -> Result<(), ViewerError> {
        self.read(move |c| {
            c.go_to_page(page.into(), options.map(Into::into));
            Ok(())
        })
        .await
    }

    pub async fn go_to_first_page(&self, options: Option<StageGoToOptions>) -> Result<(), ViewerError> {
        self.read(move |c| {
            c.go_to_first_page(options.map(Into::into));
            Ok(())
        })
        .await
    }

    pub async fn go_to_last_page(&self, options: Option<StageGoToOptions>) -> Result<(), ViewerError> {
        self.read(move |c| {
            c.go_to_last_page(options.map(Into::into));
            Ok(())
        })
        .await
    }

    pub async fn can_go_next(&self) -> Result<bool, ViewerError> {
        self.read(|c| Ok(c.can_go_next())).await
    }

    pub async fn can_go_previous(&self) -> Result<bool, ViewerError> {
        self.read(|c| Ok(c.can_go_previous())).await
    }

    pub async fn next_page(&self, options: Option<StageGoToOptions>) -> Result<(), ViewerError> {
        self.read(move |c| {
            c.next_page(options.map(Into::into));
            Ok(())
        })
        .await
    }

    pub async fn previous_page(&self, options: Option<StageGoToOptions>) -> Result<(), ViewerError> {
        self.read(move |c| {
            c.previous_page(options.map(Into::into));
            Ok(())
        })
        .await
    }

    pub async fn reset_view(&self) -> Result<(), ViewerError> {
        self.read(|c| {
            c.reset_view();
            Ok(())
        })
        .await
    }

    pub async fn reveal(&self, page: PageRef, options: Option<StageRevealOptions>) -> Result<(), ViewerError> {
        self.read(move |c| {
            c.reveal(page.into(), options.map(Into::into));
            Ok(())
        })
        .await
    }

    pub async fn reveal_rect(
        &self,
        page: PageRef,
        rect: Rect,
        options: Option<StageRevealOptions>,
    ) -> Result<(), ViewerError> {
        self.read(move |c| {
            c.reveal_rect(page.into(), rect.into(), options.map(Into::into));
            Ok(())
        })
        .await
    }

    pub async fn reveal_index(&self, index: u32, options: Option<StageRevealOptions>) -> Result<(), ViewerError> {
        self.read(move |c| {
            c.reveal_index(index as isize, options.map(Into::into));
            Ok(())
        })
        .await
    }

    // ── settings ──

    pub async fn settings(&self) -> Result<StageSettings, ViewerError> {
        self.read(|c| c.get_settings().try_into()).await
    }

    pub async fn reset_settings(&self) -> Result<(), ViewerError> {
        self.read(|c| c.reset_settings()).await
    }

    pub async fn update_settings(&self, patch: StageSettingsPatch) -> Result<(), ViewerError> {
        self.read(move |c| c.update_settings(patch.try_into()?)).await
    }

    pub async fn set_responsive_rules(&self, rules: Vec<StageResponsiveRule>) -> Result<(), ViewerError> {
        self.read(move |c| {
            let rules = rules.into_iter().map(ResponsiveRule::try_from).collect::<Result<_, _>>()?;
            c.set_responsive_rules(rules)
        })
        .await
    }

    pub async fn matches_rule(&self, name: String) -> Result<bool, ViewerError> {
        self.read(move |c| Ok(c.matches_rule(&name))).await
    }

    pub async fn active_rules(&self) -> Result<Vec<String>, ViewerError> {
        self.read(|c| Ok(c.list_active_rules())).await
    }

    pub async fn set_flow(&self, flow: StageFlow) -> Result<(), ViewerError> {
        self.read(move |c| c.set_flow(flow.into())).await
    }

    pub async fn set_layout(&self, layout: StageLayout) -> Result<(), ViewerError> {
        self.read(move |c| c.set_layout(layout.into())).await
    }

    pub async fn set_spread(&self, spread: StageSpreadMode) -> Result<(), ViewerError> {
        self.read(move |c| c.set_spread(spread.into())).await
    }

    pub async fn set_sizing(&self, sizing: StageSizingMode) -> Result<(), ViewerError> {
        self.read(move |c| c.set_sizing(sizing.into())).await
    }

    pub async fn apply_view_state(&self, view: StageViewState) -> Result<(), ViewerError> {
        self.read(move |c| {
            c.apply_view_state(view.try_into()?);
            Ok(())
        })
        .await
    }

    // ── zoom ──

    pub async fn double_tap_zoom(&self, point: Point) -> Result<(), ViewerError> {
        self.read(move |c| {
            c.double_tap_zoom(point.into());
            Ok(())
        })
        .await
    }

    pub async fn zoom_in(&self) -> Result<(), ViewerError> {
        self.read(|c| c.zoom_in()).await
    }

    pub async fn zoom_out(&self) -> Result<(), ViewerError> {
        self.read(|c| c.zoom_out()).await
    }

    /// `around` (viewport) stays fixed; `None` is the zoom-align point.
    pub async fn zoom_to(&self, target: StageZoomTarget, around: Option<Point>) -> Result<(), ViewerError> {
        self.read(move |c| c.zoom_to(target.into(), Some(ZoomToOptions { around: around.map(Into::into) })))
            .await
    }

    pub async fn zoom_by(&self, factor: f64, around: Option<Point>) -> Result<(), ViewerError> {
        self.read(move |c| c.zoom_by(factor, Some(ZoomToOptions { around: around.map(Into::into) }))).await
    }

    pub async fn fit_width(&self) -> Result<(), ViewerError> {
        self.read(|c| c.fit_width()).await
    }

    pub async fn fit_page(&self) -> Result<(), ViewerError> {
        self.read(|c| c.fit_page()).await
    }

    pub async fn fit_all(&self) -> Result<(), ViewerError> {
        self.read(|c| c.fit_all()).await
    }

    pub async fn fit_automatic(&self) -> Result<(), ViewerError> {
        self.read(|c| c.fit_automatic()).await
    }

    /// Degrees clockwise: 0, 90, 180 or 270.
    pub async fn set_view_rotation(&self, degrees: f64) -> Result<(), ViewerError> {
        self.read(move |c| c.set_view_rotation(PageRotation(degrees))).await
    }

    pub async fn rotate_view_by(&self, delta: f64) -> Result<(), ViewerError> {
        self.read(move |c| c.rotate_view_by(delta)).await
    }

    // ── lifecycle ──

    /// Report the viewport in points. The first non-zero size places the
    /// document.
    pub async fn set_viewport_size(&self, size: Size) -> Result<(), ViewerError> {
        self.read(move |c| {
            c.set_viewport_size(size.into());
            Ok(())
        })
        .await
    }

    pub async fn set_device_pixel_ratio(&self, ratio: f64) -> Result<(), ViewerError> {
        self.read(move |c| {
            c.set_device_pixel_ratio(ratio);
            Ok(())
        })
        .await
    }

    pub async fn refit(&self) -> Result<(), ViewerError> {
        self.read(|c| {
            c.refit();
            Ok(())
        })
        .await
    }

    pub async fn view_state(&self) -> Result<StageViewState, ViewerError> {
        self.read(|c| c.get_view_state().try_into()).await
    }

    pub async fn lens_id(&self) -> Result<String, ViewerError> {
        self.read(|c| Ok(c.get_lens_id())).await
    }

    /// The provider is asked on the kernel thread when the stage first
    /// places. A thrown error is reported and counts as `None`.
    pub async fn provide_initial_view(
        &self,
        priority: f64,
        provider: Arc<dyn StageInitialViewProvider>,
    ) -> Result<Arc<RegistrationHandle>, ViewerError> {
        let post = self.post.clone();
        self.bound
            .call(move |b| {
                let report = b.report.clone();
                let off = open(&b.cap)?.provide_initial_view(priority, move || {
                    match provider.initial_view() {
                        Ok(view) => view.and_then(|v| match crate::StageViewState::try_from(v) {
                            Ok(v) => Some(v),
                            Err(error) => {
                                report(&error);
                                None
                            }
                        }),
                        Err(error) => {
                            report(&error.into());
                            None
                        }
                    }
                });
                Ok(RegistrationHandle::new(post, move || off.call()))
            })
            .await
    }

    pub async fn place_initial(&self) -> Result<(), ViewerError> {
        self.read(|c| {
            c.place_initial();
            Ok(())
        })
        .await
    }
}
```

- [x] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p epdf-plugin-stage --features uniffi --test ffi`
Expected: PASS, 16 tests.

Run: `cargo test -p epdf-plugin-stage`
Expected: PASS, conformance and `api` unchanged.

- [x] **Step 6: Commit**

```bash
git add crates/plugins/stage crates/epdf-kernel
git commit -m "Export the stage plugin's handle with a kernel-timer frame scheduler"
```

### Task 4: Stage events

**Files:**
- Modify: `crates/plugins/stage/src/ffi.rs`, `crates/plugins/stage/tests/ffi.rs`

**Interfaces:**
- Consumes: `StageCapability::{on_page_changed, on_zoom_changed, on_camera_changed, on_motion_ended, on_settings_changed, on_viewport_changed}` (`crates/plugins/stage/src/services/events.rs`).
- Produces: records `StagePageChangedEvent { page: Option<PageInfo>, page_index: u32, previous_page_index: u32 }`, `StageZoomChangedEvent { level, previous_level, mode: StageZoomMode }`, `StageCameraChangedEvent { camera }`, `StageMotionEndedEvent { camera }`, `StageSettingsChangedEvent { settings, changed: Vec<StageSettingKey> }`, `StageViewportChangedEvent { size }`; enum `StageEvent` with one variant per record; trait `StageEventListener { on_event, on_end }`; `StageHandle::subscribe(listener) -> Arc<Subscription>`.

- [x] **Step 1: Write the failing tests**

Append to `crates/plugins/stage/tests/ffi.rs`:

```rust
use std::sync::atomic::{AtomicUsize, Ordering};

use epdf_plugin_stage::ffi::{StageEvent, StageEventListener, StageSettingKey};

#[derive(Default)]
struct Events(Mutex<Vec<StageEvent>>, AtomicUsize);

impl StageEventListener for Events {
    fn on_event(&self, event: StageEvent) {
        self.0.lock().unwrap().push(event);
    }
    fn on_end(&self) {
        self.1.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn navigation_zoom_and_settings_are_observed() {
    let (thread, stage, _) = frameless();
    let events = Arc::new(Events::default());
    let _sub = stage.subscribe(events.clone());
    sized(&stage);
    block_on(stage.go_to_page_index(1, instant())).unwrap();
    block_on(stage.zoom_in()).unwrap();
    block_on(stage.set_layout(StageLayout::Horizontal)).unwrap();
    drain(&thread);
    let seen = events.0.lock().unwrap();
    assert!(seen.iter().any(|e| matches!(e, StageEvent::ViewportChanged { event }
        if event.size.width == 400.0)), "{seen:?}");
    assert!(seen.iter().any(|e| matches!(e, StageEvent::PageChanged { event }
        if event.page_index == 1 && event.page.map(|p| p.page) == Some(PAGE_2))), "{seen:?}");
    assert!(seen.iter().any(|e| matches!(e, StageEvent::ZoomChanged { event }
        if event.level > event.previous_level)), "{seen:?}");
    assert!(seen.iter().any(|e| matches!(e, StageEvent::SettingsChanged { event }
        if event.changed.contains(&StageSettingKey::Layout))), "{seen:?}");
    assert!(seen.iter().any(|e| matches!(e, StageEvent::CameraChanged { .. })), "{seen:?}");
}

#[test]
fn closing_the_document_ends_stage_subscriptions() {
    let (thread, stage, _) = frameless();
    let events = Arc::new(Events::default());
    let _sub = stage.subscribe(events.clone());
    drain(&thread);
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    assert_eq!(events.1.load(Ordering::SeqCst), 1);
}
```

- [x] **Step 2: Run them to verify they fail**

Run: `cargo test -p epdf-plugin-stage --features uniffi --test ffi observed`
Expected: FAIL to compile, "cannot find `StageEvent`".

- [x] **Step 3: Write the events**

Append to `ffi.rs`:

```rust
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct StagePageChangedEvent {
    pub page: Option<PageInfo>,
    pub page_index: u32,
    pub previous_page_index: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct StageZoomChangedEvent {
    pub level: f64,
    pub previous_level: f64,
    pub mode: StageZoomMode,
}

/// Coalesced per camera write, not per frame.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct StageCameraChangedEvent {
    pub camera: Camera,
}

/// A tween or fling settled, or was stopped.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct StageMotionEndedEvent {
    pub camera: Camera,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct StageSettingsChangedEvent {
    pub settings: StageSettings,
    pub changed: Vec<StageSettingKey>,
}

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct StageViewportChangedEvent {
    pub size: Size,
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum StageEvent {
    PageChanged { event: StagePageChangedEvent },
    ZoomChanged { event: StageZoomChangedEvent },
    CameraChanged { event: StageCameraChangedEvent },
    MotionEnded { event: StageMotionEndedEvent },
    SettingsChanged { event: StageSettingsChangedEvent },
    ViewportChanged { event: StageViewportChangedEvent },
}

#[uniffi::export(with_foreign)]
pub trait StageEventListener: Send + Sync {
    fn on_event(&self, event: StageEvent);
    /// The document closed: no event follows.
    fn on_end(&self);
}

#[uniffi::export]
impl StageHandle {
    /// The listener is registered on the kernel thread after this returns.
    /// It gets `on_end` when the document closes (at once if it already
    /// has). An event that does not convert is reported, not delivered.
    pub fn subscribe(&self, listener: Arc<dyn StageEventListener>) -> Arc<Subscription> {
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
                        relay!(on_page_changed, |e| StageEvent::PageChanged {
                            event: StagePageChangedEvent {
                                page: e.page.as_ref().map(page_info).transpose()?,
                                page_index: to_u32(e.page_index, "a page index")?,
                                previous_page_index: to_u32(e.previous_page_index, "a page index")?,
                            },
                        }),
                        relay!(on_zoom_changed, |e| StageEvent::ZoomChanged {
                            event: StageZoomChangedEvent {
                                level: e.level,
                                previous_level: e.previous_level,
                                mode: e.mode.into(),
                            },
                        }),
                        relay!(on_camera_changed, |e| StageEvent::CameraChanged {
                            event: StageCameraChangedEvent { camera: e.camera.into() },
                        }),
                        relay!(on_motion_ended, |e| StageEvent::MotionEnded {
                            event: StageMotionEndedEvent { camera: e.camera.into() },
                        }),
                        relay!(on_settings_changed, |e| StageEvent::SettingsChanged {
                            event: StageSettingsChangedEvent {
                                settings: e.settings.clone().try_into()?,
                                changed: e.changed.iter().map(|k| (*k).into()).collect(),
                            },
                        }),
                        relay!(on_viewport_changed, |e| StageEvent::ViewportChanged {
                            event: StageViewportChangedEvent { size: e.size.into() },
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

If a hook's listener type isn't `Fn(&E) -> Result<(), Error>` (check `services/events.rs`), adapt the macro the way search's `subscribe` does.

- [x] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p epdf-plugin-stage --features uniffi --test ffi`
Expected: PASS, 18 tests.

- [x] **Step 5: Commit**

```bash
git add crates/plugins/stage
git commit -m "Relay the stage plugin's events through uniffi"
```

### Task 5: Render's viewport members

**Files:**
- Modify: `crates/plugins/render/src/ffi.rs`, `crates/plugins/render/tests/ffi.rs`

**Interfaces:**
- Consumes: `RenderCapability::{get_render_policy, render_source, get_source_key, conform_viewport, get_paint_settings, create_view_demand}`, `ViewDemand::{set_demand, get_plan, mark_painted, mark_unpainted, release, dispose}` (`crates/plugins/render/src/controller.rs`); `EngineRenderPolicy`, `PageRenderViewport` (`policy.rs`); `PageViewDemand`, `Velocity`, `TilePaintPlan` (`paint_plan.rs`); the existing `Raster`, `rendered`, `abortable`, `open_cap` in `ffi.rs`.
- Produces: on `RenderHandle`: `render_policy() -> RenderPolicy`, `render_source(page, scale, include_annotations, cancel) -> RenderedImage`, `source_key(page, scale, include_annotations) -> String`, `conform_viewport(page, scale) -> RenderViewport`, `paint_settings() -> RenderPaintSettings`, `create_view_demand(view_id) -> Arc<RenderViewDemandHandle>`. Records/enums: `RenderPolicy { Continuous, Lattice { lattice: RenderLatticePolicy } }`, `RenderLatticePolicy { full_page_widths, tile_sizes, tile_scales, appearance_scales, max_render_pixels, background, enforced }`, `RenderViewport { Scale { scale: Option<f64> }, Width { width } }`, `RenderPaintSettings { fade_ms, tiles }`, `RenderPageViewDemand { desired_device_width, visible_rect, velocity_x, velocity_y }`, `RenderTileSource { key, scale, rect, z }`, `RenderTilePlan { engaged, paint, fetching, stamp }`. `RenderViewDemandHandle` methods: `set_demand(page, demand, include_annotations)`, `plan(page)`, `tile_image(page, key) -> Option<RenderedImage>`, `mark_painted(page, key)`, `mark_unpainted(page, key)`, `release(page)`, `dispose()`.

- [ ] **Step 1: Write the failing tests**

Append to `crates/plugins/render/tests/ffi.rs`:

```rust
use epdf_plugin_render::ffi::{RenderPageViewDemand, RenderPolicy, RenderViewport};

#[test]
fn the_default_policy_is_continuous_and_a_viewport_conforms_to_a_width() {
    let rig = Rig::pixels();
    assert_eq!(block_on(rig.render.render_policy()).unwrap(), RenderPolicy::Continuous);
    let viewport = block_on(rig.render.conform_viewport(LARGE, 2.0)).unwrap();
    assert_eq!(viewport, RenderViewport::Width { width: 8.0 });
}

#[test]
fn a_source_key_names_the_raster_render_source_produces() {
    let rig = Rig::pixels();
    let a = block_on(rig.render.source_key(LARGE, 2.0, None)).unwrap();
    let b = block_on(rig.render.source_key(LARGE, 2.0, Some(true))).unwrap();
    let c = block_on(rig.render.source_key(LARGE, 3.0, None)).unwrap();
    assert_eq!(a, b);
    assert_ne!(a, c);
    let image = block_on(rig.render.render_source(LARGE, 2.0, None, token())).unwrap();
    assert_eq!((image.width, image.height), (8, 4));
}

#[test]
fn a_page_not_in_the_document_is_not_found() {
    let rig = Rig::pixels();
    let missing = PageRef { page_object_number: 99 };
    let error = block_on(rig.render.source_key(missing, 1.0, None)).unwrap_err();
    assert_eq!(code(&error), Some("not-found"), "{error:?}");
}

#[test]
fn a_view_demand_plans_and_disposes() {
    let rig = Rig::pixels();
    let paint = block_on(rig.render.paint_settings()).unwrap();
    let view = block_on(rig.render.create_view_demand("main".into())).unwrap();
    let demand = RenderPageViewDemand {
        desired_device_width: 8.0,
        visible_rect: None,
        velocity_x: None,
        velocity_y: None,
    };
    block_on(view.set_demand(LARGE, demand, None)).unwrap();
    let plan = block_on(view.plan(LARGE)).unwrap();
    // Tiles are off by default, so the plan does not engage.
    assert_eq!(plan.engaged, paint.tiles);
    assert!(block_on(view.tile_image(LARGE, "no-such-key".into())).unwrap().is_none());
    block_on(view.release(LARGE)).unwrap();
    block_on(view.dispose()).unwrap();
}

#[test]
fn a_closed_documents_view_demand_refuses() {
    let rig = Rig::pixels();
    let view = block_on(rig.render.create_view_demand("main".into())).unwrap();
    rig.close();
    let error = block_on(view.plan(LARGE)).unwrap_err();
    assert_eq!(code(&error), Some("instance-closed"), "{error:?}");
}
```

`Rig` exposes the handle as `rig.render` (`crates/plugins/render/tests/support/mod.rs`). If the default config's `paint.tiles` is true, the plan engages and the first assertion still holds.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p epdf-plugin-render --features uniffi --test ffi`
Expected: FAIL to compile, "no method named `render_policy`".

- [ ] **Step 3: Write the records**

Add to `crates/plugins/render/src/ffi.rs`, beside the existing records:

```rust
use crate::paint_plan::{PageViewDemand, TilePaintPlan, Velocity};
use crate::policy::{EngineRenderPolicy, PageRenderViewport};
use crate::{RenderSourceOptions, ViewDemand};

/// The deployment's render policy: render at any size, or snap to ladders.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum RenderPolicy {
    Continuous,
    Lattice { lattice: RenderLatticePolicy },
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RenderLatticePolicy {
    pub full_page_widths: Vec<f64>,
    pub tile_sizes: Vec<f64>,
    pub tile_scales: Vec<f64>,
    pub appearance_scales: Vec<f64>,
    pub max_render_pixels: Option<f64>,
    pub background: Option<String>,
    pub enforced: Option<bool>,
}

impl From<EngineRenderPolicy> for RenderPolicy {
    fn from(policy: EngineRenderPolicy) -> Self {
        match policy {
            EngineRenderPolicy::Continuous => RenderPolicy::Continuous,
            EngineRenderPolicy::Lattice(l) => RenderPolicy::Lattice {
                lattice: RenderLatticePolicy {
                    full_page_widths: l.full_page.widths,
                    tile_sizes: l.tiles.as_ref().map(|t| t.tile_sizes.clone()).unwrap_or_default(),
                    tile_scales: l.tiles.map(|t| t.scales).unwrap_or_default(),
                    appearance_scales: l.appearances.map(|a| a.scales).unwrap_or_default(),
                    max_render_pixels: l.max_render_pixels,
                    background: l.background,
                    enforced: l.enforced,
                },
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Enum)]
pub enum RenderViewport {
    /// `scale` device pixels a point; `None` is 1.
    Scale { scale: Option<f64> },
    /// Exact output width in device pixels.
    Width { width: f64 },
}

impl From<PageRenderViewport> for RenderViewport {
    fn from(v: PageRenderViewport) -> Self {
        match v {
            PageRenderViewport::Scale { scale } => RenderViewport::Scale { scale },
            PageRenderViewport::Width { width } => RenderViewport::Width { width },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct RenderPaintSettings {
    /// Tile arrival cross-fade in ms; 0 is a hard pop.
    pub fade_ms: f64,
    /// Whether the tile plane is on.
    pub tiles: bool,
}

/// What a view wants of a page: device pixels across its unrotated width,
/// the visible region (page points; `None` is the whole page), and scroll
/// velocity for prefetch direction.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct RenderPageViewDemand {
    pub desired_device_width: f64,
    #[uniffi(default = None)]
    pub visible_rect: Option<Rect>,
    #[uniffi(default = None)]
    pub velocity_x: Option<f64>,
    #[uniffi(default = None)]
    pub velocity_y: Option<f64>,
}

impl From<RenderPageViewDemand> for PageViewDemand {
    fn from(d: RenderPageViewDemand) -> Self {
        PageViewDemand {
            desired_device_width: d.desired_device_width,
            visible_rect: d.visible_rect.map(Into::into),
            velocity: match (d.velocity_x, d.velocity_y) {
                (None, None) => None,
                (dx, dy) => Some(Velocity { dx: dx.unwrap_or(0.0), dy: dy.unwrap_or(0.0) }),
            },
        }
    }
}

/// One tile to keep on screen. Fetch its pixels with `tile_image`.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RenderTileSource {
    pub key: String,
    pub scale: f64,
    /// Placement, y-down page points.
    pub rect: Rect,
    /// Stacking: coarse under fine.
    pub z: f64,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RenderTilePlan {
    pub engaged: bool,
    pub paint: Vec<RenderTileSource>,
    pub fetching: Vec<String>,
    pub stamp: String,
}

impl From<&TilePaintPlan> for RenderTilePlan {
    fn from(p: &TilePaintPlan) -> Self {
        RenderTilePlan {
            engaged: p.engaged,
            paint: p
                .paint
                .iter()
                .map(|s| RenderTileSource { key: s.key.clone(), scale: s.scale, rect: s.rect.into(), z: s.z })
                .collect(),
            fetching: p.fetching.clone(),
            stamp: p.stamp.clone(),
        }
    }
}
```

Check `crate::paint_plan`'s `Rect` type: `Into<epdf_ffi::Rect>` exists for both `epdf_geometry::Rect` and `epdf_kernel::geometry::Rect`. If `RenderSourceOptions` and `ViewDemand` aren't re-exported at the crate root, import them from `crate::host_contract` and `crate::controller`.

- [ ] **Step 4: Write the methods and the view-demand handle**

Add to `#[uniffi::export] impl RenderHandle`:

```rust
    pub async fn render_policy(&self) -> Result<RenderPolicy, ViewerError> {
        self.bound.call(|b| Ok(open(b)?.get_render_policy().into())).await
    }

    /// The viewer door: render at the scale a view shows the page,
    /// conformed through the policy's render points.
    pub async fn render_source(
        &self,
        page: PageRef,
        scale: f64,
        include_annotations: Option<bool>,
        cancel: Arc<CancelToken>,
    ) -> Result<RenderedImage, ViewerError> {
        self.bound
            .call_promise_cancellable(
                cancel,
                move |b| {
                    let cap = open(b)?;
                    let (signal, abort) = abortable();
                    let image = cap.render_source(
                        page.into(),
                        RenderSourceOptions { scale, include_annotations, signal: Some(signal) },
                    );
                    Ok(rendered(cap, &b.pixels, page, image)?.with_abort(abort))
                },
                Raster::owned,
            )
            .await
    }

    /// The identity of the raster `render_source` would produce.
    pub async fn source_key(
        &self,
        page: PageRef,
        scale: f64,
        include_annotations: Option<bool>,
    ) -> Result<String, ViewerError> {
        self.bound.call(move |b| open(b)?.get_source_key(page.into(), scale, include_annotations)).await
    }

    pub async fn conform_viewport(&self, page: PageRef, scale: f64) -> Result<RenderViewport, ViewerError> {
        self.bound.call(move |b| Ok(open(b)?.conform_viewport(page.into(), scale)?.into())).await
    }

    pub async fn paint_settings(&self) -> Result<RenderPaintSettings, ViewerError> {
        self.bound
            .call(|b| {
                let p = open(b)?.get_paint_settings();
                Ok(RenderPaintSettings { fade_ms: p.fade_ms, tiles: p.tiles })
            })
            .await
    }

    /// This view's tile surface. The same id answers the same surface;
    /// pair each with `dispose`.
    pub async fn create_view_demand(&self, view_id: String) -> Result<Arc<RenderViewDemandHandle>, ViewerError> {
        let post = self.post.clone();
        self.bound
            .call(move |b| {
                let view = open(b)?.create_view_demand(&view_id);
                Ok(Arc::new(RenderViewDemandHandle {
                    bound: KernelBound::new(
                        ViewBound { cap: b.cap.clone(), pixels: b.pixels.clone(), view },
                        post,
                    ),
                }))
            })
            .await
    }
```

Append after the `RenderHandle` impl:

```rust
struct ViewBound {
    cap: RenderCapability,
    pixels: PixelSource,
    view: ViewDemand,
}

impl ViewBound {
    fn open(&self) -> Result<&ViewDemand, Error> {
        open_cap(&self.cap)?;
        Ok(&self.view)
    }
}

#[derive(uniffi::Object)]
pub struct RenderViewDemandHandle {
    bound: KernelBound<ViewBound>,
}

#[uniffi::export]
impl RenderViewDemandHandle {
    pub async fn set_demand(
        &self,
        page: PageRef,
        demand: RenderPageViewDemand,
        include_annotations: Option<bool>,
    ) -> Result<(), ViewerError> {
        self.bound.call(move |b| b.open()?.set_demand(page.into(), demand.into(), include_annotations)).await
    }

    pub async fn plan(&self, page: PageRef) -> Result<RenderTilePlan, ViewerError> {
        self.bound.call(move |b| Ok(RenderTilePlan::from(&*b.open()?.get_plan(page.into())))).await
    }

    /// A planned tile's pixels, copied out; `None` for a key the current
    /// plan does not paint.
    pub async fn tile_image(&self, page: PageRef, key: String) -> Result<Option<RenderedImage>, ViewerError> {
        self.bound
            .call(move |b| {
                let plan = b.open()?.get_plan(page.into());
                let Some(source) = plan.paint.iter().find(|s| s.key == key) else { return Ok(None) };
                let page_width = b.cap.page_width(page.into())?;
                let mut raster = Raster::read(&source.handle, page_width, &b.pixels)?;
                raster.scale = source.scale;
                Ok(Some(raster.owned()))
            })
            .await
    }

    pub async fn mark_painted(&self, page: PageRef, key: String) -> Result<(), ViewerError> {
        self.bound
            .call(move |b| {
                b.open()?.mark_painted(page.into(), &key);
                Ok(())
            })
            .await
    }

    pub async fn mark_unpainted(&self, page: PageRef, key: String) -> Result<(), ViewerError> {
        self.bound
            .call(move |b| {
                b.open()?.mark_unpainted(page.into(), &key);
                Ok(())
            })
            .await
    }

    pub async fn release(&self, page: PageRef) -> Result<(), ViewerError> {
        self.bound
            .call(move |b| {
                b.open()?.release(page.into());
                Ok(())
            })
            .await
    }

    pub async fn dispose(&self) -> Result<(), ViewerError> {
        self.bound
            .call(|b| {
                b.open()?.dispose();
                Ok(())
            })
            .await
    }
}
```

`PixelSource` is the existing alias in `ffi.rs`; if it isn't `Clone`, it's an `Rc<dyn Fn>` and `.clone()` is fine. `RenderCapability` derives `Clone` (it's an `Rc` newtype).

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p epdf-plugin-render --features uniffi --test ffi`
Expected: PASS, every existing render test plus the 5 new ones.

- [ ] **Step 6: Commit**

```bash
git add crates/plugins/render
git commit -m "Export render's viewport members and a view-demand handle"
```

### Task 6: Host wiring and cross-plugin tests

**Files:**
- Modify: `crates/epdf-viewer-host/Cargo.toml`, `crates/epdf-viewer-host/src/api.rs`, `crates/epdf-viewer-host/src/session.rs`, `crates/epdf-viewer-host/src/lib.rs`, `crates/epdf-viewer-host/tests/viewer_host.rs`

**Interfaces:**
- Consumes: `StageHandle::new`, `StageConfig::into_config`; plan 01's `reporter(&Kernel)`.
- Produces: `ViewerConfig.stage: Option<StageConfig>`; `ViewerHost::stage(document_id: String) -> Result<Arc<StageHandle>, ViewerError>`.

- [ ] **Step 1: Write the failing tests**

Append to `crates/epdf-viewer-host/tests/viewer_host.rs`:

```rust
use epdf_ffi::Size as FfiSize;
use epdf_plugin_stage::ffi::{StageConfig, StageGoToOptions, StageScrollBehavior};

fn with(config: ViewerConfig) -> Arc<ViewerHost> {
    let host = ViewerHost::new(config, Arc::new(Delegate)).unwrap();
    block_on(host.start()).unwrap();
    host
}

#[test]
fn the_stage_plugin_lays_out_and_navigates_through_the_host() {
    let host = with(ViewerConfig { stage: Some(StageConfig::default()), ..Default::default() });
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let stage = block_on(host.stage(doc.id)).unwrap();
    block_on(stage.set_viewport_size(FfiSize { width: 390.0, height: 844.0 })).unwrap();
    assert!(!block_on(stage.visible_pages()).unwrap().is_empty());
    let instant = StageGoToOptions { behavior: Some(StageScrollBehavior::Instant), ..Default::default() };
    block_on(stage.go_to_page_index(5, Some(instant))).unwrap();
    assert_eq!(block_on(stage.current_page_index()).unwrap(), 5);
}

#[test]
fn revealing_the_active_hit_moves_the_stage() {
    let host = with(ViewerConfig {
        stage: Some(StageConfig { frames: false, ..Default::default() }),
        search: Some(SearchConfig::default()),
        ..Default::default()
    });
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let stage = block_on(host.stage(doc.id.clone())).unwrap();
    block_on(stage.set_viewport_size(FfiSize { width: 390.0, height: 844.0 })).unwrap();
    let search = block_on(host.search(doc.id)).unwrap();
    let query = SearchQuery { text: "the".into(), ..Default::default() };
    block_on(search.search(query, None, CancelToken::new())).unwrap();
    let before = block_on(stage.camera()).unwrap();
    let mut moved = false;
    // Walk hits until one is off screen; revealing it must move the camera.
    for _ in 0..50 {
        let hit = block_on(search.next_hit(None)).unwrap().unwrap();
        let after = block_on(stage.camera()).unwrap();
        if after != before {
            assert!(block_on(stage.is_page_visible(hit.page)).unwrap());
            moved = true;
            break;
        }
    }
    assert!(moved, "no hit moved the stage");
}

#[test]
fn a_pan_tool_drag_scrolls_the_stage() {
    use epdf_ffi::{Modifiers, Point, PointerKind, PointerPageHit, PointerPhase, PointerSample};
    let host = with(ViewerConfig {
        stage: Some(StageConfig { frames: false, ..Default::default() }),
        interaction: true,
        ..Default::default()
    });
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let pages = block_on(host.pages(doc.id.clone())).unwrap();
    let stage = block_on(host.stage(doc.id.clone())).unwrap();
    block_on(stage.set_viewport_size(FfiSize { width: 390.0, height: 844.0 })).unwrap();
    let interaction = block_on(host.interaction(doc.id)).unwrap();
    block_on(interaction.activate_tool("pan".into())).unwrap();
    let lens = block_on(stage.lens_id()).unwrap();
    let at = |phase, y: f64| PointerSample {
        phase,
        viewport: Point { x: 200.0, y },
        page: Some(PointerPageHit { page: pages[0].page, point: Point { x: 100.0, y }, scale: None, rotation: None, zoom: None }),
        modifiers: Modifiers::default(),
        click_count: None,
        pointer_type: Some(PointerKind::Touch),
        gesture: None,
        source: Some(lens.clone()),
    };
    let before = block_on(stage.camera()).unwrap();
    interaction.dispatch_pointer(at(PointerPhase::Down, 600.0));
    interaction.dispatch_pointer(at(PointerPhase::Move, 400.0));
    interaction.dispatch_pointer(at(PointerPhase::Up, 400.0));
    let after = block_on(stage.camera()).unwrap();
    assert!(after.y != before.y, "{before:?} -> {after:?}");
}

#[test]
fn without_its_config_the_stage_plugin_is_not_installed() {
    let host = started();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    assert!(block_on(host.stage(doc.id)).is_err());
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p epdf-viewer-host --test viewer_host stage`
Expected: FAIL to compile, "no field `stage` on type `ViewerConfig`".

- [ ] **Step 3: Wire it**

`crates/epdf-viewer-host/Cargo.toml`, alphabetical:

```toml
epdf-plugin-stage = { path = "../plugins/stage", default-features = false, features = ["uniffi"] }
```

`crates/epdf-viewer-host/src/api.rs`, import and field at the end of `ViewerConfig`:

```rust
use epdf_plugin_stage::ffi::StageConfig;
```

```rust
    /// Install `stagePlugin({ config })`: the viewport lens, one per
    /// document. Search reveals hits through it when both are installed.
    #[uniffi(default = None)]
    pub stage: Option<StageConfig>,
```

`crates/epdf-viewer-host/src/session.rs`, after the feedback block:

```rust
        if let Some(stage) = config.stage {
            // A config that does not convert ends the build, and the worker
            // goes with it, as a failed `Kernel::new` does below.
            let config = match stage.into_config() {
                Ok(config) => config,
                Err(error) => {
                    worker.join();
                    return Err(error);
                }
            };
            plugins.push(epdf_plugin_stage::stage_plugin(epdf_plugin_stage::StagePluginOptions {
                config,
                ..Default::default()
            }));
        }
```

`crates/epdf-viewer-host/src/lib.rs`, `use epdf_plugin_stage::ffi::StageHandle;` and in the exported impl:

```rust
    /// The viewport lens of an open document. Needs `ViewerConfig.stage`.
    pub async fn stage(&self, document_id: String) -> Result<Arc<StageHandle>, ViewerError> {
        let post = self.post.clone();
        self.call(move |s| {
            let cap = s.kernel.capability(&epdf_plugin_stage::STAGE, Some(&document_id))?;
            Ok(StageHandle::new(cap, post, reporter(&s.kernel)))
        })
        .await
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p epdf-viewer-host`
Expected: PASS, the four new tests and every existing one.

- [ ] **Step 5: Commit**

```bash
git add crates/epdf-viewer-host
git commit -m "Wire the stage plugin into the viewer host"
```

### Task 7: Swift `StageCapability` and render's viewport members

**Files:**
- Create: `platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Stage.swift`
- Modify: `platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Render.swift`

**Interfaces:**
- Consumes: generated `StageHandle`, `RenderViewDemandHandle`, Task 2 to 5 records.
- Produces: `StageCapability` and `PDFViewer.stage(for:)`; `StageInitialViewProvider` typealias; on `RenderCapability`: `renderPolicy()`, `renderSource(_:scale:includeAnnotations:)`, `sourceKey(_:scale:includeAnnotations:)`, `conformViewport(_:scale:)`, `paintSettings()`, `viewDemand(_:)`; `RenderViewDemand` (wraps `RenderViewDemandHandle`).

- [ ] **Step 1: Write `Stage.swift`**

```swift
public import EmbedPDFBindings

public typealias Camera = EmbedPDFBindings.Camera
public typealias ZoomMode = EmbedPDFBindings.ZoomMode
public typealias ZoomSpec = EmbedPDFBindings.ZoomSpec
public typealias ViewAnchor = EmbedPDFBindings.ViewAnchor
public typealias PageFrameInsets = EmbedPDFBindings.PageFrameInsets
public typealias StageAlign = EmbedPDFBindings.StageAlign
public typealias StageAlignValue = EmbedPDFBindings.StageAlignValue
public typealias StageAlignment = EmbedPDFBindings.StageAlignment
public typealias StageAlignmentValue = EmbedPDFBindings.StageAlignmentValue
public typealias StageDirection = EmbedPDFBindings.StageDirection
public typealias StageSpreadMode = EmbedPDFBindings.StageSpreadMode
public typealias StageSizingMode = EmbedPDFBindings.StageSizingMode
public typealias StageScrollMetrics = EmbedPDFBindings.StageScrollMetrics
public typealias StageConfig = EmbedPDFBindings.StageConfig
public typealias StageFlow = EmbedPDFBindings.StageFlow
public typealias StageLayout = EmbedPDFBindings.StageLayout
public typealias StageScrollBehavior = EmbedPDFBindings.StageScrollBehavior
public typealias StageGridColumns = EmbedPDFBindings.StageGridColumns
public typealias StageGap = EmbedPDFBindings.StageGap
public typealias StageArrivalAlignValue = EmbedPDFBindings.StageArrivalAlignValue
public typealias StageArrivalAlignment = EmbedPDFBindings.StageArrivalAlignment
public typealias StagePartialArrivalAlignment = EmbedPDFBindings.StagePartialArrivalAlignment
public typealias StageSettings = EmbedPDFBindings.StageSettings
public typealias StageSettingsPatch = EmbedPDFBindings.StageSettingsPatch
public typealias StageSettingKey = EmbedPDFBindings.StageSettingKey
public typealias StageOrientation = EmbedPDFBindings.StageOrientation
public typealias StageBoxQuery = EmbedPDFBindings.StageBoxQuery
public typealias StageResponsiveRule = EmbedPDFBindings.StageResponsiveRule
public typealias StagePageTransform = EmbedPDFBindings.StagePageTransform
public typealias StageVisiblePage = EmbedPDFBindings.StageVisiblePage
public typealias StageViewpoint = EmbedPDFBindings.StageViewpoint
public typealias StageViewState = EmbedPDFBindings.StageViewState
public typealias StageScrollToOptions = EmbedPDFBindings.StageScrollToOptions
public typealias StageGoToOptions = EmbedPDFBindings.StageGoToOptions
public typealias StageRevealWord = EmbedPDFBindings.StageRevealWord
public typealias StageRevealAnchorValue = EmbedPDFBindings.StageRevealAnchorValue
public typealias StageRevealAnchor = EmbedPDFBindings.StageRevealAnchor
public typealias StageRevealZoom = EmbedPDFBindings.StageRevealZoom
public typealias StageRevealOptions = EmbedPDFBindings.StageRevealOptions
public typealias StagePageHit = EmbedPDFBindings.StagePageHit
public typealias StageZoomMode = EmbedPDFBindings.StageZoomMode
public typealias StageZoomTarget = EmbedPDFBindings.StageZoomTarget
public typealias StageInitialViewProvider = EmbedPDFBindings.StageInitialViewProvider
public typealias StageEvent = EmbedPDFBindings.StageEvent
public typealias StagePageChangedEvent = EmbedPDFBindings.StagePageChangedEvent
public typealias StageZoomChangedEvent = EmbedPDFBindings.StageZoomChangedEvent
public typealias StageCameraChangedEvent = EmbedPDFBindings.StageCameraChangedEvent
public typealias StageMotionEndedEvent = EmbedPDFBindings.StageMotionEndedEvent
public typealias StageSettingsChangedEvent = EmbedPDFBindings.StageSettingsChangedEvent
public typealias StageViewportChangedEvent = EmbedPDFBindings.StageViewportChangedEvent

/// A document's viewport lens: camera, zoom, scroll, layout and
/// navigation. Get one with `PDFViewer.stage(for:)`; the viewer needs
/// `ViewerConfig(stage: StageConfig())`. It keeps the viewer alive. After
/// its document closes, every call throws `instance-closed`.
///
/// `setCamera`, `panBy`, `scrollBy` and `zoomAround` return at once and run
/// in order; their errors arrive on `PDFViewer.reportedErrors`.
public struct StageCapability: Sendable {
  let handle: StageHandle
  let viewer: PDFViewer

  // MARK: Read

  public func camera() async throws(PDFError) -> Camera { try await bridged { try await handle.camera() } }
  public func viewportSize() async throws(PDFError) -> Size { try await bridged { try await handle.viewportSize() } }
  public func scrollMetrics() async throws(PDFError) -> StageScrollMetrics { try await bridged { try await handle.scrollMetrics() } }
  public func visiblePages() async throws(PDFError) -> [StageVisiblePage] { try await bridged { try await handle.visiblePages() } }
  public func isPageVisible(_ page: PageRef) async throws(PDFError) -> Bool { try await bridged { try await handle.isPageVisible(page: page) } }
  public func currentPageIndex() async throws(PDFError) -> UInt32 { try await bridged { try await handle.currentPageIndex() } }
  public func currentPage() async throws(PDFError) -> PageInfo? { try await bridged { try await handle.currentPage() } }
  public func currentItemPages() async throws(PDFError) -> [PageInfo] { try await bridged { try await handle.currentItemPages() } }
  public func pageFrame(_ page: PageRef) async throws(PDFError) -> StageVisiblePage? { try await bridged { try await handle.pageFrame(page: page) } }
  public func page(at point: Point) async throws(PDFError) -> StagePageHit? { try await bridged { try await handle.pageAt(point: point) } }
  public func viewportToPage(_ page: PageRef, _ point: Point) async throws(PDFError) -> Point? { try await bridged { try await handle.viewportToPage(page: page, point: point) } }
  public func pageToWorld(_ page: PageRef, _ point: Point) async throws(PDFError) -> Point? { try await bridged { try await handle.pageToWorld(page: page, point: point) } }
  public func pageRectToViewport(_ page: PageRef, _ rect: Rect) async throws(PDFError) -> Rect? { try await bridged { try await handle.pageRectToViewport(page: page, rect: rect) } }
  public func worldToViewport(_ point: Point) async throws(PDFError) -> Point { try await bridged { try await handle.worldToViewport(point: point) } }
  public func viewportToWorld(_ point: Point) async throws(PDFError) -> Point { try await bridged { try await handle.viewportToWorld(point: point) } }
  public func pageToViewport(_ page: PageRef, _ point: Point) async throws(PDFError) -> Point? { try await bridged { try await handle.pageToViewport(page: page, point: point) } }
  public func viewRotation() async throws(PDFError) -> Double { try await bridged { try await handle.viewRotation() } }
  public func zoomLevel() async throws(PDFError) -> Double { try await bridged { try await handle.zoomLevel() } }
  public func zoomMode() async throws(PDFError) -> StageZoomMode { try await bridged { try await handle.zoomMode() } }
  public func viewpoint() async throws(PDFError) -> StageViewpoint { try await bridged { try await handle.viewpoint() } }

  // MARK: Motion

  public func fling(velocityX: Double, velocityY: Double) async throws(PDFError) { try await bridged { try await handle.fling(velocityX: velocityX, velocityY: velocityY) } }
  public func isMoving() async throws(PDFError) -> Bool { try await bridged { try await handle.isMoving() } }
  public func stopMotion() async throws(PDFError) { try await bridged { try await handle.stopMotion() } }
  public func setCamera(_ camera: Camera) { handle.setCamera(camera: camera) }
  public func panBy(dx: Double, dy: Double) { handle.panBy(dx: dx, dy: dy) }
  public func scrollTo(_ options: StageScrollToOptions) async throws(PDFError) { try await bridged { try await handle.scrollTo(options: options) } }
  public func scrollBy(_ options: StageScrollToOptions) { handle.scrollBy(options: options) }
  public func zoomAround(_ point: Point, factor: Double) { handle.zoomAround(point: point, factor: factor) }
  public func beginGesture(elastic: Bool? = nil) async throws(PDFError) { try await bridged { try await handle.beginGesture(elastic: elastic) } }
  public func endGesture() async throws(PDFError) { try await bridged { try await handle.endGesture() } }

  // MARK: Navigation

  public func goToPageIndex(_ index: UInt32, options: StageGoToOptions? = nil) async throws(PDFError) { try await bridged { try await handle.goToPageIndex(index: index, options: options) } }
  public func goToPage(_ page: PageRef, options: StageGoToOptions? = nil) async throws(PDFError) { try await bridged { try await handle.goToPage(page: page, options: options) } }
  public func goToFirstPage(options: StageGoToOptions? = nil) async throws(PDFError) { try await bridged { try await handle.goToFirstPage(options: options) } }
  public func goToLastPage(options: StageGoToOptions? = nil) async throws(PDFError) { try await bridged { try await handle.goToLastPage(options: options) } }
  public func canGoNext() async throws(PDFError) -> Bool { try await bridged { try await handle.canGoNext() } }
  public func canGoPrevious() async throws(PDFError) -> Bool { try await bridged { try await handle.canGoPrevious() } }
  public func nextPage(options: StageGoToOptions? = nil) async throws(PDFError) { try await bridged { try await handle.nextPage(options: options) } }
  public func previousPage(options: StageGoToOptions? = nil) async throws(PDFError) { try await bridged { try await handle.previousPage(options: options) } }
  public func resetView() async throws(PDFError) { try await bridged { try await handle.resetView() } }
  public func reveal(_ page: PageRef, options: StageRevealOptions? = nil) async throws(PDFError) { try await bridged { try await handle.reveal(page: page, options: options) } }
  public func revealRect(_ page: PageRef, _ rect: Rect, options: StageRevealOptions? = nil) async throws(PDFError) { try await bridged { try await handle.revealRect(page: page, rect: rect, options: options) } }
  public func revealIndex(_ index: UInt32, options: StageRevealOptions? = nil) async throws(PDFError) { try await bridged { try await handle.revealIndex(index: index, options: options) } }

  // MARK: Settings

  public func settings() async throws(PDFError) -> StageSettings { try await bridged { try await handle.settings() } }
  public func resetSettings() async throws(PDFError) { try await bridged { try await handle.resetSettings() } }
  public func updateSettings(_ patch: StageSettingsPatch) async throws(PDFError) { try await bridged { try await handle.updateSettings(patch: patch) } }
  public func setResponsiveRules(_ rules: [StageResponsiveRule]) async throws(PDFError) { try await bridged { try await handle.setResponsiveRules(rules: rules) } }
  public func matchesRule(_ name: String) async throws(PDFError) -> Bool { try await bridged { try await handle.matchesRule(name: name) } }
  public func activeRules() async throws(PDFError) -> [String] { try await bridged { try await handle.activeRules() } }
  public func setFlow(_ flow: StageFlow) async throws(PDFError) { try await bridged { try await handle.setFlow(flow: flow) } }
  public func setLayout(_ layout: StageLayout) async throws(PDFError) { try await bridged { try await handle.setLayout(layout: layout) } }
  public func setSpread(_ spread: StageSpreadMode) async throws(PDFError) { try await bridged { try await handle.setSpread(spread: spread) } }
  public func setSizing(_ sizing: StageSizingMode) async throws(PDFError) { try await bridged { try await handle.setSizing(sizing: sizing) } }
  public func applyViewState(_ view: StageViewState) async throws(PDFError) { try await bridged { try await handle.applyViewState(view: view) } }

  // MARK: Zoom

  public func doubleTapZoom(_ point: Point) async throws(PDFError) { try await bridged { try await handle.doubleTapZoom(point: point) } }
  public func zoomIn() async throws(PDFError) { try await bridged { try await handle.zoomIn() } }
  public func zoomOut() async throws(PDFError) { try await bridged { try await handle.zoomOut() } }
  public func zoomTo(_ target: StageZoomTarget, around: Point? = nil) async throws(PDFError) { try await bridged { try await handle.zoomTo(target: target, around: around) } }
  public func zoomBy(_ factor: Double, around: Point? = nil) async throws(PDFError) { try await bridged { try await handle.zoomBy(factor: factor, around: around) } }
  public func fitWidth() async throws(PDFError) { try await bridged { try await handle.fitWidth() } }
  public func fitPage() async throws(PDFError) { try await bridged { try await handle.fitPage() } }
  public func fitAll() async throws(PDFError) { try await bridged { try await handle.fitAll() } }
  public func fitAutomatic() async throws(PDFError) { try await bridged { try await handle.fitAutomatic() } }
  public func setViewRotation(_ degrees: Double) async throws(PDFError) { try await bridged { try await handle.setViewRotation(degrees: degrees) } }
  public func rotateViewBy(_ delta: Double) async throws(PDFError) { try await bridged { try await handle.rotateViewBy(delta: delta) } }

  // MARK: Lifecycle

  public func setViewportSize(_ size: Size) async throws(PDFError) { try await bridged { try await handle.setViewportSize(size: size) } }
  public func setDevicePixelRatio(_ ratio: Double) async throws(PDFError) { try await bridged { try await handle.setDevicePixelRatio(ratio: ratio) } }
  public func refit() async throws(PDFError) { try await bridged { try await handle.refit() } }
  public func viewState() async throws(PDFError) -> StageViewState { try await bridged { try await handle.viewState() } }
  public func lensId() async throws(PDFError) -> String { try await bridged { try await handle.lensId() } }

  /// `provider` is asked on the viewer's kernel thread when the stage
  /// first places this document.
  public func provideInitialView(_ provider: some StageInitialViewProvider, priority: Double = 0)
    async throws(PDFError) -> Registration
  {
    Registration(handle: try await bridged { try await handle.provideInitialView(priority: priority, provider: provider) })
  }

  public func placeInitial() async throws(PDFError) { try await bridged { try await handle.placeInitial() } }

  /// Finishes when the document closes (at once if it already has) or the
  /// viewer is destroyed.
  public var events: AsyncStream<StageEvent> {
    viewer.registry.stream { handle.subscribe(listener: $0) }
  }
}

extension EventRelay: StageEventListener where Event == StageEvent {
  func onEvent(event: StageEvent) { continuation.yield(event) }
  func onEnd() { continuation.finish() }
}

extension PDFViewer {
  public func stage(for document: PDFDocument) async throws(PDFError) -> StageCapability {
    let handle = try await bridged { try await host.stage(documentId: document.id) }
    return StageCapability(handle: handle, viewer: self)
  }
}
```

The one-line method style here is a deliberate exception to `Search.swift`'s multi-line style: 70 members each a single `bridged` call read better aligned than spread over 280 lines. Run `swift-format` if the repo has a config that rejects it; it doesn't at the time of writing (`platforms/apple` has no `.swift-format`).

- [ ] **Step 2: Add the viewport members to `Render.swift`**

Add typealiases at the top:

```swift
public typealias RenderPolicy = EmbedPDFBindings.RenderPolicy
public typealias RenderLatticePolicy = EmbedPDFBindings.RenderLatticePolicy
public typealias RenderViewport = EmbedPDFBindings.RenderViewport
public typealias RenderPaintSettings = EmbedPDFBindings.RenderPaintSettings
public typealias RenderPageViewDemand = EmbedPDFBindings.RenderPageViewDemand
public typealias RenderTileSource = EmbedPDFBindings.RenderTileSource
public typealias RenderTilePlan = EmbedPDFBindings.RenderTilePlan
```

Add to `RenderCapability`. `renderedPage(_:)` is the existing private helper in `Render.swift` that turns a `RenderedImage` into a `RenderedPage` with a `CGImage`; if it's named differently, use that name.

```swift
  public func renderPolicy() async throws(PDFError) -> RenderPolicy {
    try await bridged { try await handle.renderPolicy() }
  }

  /// Render at the scale a view shows the page (a `StagePageTransform`'s
  /// `renderScale`), conformed through the policy's render points.
  public func renderSource(_ page: PageRef, scale: Double, includeAnnotations: Bool? = nil)
    async throws(PDFError) -> RenderedPage
  {
    let image = try await bridgedCancellable { token in
      try await handle.renderSource(page: page, scale: scale, includeAnnotations: includeAnnotations, cancel: token)
    }
    return try renderedPage(image)
  }

  public func sourceKey(_ page: PageRef, scale: Double, includeAnnotations: Bool? = nil) async throws(PDFError) -> String {
    try await bridged { try await handle.sourceKey(page: page, scale: scale, includeAnnotations: includeAnnotations) }
  }

  public func conformViewport(_ page: PageRef, scale: Double) async throws(PDFError) -> RenderViewport {
    try await bridged { try await handle.conformViewport(page: page, scale: scale) }
  }

  public func paintSettings() async throws(PDFError) -> RenderPaintSettings {
    try await bridged { try await handle.paintSettings() }
  }

  /// A view's tile surface. Pair with `dispose()`.
  public func viewDemand(_ viewId: String) async throws(PDFError) -> RenderViewDemand {
    RenderViewDemand(handle: try await bridged { try await handle.createViewDemand(viewId: viewId) }, render: self)
  }
```

And after `RenderCapability`:

```swift
/// One view's tile plane over a document's pages.
public struct RenderViewDemand: Sendable {
  let handle: RenderViewDemandHandle
  let render: RenderCapability

  public func setDemand(_ page: PageRef, _ demand: RenderPageViewDemand, includeAnnotations: Bool? = nil) async throws(PDFError) {
    try await bridged { try await handle.setDemand(page: page, demand: demand, includeAnnotations: includeAnnotations) }
  }

  public func plan(_ page: PageRef) async throws(PDFError) -> RenderTilePlan {
    try await bridged { try await handle.plan(page: page) }
  }

  /// `nil` for a key the current plan doesn't paint.
  public func tileImage(_ page: PageRef, key: String) async throws(PDFError) -> RenderedPage? {
    guard let image = try await bridged({ try await handle.tileImage(page: page, key: key) }) else { return nil }
    return try render.renderedPage(image)
  }

  public func markPainted(_ page: PageRef, key: String) async throws(PDFError) { try await bridged { try await handle.markPainted(page: page, key: key) } }
  public func markUnpainted(_ page: PageRef, key: String) async throws(PDFError) { try await bridged { try await handle.markUnpainted(page: page, key: key) } }
  public func release(_ page: PageRef) async throws(PDFError) { try await bridged { try await handle.release(page: page) } }
  public func dispose() async throws(PDFError) { try await bridged { try await handle.dispose() } }
}
```

If the existing CGImage conversion in `Render.swift` is inline in `renderPage` rather than a helper, extract it into `func renderedPage(_ image: RenderedImage) throws(PDFError) -> RenderedPage` in the same commit and call it from `renderPage` too.

- [ ] **Step 3: Build**

Run: `bash platforms/apple/build-xcframework.sh && (cd platforms/apple/EmbedPDF && xcodebuild build -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17')`
Expected: BUILD SUCCEEDED.

- [ ] **Step 4: Commit**

```bash
git add platforms/apple/EmbedPDF/Sources/EmbedPDF
git commit -m "Add the Swift stage capability and render's viewport members"
```

### Task 8: Swift tests for stage and render's viewport members

**Files:**
- Create: `platforms/apple/EmbedPDF/Tests/EmbedPDFTests/StageTests.swift`, `platforms/apple/EmbedPDF/Tests/EmbedPDFTests/RenderViewportTests.swift`

- [ ] **Step 1: Write `StageTests.swift`**

```swift
import CustomDump
import EmbedPDF
import Testing

final class SavedView: StageInitialViewProvider, @unchecked Sendable {
  let view: StageViewState
  init(_ view: StageViewState) { self.view = view }
  func initialView() throws -> StageViewState? { view }
}

extension BaseSuite {
  @Suite struct StageTests {
    let instant = StageGoToOptions(behavior: .instant)

    func opened(frames: Bool = false) async throws -> (PDFViewer, PDFDocument, StageCapability) {
      let viewer = try PDFViewer(ViewerConfig(stage: StageConfig(frames: frames)))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      let stage = try await viewer.stage(for: document)
      try await stage.setViewportSize(Size(width: 390, height: 844))
      return (viewer, document, stage)
    }

    @Test func aSizedStageShowsTheFirstPage() async throws {
      let (_, _, stage) = try await opened()
      let visible = try await stage.visiblePages()
      expectNoDifference(visible.first?.pageIndex, 0)
      #expect(visible.first.map { $0.transform.renderScale > 0 } == true)
    }

    @Test func navigationIsObserved() async throws {
      let (_, _, stage) = try await opened()
      let events = stage.events
      try await stage.goToPageIndex(5, options: instant)
      for await event in events {
        guard case let .pageChanged(event) = event else { continue }
        expectNoDifference(event.pageIndex, 5)
        break
      }
      expectNoDifference(try await stage.currentPageIndex(), 5)
    }

    @Test func fitWidthThenZoomIn() async throws {
      let (_, _, stage) = try await opened()
      try await stage.fitWidth()
      expectNoDifference(try await stage.zoomMode(), .mode(mode: .fitWidth))
      let fitted = try await stage.zoomLevel()
      try await stage.zoomIn()
      #expect(try await stage.zoomLevel() > fitted)
    }

    @Test func aPanGestureMovesTheCamera() async throws {
      let (_, _, stage) = try await opened()
      let before = try await stage.camera()
      try await stage.beginGesture()
      for _ in 0..<20 { stage.panBy(dx: 0, dy: -10) }
      try await stage.endGesture()
      let after = try await stage.camera()
      #expect(after.y != before.y)
    }

    @Test func aSavedViewRestoresOnTheNextOpen() async throws {
      let (viewer, _, stage) = try await opened()
      try await stage.goToPageIndex(7, options: instant)
      let saved = try await stage.viewState()
      let second = try await viewer.open(Fixture.data("report"))
      let next = try await viewer.stage(for: second)
      _ = try await next.provideInitialView(SavedView(saved), priority: 10)
      try await next.setViewportSize(Size(width: 390, height: 844))
      expectNoDifference(try await next.currentPageIndex(), 7)
    }

    @Test func aFlingMovesThenSettles() async throws {
      let (_, _, stage) = try await opened(frames: true)
      let events = stage.events
      try await stage.fling(velocityX: 0, velocityY: -3000)
      #expect(try await stage.isMoving())
      for await event in events {
        if case .motionEnded = event { break }
      }
      #expect(try await stage.isMoving() == false)
    }

    @Test func aClosedDocumentsCapabilityThrows() async throws {
      let (viewer, document, stage) = try await opened()
      try await viewer.close(document)
      await #expect {
        _ = try await stage.zoomLevel()
      } throws: { error in
        (error as? PDFError)?.code == "instance-closed"
      }
    }

    @Test func droppingTheStreamUnsubscribes() async throws {
      let (_, _, stage) = try await opened()
      let task = Task { for await _ in stage.events {} }
      try await Task.sleep(for: .milliseconds(20))
      task.cancel()
      _ = await task.value
      try await stage.zoomIn()
    }
  }
}
```

No stage member takes a `CancelToken`, so there's no cancellation test for it.

- [ ] **Step 2: Write `RenderViewportTests.swift`**

```swift
import CustomDump
import EmbedPDF
import Testing

extension BaseSuite {
  @Suite struct RenderViewportTests {
    func opened() async throws -> (PDFViewer, PDFDocument, RenderCapability, PageRef) {
      let viewer = try PDFViewer(ViewerConfig(render: RenderConfig()))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("colors"))
      let page = try await viewer.pages(of: document)[0].page
      return (viewer, document, try await viewer.render(in: document), page)
    }

    @Test func renderSourceMatchesItsSourceKey() async throws {
      let (_, _, render, page) = try await opened()
      let a = try await render.sourceKey(page, scale: 2)
      let b = try await render.sourceKey(page, scale: 2, includeAnnotations: true)
      expectNoDifference(a, b)
      let rendered = try await render.renderSource(page, scale: 2)
      #expect(rendered.width > 0)
    }

    @Test func renderSourceCancelsWhenTheTaskIsCancelled() async throws {
      let (_, _, render, page) = try await opened()
      let task = Task { try await render.renderSource(page, scale: 8) }
      task.cancel()
      do {
        _ = try await task.value
      } catch {
        expectNoDifference((error as? PDFError)?.code, "operation-cancelled")
      }
      _ = try await render.renderSource(page, scale: 1)
    }

    @Test func aViewDemandPlansAndDisposes() async throws {
      let (_, _, render, page) = try await opened()
      let paint = try await render.paintSettings()
      let view = try await render.viewDemand("main")
      try await view.setDemand(page, RenderPageViewDemand(desiredDeviceWidth: 200))
      expectNoDifference(try await view.plan(page).engaged, paint.tiles)
      try await view.dispose()
    }

    @Test func aClosedDocumentsViewDemandThrows() async throws {
      let (viewer, document, render, page) = try await opened()
      let view = try await render.viewDemand("main")
      try await viewer.close(document)
      await #expect {
        _ = try await view.plan(page)
      } throws: { error in
        (error as? PDFError)?.code == "instance-closed"
      }
    }
  }
}
```

- [ ] **Step 3: Run them**

Run: `bash platforms/apple/build-xcframework.sh && (cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17' -only-testing:EmbedPDFTests)`
Expected: TEST SUCCEEDED, including 8 `StageTests` and 4 `RenderViewportTests`.

- [ ] **Step 4: Commit**

```bash
git add platforms/apple/EmbedPDF/Tests/EmbedPDFTests
git commit -m "Test the Swift stage capability and render's viewport members"
```

### Task 9: A SwiftUI scrolling reader in `EmbedPDFUI`

**Files:**
- Create: `platforms/apple/EmbedPDF/Sources/EmbedPDFUI/StageReaderModel.swift`, `platforms/apple/EmbedPDF/Sources/EmbedPDFUI/StageReaderView.swift`, `platforms/apple/EmbedPDF/Tests/EmbedPDFUITests/StageReaderModelTests.swift`

**Interfaces:**
- Consumes: `StageCapability`, `RenderCapability`, `PageImageView` (existing).
- Produces: `@MainActor @Observable final class StageReaderModel` with `init(stage:render:)`, read-only `visiblePages: [StageVisiblePage]`, `camera: Camera?`, `currentPageIndex: UInt32`, `error: PDFError?`, and methods `task() async`, `sizeChanged(_ size: CGSize, displayScale: CGFloat) async`, `dragChanged(translation: CGSize) async`, `dragEnded(velocity: CGSize) async`, `magnifyChanged(_ magnification: CGFloat, at location: CGPoint)`, `magnifyEnded() async`, `doubleTapped(at location: CGPoint) async`; `struct StageReaderView: View` with `init(model:)`.

- [ ] **Step 1: Write the failing model tests**

`platforms/apple/EmbedPDF/Tests/EmbedPDFUITests/StageReaderModelTests.swift`:

```swift
import CustomDump
import EmbedPDF
import EmbedPDFUI
import Foundation
import Testing

@MainActor
@Suite(.serialized, .timeLimit(.minutes(1)))
struct StageReaderModelTests {
  func model() async throws -> (StageReaderModel, Task<Void, Never>) {
    let viewer = try PDFViewer(ViewerConfig(render: RenderConfig(), stage: StageConfig(frames: false)))
    try await viewer.start()
    let url = try #require(Bundle.module.url(forResource: "report", withExtension: "pdf", subdirectory: "Fixtures"))
    let document = try await viewer.open(try Data(contentsOf: url))
    let model = StageReaderModel(
      stage: try await viewer.stage(for: document), render: try await viewer.render(in: document))
    let task = Task { await model.task() }
    return (model, task)
  }

  @Test func sizingShowsPages() async throws {
    let (model, task) = try await model()
    defer { task.cancel() }
    await model.sizeChanged(CGSize(width: 390, height: 844), displayScale: 3)
    #expect(!model.visiblePages.isEmpty)
    expectNoDifference(model.currentPageIndex, 0)
  }

  @Test func draggingScrollsAndUpdatesTheVisiblePages() async throws {
    let (model, task) = try await model()
    defer { task.cancel() }
    await model.sizeChanged(CGSize(width: 390, height: 844), displayScale: 3)
    let before = model.camera
    await model.dragChanged(translation: CGSize(width: 0, height: -400))
    await model.dragEnded(velocity: .zero)
    #expect(model.camera != before)
  }

  @Test func resizingKeepsTheCurrentPage() async throws {
    let (model, task) = try await model()
    defer { task.cancel() }
    await model.sizeChanged(CGSize(width: 390, height: 844), displayScale: 3)
    for _ in 0..<12 {
      await model.dragChanged(translation: CGSize(width: 0, height: -800))
      await model.dragEnded(velocity: .zero)
    }
    let page = model.currentPageIndex
    #expect(page > 0)
    await model.sizeChanged(CGSize(width: 844, height: 390), displayScale: 3)
    expectNoDifference(model.currentPageIndex, page)
    #expect(!model.visiblePages.isEmpty)
  }
}
```

`Bundle.module` here is the `EmbedPDFUITests` bundle, which `build-xcframework.sh` already fills with the fixtures.

- [ ] **Step 2: Run them to verify they fail**

Run: `(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17' -only-testing:EmbedPDFUITests/StageReaderModelTests)`
Expected: FAIL to build, "cannot find 'StageReaderModel' in scope".

- [ ] **Step 3: Write `StageReaderModel.swift`**

```swift
import CoreGraphics
import EmbedPDF
import Observation

/// What a scrolling reader shows, and the gestures it forwards. The stage
/// lays out; this model only mirrors its visible pages and camera onto the
/// main actor and turns SwiftUI gesture values into stage calls.
@MainActor
@Observable
public final class StageReaderModel {
  public private(set) var visiblePages: [StageVisiblePage] = []
  public private(set) var camera: Camera?
  public private(set) var currentPageIndex: UInt32 = 0
  public private(set) var error: PDFError?
  @ObservationIgnored public let stage: StageCapability
  @ObservationIgnored public let render: RenderCapability
  @ObservationIgnored private var lastTranslation = CGSize.zero
  @ObservationIgnored private var lastMagnification: CGFloat = 1
  @ObservationIgnored private var gestureOpen = false

  public init(stage: StageCapability, render: RenderCapability) {
    self.stage = stage
    self.render = render
  }

  /// Run for the view's lifetime: mirrors the stage's camera and page.
  public func task() async {
    for await event in stage.events {
      switch event {
      case .cameraChanged, .motionEnded, .viewportChanged, .zoomChanged, .settingsChanged:
        await refresh()
      case let .pageChanged(event):
        currentPageIndex = event.pageIndex
      }
    }
  }

  public func sizeChanged(_ size: CGSize, displayScale: CGFloat) async {
    do {
      try await stage.setDevicePixelRatio(Double(displayScale))
      try await stage.setViewportSize(Size(width: size.width, height: size.height))
      currentPageIndex = try await stage.currentPageIndex()
    } catch {
      self.error = error
    }
    await refresh()
  }

  public func dragChanged(translation: CGSize) async {
    do {
      if !gestureOpen {
        gestureOpen = true
        lastTranslation = .zero
        try await stage.beginGesture(elastic: true)
      }
      // The stage pans the camera; a finger moving up scrolls content up.
      stage.panBy(dx: -(translation.width - lastTranslation.width), dy: -(translation.height - lastTranslation.height))
      lastTranslation = translation
    } catch {
      self.error = error
    }
    await refresh()
  }

  /// `velocity` in points per second, as `DragGesture.Value.velocity` reports.
  public func dragEnded(velocity: CGSize) async {
    do {
      if gestureOpen {
        gestureOpen = false
        try await stage.endGesture()
      }
      if velocity != .zero {
        try await stage.fling(velocityX: -velocity.width, velocityY: -velocity.height)
      }
      currentPageIndex = try await stage.currentPageIndex()
    } catch {
      self.error = error
    }
    await refresh()
  }

  public func magnifyChanged(_ magnification: CGFloat, at location: CGPoint) {
    let factor = magnification / lastMagnification
    lastMagnification = magnification
    stage.zoomAround(Point(x: location.x, y: location.y), factor: Double(factor))
  }

  public func magnifyEnded() async {
    lastMagnification = 1
    await refresh()
  }

  public func doubleTapped(at location: CGPoint) async {
    do {
      try await stage.doubleTapZoom(Point(x: location.x, y: location.y))
    } catch {
      self.error = error
    }
    await refresh()
  }

  private func refresh() async {
    do {
      visiblePages = try await stage.visiblePages()
      camera = try await stage.camera()
    } catch {
      self.error = error
    }
  }
}
```

The stage's `pan_by` takes viewport deltas in the direction the camera moves. If the drag test shows the camera moving the wrong way for a finger moving up, drop the minus signs in `dragChanged` and `dragEnded`; check against `crates/plugins/stage/src/camera/gestures.rs` `pan_by` before deciding.

- [ ] **Step 4: Write `StageReaderView.swift`**

```swift
import EmbedPDF
import SwiftUI

/// A scrolling, zooming reader over a document's stage. Each visible page
/// sits where the stage put it and renders at the stage's render scale.
public struct StageReaderView: View {
  let model: StageReaderModel
  @Environment(\.displayScale) private var displayScale

  public init(model: StageReaderModel) {
    self.model = model
  }

  public var body: some View {
    GeometryReader { geometry in
      ZStack(alignment: .topLeading) {
        ForEach(model.visiblePages, id: \.page.pageObjectNumber) { page in
          PageImageView(render: model.render, page: page.page, scale: page.transform.renderScale)
            .frame(width: page.transform.viewWidth, height: page.transform.viewHeight)
            .offset(x: page.screenX, y: page.screenY)
        }
      }
      .frame(width: geometry.size.width, height: geometry.size.height, alignment: .topLeading)
      .clipped()
      .contentShape(Rectangle())
      .gesture(
        DragGesture(minimumDistance: 0)
          .onChanged { value in Task { await model.dragChanged(translation: value.translation) } }
          .onEnded { value in Task { await model.dragEnded(velocity: value.velocity) } }
      )
      .simultaneousGesture(
        MagnifyGesture()
          .onChanged { value in model.magnifyChanged(value.magnification, at: value.startLocation) }
          .onEnded { _ in Task { await model.magnifyEnded() } }
      )
      .onTapGesture(count: 2, coordinateSpace: .local) { location in
        Task { await model.doubleTapped(at: location) }
      }
      .task { await model.task() }
      .task(id: geometry.size) { await model.sizeChanged(geometry.size, displayScale: displayScale) }
    }
  }
}
```

`PageImageView` renders with `renderPage` at the stage's render scale. Moving it to `renderSource` and the tile plane is a separate change, so the reader works now on the full-page path the demo app already uses.

- [ ] **Step 5: Run the model tests**

Run: `bash platforms/apple/build-xcframework.sh && (cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17' -only-testing:EmbedPDFUITests)`
Expected: TEST SUCCEEDED, the 3 `StageReaderModelTests` and the existing `SearchModelTests`.

- [ ] **Step 6: Commit**

```bash
git add platforms/apple/EmbedPDF/Sources/EmbedPDFUI platforms/apple/EmbedPDF/Tests/EmbedPDFUITests
git commit -m "Add a SwiftUI scrolling reader over the stage to EmbedPDFUI"
```

### Task 10: Docs, CI, merge and clean up

**Files:**
- Modify: `.github/workflows/plugin-rust.yml`, `platforms/apple/README.md`, `docs/superpowers/plans/2026-10-01-swift-package.md`

- [ ] **Step 1: Add the crate to CI**

Append `stage` to the `crates=( ... )` list in `.github/workflows/plugin-rust.yml`, after `interaction`.

- [ ] **Step 2: Add the README row**

```markdown
| stage | `StageCapability`, `StageReaderView` (EmbedPDFUI) | `stage: StageConfig()` | `2026-10-06-swift-plugins/02-stage.md` |
```

- [ ] **Step 3: Mark the spec entries delivered**

In `2026-10-01-swift-package.md` Appendix A, change `**stage** (D, optional `actions`).` to `**stage** (D, optional `actions`), delivered by 02-stage.md.`, and in §A.1's render entry change `In B1:` to `Delivered by 02-stage.md:`.

- [ ] **Step 4: Run the definition of done**

```bash
cargo test -p epdf-plugin-stage --features uniffi
cargo test -p epdf-plugin-render --features uniffi
cargo test -p epdf-ffi -p epdf-viewer-host
cargo clippy -p epdf-plugin-stage -p epdf-plugin-render -p epdf-viewer-host --all-targets --features epdf-plugin-stage/uniffi,epdf-plugin-render/uniffi -- -D warnings
cargo test -p epdf-plugin-stage
bash platforms/apple/build-xcframework.sh
(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17')
cargo build -p epdf-viewer-host -p uniffi-bindgen
./target/debug/uniffi-bindgen generate --library target/debug/libepdf_viewer_host.dylib --language kotlin --no-format --out-dir "$TMPDIR/epdf-kotlin-check"
```

Expected: all pass. Note the simulator `libepdf_viewer_host.a` size before (from `research/swift-plugins`) and after.

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/plugin-rust.yml platforms/apple/README.md docs/superpowers/plans/2026-10-01-swift-package.md
git commit -m "List stage in CI and the Swift README

libepdf_viewer_host.a (simulator, release): <before> -> <after> bytes."
```

- [ ] **Step 6: Merge into the integration branch**

In `.claude/worktrees/swift-plugins-integration`:

```bash
git merge --no-ff research/swift-stage
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
git worktree remove .claude/worktrees/swift-stage
git branch -d research/swift-stage
git worktree prune
```

No `--force`. Tick 02 in the umbrella's list.
