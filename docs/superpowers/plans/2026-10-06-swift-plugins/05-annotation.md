# Annotation Plugin Swift Wrapper Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Export `epdf-plugin-annotation` through UniFFI as `AnnotationHandle` and four sub-handles, wire it into `ViewerHost` over the native engine's `page.annotations.*` and `annotations.listRawAll` paths, and wrap it in Swift as `AnnotationCapability`.

**Architecture:** The recipe in `2026-10-01-swift-package.md` Appendix A §A.0, split across five handle objects because the capability has 140 callable members (127 in `crates/plugins/annotation/src/harness/mod.rs:78` `MEMBERS` besides the `comments` and `links` objects, plus 10 `COMMENTS_MEMBERS` and 3 `LINKS_MEMBERS`). The records every later content plugin returns (`AnnotationRef`, flags, props, link targets) go into `crates/epdf-ffi/src/annotation.rs`, which this plan owns. The plugin keeps its DTOs as `serde_json::Value`, so every record the handles return is a typed mirror decoded from that JSON, and a value that doesn't decode is an `invalid-state` error, as `crates/plugins/metadata/src/ffi.rs` does. The native engine speaks a flat `AnnotationRef` (`pageObjectNumber` beside the id) where the kernel speaks a nested one (`page: PageRef`), and its writes take binary fields as payloads, so `translate.rs` gains a ref rewrite and the draft normalizer from `packages/engine/core/src/annotation/normalize.ts`.

**Tech Stack:** Rust 2024, `uniffi =0.32.0`, `serde_json`, Swift 6.2 tools, Swift Testing, `swift-custom-dump` 1.3, Xcode 27.0, iPhone 17 simulator.

**Spec:** `docs/superpowers/plans/2026-10-06-swift-plugins/00-umbrella.md` (this plan's row, cross-plan names, shared records, template, definition of done) and `docs/superpowers/plans/2026-10-01-swift-package.md` (Global Constraints, Appendix A §A.0 and §A.3, Appendix C). Read both.

## Global Constraints

Everything in the umbrella's Global Constraints applies. The ones this plugin hits hardest:

- `uniffi` is pinned at `=0.32.0`.
- UniFFI names are workspace-unique. Generic core names get an `Annotation` prefix: `AnnotationTextAlign`, `AnnotationBlendMode`, `AnnotationLineEnding`, `AnnotationBorder`.
- No `usize`, `isize`, `Rc` or `serde_json::Value` across the boundary. `CreationDraftAnchor.point_count` (`usize`) becomes `u32`. `Annotation.props` (`Map<String, Value>`) becomes `AnnotationProps`. `contents`, `author`, `created_at`, `modified_at` (`Value`) become `Option<String>`.
- A three-state core field (`Nullable<T>`: absent, `null`, value) becomes `Option<T>` plus a `clear` list, the shape `MetadataPatch` already uses.
- The kernel is `!Send`. Every handle call goes through `KernelBound`.
- Requires plan 01: `ViewerConfig.interaction` and `epdf_plugin_interaction` in the host. Kernel rejects an annotation plugin without it.

## Review Focus

1. A stamp created from image bytes. People expect it to land on the page. The kernel sends `source: {"$bytes": …}` and the native engine wants `source: {"resource": "r0"}` with the bytes as a payload, so a missing normalizer fails with a wire-format error. Pinned in Task 6 (`a_stamp_drafts_bytes_travel_as_a_resource_payload`) and Task 8 (`aStampFromPngBytesIsCreated`).
2. An `AnnotationRef` round trip: read an annotation, update it by the ref Swift got back. People expect the update to land on the same annotation. The engine answers flat refs and reads flat refs, the kernel keeps nested ones, so a missing rewrite in either direction fails or misaddresses. Pinned in Task 6 (`refs_are_nested_on_the_way_in_and_flattened_on_the_way_out`) and Task 7 (`an_annotation_read_back_updates_through_the_native_engine`).
3. An annotation whose `props` carry keys no `PropKey` names (vendor keys the core keeps in `extra`). People expect reads to keep working and the unknown keys to be dropped, not the whole list to fail. Pinned in Task 2 (`unknown_prop_keys_are_dropped_not_fatal`).
4. A batch where one entry fails (`delete_many` with a ref that no longer exists). People expect the others to apply and the failure to name its ref. Pinned in Task 2 (`a_batch_reports_each_failure_beside_what_applied`).
5. The file picker port answering after the user switched tools. People expect the late file to be dropped, not placed. The plugin already drops it (`services/file_picker.rs` `prompt_at`), but the adapter crosses threads, so a late answer must still post back to the kernel thread and not race the tool change. Pinned in Task 5 (`a_pick_that_lands_after_a_tool_change_places_nothing`).

## Decisions

We're exporting 98 of the 140 members. The rest drive a web canvas or are other plugins' hooks, and a native app reaches the same behaviour another way:

- Pointer and chrome members (`editPointer`, `marqueePointer`, `createPointer`, `hoverAt`, `getHitKind`, `claimsTouchAt`, `getCursorAt`, `listChromeNodes`, `getSelectionAnchor`) are what the interaction hub calls. `wire_hub` (`crates/plugins/annotation/src/lib.rs`) registers them as hub handlers in Rust, so Swift feeds pointer samples through plan 01's `InteractionHandle.dispatch_pointer` and the annotation plugin sees them.
- Render projections (`listPageItems`, `listTextItems`, `listLinkItems`, `getAppearanceEpoch`, `getBakeScale`, `renderAppearances`, `pdfToPageRect`, `getCssFontFamily`, `getToolGhost`, `getArmedStampPreview`, `getStampArmEpoch`, `getChromeSettings`, `updateChromeSettings`) build CSS and DOM overlays. A native viewer draws annotations through render's `includeAnnotations`. A native editing overlay (live ink, selection handles) needs a follow-on that exports these as typed draw lists; this plan doesn't attempt it.
- Raw DTO members (`listRaw`, `getRaw`, `createRaw`, `updateRaw`) and `getSelectionFlags` take or return engine `Value`s. They exist for tooling. `updateSelectionFlags` covers writing flags.
- `setRichText` and `draftRichText` take a rich-text paragraph tree that only the native editing follow-on below has a use for.
- Host hooks other plugins call (`registerBehavior`, `pruneEngagedSelection`, `getBehaviorFor`, `commitScriptEffects`, `deliverRemoteEvent`, `setPageViewports`, `remeasurePage`, `distanceCreationPage`, `onDraftCaptured`, `registerTool`, `getResolvedTool`, `listResolvedTools`, `getToolSubtype`) are wired in Rust by form, measurement, stamp, actions and the hub.

The cost: a Swift app can draw annotations and edit them through typed calls, but it can't draw the in-progress ink stroke or selection handles itself until that follow-on lands. Each excluded member is listed in `crates/plugins/annotation/src/ffi/mod.rs`'s module doc so a reviewer can check the split against `MEMBERS`.

The handles mirror the TypeScript capability's areas, as the spec's §A.3 asks. `AnnotationHandle` holds reads, writes, hydration, permissions, events, and returns the other four: `AnnotationSelectionHandle` (select, group, rotate, selection props), `AnnotationToolsHandle` (tools, defaults, prop specs, snap settings), `AnnotationDraftsHandle` (creation drafts, text editing, markup, stamps, placement) and `AnnotationThreadsHandle` (comments and attached links). Each sub-handle holds its own `KernelBound<AnnotationCapability>` clone, so Swift can keep one without the others.

`ffi` is a directory (`src/ffi/mod.rs` plus one file per handle), not one file. At roughly 1,800 lines it would be the biggest file in the crate.

## File Structure

```
crates/epdf-ffi/Cargo.toml                       + epdf-annotation-core
crates/epdf-ffi/src/lib.rs                       + pub mod annotation; re-exports
crates/epdf-ffi/src/annotation.rs                (extends plan 04's) flags, props, endings,
                                                 border, link targets, Callout, CreationDraftAnchor
crates/epdf-ffi/src/geometry.rs                  + From impls for epdf_annotation_core::PageRef
crates/epdf-ffi/tests/annotation.rs
crates/plugins/annotation/Cargo.toml             + uniffi feature
crates/plugins/annotation/src/lib.rs             + scaffolding, pub mod ffi
crates/plugins/annotation/src/controller.rs      + lifetime()
crates/plugins/annotation/src/ffi/mod.rs         records, decode, open(), AnnotationEvent, listener
crates/plugins/annotation/src/ffi/handle.rs      AnnotationHandle
crates/plugins/annotation/src/ffi/selection.rs   AnnotationSelectionHandle
crates/plugins/annotation/src/ffi/tools.rs       AnnotationToolsHandle
crates/plugins/annotation/src/ffi/drafts.rs      AnnotationDraftsHandle
crates/plugins/annotation/src/ffi/threads.rs     AnnotationThreadsHandle
crates/plugins/annotation/src/ffi/picker.rs      AnnotationFilePicker port and its adapter
crates/plugins/annotation/tests/ffi.rs
crates/epdf-viewer-host/{Cargo.toml, src/api.rs, src/session.rs, src/lib.rs, src/translate.rs}
crates/epdf-viewer-host/tests/{translate.rs, viewer_host.rs}
platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Annotation.swift
platforms/apple/EmbedPDF/Tests/EmbedPDFTests/AnnotationTests.swift
platforms/apple/build-xcframework.sh   + annotation_kinds.pdf, tiny.png in FIXTURES
.github/workflows/plugin-rust.yml, platforms/apple/README.md
```

## Cross-plugin tests

| Optional integration | Behaviour | Test owner |
|---|---|---|
| selection (plan 03) | `create_from_selection` and `create_markup` turn the current text selection into highlight annotations | this plan if 03 merged first (`createFromSelectionMarksTheSelectedText` in `AnnotationTests.swift`), otherwise plan 03 adds it |
| actions (plan 15) | the hover feed and the annotation commit sink | plan 15 |
| link (plan 04) | link activation reads attached links through `AnnotationThreadsHandle.link` | plan 04 |
| form (plan 06), redaction (07), measurement (08), stamp (09), signature (16) | they consume `epdf_ffi::AnnotationRef` and this plugin's capability | each of those plans |

---

### Task 1: Worktree and the shared annotation records in `epdf-ffi`

**Files:**
- Modify: `crates/epdf-ffi/Cargo.toml`
- Modify: `crates/epdf-ffi/src/lib.rs`
- Modify: `crates/epdf-ffi/src/geometry.rs`
- Create: `crates/epdf-ffi/src/annotation.rs`
- Test: `crates/epdf-ffi/tests/annotation.rs`

**Interfaces:**
- Consumes: `epdf_annotation_core::{AnnotationRef, RevisionToken, AnnotationFlags, AnnotationFlagsPatch, AnnotationProps, AnnotationPropsPatch, PropKey, LineEnding, LineEndings, LineEndingsPatch, BlendMode, TextAlign, Border, PdfLinkTarget, PdfDestination, Callout, CreationDraftAnchor, PageRef}` and `epdf_annotation_core::js::Nullable`.
- Produces, all in `epdf_ffi`, beside plan 04's `AnnotationRef`, `AnnotationRevisionToken`, `PdfDestination` and `PdfLinkTarget`: `AnnotationFlags`, `AnnotationFlagsPatch`, `AnnotationPropKey`, `AnnotationProps`, `AnnotationPropsPatch`, `AnnotationLineEnding`, `AnnotationLineEndings`, `AnnotationLineEndingsPatch`, `AnnotationBlendMode`, `AnnotationTextAlign`, `AnnotationBorder`, `AnnotationCallout`, `CreationDraftAnchor`, each with `From` to and from its core type (`TryFrom` where the core is wider). Plans 06, 07, 08, 09 and 16 use these by name.

- [x] **Step 1: Create the worktree**

```bash
git worktree add .claude/worktrees/swift-annotation -b research/swift-annotation research/swift-plugins
cd .claude/worktrees/swift-annotation
```

Fetch PDFium for `darwin-arm64`, `ios-arm64` and `ios-sim-arm64` as `platforms/apple/README.md` describes. Confirm plans 01 and 04 have merged: `grep -n 'interaction' crates/epdf-viewer-host/src/api.rs` prints the `interaction: bool` field, and `grep -n 'page.annotations.list' crates/epdf-viewer-host/src/translate.rs` prints plan 04's route.

- [x] **Step 2: Check the dependencies**

`crates/epdf-ffi/Cargo.toml` already has `epdf-annotation-core` (plan 04) and `serde_json` (umbrella Task 0d). Run `grep -n 'epdf-annotation-core\|serde_json' crates/epdf-ffi/Cargo.toml` and expect both.

- [x] **Step 3: Write the failing test**

`crates/epdf-ffi/tests/annotation_props.rs` (plan 04 owns `tests/annotation.rs`):

```rust
use epdf_annotation_core as core;
use epdf_annotation_core::js::Nullable;
use epdf_ffi::{
    AnnotationBorder, AnnotationPropKey, AnnotationPropsPatch, AnnotationRef, PageRef,
    PdfDestination, PdfLinkTarget,
};

#[test]
fn an_object_number_ref_round_trips() {
    let core = core::AnnotationRef::ObjectNumber {
        page: core::PageRef::new(4),
        annot_object_number: 17.0,
    };
    let ffi = AnnotationRef::try_from(core.clone()).unwrap();
    assert_eq!(
        ffi,
        AnnotationRef::ObjectNumber { page: PageRef { page_object_number: 4 }, annot_object_number: 17 }
    );
    assert_eq!(core::AnnotationRef::from(ffi), core);
}

#[test]
fn a_fractional_object_number_is_refused() {
    let core = core::AnnotationRef::ObjectNumber {
        page: core::PageRef::new(4),
        annot_object_number: 1.5,
    };
    assert!(AnnotationRef::try_from(core).is_err());
}

#[test]
fn a_props_patch_clears_through_its_clear_list() {
    let patch = AnnotationPropsPatch {
        color: Some("#ff0000".into()),
        clear: vec![AnnotationPropKey::InteriorColor, AnnotationPropKey::Link],
        ..Default::default()
    };
    let core = core::AnnotationPropsPatch::try_from(patch).unwrap();
    assert_eq!(core.color.as_deref(), Some("#ff0000"));
    assert_eq!(core.interior_color, Nullable::Null);
    assert_eq!(core.link, Nullable::Null);
    assert_eq!(core.keys(), ["color", "interiorColor", "link"]);
}

#[test]
fn a_patch_that_sets_and_clears_one_key_is_refused() {
    let patch = AnnotationPropsPatch {
        interior_color: Some("#000000".into()),
        clear: vec![AnnotationPropKey::InteriorColor],
        ..Default::default()
    };
    let error = core::AnnotationPropsPatch::try_from(patch).unwrap_err();
    assert_eq!(error.code(), Some("invalid-input"));
}

#[test]
fn clearing_a_key_that_has_no_null_state_is_refused() {
    let patch =
        AnnotationPropsPatch { clear: vec![AnnotationPropKey::Color], ..Default::default() };
    let error = core::AnnotationPropsPatch::try_from(patch).unwrap_err();
    assert_eq!(error.code(), Some("invalid-input"));
}

#[test]
fn a_dashed_border_round_trips() {
    let core = core::Border::Dashed { dash: vec![3.0, 2.0] };
    let ffi = AnnotationBorder::from(core.clone());
    assert_eq!(ffi, AnnotationBorder::Dashed { dash: vec![3.0, 2.0] });
    assert_eq!(core::Border::from(ffi), core);
}

#[test]
fn a_goto_link_keeps_its_page_and_drops_undefined_coordinates() {
    let core = core::PdfLinkTarget::Goto {
        destination: core::PdfDestination::Xyz {
            page: core::PageRef::new(9),
            left: Nullable::Value(72.0),
            top: Nullable::Undefined,
            zoom: Nullable::Null,
        },
    };
    let ffi = PdfLinkTarget::from(core);
    assert_eq!(
        ffi,
        PdfLinkTarget::Goto {
            destination: PdfDestination::Xyz {
                page: PageRef { page_object_number: 9 },
                left: Some(72.0),
                top: None,
                zoom: None,
            }
        }
    );
}
```

- [x] **Step 4: Run it to see it fail**

Run: `cargo test -p epdf-ffi --test annotation_props`
Expected: compile errors, `cannot find type AnnotationRef in crate epdf_ffi` and the rest.

- [x] **Step 5: Confirm plan 04's records are there**

Run: `grep -n 'pub enum AnnotationRef\|pub struct AnnotationRevisionToken\|impl From<ac::PageRef> for PageRef' crates/epdf-ffi/src/*.rs`
Expected: three hits, all from plan 04. If plan 04 hasn't merged, stop: this plan waits for it (umbrella Waves).

- [x] **Step 6: Extend `annotation.rs`**

Plan 04 created `crates/epdf-ffi/src/annotation.rs` with `AnnotationRef`, `AnnotationRevisionToken`, `PdfDestination`, `PdfLinkTarget` and the core `PageRef` conversions. Append the block below to it. Of the `use` lines, add only those the file doesn't have yet (plan 04 imports the core as `ac` and `Nullable` already; keep both aliases, `core` is what the code below uses).

```rust
use epdf_annotation_core as core;
use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error};

use crate::geometry::Point;

fn invalid(message: String) -> Error {
    plugin_error(code::INVALID_INPUT, "annotation", message, None, None)
}

// `AnnotationRef`, `AnnotationRevisionToken`, `PdfDestination` and
// `PdfLinkTarget`, with their conversions both ways, are plan 04's, earlier
// in this file. This block adds the rest.

impl AnnotationRef {
    pub fn page(&self) -> PageRef {
        match self {
            AnnotationRef::ObjectNumber { page, .. }
            | AnnotationRef::Nm { page, .. }
            | AnnotationRef::Index { page, .. } => *page,
        }
    }
}

/// The `/F` flags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct AnnotationFlags {
    pub invisible: bool,
    pub hidden: bool,
    pub print: bool,
    pub no_zoom: bool,
    pub no_rotate: bool,
    pub no_view: bool,
    pub read_only: bool,
    pub locked: bool,
    pub toggle_no_view: bool,
    pub locked_contents: bool,
}

macro_rules! flags_both_ways {
    ($ffi:ident, $core:path, $($field:ident),*) => {
        impl From<$core> for $ffi {
            fn from(f: $core) -> Self { $ffi { $($field: f.$field),* } }
        }
        impl From<$ffi> for $core {
            fn from(f: $ffi) -> Self { Self { $($field: f.$field),* } }
        }
    };
}

flags_both_ways!(AnnotationFlags, core::AnnotationFlags, invisible, hidden, print, no_zoom,
    no_rotate, no_view, read_only, locked, toggle_no_view, locked_contents);

/// A partial flags write. `None` leaves the flag alone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct AnnotationFlagsPatch {
    #[uniffi(default = None)]
    pub invisible: Option<bool>,
    #[uniffi(default = None)]
    pub hidden: Option<bool>,
    #[uniffi(default = None)]
    pub print: Option<bool>,
    #[uniffi(default = None)]
    pub no_zoom: Option<bool>,
    #[uniffi(default = None)]
    pub no_rotate: Option<bool>,
    #[uniffi(default = None)]
    pub no_view: Option<bool>,
    #[uniffi(default = None)]
    pub read_only: Option<bool>,
    #[uniffi(default = None)]
    pub locked: Option<bool>,
    #[uniffi(default = None)]
    pub toggle_no_view: Option<bool>,
    #[uniffi(default = None)]
    pub locked_contents: Option<bool>,
}

flags_both_ways!(AnnotationFlagsPatch, core::AnnotationFlagsPatch, invisible, hidden, print,
    no_zoom, no_rotate, no_view, read_only, locked, toggle_no_view, locked_contents);

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum AnnotationLineEnding {
    None,
    Square,
    Circle,
    Diamond,
    OpenArrow,
    ClosedArrow,
    Butt,
    ROpenArrow,
    RClosedArrow,
    Slash,
}

macro_rules! enum_both_ways {
    ($ffi:ident, $core:path, $($variant:ident),*) => {
        impl From<$core> for $ffi {
            fn from(v: $core) -> Self { match v { $(<$core>::$variant => $ffi::$variant),* } }
        }
        impl From<$ffi> for $core {
            fn from(v: $ffi) -> Self { match v { $($ffi::$variant => <$core>::$variant),* } }
        }
    };
}

enum_both_ways!(AnnotationLineEnding, core::LineEnding, None, Square, Circle, Diamond, OpenArrow,
    ClosedArrow, Butt, ROpenArrow, RClosedArrow, Slash);

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct AnnotationLineEndings {
    pub start: AnnotationLineEnding,
    pub end: AnnotationLineEnding,
}

impl From<core::LineEndings> for AnnotationLineEndings {
    fn from(e: core::LineEndings) -> Self {
        AnnotationLineEndings { start: e.start.into(), end: e.end.into() }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct AnnotationLineEndingsPatch {
    #[uniffi(default = None)]
    pub start: Option<AnnotationLineEnding>,
    #[uniffi(default = None)]
    pub end: Option<AnnotationLineEnding>,
}

impl From<AnnotationLineEndingsPatch> for core::LineEndingsPatch {
    fn from(p: AnnotationLineEndingsPatch) -> Self {
        core::LineEndingsPatch { start: p.start.map(Into::into), end: p.end.map(Into::into) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum AnnotationBlendMode {
    Normal, Multiply, Screen, Overlay, Darken, Lighten, ColorDodge, ColorBurn, HardLight,
    SoftLight, Difference, Exclusion, Hue, Saturation, Color, Luminosity,
}

enum_both_ways!(AnnotationBlendMode, core::BlendMode, Normal, Multiply, Screen, Overlay, Darken,
    Lighten, ColorDodge, ColorBurn, HardLight, SoftLight, Difference, Exclusion, Hue, Saturation,
    Color, Luminosity);

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum AnnotationTextAlign {
    Left,
    Center,
    Right,
}

enum_both_ways!(AnnotationTextAlign, core::TextAlign, Left, Center, Right);

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum AnnotationBorder {
    Solid,
    Dashed { dash: Vec<f64> },
    Cloudy { intensity: f64 },
}

impl From<core::Border> for AnnotationBorder {
    fn from(b: core::Border) -> Self {
        match b {
            core::Border::Solid => AnnotationBorder::Solid,
            core::Border::Dashed { dash } => AnnotationBorder::Dashed { dash },
            core::Border::Cloudy { intensity } => AnnotationBorder::Cloudy { intensity },
        }
    }
}

impl From<AnnotationBorder> for core::Border {
    fn from(b: AnnotationBorder) -> Self {
        match b {
            AnnotationBorder::Solid => core::Border::Solid,
            AnnotationBorder::Dashed { dash } => core::Border::Dashed { dash },
            AnnotationBorder::Cloudy { intensity } => core::Border::Cloudy { intensity },
        }
    }
}

/// A key of `AnnotationProps`, for `AnnotationPropsPatch.clear` and for a
/// tool's prop specs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum AnnotationPropKey {
    Color, InteriorColor, StrokeWidth, Opacity, BlendMode, Border, LineEndings, FontFamily,
    FontSize, FontColor, TextAlign, Bold, Italic, Underline, Icon, Link,
}

enum_both_ways!(AnnotationPropKey, core::PropKey, Color, InteriorColor, StrokeWidth, Opacity,
    BlendMode, Border, LineEndings, FontFamily, FontSize, FontColor, TextAlign, Bold, Italic,
    Underline, Icon, Link);

/// Editable appearance. Keys the core keeps in `extra` (vendor keys no
/// `PropKey` names) are dropped: there is no typed home for them.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationProps {
    pub color: String,
    pub interior_color: Option<String>,
    pub stroke_width: f64,
    pub opacity: f64,
    pub blend_mode: AnnotationBlendMode,
    pub border: AnnotationBorder,
    pub font_family: String,
    pub font_size: f64,
    pub font_color: String,
    pub text_align: AnnotationTextAlign,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub line_endings: AnnotationLineEndings,
    pub icon: Option<String>,
    pub link: Option<PdfLinkTarget>,
}

impl From<core::AnnotationProps> for AnnotationProps {
    fn from(p: core::AnnotationProps) -> Self {
        AnnotationProps {
            color: p.color,
            interior_color: p.interior_color,
            stroke_width: p.stroke_width,
            opacity: p.opacity,
            blend_mode: p.blend_mode.into(),
            border: p.border.into(),
            font_family: p.font_family,
            font_size: p.font_size,
            font_color: p.font_color,
            text_align: p.text_align.into(),
            bold: p.bold,
            italic: p.italic,
            underline: p.underline,
            line_endings: p.line_endings.into(),
            icon: p.icon,
            link: match p.link {
                Nullable::Value(link) => Some(link.into()),
                Nullable::Null | Nullable::Undefined => None,
            },
        }
    }
}

/// A partial property write. A `None` field is left alone, a value sets it,
/// and a key in `clear` is removed. Only `InteriorColor` and `Link` can be
/// cleared; naming another, or naming a key that is also set, is refused.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct AnnotationPropsPatch {
    #[uniffi(default = None)]
    pub color: Option<String>,
    #[uniffi(default = None)]
    pub interior_color: Option<String>,
    #[uniffi(default = None)]
    pub stroke_width: Option<f64>,
    #[uniffi(default = None)]
    pub opacity: Option<f64>,
    #[uniffi(default = None)]
    pub blend_mode: Option<AnnotationBlendMode>,
    #[uniffi(default = None)]
    pub border: Option<AnnotationBorder>,
    #[uniffi(default = None)]
    pub line_endings: Option<AnnotationLineEndingsPatch>,
    #[uniffi(default = None)]
    pub font_family: Option<String>,
    #[uniffi(default = None)]
    pub font_size: Option<f64>,
    #[uniffi(default = None)]
    pub font_color: Option<String>,
    #[uniffi(default = None)]
    pub text_align: Option<AnnotationTextAlign>,
    #[uniffi(default = None)]
    pub bold: Option<bool>,
    #[uniffi(default = None)]
    pub italic: Option<bool>,
    #[uniffi(default = None)]
    pub underline: Option<bool>,
    #[uniffi(default = None)]
    pub icon: Option<String>,
    #[uniffi(default = None)]
    pub link: Option<PdfLinkTarget>,
    #[uniffi(default = [])]
    pub clear: Vec<AnnotationPropKey>,
}

impl TryFrom<AnnotationPropsPatch> for core::AnnotationPropsPatch {
    type Error = Error;

    fn try_from(p: AnnotationPropsPatch) -> Result<Self, Error> {
        let mut interior_color = p.interior_color.map_or(Nullable::Undefined, Nullable::Value);
        let mut link = p.link.map(Into::into).map_or(Nullable::Undefined, Nullable::Value);
        for key in &p.clear {
            let slot_is_set = match key {
                AnnotationPropKey::InteriorColor => {
                    let set = !interior_color.is_undefined();
                    interior_color = Nullable::Null;
                    set
                }
                AnnotationPropKey::Link => {
                    let set = !link.is_undefined();
                    link = Nullable::Null;
                    set
                }
                other => {
                    return Err(invalid(format!(
                        "props patch clears {:?}, which has no cleared state",
                        core::PropKey::from(*other).as_str()
                    )));
                }
            };
            if slot_is_set {
                return Err(invalid(format!(
                    "props patch both sets and clears \"{}\"",
                    core::PropKey::from(*key).as_str()
                )));
            }
        }
        let mut out = core::AnnotationPropsPatch {
            color: p.color,
            interior_color,
            stroke_width: p.stroke_width,
            opacity: p.opacity,
            blend_mode: p.blend_mode.map(Into::into),
            border: p.border.map(Into::into),
            line_endings: p.line_endings.map(Into::into),
            font_family: p.font_family,
            font_size: p.font_size,
            font_color: p.font_color,
            text_align: p.text_align.map(Into::into),
            bold: p.bold,
            italic: p.italic,
            underline: p.underline,
            icon: p.icon,
            link,
            ..Default::default()
        };
        // Declaration order, the order `keys()` falls back to.
        out.order = core::PropKey::ALL
            .into_iter()
            .map(|k| k.as_str().to_string())
            .filter(|k| out.has(k))
            .collect();
        Ok(out)
    }
}

/// A free-text callout's leader.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationCallout {
    pub tip: Point,
    pub knee: Option<Point>,
    pub ending: AnnotationLineEnding,
}

impl From<core::Callout> for AnnotationCallout {
    fn from(c: core::Callout) -> Self {
        AnnotationCallout { tip: c.tip.into(), knee: c.knee.map(Into::into), ending: c.ending.into() }
    }
}

impl From<AnnotationCallout> for core::Callout {
    fn from(c: AnnotationCallout) -> Self {
        core::Callout { tip: c.tip.into(), knee: c.knee.map(Into::into), ending: c.ending.into() }
    }
}

/// An in-progress polygon or polyline: what a "Finish" button needs.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct CreationDraftAnchor {
    /// `true` for a polygon, `false` for a polyline.
    pub closed: bool,
    pub page: PageRef,
    pub bounds: crate::geometry::Rect,
    pub point_count: u32,
    pub min_points: u32,
    pub can_finish: bool,
}

impl From<core::CreationDraftAnchor> for CreationDraftAnchor {
    fn from(a: core::CreationDraftAnchor) -> Self {
        CreationDraftAnchor {
            closed: a.subtype == core::PolySubtype::Polygon,
            page: a.page.into(),
            bounds: a.bounds.into(),
            point_count: u32::try_from(a.point_count).unwrap_or(u32::MAX),
            min_points: u32::try_from(a.min_points).unwrap_or(u32::MAX),
            can_finish: a.can_finish,
        }
    }
}
```

`CreationDraftKind` has one variant (`Poly`), so the record drops it.

- [x] **Step 7: Export the module**

In `crates/epdf-ffi/src/lib.rs`, plan 04 already declared `pub mod annotation;` and re-exported its four records. Add a second re-export:

```rust
pub use annotation::{
    AnnotationBlendMode, AnnotationBorder, AnnotationCallout, AnnotationFlags,
    AnnotationFlagsPatch, AnnotationLineEnding, AnnotationLineEndings, AnnotationLineEndingsPatch,
    AnnotationPropKey, AnnotationProps, AnnotationPropsPatch, AnnotationTextAlign,
    CreationDraftAnchor,
};
```

- [x] **Step 8: Run the tests**

Run: `cargo test -p epdf-ffi --test annotation_props`
Expected: 7 passed.

Run: `cargo test -p epdf-ffi`
Expected: every existing `epdf-ffi` test still passes.

- [x] **Step 9: Commit**

```bash
git add crates/epdf-ffi
git commit -m "Add the shared annotation records to epdf-ffi"
```

---

### Task 2: `AnnotationHandle`: feature, records, reads, writes, hydration and events

**Files:**
- Modify: `crates/plugins/annotation/Cargo.toml`
- Modify: `crates/plugins/annotation/src/lib.rs`
- Modify: `crates/plugins/annotation/src/controller.rs`
- Create: `crates/plugins/annotation/src/ffi/mod.rs`
- Create: `crates/plugins/annotation/src/ffi/handle.rs`
- Test: `crates/plugins/annotation/tests/ffi.rs`

**Interfaces:**
- Consumes: Task 1's records; `epdf_ffi::{KernelBound, KernelPost, CancelToken, Subscription, ViewerError, PageRef, Rect, Point, TextQuad, PluginErrorInfo}`.
- Produces: `epdf_plugin_annotation::ffi::{AnnotationHandle, Annotation, AnnotationGeometry, AnnotationGeometryPatch, AnnotationPatch, CreateAnnotationInput, AnnotationFilter, AnnotationHydration, AnnotationBatchResult, AnnotationBatchFailure, AnnotationEvent, AnnotationEventListener, AnnotationChangedEvent, AnnotationDeletedEvent, AnnotationResyncedPages, AnnotationConfig, AnnotationSnapSettings, AnnotationSnapSettingsPatch}`. `AnnotationHandle::new(cap: AnnotationCapability, post: KernelPost) -> Arc<AnnotationHandle>`. Task 7 calls that constructor. Tasks 3 and 4 add `selection()`, `tools()`, `drafts()`, `threads()` to it.

Members exported here, under their FFI names (TypeScript name in parentheses where it differs): `get`, `list`, `selected` (`listSelected`), `hit_test_at` (`hitTestAt`), `status` (`getStatus`), `hydration` (`getHydration`), `ensure_hydrated`, `refresh`, `reload_page`, `create`, `create_many`, `update`, `update_many`, `set_rotation`, `rotate_by`, `delete`, `delete_many`, `can_read`, `can_create`, `can_edit`, `can_delete`, `subscribe` (`onResynced`, `onCreated`, `onUpdated`, `onDeleted`, `onSelectionChanged`, `onDraftChanged`, `onEditingChanged`, and the comments lens's `onThreadChanged`).

- [x] **Step 1: Add the feature**

In `crates/plugins/annotation/Cargo.toml`:

```toml
[features]
default = ["harness"]
harness = [
  "dep:plugin-conformance",
  "epdf-plugin-interaction/harness",
  "epdf-plugin-selection/harness",
  "epdf-plugin-actions/harness",
]
# The uniffi exports (`ffi` module), for `epdf-viewer-host`.
uniffi = ["dep:uniffi", "dep:epdf-ffi"]
```

and under `[dependencies]`:

```toml
epdf-ffi = { path = "../../epdf-ffi", optional = true }
uniffi = { version = "=0.32.0", optional = true }
```

and a dev-dependency block:

```toml
[dev-dependencies]
futures = { version = "0.3", default-features = false, features = ["std", "executor"] }
plugin-conformance = { path = "../../plugin-conformance" }

[[test]]
name = "ffi"
required-features = ["uniffi"]
```

In `src/lib.rs`, after `pub mod write;`:

```rust
#[cfg(feature = "uniffi")]
pub mod ffi;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();
```

In `src/controller.rs`, inside `impl AnnotationCapability`:

```rust
    /// The instance lifetime, aborted when the document closes. A foreign
    /// handle outlives the close, checks this before every call, and ends
    /// its event streams when it aborts.
    #[cfg(feature = "uniffi")]
    pub(crate) fn lifetime(&self) -> &epdf_kernel::signal::AbortSignal {
        self.0.cx.lifetime()
    }
```

- [x] **Step 2: Write the failing tests**

`crates/plugins/annotation/tests/ffi.rs`:

```rust
#![cfg(feature = "uniffi")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use epdf_ffi::{
    AnnotationPropsPatch, AnnotationRef, CancelToken, KernelThread, PageRef, Rect, ViewerError,
};
use epdf_plugin_annotation::ffi::{
    AnnotationEvent, AnnotationEventListener, AnnotationFilter, AnnotationHandle,
    AnnotationHydration, AnnotationPatch, CreateAnnotationInput,
};
use epdf_plugin_annotation::{ANNOTATION, AnnotationConfig, annotation_plugin};
use epdf_plugin_interaction::{InteractionConfig, interaction_plugin};
use futures::executor::block_on;
use plugin_conformance::test_kernel::{Reply, TestKernel, TestSpec};
use serde_json::{Value, json};

fn page_json(pon: u32) -> Value {
    json!({ "kind": "objectNumber", "pageObjectNumber": pon })
}

fn ref_json(pon: u32, annot: u32) -> Value {
    json!({ "kind": "objectNumber", "page": page_json(pon), "annotObjectNumber": annot })
}

/// A square annotation DTO as the kernel reads one (the vectors' shape,
/// `tooling/plugin-conformance/vectors/plugins/annotation-crud.json`).
fn square(annot: u32, extra_props: Value) -> Value {
    let mut dto = json!({
        "ref": ref_json(1, annot), "page": page_json(1), "index": annot,
        "identityQuality": "durable", "nm": null,
        "flags": { "invisible": false, "hidden": false, "print": true, "noZoom": false,
            "noRotate": false, "noView": false, "readOnly": false, "locked": false,
            "toggleNoView": false, "lockedContents": false },
        "rect": { "left": 100, "bottom": 600, "right": 200, "top": 700 },
        "contents": "Note", "subject": null, "author": "Ada", "created": null, "modified": null,
        "blendMode": "normal", "inReplyTo": null, "replyType": null, "subtype": "square",
        "color": { "r": 239, "g": 68, "b": 68 }, "interiorColor": null, "opacity": 1,
        "strokeWidth": 2, "borderStyle": "solid", "cloudyIntensity": null, "rectDifferences": null
    });
    if let (Value::Object(dto), Value::Object(extra)) = (&mut dto, extra_props) {
        dto.extend(extra);
    }
    dto
}

fn spec(annotations: Vec<Value>) -> TestSpec {
    let bx = json!({ "left": 0, "bottom": 0, "right": 600, "top": 800 });
    let pages = json!([{ "index": 0, "ref": page_json(1), "label": null,
        "size": { "width": 600, "height": 800 }, "rotation": 0, "userUnit": 1,
        "boxes": { "media": bx, "crop": bx } }]);
    let snapshot = json!({ "pages": [{
        "pageState": { "page": page_json(1) }, "annotations": annotations }] });
    TestSpec::new(json!({ "documents": { "a": {
        "id": "a", "security": { "allows": "*" }, "pages": pages } } }))
        .respond("annotations.listRawAll", move |_| Ok(Reply::value(snapshot.clone())))
        .respond_page("page.annotations.create", |_, args| {
            let mut created = square(10, json!({}));
            created["subtype"] = args[0]["subtype"].clone();
            Ok(Reply::value(json!({ "created": created })))
        })
        .respond_page("page.annotations.update", |_, args| {
            let annot = args[0]["annotObjectNumber"].as_u64().unwrap() as u32;
            Ok(Reply::value(json!({ "updated": square(annot, json!({ "contents": "Edited" })) })))
        })
        .respond_page("page.annotations.delete", |_, args| {
            if args[0]["annotObjectNumber"] == 99 {
                return Err(epdf_kernel::Error::engine("NotFound", "no annotation 99"));
            }
            Ok(Reply::value(json!({})))
        })
}

fn handle_over(
    annotations: Vec<Value>,
) -> (KernelThread<TestKernel>, Arc<AnnotationHandle>) {
    let thread = KernelThread::spawn(move |_| {
        TestKernel::open(
            vec![
                interaction_plugin(InteractionConfig::default()),
                annotation_plugin(AnnotationConfig::default()),
            ],
            spec(annotations),
        )
    })
    .unwrap();
    let post = thread.post();
    let handle = block_on(
        thread.call(move |k| AnnotationHandle::new(k.capability(&ANNOTATION).unwrap(), post)),
    )
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
struct Events(Mutex<Vec<AnnotationEvent>>, AtomicUsize);

impl AnnotationEventListener for Events {
    fn on_event(&self, event: AnnotationEvent) {
        self.0.lock().unwrap().push(event);
    }
    fn on_end(&self) {
        self.1.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn hydration_reads_every_annotation_as_a_typed_record() {
    let (_thread, annotations) = handle_over(vec![square(5, json!({}))]);
    block_on(annotations.refresh(token())).unwrap();
    assert_eq!(block_on(annotations.hydration()).unwrap(), AnnotationHydration::Complete);
    let all = block_on(annotations.list(None)).unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].annotation_ref, obj(5));
    assert_eq!(all[0].subtype, "square");
    assert_eq!(all[0].contents.as_deref(), Some("Note"));
    assert_eq!(all[0].author.as_deref(), Some("Ada"));
    assert_eq!(all[0].props.stroke_width, 2.0);
    assert!(all[0].flags.print);
    let filtered = block_on(annotations.list(Some(AnnotationFilter {
        subtype: Some("ink".into()),
        ..Default::default()
    })))
    .unwrap();
    assert!(filtered.is_empty());
}

#[test]
fn unknown_prop_keys_are_dropped_not_fatal() {
    let (_thread, annotations) =
        handle_over(vec![square(5, json!({ "vendorThing": { "nested": true } }))]);
    block_on(annotations.refresh(token())).unwrap();
    let got = block_on(annotations.get(obj(5))).unwrap().unwrap();
    assert_eq!(got.subtype, "square");
}

#[test]
fn create_then_update_then_delete_is_observed() {
    let (_thread, annotations) = handle_over(vec![]);
    block_on(annotations.refresh(token())).unwrap();
    let events = Arc::new(Events::default());
    let _sub = annotations.subscribe(events.clone());
    let created = block_on(annotations.create(
        CreateAnnotationInput {
            page: PageRef { page_object_number: 1 },
            subtype: "square".into(),
            bounds: Some(Rect { x: 100.0, y: 100.0, width: 100.0, height: 100.0 }),
            ..Default::default()
        },
        token(),
    ))
    .unwrap();
    assert_eq!(created, obj(10));
    block_on(annotations.update(
        created.clone(),
        AnnotationPatch { contents: Some("Edited".into()), ..Default::default() },
        token(),
    ))
    .unwrap();
    block_on(annotations.delete(created, token())).unwrap();
    let seen = events.0.lock().unwrap();
    assert!(seen.iter().any(|e| matches!(e, AnnotationEvent::Created { .. })), "{seen:?}");
    assert!(seen.iter().any(|e| matches!(e, AnnotationEvent::Updated { .. })), "{seen:?}");
    assert!(seen.iter().any(|e| matches!(e, AnnotationEvent::Deleted { .. })), "{seen:?}");
}

#[test]
fn a_props_patch_reaches_the_engine() {
    let (_thread, annotations) = handle_over(vec![square(5, json!({}))]);
    block_on(annotations.refresh(token())).unwrap();
    block_on(annotations.update(
        obj(5),
        AnnotationPatch {
            props: Some(AnnotationPropsPatch { opacity: Some(0.5), ..Default::default() }),
            ..Default::default()
        },
        token(),
    ))
    .unwrap();
}

#[test]
fn a_batch_reports_each_failure_beside_what_applied() {
    let (_thread, annotations) = handle_over(vec![square(5, json!({})), square(6, json!({}))]);
    block_on(annotations.refresh(token())).unwrap();
    let result = block_on(annotations.delete_many(vec![obj(5), obj(99)], token())).unwrap();
    assert_eq!(result.applied, vec![obj(5)]);
    assert_eq!(result.failed.len(), 1);
    assert_eq!(result.failed[0].annotation_ref, obj(99));
}

#[test]
fn a_closed_documents_capability_refuses_with_instance_closed() {
    let (thread, annotations) = handle_over(vec![]);
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    for error in [
        block_on(annotations.list(None)).map(|_| ()).unwrap_err(),
        block_on(annotations.can_create()).map(|_| ()).unwrap_err(),
        block_on(annotations.refresh(token())).unwrap_err(),
    ] {
        assert_eq!(code(&error), Some("instance-closed"), "{error:?}");
    }
}

#[test]
fn closing_the_document_ends_its_subscriptions() {
    let (thread, annotations) = handle_over(vec![]);
    let events = Arc::new(Events::default());
    let _sub = annotations.subscribe(events.clone());
    block_on(thread.call(|_| ())).unwrap();
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    assert_eq!(events.1.load(Ordering::SeqCst), 1);
}

#[test]
fn cancelling_a_create_before_it_runs_creates_nothing() {
    let (thread, annotations) = handle_over(vec![]);
    block_on(annotations.refresh(token())).unwrap();
    let (release, gate) = std::sync::mpsc::channel::<()>();
    let (parked_tx, parked) = std::sync::mpsc::channel::<()>();
    let blocker = thread.call(move |_| {
        parked_tx.send(()).unwrap();
        gate.recv().unwrap();
    });
    parked.recv().unwrap();
    let cancel = token();
    let create = annotations.create(
        CreateAnnotationInput {
            page: PageRef { page_object_number: 1 },
            subtype: "square".into(),
            bounds: Some(Rect { x: 0.0, y: 0.0, width: 10.0, height: 10.0 }),
            ..Default::default()
        },
        cancel.clone(),
    );
    cancel.cancel();
    release.send(()).unwrap();
    block_on(blocker).unwrap();
    let error = block_on(create).unwrap_err();
    assert_eq!(code(&error), Some("operation-cancelled"), "{error:?}");
    assert!(block_on(annotations.list(None)).unwrap().is_empty());
}
```

- [x] **Step 3: Run them to see them fail**

Run: `cargo test -p epdf-plugin-annotation --features uniffi --test ffi`
Expected: compile errors, `could not find ffi in epdf_plugin_annotation`.

- [x] **Step 4: Write `ffi/mod.rs`**

```rust
//! The annotation plugin through uniffi: five handles over the one
//! capability, typed records decoded from the plugin's JSON, one event enum.
//!
//! Not exported (see the plan's Decisions): the hub's pointer and chrome
//! members (`editPointer`, `marqueePointer`, `createPointer`, `hoverAt`,
//! `getHitKind`, `claimsTouchAt`, `getCursorAt`, `listChromeNodes`,
//! `getSelectionAnchor`), the web render projections (`listPageItems`,
//! `listTextItems`, `listLinkItems`, `getAppearanceEpoch`, `getBakeScale`,
//! `renderAppearances`, `pdfToPageRect`, `getCssFontFamily`, `getToolGhost`,
//! `getArmedStampPreview`, `getStampArmEpoch`, `getChromeSettings`,
//! `updateChromeSettings`), the raw DTO members (`listRaw`, `getRaw`,
//! `createRaw`, `updateRaw`, `getSelectionFlags`), the rich-text editing
//! members (`setRichText`, `draftRichText`), and the hooks other plugins call
//! (`registerBehavior`, `pruneEngagedSelection`, `getBehaviorFor`,
//! `commitScriptEffects`, `deliverRemoteEvent`, `setPageViewports`,
//! `remeasurePage`, `distanceCreationPage`, `onDraftCaptured`,
//! `registerTool`, `getResolvedTool`, `listResolvedTools`, `getToolSubtype`).

mod drafts;
mod handle;
mod picker;
mod selection;
mod threads;
mod tools;

use epdf_annotation_core as core;
use epdf_ffi::{
    AnnotationCallout, AnnotationFlags, AnnotationFlagsPatch, AnnotationProps,
    AnnotationPropsPatch, AnnotationRef, CreationDraftAnchor, PageRef, PluginErrorInfo, Point,
    Rect, TextQuad,
};
use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::AnnotationCapability;

pub use drafts::{
    AnnotationAttachmentFile, AnnotationDraftsHandle, AnnotationStampPlacement, AnnotationStampSource,
};
pub use handle::{AnnotationHandle, AnnotationUpdate};
pub use picker::{AnnotationFilePicker, AnnotationFilePromptRequest, AnnotationPickedFile};
pub use selection::{AnnotationSelectionHandle, AnnotationSelectionProps, AnnotationTextFormat};
pub use threads::{
    AnnotationComment, AnnotationCommentPermissions, AnnotationCommentThread,
    AnnotationThreadDeleteFailure, AnnotationThreadDeleteResult, AnnotationThreadsHandle,
    AnnotationThreadStatus,
};
pub use tools::{AnnotationPropSpec, AnnotationTool, AnnotationToolsHandle};

pub(crate) fn decode<T: DeserializeOwned>(value: Value) -> Result<T, Error> {
    serde_json::from_value(value).map_err(|e| {
        Error::named("InvalidState", format!("[annotation] a value does not decode: {e}"))
            .with("code", "invalid-state")
    })
}

pub(crate) fn invalid(message: impl Into<String>) -> Error {
    plugin_error(code::INVALID_INPUT, "annotation", message.into(), None, None)
}

/// The capability, if its document is still open.
pub(crate) fn open(cap: &AnnotationCapability) -> Result<&AnnotationCapability, Error> {
    if cap.lifetime().aborted() {
        return Err(plugin_error(
            code::INSTANCE_CLOSED,
            "annotation",
            "the document this capability belongs to is closed",
            None,
            None,
        ));
    }
    Ok(cap)
}

pub(crate) fn ref_out(r: crate::AnnotationRef) -> Result<AnnotationRef, Error> {
    AnnotationRef::try_from(r)
}

pub(crate) fn refs_out(refs: Vec<crate::AnnotationRef>) -> Result<Vec<AnnotationRef>, Error> {
    refs.into_iter().map(ref_out).collect()
}

fn text(value: &Value) -> Option<String> {
    value.as_str().map(str::to_string)
}

/// Subtype-specific geometry in page space.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum AnnotationGeometry {
    Rect { bounds: Rect, rotation: f64 },
    Line { from: Point, to: Point },
    Polygon { vertices: Vec<Point> },
    Polyline { vertices: Vec<Point> },
    Ink { strokes: Vec<Vec<Point>> },
    Markup { quads: Vec<TextQuad> },
    Caret { bounds: Rect },
    Text { bounds: Rect, rotation: f64, callout: Option<AnnotationCallout> },
}

fn points(v: Vec<core::Point>) -> Vec<Point> {
    v.into_iter().map(Into::into).collect()
}

impl From<crate::AnnotationGeometry> for AnnotationGeometry {
    fn from(g: crate::AnnotationGeometry) -> Self {
        use crate::AnnotationGeometry as G;
        match g {
            G::Rect { bounds, rotation } => AnnotationGeometry::Rect { bounds: bounds.into(), rotation },
            G::Line { from, to } => AnnotationGeometry::Line { from: from.into(), to: to.into() },
            G::Polygon { vertices } => AnnotationGeometry::Polygon { vertices: points(vertices) },
            G::Polyline { vertices } => AnnotationGeometry::Polyline { vertices: points(vertices) },
            G::Ink { strokes } => {
                AnnotationGeometry::Ink { strokes: strokes.into_iter().map(points).collect() }
            }
            G::Markup { quads } => {
                AnnotationGeometry::Markup { quads: quads.into_iter().map(Into::into).collect() }
            }
            G::Caret { bounds } => AnnotationGeometry::Caret { bounds: bounds.into() },
            G::Text { bounds, rotation, callout } => AnnotationGeometry::Text {
                bounds: bounds.into(),
                rotation,
                callout: callout.map(Into::into),
            },
        }
    }
}

/// The page-space record every read returns.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct Annotation {
    pub annotation_ref: AnnotationRef,
    pub page: PageRef,
    pub subtype: String,
    pub bounds: Rect,
    pub geometry: AnnotationGeometry,
    pub props: AnnotationProps,
    pub flags: AnnotationFlags,
    pub contents: Option<String>,
    pub author: Option<String>,
    /// ISO 8601, as the engine reports it.
    pub created_at: Option<String>,
    pub modified_at: Option<String>,
    pub group: Option<AnnotationRef>,
    pub in_reply_to: Option<AnnotationRef>,
    pub can_update: bool,
    pub can_delete: bool,
}

impl TryFrom<crate::Annotation> for Annotation {
    type Error = Error;

    fn try_from(a: crate::Annotation) -> Result<Self, Error> {
        // `props` is the core's flat bag. `AnnotationProps` deserializes it
        // and keeps unknown keys in `extra`, which the record drops.
        let props: core::AnnotationProps = decode(Value::Object(a.props))?;
        let in_reply_to = match a.in_reply_to {
            Value::Null => None,
            other => Some(ref_out(decode(other)?)?),
        };
        Ok(Annotation {
            annotation_ref: ref_out(a.ref_)?,
            page: a.page.into(),
            subtype: a.subtype,
            bounds: a.bounds.into(),
            geometry: a.geometry.into(),
            props: props.into(),
            flags: a.flags.into(),
            contents: text(&a.contents),
            author: text(&a.author),
            created_at: text(&a.created_at),
            modified_at: text(&a.modified_at),
            group: a.group.map(ref_out).transpose()?,
            in_reply_to,
            can_update: a.authority.update,
            can_delete: a.authority.delete,
        })
    }
}

pub(crate) fn annotations_out(list: Vec<crate::Annotation>) -> Result<Vec<Annotation>, Error> {
    list.into_iter().map(Annotation::try_from).collect()
}

/// A geometry write. `rotation: None` keeps the current rotation.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum AnnotationGeometryPatch {
    Rect { bounds: Rect, rotation: Option<f64> },
    Line { from: Point, to: Point },
    Polygon { vertices: Vec<Point> },
    Polyline { vertices: Vec<Point> },
    Ink { strokes: Vec<Vec<Point>> },
    Markup { quads: Vec<TextQuad> },
    /// `remove_callout` drops the leader; `callout` sets one; neither leaves it.
    Text { bounds: Rect, rotation: Option<f64>, callout: Option<AnnotationCallout>, remove_callout: bool },
}

fn core_points(v: Vec<Point>) -> Vec<core::Point> {
    v.into_iter().map(Into::into).collect()
}

impl TryFrom<AnnotationGeometryPatch> for crate::AnnotationGeometryPatch {
    type Error = Error;

    fn try_from(g: AnnotationGeometryPatch) -> Result<Self, Error> {
        use crate::AnnotationGeometryPatch as G;
        use core::js::Nullable;
        Ok(match g {
            AnnotationGeometryPatch::Rect { bounds, rotation } => G::Rect { bounds: bounds.into(), rotation },
            AnnotationGeometryPatch::Line { from, to } => G::Line { from: from.into(), to: to.into() },
            AnnotationGeometryPatch::Polygon { vertices } => G::Polygon { vertices: core_points(vertices) },
            AnnotationGeometryPatch::Polyline { vertices } => G::Polyline { vertices: core_points(vertices) },
            AnnotationGeometryPatch::Ink { strokes } => {
                G::Ink { strokes: strokes.into_iter().map(core_points).collect() }
            }
            AnnotationGeometryPatch::Markup { quads } => {
                G::Markup { quads: quads.into_iter().map(Into::into).collect() }
            }
            AnnotationGeometryPatch::Text { bounds, rotation, callout, remove_callout } => {
                let callout = match (callout, remove_callout) {
                    (Some(_), true) => {
                        return Err(invalid("a text geometry patch both sets and removes its callout"));
                    }
                    (Some(c), false) => Nullable::Value(c.into()),
                    (None, true) => Nullable::Null,
                    (None, false) => Nullable::Undefined,
                };
                G::Text { bounds: bounds.into(), rotation, callout }
            }
        })
    }
}

/// A page-space patch. `None` parts are untouched.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct AnnotationPatch {
    #[uniffi(default = None)]
    pub bounds: Option<Rect>,
    #[uniffi(default = None)]
    pub geometry: Option<AnnotationGeometryPatch>,
    #[uniffi(default = None)]
    pub props: Option<AnnotationPropsPatch>,
    #[uniffi(default = None)]
    pub flags: Option<AnnotationFlagsPatch>,
    #[uniffi(default = None)]
    pub contents: Option<String>,
}

impl TryFrom<AnnotationPatch> for crate::AnnotationPatch {
    type Error = Error;

    fn try_from(p: AnnotationPatch) -> Result<Self, Error> {
        Ok(crate::AnnotationPatch {
            bounds: p.bounds.map(Into::into),
            geometry: p.geometry.map(TryInto::try_into).transpose()?,
            props: p.props.map(TryInto::try_into).transpose()?,
            flags: p.flags.map(Into::into),
            contents: p.contents,
            rich_text: None,
        })
    }
}

/// What `create` takes. `subtype` is the PDF subtype in kebab case
/// (`square`, `ink`, `free-text`); the geometry fields that subtype reads
/// must be set, as `geometry_from_input` checks.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct CreateAnnotationInput {
    pub page: PageRef,
    pub subtype: String,
    #[uniffi(default = None)]
    pub tool: Option<String>,
    #[uniffi(default = None)]
    pub props: Option<AnnotationPropsPatch>,
    #[uniffi(default = None)]
    pub flags: Option<AnnotationFlagsPatch>,
    #[uniffi(default = None)]
    pub select: Option<bool>,
    #[uniffi(default = None)]
    pub bounds: Option<Rect>,
    #[uniffi(default = None)]
    pub rotation: Option<f64>,
    #[uniffi(default = None)]
    pub from: Option<Point>,
    #[uniffi(default = None)]
    pub to: Option<Point>,
    #[uniffi(default = None)]
    pub vertices: Option<Vec<Point>>,
    #[uniffi(default = None)]
    pub strokes: Option<Vec<Vec<Point>>>,
    #[uniffi(default = None)]
    pub quads: Option<Vec<TextQuad>>,
    #[uniffi(default = None)]
    pub callout: Option<AnnotationCallout>,
}

impl TryFrom<CreateAnnotationInput> for crate::CreateAnnotationInput {
    type Error = Error;

    fn try_from(i: CreateAnnotationInput) -> Result<Self, Error> {
        Ok(crate::CreateAnnotationInput {
            page: i.page.into(),
            subtype: i.subtype,
            tool: i.tool,
            props: i.props.map(TryInto::try_into).transpose()?,
            flags: i.flags.map(Into::into),
            select: i.select,
            bounds: i.bounds.map(|r| serde_json::to_value(core::Rect::from(r)).expect("a rect")),
            rotation: i.rotation,
            from: i.from.map(Into::into),
            to: i.to.map(Into::into),
            vertices: i.vertices.map(core_points),
            strokes: i.strokes.map(|s| s.into_iter().map(core_points).collect()),
            quads: i.quads.map(|q| q.into_iter().map(Into::into).collect()),
            callout: i.callout.map(Into::into),
        })
    }
}

/// Every member optional.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct AnnotationFilter {
    #[uniffi(default = None)]
    pub page: Option<PageRef>,
    #[uniffi(default = None)]
    pub subtype: Option<String>,
    #[uniffi(default = None)]
    pub author: Option<String>,
    #[uniffi(default = None)]
    pub group: Option<AnnotationRef>,
}

impl From<AnnotationFilter> for crate::AnnotationFilter {
    fn from(f: AnnotationFilter) -> Self {
        crate::AnnotationFilter {
            page: f.page.map(Into::into),
            subtype: f.subtype,
            author: f.author,
            group: f.group.map(Into::into),
        }
    }
}

/// Whole-document hydration.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum AnnotationHydration {
    Loading,
    Complete,
    Failed { error: PluginErrorInfo },
    /// The scope lacks `doc.annotate.read`.
    Forbidden,
}

impl From<crate::AnnotationHydration> for AnnotationHydration {
    fn from(h: crate::AnnotationHydration) -> Self {
        match h {
            crate::AnnotationHydration::Loading => AnnotationHydration::Loading,
            crate::AnnotationHydration::Complete => AnnotationHydration::Complete,
            crate::AnnotationHydration::Forbidden => AnnotationHydration::Forbidden,
            crate::AnnotationHydration::Error(e) => {
                AnnotationHydration::Failed { error: plugin_error_info(&e) }
            }
        }
    }
}

pub(crate) fn plugin_error_info(error: &Error) -> PluginErrorInfo {
    let info = error.plugin_error_info();
    PluginErrorInfo {
        code: info.get("code").and_then(Value::as_str).unwrap_or("unknown").into(),
        message: info.get("message").and_then(Value::as_str).unwrap_or(&error.message).into(),
        capability: info.get("capability").and_then(Value::as_str).unwrap_or("annotation").into(),
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationBatchFailure {
    pub annotation_ref: AnnotationRef,
    pub error: PluginErrorInfo,
}

/// What a batch did, best effort per entry.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationBatchResult {
    pub applied: Vec<AnnotationRef>,
    pub failed: Vec<AnnotationBatchFailure>,
}

impl TryFrom<crate::BatchResult<crate::AnnotationRef>> for AnnotationBatchResult {
    type Error = Error;

    fn try_from(b: crate::BatchResult<crate::AnnotationRef>) -> Result<Self, Error> {
        Ok(AnnotationBatchResult {
            applied: refs_out(b.applied)?,
            failed: b
                .failed
                .into_iter()
                .map(|(r, e)| {
                    Ok(AnnotationBatchFailure { annotation_ref: ref_out(r)?, error: plugin_error_info(&e) })
                })
                .collect::<Result<_, Error>>()?,
        })
    }
}

/// `createMany`'s batch: each entry is its input's index, since a failed
/// create has no ref.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationCreateFailure {
    pub index: u32,
    pub error: PluginErrorInfo,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationCreateBatchResult {
    pub applied: Vec<AnnotationRef>,
    pub failed: Vec<AnnotationCreateFailure>,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationSnapSettings {
    pub guides: bool,
    pub guide_threshold: f64,
    pub rotation: bool,
    pub rotation_angles: Vec<f64>,
    pub rotation_threshold: f64,
}

impl From<core::SnapSettings> for AnnotationSnapSettings {
    fn from(s: core::SnapSettings) -> Self {
        AnnotationSnapSettings {
            guides: s.guides,
            guide_threshold: s.guide_threshold,
            rotation: s.rotation,
            rotation_angles: s.rotation_angles,
            rotation_threshold: s.rotation_threshold,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct AnnotationSnapSettingsPatch {
    #[uniffi(default = None)]
    pub guides: Option<bool>,
    #[uniffi(default = None)]
    pub guide_threshold: Option<f64>,
    #[uniffi(default = None)]
    pub rotation: Option<bool>,
    #[uniffi(default = None)]
    pub rotation_angles: Option<Vec<f64>>,
    #[uniffi(default = None)]
    pub rotation_threshold: Option<f64>,
}

impl From<AnnotationSnapSettingsPatch> for core::SnapSettingsPatch {
    fn from(p: AnnotationSnapSettingsPatch) -> Self {
        core::SnapSettingsPatch {
            guides: p.guides,
            guide_threshold: p.guide_threshold,
            rotation: p.rotation,
            rotation_angles: p.rotation_angles,
            rotation_threshold: p.rotation_threshold,
        }
    }
}

/// `annotationPlugin(config)`, as Swift sets it. Tool overrides and the
/// chrome look stay Rust-side (see the plan's Decisions).
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct AnnotationConfig {
    #[uniffi(default = None)]
    pub snap: Option<AnnotationSnapSettingsPatch>,
}

impl From<AnnotationConfig> for crate::AnnotationConfig {
    fn from(c: AnnotationConfig) -> Self {
        crate::AnnotationConfig { snap: c.snap.map(Into::into), ..Default::default() }
    }
}

/// A confirmed creation or update, whoever caused it. `annotation` is
/// `None` when the plugin could not project the DTO (an unsupported
/// subtype). The plugin's `origin` is dropped: no foreign caller acts on it.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationChangedEvent {
    pub annotation_ref: AnnotationRef,
    pub page: PageRef,
    pub annotation: Option<Annotation>,
}

impl TryFrom<&crate::AnnotationChangedEvent> for AnnotationChangedEvent {
    type Error = Error;

    fn try_from(e: &crate::AnnotationChangedEvent) -> Result<Self, Error> {
        Ok(AnnotationChangedEvent {
            annotation_ref: ref_out(e.ref_.clone())?,
            page: e.page.into(),
            annotation: e.annotation.clone().map(Annotation::try_from).transpose()?,
        })
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationDeletedEvent {
    pub annotation_ref: AnnotationRef,
    pub page: PageRef,
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum AnnotationResyncedPages {
    All,
    Pages { pages: Vec<PageRef> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum AnnotationThreadChange {
    Reply,
    Text,
    Status,
    Marked,
    Deleted,
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum AnnotationEvent {
    Resynced { pages: AnnotationResyncedPages },
    Created { event: AnnotationChangedEvent },
    Updated { event: AnnotationChangedEvent },
    Deleted { event: AnnotationDeletedEvent },
    SelectionChanged { refs: Vec<AnnotationRef>, previous_refs: Vec<AnnotationRef> },
    DraftChanged { draft: Option<CreationDraftAnchor> },
    EditingChanged { annotation_ref: Option<AnnotationRef> },
    ThreadChanged { root_ref: AnnotationRef, change: AnnotationThreadChange },
}

#[uniffi::export(with_foreign)]
pub trait AnnotationEventListener: Send + Sync {
    fn on_event(&self, event: AnnotationEvent);
    /// The document closed: no event follows. Called once, and at once for
    /// a subscription made after the close.
    fn on_end(&self);
}
```

`Error::plugin_error_info` is the kernel's existing canonical face (`crates/epdf-kernel/src/error.rs`), which `BatchResult::to_json` already uses.

- [x] **Step 5: Write `ffi/handle.rs`**

```rust
use std::sync::Arc;

use epdf_ffi::{AnnotationRef, CancelToken, KernelBound, KernelPost, PageRef, Point, Subscription, ViewerError};
use epdf_kernel::Unsubscribe;

use super::{
    Annotation, AnnotationBatchResult, AnnotationChangedEvent, AnnotationCreateBatchResult,
    AnnotationCreateFailure, AnnotationDeletedEvent, AnnotationEvent, AnnotationEventListener,
    AnnotationFilter, AnnotationHydration, AnnotationPatch, AnnotationResyncedPages,
    AnnotationThreadChange, CreateAnnotationInput, annotations_out, open, plugin_error_info,
    ref_out, refs_out,
};
use crate::AnnotationCapability;

#[derive(uniffi::Object)]
pub struct AnnotationHandle {
    pub(crate) cap: KernelBound<AnnotationCapability>,
    pub(crate) post: KernelPost,
}

impl AnnotationHandle {
    /// On the kernel thread.
    pub fn new(cap: AnnotationCapability, post: KernelPost) -> Arc<Self> {
        Arc::new(AnnotationHandle { cap: KernelBound::new(cap, post.clone()), post })
    }
}

#[uniffi::export]
impl AnnotationHandle {
    pub async fn get(&self, annotation_ref: AnnotationRef) -> Result<Option<Annotation>, ViewerError> {
        self.cap
            .call(move |c| open(c)?.get(&annotation_ref.into()).map(Annotation::try_from).transpose())
            .await
    }

    pub async fn list(&self, filter: Option<AnnotationFilter>) -> Result<Vec<Annotation>, ViewerError> {
        self.cap
            .call(move |c| {
                let filter = filter.map(crate::AnnotationFilter::from);
                annotations_out(open(c)?.list(filter.as_ref()))
            })
            .await
    }

    pub async fn selected(&self) -> Result<Vec<Annotation>, ViewerError> {
        self.cap.call(|c| annotations_out(open(c)?.list_selected())).await
    }

    /// The topmost annotation under a page-space point.
    pub async fn hit_test_at(&self, page: PageRef, point: Point) -> Result<Option<AnnotationRef>, ViewerError> {
        self.cap
            .call(move |c| open(c)?.hit_test_at(page.into(), point.into()).map(ref_out).transpose())
            .await
    }

    /// `"loading"`, `"ready"`, `"forbidden"` or `"error"`, as the plugin
    /// reports its whole-document state.
    pub async fn status(&self) -> Result<String, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.get_status().to_string())).await
    }

    pub async fn hydration(&self) -> Result<AnnotationHydration, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.get_hydration().into())).await
    }

    /// Start hydration if it has not run. Returns before it finishes;
    /// `refresh` waits.
    pub async fn ensure_hydrated(&self) -> Result<(), ViewerError> {
        self.cap.call(|c| Ok(open(c)?.ensure_hydrated())).await
    }

    /// Re-read every annotation from the engine and wait for it.
    pub async fn refresh(&self, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap.call_promise_cancellable(cancel, |c| Ok(open(c)?.refresh()), |()| ()).await
    }

    pub async fn reload_page(&self, page: PageRef, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(cancel, move |c| Ok(open(c)?.reload_page(page.into())), |()| ())
            .await
    }

    /// Resolves with the created annotation's ref once the engine confirmed it.
    pub async fn create(
        &self,
        input: CreateAnnotationInput,
        cancel: Arc<CancelToken>,
    ) -> Result<AnnotationRef, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.create(input.try_into()?)),
                |r| ref_out(r).map_err(ViewerError::from),
            )
            .await?
    }

    pub async fn create_many(
        &self,
        inputs: Vec<CreateAnnotationInput>,
        cancel: Arc<CancelToken>,
    ) -> Result<AnnotationCreateBatchResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let inputs = inputs
                        .into_iter()
                        .map(|i| {
                            let input = crate::CreateAnnotationInput::try_from(i)?;
                            Ok(create_input_json(&input))
                        })
                        .collect::<Result<Vec<_>, epdf_kernel::Error>>()?;
                    Ok(open(c)?.create_many(inputs))
                },
                |batch| {
                    let applied = batch
                        .applied
                        .into_iter()
                        .map(ref_out)
                        .collect::<Result<Vec<_>, _>>()?;
                    let failed = batch
                        .failed
                        .into_iter()
                        .map(|(entry, e)| AnnotationCreateFailure {
                            index: entry.get("index").and_then(serde_json::Value::as_u64).unwrap_or(0) as u32,
                            error: plugin_error_info(&e),
                        })
                        .collect();
                    Ok::<_, ViewerError>(AnnotationCreateBatchResult { applied, failed })
                },
            )
            .await?
    }

    pub async fn update(
        &self,
        annotation_ref: AnnotationRef,
        patch: AnnotationPatch,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.update(annotation_ref.into(), patch.try_into()?)),
                |()| (),
            )
            .await
    }

    pub async fn update_many(
        &self,
        updates: Vec<AnnotationUpdate>,
        cancel: Arc<CancelToken>,
    ) -> Result<AnnotationBatchResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let updates = updates
                        .into_iter()
                        .map(|u| Ok((u.annotation_ref.into(), u.patch.try_into()?)))
                        .collect::<Result<Vec<_>, epdf_kernel::Error>>()?;
                    Ok(open(c)?.update_many(updates))
                },
                |b| AnnotationBatchResult::try_from(b).map_err(ViewerError::from),
            )
            .await?
    }

    pub async fn set_rotation(
        &self,
        annotation_ref: AnnotationRef,
        degrees: f64,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(cancel, move |c| Ok(open(c)?.set_rotation(annotation_ref.into(), degrees)), |()| ())
            .await
    }

    pub async fn rotate_by(
        &self,
        annotation_ref: AnnotationRef,
        delta: f64,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(cancel, move |c| Ok(open(c)?.rotate_by(annotation_ref.into(), delta)), |()| ())
            .await
    }

    pub async fn delete(&self, annotation_ref: AnnotationRef, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(cancel, move |c| Ok(open(c)?.delete(annotation_ref.into())), |()| ())
            .await
    }

    pub async fn delete_many(
        &self,
        refs: Vec<AnnotationRef>,
        cancel: Arc<CancelToken>,
    ) -> Result<AnnotationBatchResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.delete_many(refs.into_iter().map(Into::into).collect())),
                |b| AnnotationBatchResult::try_from(b).map_err(ViewerError::from),
            )
            .await?
    }

    pub async fn can_read(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.can_read())).await
    }

    pub async fn can_create(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.can_create())).await
    }

    pub async fn can_edit(&self, annotation_ref: AnnotationRef) -> Result<bool, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.can_edit(&annotation_ref.into()))).await
    }

    pub async fn can_delete(&self, annotation_ref: AnnotationRef) -> Result<bool, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.can_delete(&annotation_ref.into()))).await
    }

    /// Every hook in one registration. An event that does not decode is
    /// reported through the kernel's report sink, not delivered.
    pub fn subscribe(&self, listener: Arc<dyn AnnotationEventListener>) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.cap.fire(move |c| {
            let ending = listener.clone();
            slot.fill_until_end(
                c.lifetime(),
                || {
                    let l = listener.clone();
                    let resynced = c.on_resynced(move |e| {
                        let pages = match &e.pages {
                            crate::ResyncedPages::All => AnnotationResyncedPages::All,
                            crate::ResyncedPages::Pages(p) => {
                                AnnotationResyncedPages::Pages { pages: p.iter().map(|p| (*p).into()).collect() }
                            }
                        };
                        l.on_event(AnnotationEvent::Resynced { pages });
                        Ok(())
                    });
                    let l = listener.clone();
                    let created = c.on_created(move |e| {
                        l.on_event(AnnotationEvent::Created { event: AnnotationChangedEvent::try_from(e)? });
                        Ok(())
                    });
                    let l = listener.clone();
                    let updated = c.on_updated(move |e| {
                        l.on_event(AnnotationEvent::Updated { event: AnnotationChangedEvent::try_from(e)? });
                        Ok(())
                    });
                    let l = listener.clone();
                    let deleted = c.on_deleted(move |e| {
                        l.on_event(AnnotationEvent::Deleted {
                            event: AnnotationDeletedEvent { annotation_ref: ref_out(e.ref_.clone())?, page: e.page.into() },
                        });
                        Ok(())
                    });
                    let l = listener.clone();
                    let selection = c.on_selection_changed(move |e| {
                        l.on_event(AnnotationEvent::SelectionChanged {
                            refs: refs_out(e.refs.clone())?,
                            previous_refs: refs_out(e.previous_refs.clone())?,
                        });
                        Ok(())
                    });
                    let l = listener.clone();
                    let draft = c.on_draft_changed(move |e| {
                        l.on_event(AnnotationEvent::DraftChanged { draft: e.draft.clone().map(Into::into) });
                        Ok(())
                    });
                    let l = listener.clone();
                    let editing = c.on_editing_changed(move |e| {
                        l.on_event(AnnotationEvent::EditingChanged {
                            annotation_ref: e.ref_.clone().map(ref_out).transpose()?,
                        });
                        Ok(())
                    });
                    let l = listener.clone();
                    let thread = c.comments().on_thread_changed(move |e| {
                        let change = match e.change {
                            crate::ThreadChange::Reply => AnnotationThreadChange::Reply,
                            crate::ThreadChange::Text => AnnotationThreadChange::Text,
                            crate::ThreadChange::Status => AnnotationThreadChange::Status,
                            crate::ThreadChange::Marked => AnnotationThreadChange::Marked,
                            crate::ThreadChange::Deleted => AnnotationThreadChange::Deleted,
                        };
                        l.on_event(AnnotationEvent::ThreadChanged { root_ref: ref_out(e.root_ref.clone())?, change });
                        Ok(())
                    });
                    let offs = [resynced, created, updated, deleted, selection, draft, editing, thread];
                    Unsubscribe::new(move || offs.iter().for_each(Unsubscribe::call))
                },
                move || ending.on_end(),
            );
        });
        subscription
    }
}

/// One entry of `update_many`.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationUpdate {
    pub annotation_ref: AnnotationRef,
    pub patch: AnnotationPatch,
}

/// `createMany` takes JSON inputs (`crud.rs` `create_many(Vec<Value>)`); the
/// input's fields serialize back to the shape `CreateAnnotationInput`
/// deserializes from.
fn create_input_json(input: &crate::CreateAnnotationInput) -> serde_json::Value {
    let mut out = serde_json::json!({ "page": input.page, "subtype": input.subtype });
    let map = out.as_object_mut().expect("an object");
    if let Some(tool) = &input.tool {
        map.insert("tool".into(), tool.clone().into());
    }
    if let Some(props) = &input.props {
        map.insert("props".into(), props.to_json());
    }
    if let Some(flags) = &input.flags {
        map.insert("flags".into(), serde_json::to_value(flags).expect("flags"));
    }
    if let Some(select) = input.select {
        map.insert("select".into(), select.into());
    }
    if let Some(bounds) = &input.bounds {
        map.insert("bounds".into(), bounds.clone());
    }
    if let Some(rotation) = input.rotation {
        map.insert("rotation".into(), rotation.into());
    }
    for (key, value) in [
        ("from", input.from.map(|p| serde_json::to_value(p).expect("a point"))),
        ("to", input.to.map(|p| serde_json::to_value(p).expect("a point"))),
        ("vertices", input.vertices.as_ref().map(|v| serde_json::to_value(v).expect("points"))),
        ("strokes", input.strokes.as_ref().map(|s| serde_json::to_value(s).expect("strokes"))),
        ("quads", input.quads.as_ref().map(|q| serde_json::to_value(q).expect("quads"))),
        ("callout", input.callout.map(|c| serde_json::to_value(c).expect("a callout"))),
    ] {
        if let Some(value) = value {
            map.insert(key.into(), value);
        }
    }
    out
}

```

Before writing `create_input_json`, check the core: `rg -n 'fn to_json' crates/plugins/annotation-core/src/types.rs` must show `AnnotationPropsPatch::to_json`. If it isn't there, serialize the patch with the impl `crates/plugins/annotation-core/src/types.rs` already gives it (`impl Serialize for AnnotationPropsPatch`) and use `serde_json::to_value(props)`.

Leave `selection()`, `tools()`, `drafts()` and `threads()` for Tasks 3 and 4, and create `ffi/{selection,tools,drafts,threads,picker}.rs` with only their `use` lines and the empty handle structs so the module compiles:

```rust
// ffi/selection.rs (Task 3 fills it)
#[derive(uniffi::Object)]
pub struct AnnotationSelectionHandle;
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationSelectionProps { pub mixed: Vec<epdf_ffi::AnnotationPropKey> }
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum AnnotationTextFormat { Bold, Italic, Underline }
```

Do the same for the others with the names `ffi/mod.rs` re-exports. Tasks 3 to 5 replace each file whole.

- [x] **Step 6: Run the tests**

Run: `cargo test -p epdf-plugin-annotation --features uniffi --test ffi`
Expected: 8 passed.

If `unknown_prop_keys_are_dropped_not_fatal` fails with `invalid-state`, `core::AnnotationProps`'s `#[serde(flatten)] extra` did not catch the vendor key, which means the projection put it outside `props`. Read `crates/plugins/annotation/src/repository/projection.rs` and decode from the field it actually lands in. Don't relax the decode to a default.

Run: `cargo test -p epdf-plugin-annotation`
Expected: the conformance vectors still pass with the feature off.

- [x] **Step 7: Commit**

```bash
git add crates/plugins/annotation
git commit -m "Export the annotation plugin's reads, writes and events through uniffi"
```

---

### Task 3: `AnnotationSelectionHandle` and `AnnotationToolsHandle`

**Files:**
- Modify: `crates/plugins/annotation/src/ffi/selection.rs`
- Modify: `crates/plugins/annotation/src/ffi/tools.rs`
- Modify: `crates/plugins/annotation/src/ffi/handle.rs`
- Test: `crates/plugins/annotation/tests/ffi.rs`

**Interfaces:**
- Consumes: `AnnotationHandle { cap, post }` from Task 2.
- Produces: `AnnotationHandle::selection() -> Arc<AnnotationSelectionHandle>`, `AnnotationHandle::tools() -> Arc<AnnotationToolsHandle>`, and the records `AnnotationSelectionProps`, `AnnotationTextFormat`, `AnnotationTool`, `AnnotationPropSpec`.

Members: selection handle `select`, `select_all`, `select_in_rect`, `clear` (`clearSelection`), `refs` (`getSelection`), `props` (`getSelectionProps`), `update` (`updateSelection`), `update_flags` (`updateSelectionFlags`), `delete` (`deleteSelection`), `rotate_by` (`rotateSelectionBy`), `reset_rotation` (`resetSelectionRotation`), `toggle_text_format`, `group`, `ungroup`, `can_group`, `can_ungroup`. Tools handle `tools` (`listTools`), `tool` (`getTool`), `defaults` (`getToolDefaults`), `set_defaults` (`setToolDefaults`), `prop_specs` (`listPropSpecs`), `snap_settings` (`getSnapSettings`), `update_snap_settings`. `getSelectionFlags` returns a free-form `Map<String, Value>` and isn't exported; `update_flags` covers writing them.

- [x] **Step 1: Write the failing tests**

Append to `tests/ffi.rs`:

```rust
use epdf_plugin_annotation::ffi::AnnotationTextFormat;

#[test]
fn selecting_two_annotations_and_grouping_them() {
    let (_thread, annotations) = handle_over(vec![square(5, json!({})), square(6, json!({}))]);
    block_on(annotations.refresh(token())).unwrap();
    let selection = annotations.selection();
    block_on(selection.select(vec![obj(5), obj(6)], None)).unwrap();
    assert_eq!(block_on(selection.refs()).unwrap(), vec![obj(5), obj(6)]);
    assert!(block_on(selection.can_group()).unwrap());
    block_on(selection.clear()).unwrap();
    assert!(block_on(selection.refs()).unwrap().is_empty());
}

#[test]
fn a_selection_props_read_names_what_is_mixed() {
    let (_thread, annotations) = handle_over(vec![
        square(5, json!({ "opacity": 1 })),
        square(6, json!({ "opacity": 0.5 })),
    ]);
    block_on(annotations.refresh(token())).unwrap();
    let selection = annotations.selection();
    block_on(selection.select(vec![obj(5), obj(6)], None)).unwrap();
    let props = block_on(selection.props()).unwrap();
    assert!(props.mixed.contains(&epdf_ffi::AnnotationPropKey::Opacity), "{props:?}");
}

#[test]
fn toggling_bold_without_a_free_text_selection_is_a_no_op() {
    let (_thread, annotations) = handle_over(vec![square(5, json!({}))]);
    block_on(annotations.refresh(token())).unwrap();
    let selection = annotations.selection();
    block_on(selection.select(vec![obj(5)], None)).unwrap();
    block_on(selection.toggle_text_format(AnnotationTextFormat::Bold, token())).unwrap();
}

#[test]
fn the_default_tools_are_listed_with_their_specs() {
    let (_thread, annotations) = handle_over(vec![]);
    let tools = annotations.tools();
    let all = block_on(tools.tools()).unwrap();
    let ink = all.iter().find(|t| t.subtype == "ink").expect("an ink tool");
    let specs = block_on(tools.prop_specs(ink.id.clone())).unwrap();
    assert!(specs.iter().any(|s| s.key == epdf_ffi::AnnotationPropKey::StrokeWidth), "{specs:?}");
    block_on(tools.set_defaults(
        ink.id.clone(),
        epdf_ffi::AnnotationPropsPatch { stroke_width: Some(4.0), ..Default::default() },
    ))
    .unwrap();
    assert_eq!(block_on(tools.defaults(ink.id.clone())).unwrap().stroke_width, 4.0);
}
```

- [x] **Step 2: Run them to see them fail**

Run: `cargo test -p epdf-plugin-annotation --features uniffi --test ffi`
Expected: compile errors, `no method named selection found for struct Arc<AnnotationHandle>`.

- [x] **Step 3: Write `ffi/selection.rs`**

```rust
use std::sync::Arc;

use epdf_ffi::{
    AnnotationFlagsPatch, AnnotationPropKey, AnnotationProps, AnnotationPropsPatch, AnnotationRef,
    CancelToken, KernelBound, PageRef, Rect, ViewerError,
};

use super::{AnnotationBatchResult, decode, open, refs_out};
use crate::AnnotationCapability;

/// The selected annotations: what to select, and what to do to all of them.
#[derive(uniffi::Object)]
pub struct AnnotationSelectionHandle {
    pub(crate) cap: KernelBound<AnnotationCapability>,
}

/// What a property panel shows for the selection: the shared values, and
/// which keys differ across the selected annotations.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationSelectionProps {
    /// `None` when nothing is selected.
    pub values: Option<AnnotationProps>,
    pub editable: Vec<AnnotationPropKey>,
    pub mixed: Vec<AnnotationPropKey>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum AnnotationTextFormat {
    Bold,
    Italic,
    Underline,
}

impl From<AnnotationTextFormat> for crate::TextFormat {
    fn from(f: AnnotationTextFormat) -> Self {
        match f {
            AnnotationTextFormat::Bold => crate::TextFormat::Bold,
            AnnotationTextFormat::Italic => crate::TextFormat::Italic,
            AnnotationTextFormat::Underline => crate::TextFormat::Underline,
        }
    }
}

#[uniffi::export]
impl AnnotationSelectionHandle {
    /// `add` keeps the current selection and adds to it.
    pub async fn select(&self, refs: Vec<AnnotationRef>, add: Option<bool>) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                let refs: Vec<crate::AnnotationRef> = refs.into_iter().map(Into::into).collect();
                open(c)?.select(&refs, add);
                Ok(())
            })
            .await
    }

    pub async fn select_all(&self, page: Option<PageRef>) -> Result<(), ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.select_all(page.map(Into::into)))).await
    }

    pub async fn select_in_rect(&self, page: PageRef, rect: Rect, add: Option<bool>) -> Result<(), ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.select_in_rect(page.into(), rect.into(), add))).await
    }

    pub async fn clear(&self) -> Result<(), ViewerError> {
        self.cap.call(|c| Ok(open(c)?.clear_selection())).await
    }

    pub async fn refs(&self) -> Result<Vec<AnnotationRef>, ViewerError> {
        self.cap.call(|c| refs_out(open(c)?.get_selection())).await
    }

    pub async fn props(&self) -> Result<AnnotationSelectionProps, ViewerError> {
        self.cap
            .call(|c| {
                let read = open(c)?.get_selection_props();
                let values = if read.values.is_empty() {
                    None
                } else {
                    let props: epdf_annotation_core::AnnotationProps =
                        decode(serde_json::Value::Object(read.values))?;
                    Some(props.into())
                };
                Ok(AnnotationSelectionProps {
                    values,
                    editable: read.specs.iter().map(|s| s.key.into()).collect(),
                    mixed: read.mixed.into_iter().map(Into::into).collect(),
                })
            })
            .await
    }

    /// Patch every selected annotation's props.
    pub async fn update(&self, patch: AnnotationPropsPatch, cancel: Arc<CancelToken>) -> Result<AnnotationBatchResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.update_selection(patch.try_into()?)),
                |b| AnnotationBatchResult::try_from(b).map_err(ViewerError::from),
            )
            .await?
    }

    pub async fn update_flags(&self, patch: AnnotationFlagsPatch, cancel: Arc<CancelToken>) -> Result<AnnotationBatchResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.update_selection_flags(patch.into())),
                |b| AnnotationBatchResult::try_from(b).map_err(ViewerError::from),
            )
            .await?
    }

    pub async fn delete(&self, cancel: Arc<CancelToken>) -> Result<AnnotationBatchResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                |c| Ok(open(c)?.delete_selection()),
                |b| AnnotationBatchResult::try_from(b).map_err(ViewerError::from),
            )
            .await?
    }

    pub async fn rotate_by(&self, delta: f64, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap.call_promise_cancellable(cancel, move |c| Ok(open(c)?.rotate_selection_by(delta)), |()| ()).await
    }

    pub async fn reset_rotation(&self, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap.call_promise_cancellable(cancel, |c| Ok(open(c)?.reset_selection_rotation()), |()| ()).await
    }

    pub async fn toggle_text_format(&self, format: AnnotationTextFormat, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(cancel, move |c| Ok(open(c)?.toggle_text_format(format.into())), |()| ())
            .await
    }

    pub async fn group(&self, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap.call_promise_cancellable(cancel, |c| Ok(open(c)?.group()), |()| ()).await
    }

    pub async fn ungroup(&self, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap.call_promise_cancellable(cancel, |c| Ok(open(c)?.ungroup()), |()| ()).await
    }

    pub async fn can_group(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.can_group())).await
    }

    pub async fn can_ungroup(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.can_ungroup())).await
    }
}
```

Check two signatures before running: `rg -n 'pub fn update_selection|pub fn update_selection_flags' -A4 crates/plugins/annotation/src/write/selection.rs`. They take a patch and return `Promise<BatchResult<AnnotationRef>>`. If either returns `Promise<()>`, change that method's return type to `()` and its map to `|()| ()`. Same for `rg -n 'pub struct PropSpec' -A8 crates/plugins/annotation-core/src/kinds.rs`: the code assumes a `key: PropKey` field.

- [x] **Step 4: Write `ffi/tools.rs`**

```rust
use epdf_ffi::{AnnotationFlagsPatch, AnnotationPropKey, AnnotationProps, AnnotationPropsPatch, KernelBound, ViewerError};

use super::{AnnotationSnapSettings, AnnotationSnapSettingsPatch, open};
use crate::AnnotationCapability;

/// The authoring tools and their per-tool defaults.
#[derive(uniffi::Object)]
pub struct AnnotationToolsHandle {
    pub(crate) cap: KernelBound<AnnotationCapability>,
}

/// One authoring tool. Activate it through plan 01's
/// `InteractionHandle.activate_tool(id)`.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationTool {
    pub id: String,
    pub subtype: String,
    pub preset: String,
    pub cursor: String,
    pub enables: Vec<String>,
    pub upright: bool,
    pub flags: Option<AnnotationFlagsPatch>,
}

impl From<crate::AnnotationTool> for AnnotationTool {
    fn from(t: crate::AnnotationTool) -> Self {
        AnnotationTool {
            id: t.id,
            subtype: t.subtype,
            preset: t.preset,
            cursor: t.cursor,
            enables: t.enables,
            upright: t.upright,
            flags: t.flags.map(Into::into),
        }
    }
}

/// Which property a tool's panel shows, and its bounds.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationPropSpec {
    pub key: AnnotationPropKey,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
}

impl From<epdf_annotation_core::PropSpec> for AnnotationPropSpec {
    fn from(s: epdf_annotation_core::PropSpec) -> Self {
        AnnotationPropSpec { key: s.key.into(), min: s.min, max: s.max, step: s.step }
    }
}

#[uniffi::export]
impl AnnotationToolsHandle {
    pub async fn tools(&self) -> Result<Vec<AnnotationTool>, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.list_tools().into_iter().map(Into::into).collect())).await
    }

    pub async fn tool(&self, id: String) -> Result<Option<AnnotationTool>, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.get_tool(&id).map(Into::into))).await
    }

    pub async fn defaults(&self, tool_id: String) -> Result<AnnotationProps, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.get_tool_defaults(&tool_id).into())).await
    }

    pub async fn set_defaults(&self, tool_id: String, patch: AnnotationPropsPatch) -> Result<(), ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.set_tool_defaults(&tool_id, patch.try_into()?))).await
    }

    pub async fn prop_specs(&self, tool_id: String) -> Result<Vec<AnnotationPropSpec>, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.list_prop_specs(&tool_id).into_iter().map(Into::into).collect())).await
    }

    pub async fn snap_settings(&self) -> Result<AnnotationSnapSettings, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.get_snap_settings().into())).await
    }

    pub async fn update_snap_settings(&self, patch: AnnotationSnapSettingsPatch) -> Result<(), ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.update_snap_settings(patch.into()))).await
    }
}
```

The fields of `PropSpec` (`crates/plugins/annotation-core/src/kinds.rs:16`) decide `AnnotationPropSpec`'s. Read the struct and mirror every field it has whose type UniFFI can carry, renaming nothing; drop only a `Value` field, and say so in the record's doc.

- [x] **Step 5: Add the accessors to `AnnotationHandle`**

In `ffi/handle.rs`, inside `#[uniffi::export] impl AnnotationHandle`:

```rust
    pub fn selection(&self) -> Arc<super::AnnotationSelectionHandle> {
        Arc::new(super::AnnotationSelectionHandle { cap: self.cap.clone() })
    }

    pub fn tools(&self) -> Arc<super::AnnotationToolsHandle> {
        Arc::new(super::AnnotationToolsHandle { cap: self.cap.clone() })
    }
```

`KernelBound` is `Clone` (umbrella Task 0c): clones share one slab entry, freed when the last one drops.

- [x] **Step 6: Run the tests**

Run: `cargo test -p epdf-plugin-annotation --features uniffi --test ffi`
Expected: 12 passed.

- [x] **Step 7: Commit**

```bash
git add crates/plugins/annotation
git commit -m "Export the annotation selection and tools handles"
```

---

### Task 4: `AnnotationDraftsHandle` and `AnnotationThreadsHandle`

**Files:**
- Modify: `crates/plugins/annotation/src/ffi/drafts.rs`
- Modify: `crates/plugins/annotation/src/ffi/threads.rs`
- Modify: `crates/plugins/annotation/src/ffi/handle.rs`
- Test: `crates/plugins/annotation/tests/ffi.rs`

**Interfaces:**
- Consumes: `AnnotationHandle { cap, post }`, `KernelBound: Clone` from Task 3.
- Produces: `AnnotationHandle::drafts() -> Arc<AnnotationDraftsHandle>`, `AnnotationHandle::threads() -> Arc<AnnotationThreadsHandle>`; records `AnnotationStampSource`, `AnnotationStampPlacement`, `AnnotationCommentThread`, `AnnotationComment`, `AnnotationCommentPermissions`, `AnnotationThreadStatus`, `AnnotationThreadDeleteResult`.

Members: drafts handle `creation_draft` (`getCreationDraft`), `has_creation_draft`, `finish_creation_draft`, `finish_ink_draft`, `cancel_creation_draft`, `cancel`, `set_contents`, `begin_text_edit`, `begin_text_edit_at`, `end_text_edit`, `editing_ref` (`getEditingRef`), `editing_id` (`getEditingId`), `draft_contents`, `set_text_selection`, `create_markup`, `create_caret`, `create_replace_text`, `preview_markup`, `clear_markup_preview`, `create_from_selection`, `arm_stamp`, `disarm_stamp`, `place_stamp`, `has_armed_stamp`, `place_armed_stamp`, `request_stamp_at`, `hover_ghost_at`, `clear_ghost`, `set_placement_preview`, `clear_placement_preview`, `place_at`, `create_attachment`, `read_attachment`. Threads handle `threads` (`listThreads`), `thread` (`getThread`), `reply`, `set_text`, `set_status`, `set_marked`, `delete`, `delete_thread`, `permissions` (`getPermissions`), `link` (`links.get`), `set_link` (`links.set`), `clear_link` (`links.clear`). `setRichText` and `draftRichText` take a rich-text paragraph tree; they're left for the native editing follow-on with the other text-overlay members.

- [x] **Step 1: Read the signatures this task wraps**

Run each and keep the output open while writing Step 3:

```bash
rg -n 'pub fn (create_markup|create_caret|create_replace_text|preview_markup|create_from_selection)' -A8 crates/plugins/annotation/src/write/markup.rs
rg -n 'pub fn (place_stamp|place_armed_stamp|request_stamp_at)' -A8 crates/plugins/annotation/src/write/stamps.rs
rg -n 'pub fn (begin_text_edit_at|hover_ghost_at)' -A8 crates/plugins/annotation/src/write/text_editing.rs crates/plugins/annotation/src/write/ghost.rs
rg -n 'pub fn (create_attachment|read_attachment)' -A10 crates/plugins/annotation/src/write/icons.rs
```

The code below assumes what those signatures were on 2026-10-06. Where one differs, change only that method's argument conversion.

- [x] **Step 2: Write the failing tests**

Append to `tests/ffi.rs`:

```rust
use epdf_plugin_annotation::ffi::{AnnotationStampPlacement, AnnotationStampSource, AnnotationThreadStatus};

#[test]
fn a_stamp_placed_from_png_bytes_sends_its_bytes() {
    let png = include_bytes!("../../../../tooling/engine-conformance/fixtures/tiny.png").to_vec();
    let (thread, annotations) = handle_over(vec![]);
    block_on(annotations.refresh(token())).unwrap();
    let placed = block_on(annotations.drafts().place_stamp(
        AnnotationStampSource { bytes: png, mime_type: Some("image/png".into()), name: None, subject: None },
        AnnotationStampPlacement {
            page: PageRef { page_object_number: 1 },
            at: epdf_ffi::Point { x: 300.0, y: 400.0 },
            target_width: Some(120.0),
            rotation: None,
        },
        token(),
    ))
    .unwrap();
    assert_eq!(placed, obj(10));
    let sent = block_on(thread.call(|k| k.engine.calls("page.annotations.create"))).unwrap();
    assert!(sent.last().unwrap()[0]["source"].get("$bytes").is_some(), "{sent:?}");
}

#[test]
fn a_reply_lands_in_its_thread() {
    let (_thread, annotations) = handle_over(vec![square(5, json!({}))]);
    block_on(annotations.refresh(token())).unwrap();
    let threads = annotations.threads();
    let reply = block_on(threads.reply(obj(5), "Agreed".into(), token())).unwrap();
    assert_eq!(reply, obj(10));
    let permissions = block_on(threads.permissions(obj(5))).unwrap();
    assert!(permissions.can_reply);
    block_on(threads.set_status(obj(5), AnnotationThreadStatus::Accepted, token())).unwrap();
}

#[test]
fn an_ink_draft_has_nothing_to_finish_before_drawing() {
    let (_thread, annotations) = handle_over(vec![]);
    let drafts = annotations.drafts();
    assert!(!block_on(drafts.has_creation_draft()).unwrap());
    assert_eq!(block_on(drafts.finish_creation_draft(token())).unwrap(), None);
}
```

`tooling/engine-conformance/fixtures/tiny.png` must exist. Run `ls tooling/engine-conformance/fixtures/*.png`; if there is no PNG, add a 1x1 PNG there in this step (`printf '\x89PNG\r\n\x1a\n...'` is fragile, so generate it with `python3 -c 'import zlib,struct;...'` or copy any PNG from `packages/engine/main/test/fixtures/`), and commit it with this task. `FakeEngine::calls(path)` must also exist: `rg -n 'pub fn calls' crates/plugin-conformance/src/fake_engine.rs`. If it doesn't, assert through a `respond_page` closure that pushes `args[0]` into an `Rc<RefCell<Vec<Value>>>` instead.

- [x] **Step 3: Run them to see them fail**

Run: `cargo test -p epdf-plugin-annotation --features uniffi --test ffi`
Expected: compile errors, `no method named drafts`.

- [x] **Step 4: Write `ffi/drafts.rs`**

```rust
use std::sync::Arc;

use epdf_ffi::{AnnotationRef, CancelToken, CreationDraftAnchor, KernelBound, PageRef, Point, Rect, TextQuad, ViewerError};
use epdf_kernel::bytes::Bytes;
use serde_json::json;

use super::{open, ref_out};
use crate::AnnotationCapability;

/// Work in progress: creation drafts, text editing, markup from text,
/// stamps and click placement.
#[derive(uniffi::Object)]
pub struct AnnotationDraftsHandle {
    pub(crate) cap: KernelBound<AnnotationCapability>,
}

/// An image or single-page PDF to stamp. The engine sniffs the format from
/// the bytes; `mime_type` is advisory.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationStampSource {
    pub bytes: Vec<u8>,
    #[uniffi(default = None)]
    pub mime_type: Option<String>,
    #[uniffi(default = None)]
    pub name: Option<String>,
    #[uniffi(default = None)]
    pub subject: Option<String>,
}

impl AnnotationStampSource {
    /// A `BinarySource` as canonical JSON: `{data, mimeType?}`.
    fn source_json(&self) -> serde_json::Value {
        let mut source = json!({ "data": Bytes(self.bytes.clone()).to_json() });
        if let Some(mime) = &self.mime_type {
            source["mimeType"] = mime.clone().into();
        }
        source
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationStampPlacement {
    pub page: PageRef,
    pub at: Point,
    #[uniffi(default = None)]
    pub target_width: Option<f64>,
    #[uniffi(default = None)]
    pub rotation: Option<f64>,
}

impl From<AnnotationStampPlacement> for crate::StampPlacement {
    fn from(p: AnnotationStampPlacement) -> Self {
        crate::StampPlacement { page: p.page.into(), at: p.at.into(), target_width: p.target_width, rotation: p.rotation }
    }
}

/// A file to attach: bytes plus the name the PDF stores (`/UF`).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationAttachmentFile {
    pub bytes: Vec<u8>,
    pub name: String,
    #[uniffi(default = None)]
    pub mime_type: Option<String>,
    #[uniffi(default = None)]
    pub description: Option<String>,
}

#[uniffi::export]
impl AnnotationDraftsHandle {
    pub async fn creation_draft(&self) -> Result<Option<CreationDraftAnchor>, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.get_creation_draft().map(Into::into))).await
    }

    pub async fn has_creation_draft(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.has_creation_draft())).await
    }

    /// Commit the polygon or polyline being drawn. `None` when there was
    /// nothing to finish.
    pub async fn finish_creation_draft(&self, cancel: Arc<CancelToken>) -> Result<Option<AnnotationRef>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                |c| Ok(open(c)?.finish_creation_draft()),
                |r| r.map(ref_out).transpose().map_err(ViewerError::from),
            )
            .await?
    }

    pub async fn finish_ink_draft(&self) -> Result<(), ViewerError> {
        self.cap.call(|c| Ok(open(c)?.finish_ink_draft())).await
    }

    pub async fn cancel_creation_draft(&self) -> Result<(), ViewerError> {
        self.cap.call(|c| Ok(open(c)?.cancel_creation_draft())).await
    }

    /// Drop every gesture in progress: drafts, ghosts, placement previews.
    pub async fn cancel(&self) -> Result<(), ViewerError> {
        self.cap.call(|c| Ok(open(c)?.cancel())).await
    }

    pub async fn set_contents(&self, annotation_ref: AnnotationRef, text: String, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(cancel, move |c| Ok(open(c)?.set_contents(annotation_ref.into(), text)), |()| ())
            .await
    }

    pub async fn begin_text_edit(&self, annotation_ref: AnnotationRef) -> Result<(), ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.begin_text_edit(&annotation_ref.into()))).await
    }

    /// Start editing whatever free text is under a page-space point.
    pub async fn begin_text_edit_at(&self, page: PageRef, point: Point) -> Result<bool, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.begin_text_edit_at(page.into(), point.into()))).await
    }

    pub async fn end_text_edit(&self, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap.call_promise_cancellable(cancel, |c| Ok(open(c)?.end_text_edit()), |()| ()).await
    }

    pub async fn editing_ref(&self) -> Result<Option<AnnotationRef>, ViewerError> {
        self.cap.call(|c| open(c)?.get_editing_ref().map(ref_out).transpose()).await
    }

    pub async fn editing_id(&self) -> Result<Option<String>, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.get_editing_id())).await
    }

    /// Keystroke-level text, kept as a draft until `end_text_edit`.
    pub async fn draft_contents(&self, annotation_ref: AnnotationRef, text: String) -> Result<(), ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.draft_contents(annotation_ref.into(), text))).await
    }

    /// The caret or selection inside the edited text, in UTF-16 code units.
    /// `None` clears it.
    pub async fn set_text_selection(&self, annotation_ref: AnnotationRef, start: Option<u32>, end: Option<u32>) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                let range = start.zip(end).map(|(s, e)| (f64::from(s), f64::from(e)));
                open(c)?.set_text_selection(&annotation_ref.into(), range);
                Ok(())
            })
            .await
    }

    /// Highlight, underline, squiggly or strikeout over text quads.
    pub async fn create_markup(&self, page: PageRef, subtype: String, quads: Vec<TextQuad>, cancel: Arc<CancelToken>) -> Result<AnnotationRef, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.create_markup(page.into(), &subtype, quads.into_iter().map(Into::into).collect())),
                |r| ref_out(r).map_err(ViewerError::from),
            )
            .await?
    }

    /// An insert caret at the end of a glyph. `advance` is `1` toward the
    /// text's end, `-1` toward its start.
    pub async fn create_caret(&self, page: PageRef, glyph_quad: TextQuad, advance: f64) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                let anchor = epdf_annotation_core::TextEndAnchor { glyph_quad: glyph_quad.into(), advance };
                open(c)?.create_caret(page.into(), anchor);
                Ok(())
            })
            .await
    }

    pub async fn create_replace_text(&self, page: PageRef, quads: Vec<TextQuad>, cancel: Arc<CancelToken>) -> Result<AnnotationRef, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.create_replace_text(page.into(), quads.into_iter().map(Into::into).collect())),
                |r| ref_out(r).map_err(ViewerError::from),
            )
            .await?
    }

    pub async fn preview_markup(&self, page: PageRef, subtype: String, quads: Vec<TextQuad>) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| Ok(open(c)?.preview_markup(page.into(), &subtype, quads.into_iter().map(Into::into).collect())))
            .await
    }

    pub async fn clear_markup_preview(&self) -> Result<(), ViewerError> {
        self.cap.call(|c| Ok(open(c)?.clear_markup_preview())).await
    }

    /// Mark the current text selection (needs the selection plugin) with
    /// `subtype`. Empty when nothing is selected.
    pub async fn create_from_selection(&self, subtype: String, cancel: Arc<CancelToken>) -> Result<Vec<AnnotationRef>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.create_from_selection(&subtype)),
                |refs| super::refs_out(refs).map_err(ViewerError::from),
            )
            .await?
    }

    /// Arm a stamp: the next click on a page places it.
    pub async fn arm_stamp(&self, source: AnnotationStampSource, target_width: Option<f64>, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let input = crate::StampToolInput {
                        source: source.source_json(),
                        target_width,
                        name: source.name.clone(),
                        subject: source.subject.clone(),
                        ..Default::default()
                    };
                    Ok(open(c)?.arm_stamp(input))
                },
                |()| (),
            )
            .await
    }

    pub async fn disarm_stamp(&self) -> Result<(), ViewerError> {
        self.cap.call(|c| Ok(open(c)?.disarm_stamp())).await
    }

    pub async fn has_armed_stamp(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.has_armed_stamp())).await
    }

    /// Place a stamp at a point, without arming.
    pub async fn place_stamp(&self, source: AnnotationStampSource, placement: AnnotationStampPlacement, cancel: Arc<CancelToken>) -> Result<AnnotationRef, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let input = crate::StampToolInput {
                        source: source.source_json(),
                        target_width: placement.target_width,
                        name: source.name.clone(),
                        subject: source.subject.clone(),
                        ..Default::default()
                    };
                    open(c)?.place_stamp(input, placement.into())
                },
                |r| ref_out(r).map_err(ViewerError::from),
            )
            .await?
    }

    pub async fn place_armed_stamp(&self, placement: AnnotationStampPlacement, cancel: Arc<CancelToken>) -> Result<Option<AnnotationRef>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.place_armed_stamp(placement.into())),
                |r| r.map(ref_out).transpose().map_err(ViewerError::from),
            )
            .await?
    }

    /// Ask the file-picker port for a stamp image and place it at `point`.
    pub async fn request_stamp_at(&self, page: PageRef, point: Point) -> Result<bool, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.request_stamp_at(page.into(), point.into()))).await
    }

    pub async fn hover_ghost_at(&self, page: PageRef, point: Point) -> Result<(), ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.hover_ghost_at(page.into(), point.into()))).await
    }

    pub async fn clear_ghost(&self) -> Result<(), ViewerError> {
        self.cap.call(|c| Ok(open(c)?.clear_ghost())).await
    }

    pub async fn set_placement_preview(&self, tool_id: String, page: PageRef, bounds: Rect) -> Result<(), ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.set_placement_preview(&tool_id, page.into(), bounds.into()))).await
    }

    pub async fn clear_placement_preview(&self) -> Result<(), ViewerError> {
        self.cap.call(|c| Ok(open(c)?.clear_placement_preview())).await
    }

    /// Click-place with the active tool (a note, a stamp). `false` when the
    /// tool places nothing on click.
    pub async fn place_at(&self, page: PageRef, point: Point, display_rotation: Option<f64>) -> Result<bool, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.place_at(page.into(), point.into(), display_rotation))).await
    }

    pub async fn create_attachment(&self, page: PageRef, point: Point, file: AnnotationAttachmentFile, cancel: Arc<CancelToken>) -> Result<AnnotationRef, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let mut source = json!({ "data": Bytes(file.bytes).to_json(), "name": file.name });
                    if let Some(mime) = file.mime_type {
                        source["mimeType"] = mime.into();
                    }
                    if let Some(description) = file.description {
                        source["description"] = description.into();
                    }
                    Ok(open(c)?.create_attachment(page.into(), point.into(), source))
                },
                |r| ref_out(r).map_err(ViewerError::from),
            )
            .await?
    }

    /// An attachment's embedded bytes.
    pub async fn read_attachment(&self, annotation_ref: AnnotationRef, cancel: Arc<CancelToken>) -> Result<Vec<u8>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.read_attachment(annotation_ref.into())),
                |file| {
                    let data = file.get("data").cloned().unwrap_or(file);
                    Bytes::from_json(&data).map(|b| b.0).ok_or_else(|| {
                        ViewerError::from(super::invalid("an attachment read answered no bytes"))
                    })
                },
            )
            .await?
    }
}
```

- [x] **Step 5: Write `ffi/threads.rs`**

```rust
use std::sync::Arc;

use epdf_ffi::{AnnotationRef, CancelToken, KernelBound, PdfLinkTarget, PluginErrorInfo, ViewerError};
use serde::Deserialize;
use serde_json::Value;

use super::{decode, open, plugin_error_info, ref_out, refs_out};
use crate::AnnotationCapability;

/// Comment threads and the links attached to annotations.
#[derive(uniffi::Object)]
pub struct AnnotationThreadsHandle {
    pub(crate) cap: KernelBound<AnnotationCapability>,
}

/// One comment: a thread's root or a reply.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationComment {
    pub annotation_ref: AnnotationRef,
    pub author: Option<String>,
    pub contents: Option<String>,
    pub modified_at: Option<String>,
}

#[derive(Deserialize)]
struct CommentDto {
    #[serde(rename = "ref")]
    ref_: crate::AnnotationRef,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    contents: Option<String>,
    #[serde(default)]
    modified: Option<String>,
}

fn comment(dto: &Value) -> Result<AnnotationComment, epdf_kernel::Error> {
    let dto: CommentDto = decode(dto.clone())?;
    Ok(AnnotationComment {
        annotation_ref: ref_out(dto.ref_)?,
        author: dto.author,
        contents: dto.contents,
        modified_at: dto.modified,
    })
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationCommentThread {
    pub root: AnnotationComment,
    pub replies: Vec<AnnotationComment>,
    /// The latest review state anyone set (`Accepted`, `Rejected`, ...).
    pub status: Option<String>,
    pub marked_by: Vec<String>,
}

impl TryFrom<crate::CommentThread> for AnnotationCommentThread {
    type Error = epdf_kernel::Error;

    fn try_from(t: crate::CommentThread) -> Result<Self, Self::Error> {
        Ok(AnnotationCommentThread {
            root: comment(&t.root)?,
            replies: t.replies.iter().map(comment).collect::<Result<_, _>>()?,
            status: t.review.last_change.map(|s| s.state),
            marked_by: t.review.marked_by,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct AnnotationCommentPermissions {
    pub can_reply: bool,
    pub can_edit_text: bool,
    pub can_delete: bool,
    pub can_set_status: bool,
    pub can_delete_thread: bool,
}

/// The review states `/StateModel Review` defines.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum AnnotationThreadStatus {
    Accepted,
    Rejected,
    Cancelled,
    Completed,
    None,
}

impl AnnotationThreadStatus {
    fn as_str(self) -> &'static str {
        match self {
            AnnotationThreadStatus::Accepted => "Accepted",
            AnnotationThreadStatus::Rejected => "Rejected",
            AnnotationThreadStatus::Cancelled => "Cancelled",
            AnnotationThreadStatus::Completed => "Completed",
            AnnotationThreadStatus::None => "None",
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationThreadDeleteFailure {
    pub annotation_ref: AnnotationRef,
    pub error: PluginErrorInfo,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationThreadDeleteResult {
    pub deleted: Vec<AnnotationRef>,
    pub failed: Vec<AnnotationThreadDeleteFailure>,
}

#[uniffi::export]
impl AnnotationThreadsHandle {
    pub async fn threads(&self) -> Result<Vec<AnnotationCommentThread>, ViewerError> {
        self.cap
            .call(|c| open(c)?.comments().list_threads().into_iter().map(TryInto::try_into).collect::<Result<_, _>>())
            .await
    }

    /// The thread any member of it belongs to.
    pub async fn thread(&self, annotation_ref: AnnotationRef) -> Result<Option<AnnotationCommentThread>, ViewerError> {
        self.cap
            .call(move |c| open(c)?.comments().get_thread(&annotation_ref.into()).map(TryInto::try_into).transpose())
            .await
    }

    pub async fn reply(&self, annotation_ref: AnnotationRef, text: String, cancel: Arc<CancelToken>) -> Result<AnnotationRef, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.comments().reply(annotation_ref.into(), text)),
                |r| ref_out(r).map_err(ViewerError::from),
            )
            .await?
    }

    pub async fn set_text(&self, annotation_ref: AnnotationRef, text: String, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(cancel, move |c| Ok(open(c)?.comments().set_text(annotation_ref.into(), text)), |()| ())
            .await
    }

    pub async fn set_status(&self, annotation_ref: AnnotationRef, status: AnnotationThreadStatus, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.comments().set_status(annotation_ref.into(), status.as_str().into())),
                |()| (),
            )
            .await
    }

    pub async fn set_marked(&self, annotation_ref: AnnotationRef, marked: bool, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(cancel, move |c| Ok(open(c)?.comments().set_marked(annotation_ref.into(), marked)), |()| ())
            .await
    }

    pub async fn delete(&self, annotation_ref: AnnotationRef, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(cancel, move |c| Ok(open(c)?.comments().delete(annotation_ref.into())), |()| ())
            .await
    }

    pub async fn delete_thread(&self, annotation_ref: AnnotationRef, cancel: Arc<CancelToken>) -> Result<AnnotationThreadDeleteResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.comments().delete_thread(annotation_ref.into())),
                |r| {
                    Ok::<_, ViewerError>(AnnotationThreadDeleteResult {
                        deleted: refs_out(r.deleted)?,
                        failed: r
                            .failed
                            .into_iter()
                            .map(|f| {
                                Ok(AnnotationThreadDeleteFailure {
                                    annotation_ref: ref_out(f.ref_)?,
                                    error: plugin_error_info(&f.error),
                                })
                            })
                            .collect::<Result<_, epdf_kernel::Error>>()?,
                    })
                },
            )
            .await?
    }

    pub async fn permissions(&self, annotation_ref: AnnotationRef) -> Result<AnnotationCommentPermissions, ViewerError> {
        self.cap
            .call(move |c| {
                let p = open(c)?.comments().get_permissions(&annotation_ref.into());
                Ok(AnnotationCommentPermissions {
                    can_reply: p.can_reply,
                    can_edit_text: p.can_edit_text,
                    can_delete: p.can_delete,
                    can_set_status: p.can_set_status,
                    can_delete_thread: p.can_delete_thread,
                })
            })
            .await
    }

    /// The link attached to an annotation (a shape that acts as a link).
    pub async fn link(&self, annotation_ref: AnnotationRef) -> Result<Option<PdfLinkTarget>, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.links().get(&annotation_ref.into()).map(Into::into))).await
    }

    pub async fn set_link(&self, annotation_ref: AnnotationRef, target: PdfLinkTarget, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(cancel, move |c| Ok(open(c)?.links().set(&annotation_ref.into(), target.into())), |()| ())
            .await
    }

    pub async fn clear_link(&self, annotation_ref: AnnotationRef, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(cancel, move |c| Ok(open(c)?.links().clear(&annotation_ref.into())), |()| ())
            .await
    }
}
```

`CommentThread.review.last_change` is `Option<ReviewStatus>`, and `ReviewStatus.state` is a `String` (`crates/plugins/annotation/src/comments/build.rs`). Check with `rg -n 'pub struct ReviewStatus' -A8 crates/plugins/annotation/src/comments/build.rs` before compiling.

- [x] **Step 6: Add the accessors**

In `ffi/handle.rs`:

```rust
    pub fn drafts(&self) -> Arc<super::AnnotationDraftsHandle> {
        Arc::new(super::AnnotationDraftsHandle { cap: self.cap.clone() })
    }

    pub fn threads(&self) -> Arc<super::AnnotationThreadsHandle> {
        Arc::new(super::AnnotationThreadsHandle { cap: self.cap.clone() })
    }
```

- [x] **Step 7: Run the tests**

Run: `cargo test -p epdf-plugin-annotation --features uniffi --test ffi`
Expected: 15 passed.

- [x] **Step 8: Commit**

```bash
git add crates/plugins/annotation tooling/engine-conformance/fixtures
git commit -m "Export the annotation drafts and threads handles"
```

---

### Task 5: The file-picker port

**Files:**
- Modify: `crates/plugins/annotation/src/ffi/picker.rs`
- Modify: `crates/plugins/annotation/src/ffi/handle.rs`
- Test: `crates/plugins/annotation/tests/ffi.rs`

**Interfaces:**
- Consumes: `crate::{FilePickerProvider, FilePromptRequest}` (`contract.rs:675-690`), `AnnotationCapability::set_file_picker_provider`.
- Produces: `#[uniffi::export(with_foreign)] trait AnnotationFilePicker: Send + Sync { async fn pick(&self, request: AnnotationFilePromptRequest) -> Option<AnnotationPickedFile>; }`, records `AnnotationFilePromptRequest`, `AnnotationPickedFile`, and `AnnotationHandle::set_file_picker(picker: Option<Arc<dyn AnnotationFilePicker>>)`. Task 8's Swift implements it with `UIDocumentPickerViewController`.

The provider is called on the kernel thread and returns a kernel `Promise`. The foreign `pick` is an async Swift call that completes on Swift's executor, which the kernel's single-threaded runtime can't poll. The adapter runs it on a short-lived thread with `futures::executor::block_on` and posts the answer back with `KernelPost`. One thread per prompt is fine: a prompt is a user tapping "choose image".

- [ ] **Step 1: Write the failing tests**

Append to `tests/ffi.rs`:

```rust
use epdf_plugin_annotation::ffi::{AnnotationFilePicker, AnnotationFilePromptRequest, AnnotationPickedFile};

struct OnePng(Mutex<Vec<AnnotationFilePromptRequest>>);

#[async_trait::async_trait]
impl AnnotationFilePicker for OnePng {
    async fn pick(&self, request: AnnotationFilePromptRequest) -> Option<AnnotationPickedFile> {
        self.0.lock().unwrap().push(request);
        Some(AnnotationPickedFile {
            bytes: include_bytes!("../../../../tooling/engine-conformance/fixtures/tiny.png").to_vec(),
            name: Some("tiny.png".into()),
            mime_type: Some("image/png".into()),
        })
    }
}

struct Cancels;

#[async_trait::async_trait]
impl AnnotationFilePicker for Cancels {
    async fn pick(&self, _request: AnnotationFilePromptRequest) -> Option<AnnotationPickedFile> {
        None
    }
}

#[test]
fn a_requested_stamp_asks_the_picker_and_places_what_it_returns() {
    let (_thread, annotations) = handle_over(vec![]);
    block_on(annotations.refresh(token())).unwrap();
    let picker = Arc::new(OnePng(Mutex::new(Vec::new())));
    block_on(annotations.set_file_picker(Some(picker.clone()))).unwrap();
    let events = Arc::new(Events::default());
    let _sub = annotations.subscribe(events.clone());
    assert!(block_on(annotations.drafts().request_stamp_at(
        PageRef { page_object_number: 1 },
        epdf_ffi::Point { x: 100.0, y: 100.0 },
    ))
    .unwrap());
    // The pick runs off-thread, then posts back; wait for the create.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !events.0.lock().unwrap().iter().any(|e| matches!(e, AnnotationEvent::Created { .. })) {
        assert!(std::time::Instant::now() < deadline, "no stamp was created");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(picker.0.lock().unwrap().len(), 1);
}

#[test]
fn a_cancelled_pick_places_nothing() {
    let (thread, annotations) = handle_over(vec![]);
    block_on(annotations.refresh(token())).unwrap();
    block_on(annotations.set_file_picker(Some(Arc::new(Cancels)))).unwrap();
    block_on(annotations.drafts().request_stamp_at(
        PageRef { page_object_number: 1 },
        epdf_ffi::Point { x: 100.0, y: 100.0 },
    ))
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(100));
    block_on(thread.call(|_| ())).unwrap();
    assert!(block_on(annotations.list(None)).unwrap().is_empty());
}

#[test]
fn a_pick_that_lands_after_a_tool_change_places_nothing() {
    struct Slow;
    #[async_trait::async_trait]
    impl AnnotationFilePicker for Slow {
        async fn pick(&self, _r: AnnotationFilePromptRequest) -> Option<AnnotationPickedFile> {
            std::thread::sleep(std::time::Duration::from_millis(150));
            Some(AnnotationPickedFile { bytes: vec![0x89, b'P', b'N', b'G'], name: None, mime_type: None })
        }
    }
    let (_thread, annotations) = handle_over(vec![]);
    block_on(annotations.refresh(token())).unwrap();
    block_on(annotations.set_file_picker(Some(Arc::new(Slow)))).unwrap();
    block_on(annotations.drafts().request_stamp_at(
        PageRef { page_object_number: 1 },
        epdf_ffi::Point { x: 100.0, y: 100.0 },
    ))
    .unwrap();
    block_on(annotations.drafts().cancel()).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(block_on(annotations.list(None)).unwrap().is_empty());
}
```

Add `async-trait = "0.1"` to `[dev-dependencies]`. UniFFI's async foreign traits are implemented in Rust tests through `#[async_trait]` (the uniffi 0.32 docs, "Async foreign traits"); check `rg -n 'async_trait' ~/.cargo/registry/src/*/uniffi_macros-0.32.0/src` shows the macro accepts it before relying on it, and if it doesn't, declare the trait method `async fn` and implement it with a plain `async fn` in the test.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p epdf-plugin-annotation --features uniffi --test ffi`
Expected: compile errors, `no method named set_file_picker`.

- [ ] **Step 3: Write `ffi/picker.rs`**

```rust
use std::rc::Rc;
use std::sync::Arc;

use epdf_ffi::{KernelPost, PageRef, Point};
use epdf_kernel::bytes::Bytes;
use epdf_kernel::rt::Promise;
use serde_json::{Value, json};

/// What the picker is asked for: a file for `tool_id` at a page point.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationFilePromptRequest {
    pub tool_id: String,
    pub subtype: String,
    /// A MIME filter (`image/png,image/jpeg,application/pdf`), when the tool has one.
    pub accept: Option<String>,
    pub page: PageRef,
    pub point: Point,
}

impl From<crate::FilePromptRequest> for AnnotationFilePromptRequest {
    fn from(r: crate::FilePromptRequest) -> Self {
        AnnotationFilePromptRequest {
            tool_id: r.tool_id,
            subtype: r.subtype,
            accept: r.accept,
            page: r.page.into(),
            point: r.point.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationPickedFile {
    pub bytes: Vec<u8>,
    pub name: Option<String>,
    pub mime_type: Option<String>,
}

/// The host's file picker: asked for a file when a tool places one (a
/// stamp image, an attachment). `None` is the user cancelling.
#[uniffi::export(with_foreign)]
#[async_trait::async_trait]
pub trait AnnotationFilePicker: Send + Sync {
    async fn pick(&self, request: AnnotationFilePromptRequest) -> Option<AnnotationPickedFile>;
}

/// A `FilePickerProvider` over the foreign picker. Called on the kernel
/// thread; the pick runs on its own thread and its answer is posted back.
pub(crate) fn provider(picker: Arc<dyn AnnotationFilePicker>, post: KernelPost) -> crate::FilePickerProvider {
    Rc::new(move |request: crate::FilePromptRequest| {
        let (promise, resolver) = Promise::<Value>::pending();
        let settle = epdf_ffi::ForeignSettle::new(resolver, post.clone(), "the file picker");
        let picker = picker.clone();
        let request = AnnotationFilePromptRequest::from(request);
        std::thread::spawn(move || {
            let picked = futures::executor::block_on(picker.pick(request));
            // `settle` posts to the kernel thread, where the JSON is built.
            settle.settle(move || {
                Ok(match picked {
                    None => Value::Null,
                    Some(file) => {
                        let mut source = json!({ "data": Bytes(file.bytes).to_json() });
                        if let Some(name) = file.name {
                            source["name"] = name.into();
                        }
                        if let Some(mime) = file.mime_type {
                            source["mimeType"] = mime.into();
                        }
                        source
                    }
                })
            });
        });
        promise
    })
}
```

`Resolver<Value>` is `!Send` (it holds an `Rc`), so it can't ride the spawned thread directly. `epdf_ffi::ForeignSettle` (umbrella Task 0c) parks it on the kernel thread and settles it from any thread. If the picker thread panics before answering, `settle` drops unsettled and the promise rejects with `operation-failed` instead of hanging.

Add `async-trait = "0.1"` to the annotation crate's `[dependencies]` under the `uniffi` feature (`async-trait = { version = "0.1", optional = true }`, and `"dep:async-trait"` in the feature list).

- [ ] **Step 4: Add `set_file_picker`**

In `ffi/handle.rs`, inside the export block:

```rust
    /// Install the host's file picker, or `None` to remove it. Without one,
    /// a tool that prompts for a file declines the click.
    pub async fn set_file_picker(&self, picker: Option<Arc<dyn super::AnnotationFilePicker>>) -> Result<(), ViewerError> {
        let post = self.post.clone();
        self.cap
            .call(move |c| {
                let provider = picker.map(|p| super::picker::provider(p, post));
                // The remover is dropped: the slot is last-write-wins, and
                // `None` clears it.
                let _ = open(c)?.set_file_picker_provider(provider);
                Ok(())
            })
            .await
    }
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p epdf-plugin-annotation --features uniffi --test ffi`
Expected: all pass, including the 3 picker tests (18 in `ffi`).

- [ ] **Step 6: Commit**

```bash
git add crates/plugins/annotation
git commit -m "Add the annotation file-picker port"
```

---

### Task 6: Engine paths: `page.annotations.*` and `annotations.listRawAll`

**Files:**
- Modify: `crates/epdf-viewer-host/src/translate.rs`
- Test: `crates/epdf-viewer-host/tests/translate.rs`
- Copy: `tooling/engine-conformance/fixtures/annotation_kinds.pdf` is read in place; nothing is copied for Rust.

**Interfaces:**
- Consumes: the wire kinds in `crates/epdf-engine/src/host.rs:477-655` (`annotations.listRawPage`, `annotations.listRawAll`, `annotations.create`, `annotations.update`, `annotations.delete`), each reading `pageObjectNumber` (`page_field`), `ref` (flat `crate::identity::AnnotationRef`, `identity.rs:131`), `draft`, `patch`, `resources`.
- Consumes: plan 04's `page.annotations.list` route, `Decode::Annotations`, `flatten_refs` and `nest_refs` in `translate.rs`.
- Produces: `translate::route` rows for `page.annotations.create`, `page.annotations.update`, `page.annotations.delete`, `annotations.listRawAll`; `Decode::AnnotationMutation`.

The kernel spells an annotation ref `{kind, page: {kind: "objectNumber", pageObjectNumber}, annotObjectNumber}`; the host spells it `{kind, pageObjectNumber, annotObjectNumber}`. Plan 04 wrote the rewrite both ways (`flatten_refs`, `nest_refs`). Outgoing args here get every ref flattened; incoming results get every ref nested.

- [ ] **Step 1: Capture the host's real shapes**

Add an ignored test to `crates/epdf-viewer-host/tests/translate.rs` that prints what the host sends for the fixture, then run it once:

```rust
#[test]
#[ignore = "prints the host's annotation JSON; run by hand when the wire changes"]
fn dump_annotation_shapes() {
    let pdf = test_support::fixture("tooling/engine-conformance/fixtures/annotation_kinds.pdf");
    let (doc, call) = on_kernel(move |engine| {
        let opened = engine.open(json!({ "$host": HostTable::put(pdf) }), None);
        opened
    });
    let listing = on_kernel(move |_| doc.call("annotations.listRawAll", None, vec![]));
    println!("{}", serde_json::to_string_pretty(&listing).unwrap());
    let _ = call;
}
```

Use the file's existing `on_kernel` and `open_input` helpers (`tests/translate.rs:211-228`) instead of the sketch's names where they differ: the helper that opens through `NativeEngine` is the one `the_native_engine_opens_a_pdf_and_lists_its_pages` uses. Run:

```bash
cargo test -p epdf-viewer-host --test translate dump_annotation_shapes -- --ignored --nocapture > "$TMPDIR/annotation-shapes.json"
```

Read the output and confirm three things before writing the rewrite: each annotation's `ref` is flat, each annotation has `page` as a number or lacks it, and `pages[].pageState.page` is a number or a `PageRef`. The rewrite below assumes flat refs and numeric pages. Write down any other difference from the vectors' DTO (`square()` in Task 2's test) as a comment at the top of the rewrite, and handle it there.

- [ ] **Step 2: Write the failing tests**

Append to `tests/translate.rs`:

```rust
#[test]
fn refs_are_nested_on_the_way_in_and_flattened_on_the_way_out() {
    let nested = json!({ "kind": "objectNumber",
        "page": { "kind": "objectNumber", "pageObjectNumber": 3 }, "annotObjectNumber": 17 });
    let call = translate("d1", "page.annotations.delete", Some(3), &[nested.clone()], 12).unwrap();
    assert_eq!(
        call.request["ref"],
        json!({ "kind": "objectNumber", "pageObjectNumber": 3, "annotObjectNumber": 17 })
    );
    let mut back = json!({ "created": {
        "ref": { "kind": "objectNumber", "pageObjectNumber": 3, "annotObjectNumber": 17 },
        "page": 3 } });
    epdf_viewer_host::translate::nest_refs(&mut back);
    assert_eq!(back["created"]["ref"], nested);
    assert_eq!(back["created"]["page"], json!({ "kind": "objectNumber", "pageObjectNumber": 3 }));
}

#[test]
fn a_stamp_drafts_bytes_travel_as_a_resource_payload() {
    let draft = json!({ "subtype": "stamp", "source": { "data": { "$bytes": "iVBORw==" }, "mimeType": "image/png" } });
    let call = translate("d1", "page.annotations.create", Some(3), &[draft], 13).unwrap();
    assert_eq!(call.request["kind"], "annotations.create");
    assert_eq!(call.request["pageObjectNumber"], 3);
    assert_eq!(call.request["draft"]["source"], json!({ "resource": "r0" }));
    assert_eq!(call.request["resources"], json!({ "r0": { "payload": 0, "mimeType": "image/png" } }));
    assert_eq!(call.payloads, vec![vec![0x89, b'P', b'N', b'G']]);
}

#[test]
fn an_attachment_drafts_file_travels_as_a_resource_with_its_name() {
    let draft = json!({ "subtype": "file-attachment",
        "file": { "data": { "$bytes": "aGk=" }, "name": "notes.txt" } });
    let call = translate("d1", "page.annotations.create", Some(3), &[draft], 14).unwrap();
    assert_eq!(call.request["draft"]["file"], json!({ "resource": "r0", "name": "notes.txt" }));
    assert_eq!(call.request["resources"]["r0"]["payload"], 0);
    assert_eq!(call.payloads, vec![b"hi".to_vec()]);
}

#[test]
fn an_update_carries_its_ref_and_patch() {
    let nested = json!({ "kind": "nm",
        "page": { "kind": "objectNumber", "pageObjectNumber": 3 }, "nm": "abc" });
    let call = translate("d1", "page.annotations.update", Some(3), &[nested, json!({ "contents": "x" })], 15).unwrap();
    assert_eq!(call.request["ref"], json!({ "kind": "nm", "pageObjectNumber": 3, "nm": "abc" }));
    assert_eq!(call.request["patch"], json!({ "contents": "x" }));
    assert!(matches!(call.decode, Decode::Mutation));
}

#[test]
fn list_raw_all_answers_nested_refs() {
    let json = r#"{"result":{"pages":[{"pageState":{"page":3},"annotations":[{"ref":{"kind":"objectNumber","pageObjectNumber":3,"annotObjectNumber":17},"page":3}]}]}}"#;
    let call = translate("d1", "annotations.listRawAll", None, &[], 16).unwrap();
    let decoded = decode(&call.decode, answer(json)).unwrap().value;
    assert_eq!(decoded["pages"][0]["pageState"]["page"], json!({ "kind": "objectNumber", "pageObjectNumber": 3 }));
    assert_eq!(decoded["pages"][0]["annotations"][0]["ref"]["page"]["pageObjectNumber"], 3);
}
```

- [ ] **Step 3: Run them to see them fail**

Run: `cargo test -p epdf-viewer-host --test translate`
Expected: the new tests fail with code `unsupported` from `translate`.

- [ ] **Step 4: Add the routes and decoders**

In `translate.rs`, extend `Decode` beside plan 04's `Annotations`:

```rust
    /// A mutation whose answer carries annotation refs.
    AnnotationMutation,
```

Extend `route`:

```rust
        "annotations.listRawAll" => ("annotations.listRawAll", Decode::Annotations),
        "page.annotations.create" => ("annotations.create", Decode::AnnotationMutation),
        "page.annotations.update" => ("annotations.update", Decode::AnnotationMutation),
        "page.annotations.delete" => ("annotations.delete", Decode::AnnotationMutation),
```

Add to `PATHS` (umbrella Task 0e), one line each in plan order:

```rust
    "annotations.listRawAll",
    "page.annotations.create",
    "page.annotations.update",
    "page.annotations.delete",
```

Add arms to `translate`'s `extra` match. Don't replace the match: earlier plans' arms are in it. If `translate` doesn't have `let mut payloads = Vec::new();` yet (plan 10 adds it), add it above the match and return `Ok(WireCall { request: Value::Object(request), payloads, decode })`.

```rust
        "annotations.create" => {
            let (draft, resources) = normalize_binary(arg(0), &mut payloads)?;
            vec![("pageObjectNumber", page()?), ("draft", draft), ("resources", resources)]
        }
        "annotations.update" => {
            let mut target = arg(0);
            flatten_refs(&mut target);
            let (patch, resources) = normalize_binary(arg(1), &mut payloads)?;
            vec![("ref", target), ("patch", patch), ("resources", resources)]
        }
        "annotations.delete" => {
            let mut target = arg(0);
            flatten_refs(&mut target);
            vec![("ref", target)]
        }
```

A `resources` of `{}` is dropped by the existing `is_null` filter only if it is `Null`, so have `normalize_binary` answer `Value::Null` when it moved nothing.

Add the helper:

```rust
/// `normalizeAnnotationDraft` (`packages/engine/core/src/annotation/
/// normalize.ts`): a stamp's `source` and an attachment's `file.data`, each
/// a `BinarySource` the kernel spells `{data: {"$bytes": …}, mimeType?,
/// name?}` or bare `{"$bytes": …}`, become `{resource: "rN"}` with the
/// bytes moved into a payload. Every other draft goes as it is.
fn normalize_binary(mut draft: Value, payloads: &mut Vec<Vec<u8>>) -> Result<(Value, Value), Error> {
    let mut resources = Map::new();
    let mut take = |source: &Value, payloads: &mut Vec<Vec<u8>>| -> Result<(String, Option<String>, Option<String>), Error> {
        let data = source.get("data").unwrap_or(source);
        let bytes = epdf_kernel::bytes::Bytes::from_json(data).ok_or_else(|| {
            coded("invalid-input", format!("[epdf-viewer-host] a binary source without bytes: {source}"))
        })?;
        let key = format!("r{}", resources.len());
        let mime = source.get("mimeType").and_then(Value::as_str).map(str::to_string);
        let name = source.get("name").and_then(Value::as_str).map(str::to_string);
        let mut resource = Map::new();
        resource.insert("payload".into(), payloads.len().into());
        if let Some(mime) = &mime {
            resource.insert("mimeType".into(), mime.clone().into());
        }
        resources.insert(key.clone(), Value::Object(resource));
        payloads.push(bytes.0);
        Ok((key, mime, name))
    };
    match draft.get("subtype").and_then(Value::as_str) {
        Some("stamp") => {
            if let Some(source) = draft.get("source").cloned() {
                let (key, _, _) = take(&source, payloads)?;
                draft["source"] = serde_json::json!({ "resource": key });
            }
        }
        Some("file-attachment") => {
            if let Some(file) = draft.get("file").cloned() {
                let (key, mime, name) = take(&file, payloads)?;
                let name = file.get("name").and_then(Value::as_str).map(str::to_string).or(name).ok_or_else(|| {
                    coded("invalid-input", "[epdf-viewer-host] an attachment file needs a name".into())
                })?;
                let mut wire = serde_json::json!({ "resource": key, "name": name });
                if let Some(mime) = file.get("mimeType").and_then(Value::as_str).map(str::to_string).or(mime) {
                    wire["mimeType"] = mime.into();
                }
                if let Some(description) = file.get("description") {
                    wire["description"] = description.clone();
                }
                draft["file"] = wire;
            }
        }
        _ => {}
    }
    let resources = if resources.is_empty() { Value::Null } else { Value::Object(resources) };
    Ok((draft, resources))
}
```

`take` borrows `resources` mutably inside a closure that also gets `payloads` passed in; if the borrow checker refuses it, make `take` a plain `fn` that takes `&mut Map<String, Value>` as well.

In `decode`, add the arm beside `Decode::Mutation`:

```rust
        Decode::AnnotationMutation => {
            let mut value = take(&mut result, "result")?;
            nest_refs(&mut value);
            let mut event = result.remove("event");
            if let Some(event) = event.as_mut() {
                nest_refs(event);
            }
            return Ok(Decoded { value, event });
        }
```

and add `Decode::AnnotationMutation` to the `Decode::Unit => unreachable!` neighbourhood only if the match demands it.

The event a mutation publishes goes to the plugin's document-event subscription (`sync/document_events.rs`), which reads `ref` and `page` the kernel's way too, so it is nested here as well.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p epdf-viewer-host --test translate`
Expected: every test passes, the 5 new ones included.

- [ ] **Step 6: Commit**

```bash
git add crates/epdf-viewer-host/src/translate.rs crates/epdf-viewer-host/tests/translate.rs
git commit -m "Map the annotation engine paths onto the native host"
```

---

### Task 7: Host wiring

**Files:**
- Modify: `crates/epdf-viewer-host/Cargo.toml`
- Modify: `crates/epdf-viewer-host/src/api.rs`
- Modify: `crates/epdf-viewer-host/src/session.rs`
- Modify: `crates/epdf-viewer-host/src/lib.rs`
- Test: `crates/epdf-viewer-host/tests/viewer_host.rs`

**Interfaces:**
- Consumes: `epdf_plugin_annotation::ffi::{AnnotationHandle, AnnotationConfig}`, plan 01's `ViewerConfig.interaction: bool`.
- Produces: `ViewerConfig.annotation: Option<AnnotationConfig>`; `ViewerHost::annotation(document_id: String) -> Result<Arc<AnnotationHandle>, ViewerError>`.

- [ ] **Step 1: Write the failing tests**

Append to `tests/viewer_host.rs`:

```rust
use epdf_plugin_annotation::ffi::{AnnotationConfig, AnnotationPatch, CreateAnnotationInput};

const ANNOTATED: &str = "tooling/engine-conformance/fixtures/annotation_kinds.pdf";

fn with_annotation() -> Arc<ViewerHost> {
    let config = ViewerConfig { interaction: true, annotation: Some(AnnotationConfig::default()), ..Default::default() };
    let host = ViewerHost::new(config, Arc::new(Delegate)).unwrap();
    block_on(host.start()).unwrap();
    host
}

#[test]
fn the_annotation_plugin_hydrates_through_the_native_engine() {
    let host = with_annotation();
    let doc = block_on(host.open_document(test_support::fixture(ANNOTATED), options())).unwrap();
    let annotations = block_on(host.annotation(doc.id)).unwrap();
    block_on(annotations.refresh(CancelToken::new())).unwrap();
    let all = block_on(annotations.list(None)).unwrap();
    assert!(!all.is_empty(), "annotation_kinds.pdf has annotations");
}

#[test]
fn an_annotation_read_back_updates_through_the_native_engine() {
    let host = with_annotation();
    let doc = block_on(host.open_document(test_support::fixture(ANNOTATED), options())).unwrap();
    let annotations = block_on(host.annotation(doc.id)).unwrap();
    block_on(annotations.refresh(CancelToken::new())).unwrap();
    let first = block_on(annotations.list(None)).unwrap().remove(0);
    block_on(annotations.update(
        first.annotation_ref.clone(),
        AnnotationPatch { contents: Some("Edited from Rust".into()), ..Default::default() },
        CancelToken::new(),
    ))
    .unwrap();
    let again = block_on(annotations.get(first.annotation_ref)).unwrap().unwrap();
    assert_eq!(again.contents.as_deref(), Some("Edited from Rust"));
}

#[test]
fn a_square_created_on_report_is_listed() {
    let host = with_annotation();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options())).unwrap();
    let pages = block_on(host.pages(doc.id.clone())).unwrap();
    let annotations = block_on(host.annotation(doc.id)).unwrap();
    block_on(annotations.refresh(CancelToken::new())).unwrap();
    let created = block_on(annotations.create(
        CreateAnnotationInput {
            page: pages[0].page,
            subtype: "square".into(),
            bounds: Some(epdf_ffi::Rect { x: 72.0, y: 72.0, width: 100.0, height: 50.0 }),
            ..Default::default()
        },
        CancelToken::new(),
    ))
    .unwrap();
    assert!(block_on(annotations.get(created)).unwrap().is_some());
}

#[test]
fn a_plugin_without_its_requirement_fails_to_build() {
    let config = ViewerConfig { interaction: false, annotation: Some(AnnotationConfig::default()), ..Default::default() };
    let error = ViewerHost::new(config, Arc::new(Delegate)).err().expect("no interaction plugin");
    assert!(format!("{error}").contains("interaction"), "{error}");
}

#[test]
fn without_its_config_the_annotation_plugin_is_not_installed() {
    let host = started();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options())).unwrap();
    assert!(block_on(host.annotation(doc.id)).is_err());
}
```

Plan 03 owns `a_plugin_without_its_requirement_fails_to_build` (umbrella Review Focus 2). If it already exists in the file under that name, rename this one `annotation_without_interaction_fails_to_build`.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p epdf-viewer-host --test viewer_host`
Expected: compile errors, `no field annotation on ViewerConfig`.

- [ ] **Step 3: Wire it in**

`Cargo.toml`, alphabetically in `[dependencies]`:

```toml
epdf-plugin-annotation = { path = "../plugins/annotation", default-features = false, features = ["uniffi"] }
```

`src/api.rs`, after plan 01's field in `ViewerConfig`:

```rust
    /// Install `annotationPlugin(config)`: read, create, edit and comment
    /// on annotations. Needs `interaction`.
    #[uniffi(default = None)]
    pub annotation: Option<epdf_plugin_annotation::ffi::AnnotationConfig>,
```

`src/session.rs`, in `Session::build` after the render block:

```rust
        if let Some(annotation) = config.annotation {
            plugins.push(epdf_plugin_annotation::annotation_plugin(annotation.into()));
        }
```

`src/lib.rs`: `use epdf_plugin_annotation::ffi::AnnotationHandle;` beside the other handle imports, and in the export block after `render`:

```rust
    /// The annotation capability of an open document. Needs
    /// `ViewerConfig.annotation` and `ViewerConfig.interaction`.
    pub async fn annotation(&self, document_id: String) -> Result<Arc<AnnotationHandle>, ViewerError> {
        let post = self.post.clone();
        self.call(move |s| {
            let cap = s.kernel.capability(&epdf_plugin_annotation::ANNOTATION, Some(&document_id))?;
            Ok(AnnotationHandle::new(cap, post))
        })
        .await
    }
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p epdf-viewer-host --test viewer_host`
Expected: all pass. If `the_annotation_plugin_hydrates_through_the_native_engine` reports `invalid-state`, a field Task 6's dump showed differs from the kernel's DTO and the rewrite missed it. Go back to Task 6 Step 1's output and add that field to `nest_refs`.

Run: `cargo test -p epdf-ffi -p epdf-viewer-host`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add crates/epdf-viewer-host
git commit -m "Install the annotation plugin in the viewer host"
```

---

### Task 8: Swift capability

**Files:**
- Create: `platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Annotation.swift`
- Create: `platforms/apple/EmbedPDF/Tests/EmbedPDFTests/AnnotationTests.swift`
- Modify: `platforms/apple/build-xcframework.sh` (two `FIXTURES` lines)

**Interfaces:**
- Consumes: the generated `AnnotationHandle`, sub-handles, records and `AnnotationEventListener` in `EmbedPDFBindings`; `PDFViewer`, `bridged`, `bridgedCancellable`, `EventRelay`, `StreamRegistry` in `EmbedPDF`.
- Produces: `AnnotationCapability`, `AnnotationSelectionCapability`, `AnnotationToolsCapability`, `AnnotationDraftsCapability`, `AnnotationThreadsCapability`, `PDFViewer.annotations(for:)`, `AnnotationCapability.setFilePicker(_:)` taking a Swift `AnnotationFilePicker`.

- [ ] **Step 1: Build the bindings**

Run: `bash platforms/apple/build-xcframework.sh`
Expected: it finishes, and `rg -n 'class AnnotationHandle' platforms/apple/EmbedPDF/Sources/EmbedPDFBindings` finds the generated class.

- [ ] **Step 2: Write the failing tests**

`platforms/apple/EmbedPDF/Tests/EmbedPDFTests/AnnotationTests.swift`:

```swift
import CustomDump
import EmbedPDF
import Foundation
import Testing

extension BaseSuite {
  @Suite struct AnnotationTests {
    func opened(_ fixture: String = "annotation_kinds") async throws
      -> (PDFViewer, PDFDocument, AnnotationCapability)
    {
      let viewer = try PDFViewer(ViewerConfig(interaction: true, annotation: AnnotationConfig()))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data(fixture))
      let annotations = try await viewer.annotations(for: document)
      try await annotations.refresh()
      return (viewer, document, annotations)
    }

    @Test func hydrationListsTheFixturesAnnotations() async throws {
      let (_, _, annotations) = try await opened()
      let hydration = try await annotations.hydration()
      expectNoDifference(hydration, .complete)
      let all = try await annotations.list()
      #expect(!all.isEmpty)
    }

    @Test func createUpdateDeleteIsObserved() async throws {
      let (viewer, document, annotations) = try await opened("report")
      let page = try await viewer.pages(of: document)[0].page
      let events = annotations.events
      let created = try await annotations.create(
        CreateAnnotationInput(
          page: page, subtype: "square",
          bounds: Rect(x: 72, y: 72, width: 100, height: 50)))
      try await annotations.update(created, AnnotationPatch(contents: "From Swift"))
      let read = try await annotations.get(created)
      expectNoDifference(read?.contents, "From Swift")
      try await annotations.delete(created)
      var sawCreated = false
      for await event in events {
        if case .created = event { sawCreated = true }
        if case .deleted = event { break }
      }
      #expect(sawCreated)
    }

    @Test func aStampFromPngBytesIsCreated() async throws {
      let (viewer, document, annotations) = try await opened("report")
      let page = try await viewer.pages(of: document)[0].page
      let png = try Data(contentsOf: #require(Bundle.module.url(forResource: "tiny", withExtension: "png", subdirectory: "Fixtures")))
      let stamp = try await annotations.drafts.placeStamp(
        AnnotationStampSource(bytes: png, mimeType: "image/png"),
        at: AnnotationStampPlacement(page: page, at: Point(x: 200, y: 200), targetWidth: 80))
      let read = try await annotations.get(stamp)
      expectNoDifference(read?.subtype, "stamp")
    }

    @Test func selectingAndClearing() async throws {
      let (_, _, annotations) = try await opened()
      let first = try #require(try await annotations.list().first)
      try await annotations.selection.select([first.annotationRef])
      let refs = try await annotations.selection.refs()
      expectNoDifference(refs, [first.annotationRef])
      try await annotations.selection.clear()
      let cleared = try await annotations.selection.refs()
      expectNoDifference(cleared, [])
    }

    @Test func aClosedDocumentsCapabilityThrows() async throws {
      let (viewer, document, annotations) = try await opened()
      try await viewer.close(document)
      await #expect {
        _ = try await annotations.list()
      } throws: { error in
        let error = error as? PDFError
        return error?.code == "instance-closed" && error?.name == "PluginError"
      }
    }

    @Test func droppingTheStreamUnsubscribes() async throws {
      let (_, _, annotations) = try await opened()
      let task = Task { for await _ in annotations.events {} }
      try await Task.sleep(for: .milliseconds(20))
      task.cancel()
      _ = await task.value
      try await annotations.refresh()
    }

    @Test func refreshCancelsWhenTheTaskIsCancelled() async throws {
      let (_, _, annotations) = try await opened()
      let task = Task { try await annotations.refresh() }
      task.cancel()
      do {
        try await task.value
      } catch {
        expectNoDifference((error as? PDFError)?.code, "operation-cancelled")
      }
      _ = try await annotations.list()
    }

    @Test func createCancelsWhenTheTaskIsCancelled() async throws {
      let (viewer, document, annotations) = try await opened("report")
      let page = try await viewer.pages(of: document)[0].page
      let before = try await annotations.list().count
      let task = Task {
        try await annotations.create(
          CreateAnnotationInput(page: page, subtype: "square", bounds: Rect(x: 0, y: 0, width: 10, height: 10)))
      }
      task.cancel()
      _ = try? await task.value
      let after = try await annotations.list().count
      #expect(after == before || after == before + 1)
    }
  }
}
```

`createCancelsWhenTheTaskIsCancelled` accepts both counts on purpose. A cancel that lands after the engine write was sent can't recall it (`MetadataHandle.update`'s doc says the same), so the test pins only that the call returns and the capability still answers. The Rust test in Task 2 pins the deterministic before-it-runs case.

- [ ] **Step 3: Run them to see them fail**

Run: `(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17' -only-testing:EmbedPDFTests/BaseSuite/AnnotationTests)`
Expected: build failure, `value of type PDFViewer has no member annotations`.

- [ ] **Step 4: Write `Annotation.swift`**

```swift
public import EmbedPDFBindings
import Foundation

public typealias AnnotationRef = EmbedPDFBindings.AnnotationRef
public typealias AnnotationRevisionToken = EmbedPDFBindings.AnnotationRevisionToken
public typealias AnnotationFlags = EmbedPDFBindings.AnnotationFlags
public typealias AnnotationFlagsPatch = EmbedPDFBindings.AnnotationFlagsPatch
public typealias AnnotationPropKey = EmbedPDFBindings.AnnotationPropKey
public typealias AnnotationProps = EmbedPDFBindings.AnnotationProps
public typealias AnnotationPropsPatch = EmbedPDFBindings.AnnotationPropsPatch
public typealias AnnotationLineEnding = EmbedPDFBindings.AnnotationLineEnding
public typealias AnnotationLineEndings = EmbedPDFBindings.AnnotationLineEndings
public typealias AnnotationLineEndingsPatch = EmbedPDFBindings.AnnotationLineEndingsPatch
public typealias AnnotationBlendMode = EmbedPDFBindings.AnnotationBlendMode
public typealias AnnotationTextAlign = EmbedPDFBindings.AnnotationTextAlign
public typealias AnnotationBorder = EmbedPDFBindings.AnnotationBorder
public typealias AnnotationCallout = EmbedPDFBindings.AnnotationCallout
public typealias PdfLinkTarget = EmbedPDFBindings.PdfLinkTarget
public typealias PdfDestination = EmbedPDFBindings.PdfDestination
public typealias CreationDraftAnchor = EmbedPDFBindings.CreationDraftAnchor
public typealias Annotation = EmbedPDFBindings.Annotation
public typealias AnnotationGeometry = EmbedPDFBindings.AnnotationGeometry
public typealias AnnotationGeometryPatch = EmbedPDFBindings.AnnotationGeometryPatch
public typealias AnnotationPatch = EmbedPDFBindings.AnnotationPatch
public typealias AnnotationUpdate = EmbedPDFBindings.AnnotationUpdate
public typealias CreateAnnotationInput = EmbedPDFBindings.CreateAnnotationInput
public typealias AnnotationFilter = EmbedPDFBindings.AnnotationFilter
public typealias AnnotationHydration = EmbedPDFBindings.AnnotationHydration
public typealias AnnotationBatchResult = EmbedPDFBindings.AnnotationBatchResult
public typealias AnnotationBatchFailure = EmbedPDFBindings.AnnotationBatchFailure
public typealias AnnotationCreateBatchResult = EmbedPDFBindings.AnnotationCreateBatchResult
public typealias AnnotationCreateFailure = EmbedPDFBindings.AnnotationCreateFailure
public typealias AnnotationSnapSettings = EmbedPDFBindings.AnnotationSnapSettings
public typealias AnnotationSnapSettingsPatch = EmbedPDFBindings.AnnotationSnapSettingsPatch
public typealias AnnotationConfig = EmbedPDFBindings.AnnotationConfig
public typealias AnnotationEvent = EmbedPDFBindings.AnnotationEvent
public typealias AnnotationChangedEvent = EmbedPDFBindings.AnnotationChangedEvent
public typealias AnnotationDeletedEvent = EmbedPDFBindings.AnnotationDeletedEvent
public typealias AnnotationResyncedPages = EmbedPDFBindings.AnnotationResyncedPages
public typealias AnnotationThreadChange = EmbedPDFBindings.AnnotationThreadChange
public typealias AnnotationSelectionProps = EmbedPDFBindings.AnnotationSelectionProps
public typealias AnnotationTextFormat = EmbedPDFBindings.AnnotationTextFormat
public typealias AnnotationTool = EmbedPDFBindings.AnnotationTool
public typealias AnnotationPropSpec = EmbedPDFBindings.AnnotationPropSpec
public typealias AnnotationStampSource = EmbedPDFBindings.AnnotationStampSource
public typealias AnnotationStampPlacement = EmbedPDFBindings.AnnotationStampPlacement
public typealias AnnotationAttachmentFile = EmbedPDFBindings.AnnotationAttachmentFile
public typealias AnnotationComment = EmbedPDFBindings.AnnotationComment
public typealias AnnotationCommentThread = EmbedPDFBindings.AnnotationCommentThread
public typealias AnnotationCommentPermissions = EmbedPDFBindings.AnnotationCommentPermissions
public typealias AnnotationThreadStatus = EmbedPDFBindings.AnnotationThreadStatus
public typealias AnnotationThreadDeleteResult = EmbedPDFBindings.AnnotationThreadDeleteResult
public typealias AnnotationFilePromptRequest = EmbedPDFBindings.AnnotationFilePromptRequest
public typealias AnnotationPickedFile = EmbedPDFBindings.AnnotationPickedFile
public typealias AnnotationFilePicker = EmbedPDFBindings.AnnotationFilePicker

/// A document's annotations. Get one with `PDFViewer.annotations(for:)`;
/// the viewer needs `ViewerConfig(interaction: true, annotation:
/// AnnotationConfig())`. It keeps the viewer alive. After its document
/// closes, every call throws `instance-closed`.
public struct AnnotationCapability: Sendable {
  let handle: AnnotationHandle
  let viewer: PDFViewer

  public var selection: AnnotationSelectionCapability {
    AnnotationSelectionCapability(handle: handle.selection())
  }

  public var tools: AnnotationToolsCapability {
    AnnotationToolsCapability(handle: handle.tools())
  }

  public var drafts: AnnotationDraftsCapability {
    AnnotationDraftsCapability(handle: handle.drafts())
  }

  public var threads: AnnotationThreadsCapability {
    AnnotationThreadsCapability(handle: handle.threads())
  }

  public func get(_ ref: AnnotationRef) async throws(PDFError) -> Annotation? {
    try await bridged { try await handle.get(annotationRef: ref) }
  }

  public func list(_ filter: AnnotationFilter? = nil) async throws(PDFError) -> [Annotation] {
    try await bridged { try await handle.list(filter: filter) }
  }

  public func selected() async throws(PDFError) -> [Annotation] {
    try await bridged { try await handle.selected() }
  }

  public func hitTest(at point: Point, on page: PageRef) async throws(PDFError) -> AnnotationRef? {
    try await bridged { try await handle.hitTestAt(page: page, point: point) }
  }

  public func status() async throws(PDFError) -> String {
    try await bridged { try await handle.status() }
  }

  public func hydration() async throws(PDFError) -> AnnotationHydration {
    try await bridged { try await handle.hydration() }
  }

  public func ensureHydrated() async throws(PDFError) {
    try await bridged { try await handle.ensureHydrated() }
  }

  /// Re-read every annotation and wait for it.
  public func refresh() async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.refresh(cancel: token) }
  }

  public func reload(_ page: PageRef) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.reloadPage(page: page, cancel: token) }
  }

  public func create(_ input: CreateAnnotationInput) async throws(PDFError) -> AnnotationRef {
    try await bridgedCancellable { token in try await handle.create(input: input, cancel: token) }
  }

  public func create(_ inputs: [CreateAnnotationInput]) async throws(PDFError) -> AnnotationCreateBatchResult {
    try await bridgedCancellable { token in try await handle.createMany(inputs: inputs, cancel: token) }
  }

  public func update(_ ref: AnnotationRef, _ patch: AnnotationPatch) async throws(PDFError) {
    try await bridgedCancellable { token in
      try await handle.update(annotationRef: ref, patch: patch, cancel: token)
    }
  }

  public func update(_ updates: [AnnotationUpdate]) async throws(PDFError) -> AnnotationBatchResult {
    try await bridgedCancellable { token in try await handle.updateMany(updates: updates, cancel: token) }
  }

  public func setRotation(_ ref: AnnotationRef, degrees: Double) async throws(PDFError) {
    try await bridgedCancellable { token in
      try await handle.setRotation(annotationRef: ref, degrees: degrees, cancel: token)
    }
  }

  public func rotate(_ ref: AnnotationRef, by delta: Double) async throws(PDFError) {
    try await bridgedCancellable { token in
      try await handle.rotateBy(annotationRef: ref, delta: delta, cancel: token)
    }
  }

  public func delete(_ ref: AnnotationRef) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.delete(annotationRef: ref, cancel: token) }
  }

  public func delete(_ refs: [AnnotationRef]) async throws(PDFError) -> AnnotationBatchResult {
    try await bridgedCancellable { token in try await handle.deleteMany(refs: refs, cancel: token) }
  }

  public func canRead() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canRead() }
  }

  public func canCreate() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canCreate() }
  }

  public func canEdit(_ ref: AnnotationRef) async throws(PDFError) -> Bool {
    try await bridged { try await handle.canEdit(annotationRef: ref) }
  }

  public func canDelete(_ ref: AnnotationRef) async throws(PDFError) -> Bool {
    try await bridged { try await handle.canDelete(annotationRef: ref) }
  }

  /// Install the file picker a placing tool asks for an image or a file.
  /// `nil` removes it.
  public func setFilePicker(_ picker: (any AnnotationFilePicker)?) async throws(PDFError) {
    try await bridged { try await handle.setFilePicker(picker: picker) }
  }

  /// Confirmed changes, whoever made them. Finishes when the document
  /// closes (at once if it already has) or the viewer is destroyed.
  public var events: AsyncStream<AnnotationEvent> {
    viewer.registry.stream { handle.subscribe(listener: $0) }
  }
}

public struct AnnotationSelectionCapability: Sendable {
  let handle: AnnotationSelectionHandle

  public func select(_ refs: [AnnotationRef], adding: Bool = false) async throws(PDFError) {
    try await bridged { try await handle.select(refs: refs, add: adding) }
  }

  public func selectAll(on page: PageRef? = nil) async throws(PDFError) {
    try await bridged { try await handle.selectAll(page: page) }
  }

  public func select(in rect: Rect, on page: PageRef, adding: Bool = false) async throws(PDFError) {
    try await bridged { try await handle.selectInRect(page: page, rect: rect, add: adding) }
  }

  public func clear() async throws(PDFError) {
    try await bridged { try await handle.clear() }
  }

  public func refs() async throws(PDFError) -> [AnnotationRef] {
    try await bridged { try await handle.refs() }
  }

  public func props() async throws(PDFError) -> AnnotationSelectionProps {
    try await bridged { try await handle.props() }
  }

  public func update(_ patch: AnnotationPropsPatch) async throws(PDFError) -> AnnotationBatchResult {
    try await bridgedCancellable { token in try await handle.update(patch: patch, cancel: token) }
  }

  public func updateFlags(_ patch: AnnotationFlagsPatch) async throws(PDFError) -> AnnotationBatchResult {
    try await bridgedCancellable { token in try await handle.updateFlags(patch: patch, cancel: token) }
  }

  public func delete() async throws(PDFError) -> AnnotationBatchResult {
    try await bridgedCancellable { token in try await handle.delete(cancel: token) }
  }

  public func rotate(by delta: Double) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.rotateBy(delta: delta, cancel: token) }
  }

  public func resetRotation() async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.resetRotation(cancel: token) }
  }

  public func toggle(_ format: AnnotationTextFormat) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.toggleTextFormat(format: format, cancel: token) }
  }

  public func group() async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.group(cancel: token) }
  }

  public func ungroup() async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.ungroup(cancel: token) }
  }

  public func canGroup() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canGroup() }
  }

  public func canUngroup() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canUngroup() }
  }
}

public struct AnnotationToolsCapability: Sendable {
  let handle: AnnotationToolsHandle

  public func tools() async throws(PDFError) -> [AnnotationTool] {
    try await bridged { try await handle.tools() }
  }

  public func tool(_ id: String) async throws(PDFError) -> AnnotationTool? {
    try await bridged { try await handle.tool(id: id) }
  }

  public func defaults(for toolID: String) async throws(PDFError) -> AnnotationProps {
    try await bridged { try await handle.defaults(toolId: toolID) }
  }

  public func setDefaults(_ patch: AnnotationPropsPatch, for toolID: String) async throws(PDFError) {
    try await bridged { try await handle.setDefaults(toolId: toolID, patch: patch) }
  }

  public func propSpecs(for toolID: String) async throws(PDFError) -> [AnnotationPropSpec] {
    try await bridged { try await handle.propSpecs(toolId: toolID) }
  }

  public func snapSettings() async throws(PDFError) -> AnnotationSnapSettings {
    try await bridged { try await handle.snapSettings() }
  }

  public func updateSnapSettings(_ patch: AnnotationSnapSettingsPatch) async throws(PDFError) {
    try await bridged { try await handle.updateSnapSettings(patch: patch) }
  }
}

public struct AnnotationDraftsCapability: Sendable {
  let handle: AnnotationDraftsHandle

  public func creationDraft() async throws(PDFError) -> CreationDraftAnchor? {
    try await bridged { try await handle.creationDraft() }
  }

  public func hasCreationDraft() async throws(PDFError) -> Bool {
    try await bridged { try await handle.hasCreationDraft() }
  }

  public func finishCreationDraft() async throws(PDFError) -> AnnotationRef? {
    try await bridgedCancellable { token in try await handle.finishCreationDraft(cancel: token) }
  }

  public func finishInkDraft() async throws(PDFError) {
    try await bridged { try await handle.finishInkDraft() }
  }

  public func cancelCreationDraft() async throws(PDFError) {
    try await bridged { try await handle.cancelCreationDraft() }
  }

  public func cancel() async throws(PDFError) {
    try await bridged { try await handle.cancel() }
  }

  public func setContents(_ text: String, of ref: AnnotationRef) async throws(PDFError) {
    try await bridgedCancellable { token in
      try await handle.setContents(annotationRef: ref, text: text, cancel: token)
    }
  }

  public func beginTextEdit(_ ref: AnnotationRef) async throws(PDFError) {
    try await bridged { try await handle.beginTextEdit(annotationRef: ref) }
  }

  public func beginTextEdit(at point: Point, on page: PageRef) async throws(PDFError) -> Bool {
    try await bridged { try await handle.beginTextEditAt(page: page, point: point) }
  }

  public func endTextEdit() async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.endTextEdit(cancel: token) }
  }

  public func editingRef() async throws(PDFError) -> AnnotationRef? {
    try await bridged { try await handle.editingRef() }
  }

  public func editingID() async throws(PDFError) -> String? {
    try await bridged { try await handle.editingId() }
  }

  public func draftContents(_ text: String, of ref: AnnotationRef) async throws(PDFError) {
    try await bridged { try await handle.draftContents(annotationRef: ref, text: text) }
  }

  /// `range` is in UTF-16 code units, as `NSRange` is. `nil` clears it.
  public func setTextSelection(_ range: Range<Int>?, in ref: AnnotationRef) async throws(PDFError) {
    try await bridged {
      try await handle.setTextSelection(
        annotationRef: ref, start: range.map { UInt32($0.lowerBound) }, end: range.map { UInt32($0.upperBound) })
    }
  }

  public func createMarkup(_ subtype: String, quads: [TextQuad], on page: PageRef) async throws(PDFError) -> AnnotationRef {
    try await bridgedCancellable { token in
      try await handle.createMarkup(page: page, subtype: subtype, quads: quads, cancel: token)
    }
  }

  public func createCaret(after glyph: TextQuad, advance: Double, on page: PageRef) async throws(PDFError) {
    try await bridged { try await handle.createCaret(page: page, glyphQuad: glyph, advance: advance) }
  }

  public func createReplaceText(quads: [TextQuad], on page: PageRef) async throws(PDFError) -> AnnotationRef {
    try await bridgedCancellable { token in
      try await handle.createReplaceText(page: page, quads: quads, cancel: token)
    }
  }

  public func previewMarkup(_ subtype: String, quads: [TextQuad], on page: PageRef) async throws(PDFError) {
    try await bridged { try await handle.previewMarkup(page: page, subtype: subtype, quads: quads) }
  }

  public func clearMarkupPreview() async throws(PDFError) {
    try await bridged { try await handle.clearMarkupPreview() }
  }

  public func createFromSelection(_ subtype: String) async throws(PDFError) -> [AnnotationRef] {
    try await bridgedCancellable { token in try await handle.createFromSelection(subtype: subtype, cancel: token) }
  }

  public func armStamp(_ source: AnnotationStampSource, targetWidth: Double? = nil) async throws(PDFError) {
    try await bridgedCancellable { token in
      try await handle.armStamp(source: source, targetWidth: targetWidth, cancel: token)
    }
  }

  public func disarmStamp() async throws(PDFError) {
    try await bridged { try await handle.disarmStamp() }
  }

  public func hasArmedStamp() async throws(PDFError) -> Bool {
    try await bridged { try await handle.hasArmedStamp() }
  }

  public func placeStamp(_ source: AnnotationStampSource, at placement: AnnotationStampPlacement)
    async throws(PDFError) -> AnnotationRef
  {
    try await bridgedCancellable { token in
      try await handle.placeStamp(source: source, placement: placement, cancel: token)
    }
  }

  public func placeArmedStamp(at placement: AnnotationStampPlacement) async throws(PDFError) -> AnnotationRef? {
    try await bridgedCancellable { token in try await handle.placeArmedStamp(placement: placement, cancel: token) }
  }

  public func requestStamp(at point: Point, on page: PageRef) async throws(PDFError) -> Bool {
    try await bridged { try await handle.requestStampAt(page: page, point: point) }
  }

  public func hoverGhost(at point: Point, on page: PageRef) async throws(PDFError) {
    try await bridged { try await handle.hoverGhostAt(page: page, point: point) }
  }

  public func clearGhost() async throws(PDFError) {
    try await bridged { try await handle.clearGhost() }
  }

  public func setPlacementPreview(tool toolID: String, bounds: Rect, on page: PageRef) async throws(PDFError) {
    try await bridged { try await handle.setPlacementPreview(toolId: toolID, page: page, bounds: bounds) }
  }

  public func clearPlacementPreview() async throws(PDFError) {
    try await bridged { try await handle.clearPlacementPreview() }
  }

  public func place(at point: Point, on page: PageRef, displayRotation: Double? = nil) async throws(PDFError) -> Bool {
    try await bridged { try await handle.placeAt(page: page, point: point, displayRotation: displayRotation) }
  }

  public func createAttachment(_ file: AnnotationAttachmentFile, at point: Point, on page: PageRef)
    async throws(PDFError) -> AnnotationRef
  {
    try await bridgedCancellable { token in
      try await handle.createAttachment(page: page, point: point, file: file, cancel: token)
    }
  }

  public func readAttachment(_ ref: AnnotationRef) async throws(PDFError) -> Data {
    try await bridgedCancellable { token in try await handle.readAttachment(annotationRef: ref, cancel: token) }
  }
}

public struct AnnotationThreadsCapability: Sendable {
  let handle: AnnotationThreadsHandle

  public func threads() async throws(PDFError) -> [AnnotationCommentThread] {
    try await bridged { try await handle.threads() }
  }

  public func thread(_ ref: AnnotationRef) async throws(PDFError) -> AnnotationCommentThread? {
    try await bridged { try await handle.thread(annotationRef: ref) }
  }

  public func reply(to ref: AnnotationRef, _ text: String) async throws(PDFError) -> AnnotationRef {
    try await bridgedCancellable { token in try await handle.reply(annotationRef: ref, text: text, cancel: token) }
  }

  public func setText(_ text: String, of ref: AnnotationRef) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.setText(annotationRef: ref, text: text, cancel: token) }
  }

  public func setStatus(_ status: AnnotationThreadStatus, of ref: AnnotationRef) async throws(PDFError) {
    try await bridgedCancellable { token in
      try await handle.setStatus(annotationRef: ref, status: status, cancel: token)
    }
  }

  public func setMarked(_ marked: Bool, _ ref: AnnotationRef) async throws(PDFError) {
    try await bridgedCancellable { token in
      try await handle.setMarked(annotationRef: ref, marked: marked, cancel: token)
    }
  }

  public func delete(_ ref: AnnotationRef) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.delete(annotationRef: ref, cancel: token) }
  }

  public func deleteThread(_ ref: AnnotationRef) async throws(PDFError) -> AnnotationThreadDeleteResult {
    try await bridgedCancellable { token in try await handle.deleteThread(annotationRef: ref, cancel: token) }
  }

  public func permissions(_ ref: AnnotationRef) async throws(PDFError) -> AnnotationCommentPermissions {
    try await bridged { try await handle.permissions(annotationRef: ref) }
  }

  public func link(_ ref: AnnotationRef) async throws(PDFError) -> PdfLinkTarget? {
    try await bridged { try await handle.link(annotationRef: ref) }
  }

  public func setLink(_ target: PdfLinkTarget, on ref: AnnotationRef) async throws(PDFError) {
    try await bridgedCancellable { token in
      try await handle.setLink(annotationRef: ref, target: target, cancel: token)
    }
  }

  public func clearLink(_ ref: AnnotationRef) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.clearLink(annotationRef: ref, cancel: token) }
  }
}

extension EventRelay: AnnotationEventListener where Event == AnnotationEvent {
  func onEvent(event: AnnotationEvent) {
    continuation.yield(event)
  }

  func onEnd() {
    continuation.finish()
  }
}

extension PDFViewer {
  public func annotations(for document: PDFDocument) async throws(PDFError) -> AnnotationCapability {
    let handle = try await bridged { try await host.annotation(documentId: document.id) }
    return AnnotationCapability(handle: handle, viewer: self)
  }
}
```

UniFFI renames `annotation_ref` to `annotationRef` and `tool_id` to `toolId` in the generated Swift. If the compiler reports a different label for any call, use the generated one; the generated file is the source of truth.

- [ ] **Step 5: Add the fixtures and run the tests**

In `platforms/apple/build-xcframework.sh`, add to the `FIXTURES` list (umbrella Task 0f), one line each, after the last plan's lines:

```bash
  tooling/engine-conformance/fixtures/annotation_kinds.pdf
  tooling/engine-conformance/fixtures/tiny.png
```

The `Fixtures` directories are ignored (`platforms/apple/.gitignore`), so a file copied there by hand is gone on the next build and never committed. Then:

```bash
bash platforms/apple/build-xcframework.sh
(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17' -only-testing:EmbedPDFTests/BaseSuite/AnnotationTests)
```

Expected: 8 tests pass.

If plan 03 (selection) has merged into `research/swift-plugins`, add the cross-plugin test now:

```swift
    @Test func createFromSelectionMarksTheSelectedText() async throws {
      let viewer = try PDFViewer(ViewerConfig(interaction: true, selection: true, annotation: AnnotationConfig()))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      let page = try await viewer.pages(of: document)[0].page
      let selection = try await viewer.selection(for: document)
      try await selection.selectPage(page)
      let annotations = try await viewer.annotations(for: document)
      try await annotations.refresh()
      let refs = try await annotations.drafts.createFromSelection("highlight")
      #expect(!refs.isEmpty)
    }
```

Use plan 03's real method name for selecting a page (`rg -n 'func selectPage\|func select' platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Selection.swift`).

- [ ] **Step 6: Run the whole Swift suite**

Run: `(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17')`
Expected: every suite passes.

- [ ] **Step 7: Commit**

```bash
git add platforms/apple/build-xcframework.sh platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Annotation.swift platforms/apple/EmbedPDF/Tests/EmbedPDFTests
git commit -m "Add EmbedPDF's AnnotationCapability"
```

---

### Task 9: Docs, CI, merge and cleanup

**Files:**
- Modify: `.github/workflows/plugin-rust.yml`
- Modify: `platforms/apple/README.md`
- Modify: `docs/superpowers/plans/2026-10-01-swift-package.md` (Appendix A §A.3, the annotation entry)

- [ ] **Step 1: CI and README**

Append `annotation` to the `crates=( … )` list in `plugin-rust.yml`'s "Test the uniffi exports" step, after plan 01's `interaction`. Add a row to the README's plugin table:

```markdown
| annotation | `AnnotationCapability` | `interaction: true, annotation: AnnotationConfig()` | `2026-10-06-swift-plugins/05-annotation.md` |
```

In the spec's §A.3, change `**annotation** (D, requires …)` to begin `**annotation** (D), delivered by 05-annotation.md, requires …`.

- [ ] **Step 2: Record the binary size**

```bash
ls -l target/aarch64-apple-ios-sim/release/libepdf_viewer_host.a
ls -l ../swift-plugins-integration/target/aarch64-apple-ios-sim/release/libepdf_viewer_host.a
```

The first is this plan's archive, the second the integration branch's last build. Put both sizes in the commit message below.

- [ ] **Step 3: Run the definition of done**

```bash
cargo test -p epdf-plugin-annotation --features uniffi
cargo test -p epdf-ffi -p epdf-viewer-host
cargo clippy -p epdf-plugin-annotation -p epdf-viewer-host --all-targets --features epdf-plugin-annotation/uniffi -- -D warnings
cargo test -p epdf-plugin-annotation
bash platforms/apple/build-xcframework.sh
(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17')
cargo build -p epdf-viewer-host -p uniffi-bindgen
./target/debug/uniffi-bindgen generate --library target/debug/libepdf_viewer_host.dylib --language kotlin --no-format --out-dir "$TMPDIR/epdf-kotlin-check"
```

Expected: every command exits 0.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/plugin-rust.yml platforms/apple/README.md docs/superpowers/plans/2026-10-01-swift-package.md
git commit -m "List the annotation slice in CI and the README

libepdf_viewer_host.a (aarch64-apple-ios-sim, release): <before> -> <after> bytes."
```

- [ ] **Step 5: Merge**

From the integration worktree (`.claude/worktrees/swift-plugins-integration`):

```bash
git merge --no-ff research/swift-annotation
```

Resolve conflicts in the umbrella's shared files by keeping both blocks in plan-number order.

- [ ] **Step 6: Rerun after the merge**

On `research/swift-plugins`:

```bash
bash platforms/apple/build-xcframework.sh
(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17')
cargo build -p epdf-viewer-host -p uniffi-bindgen
./target/debug/uniffi-bindgen generate --library target/debug/libepdf_viewer_host.dylib --language kotlin --no-format --out-dir "$TMPDIR/epdf-kotlin-check"
```

Expected: all pass.

- [ ] **Step 7: Remove the worktree and branch**

```bash
git worktree remove .claude/worktrees/swift-annotation
git branch -d research/swift-annotation
git worktree prune
```

Don't pass `--force`. If either refuses, find out what was left uncommitted or unmerged first. Then tick 05 in the umbrella's Tasks 1 to 16 list.
