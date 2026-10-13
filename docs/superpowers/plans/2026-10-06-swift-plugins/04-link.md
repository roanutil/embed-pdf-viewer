# Link Swift Wrapper Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Export the link plugin (a page's link areas, what each one targets, and activating them) through UniFFI, map the one engine path it reads, wire it into `ViewerHost`, and give Swift a `LinkCapability` that tells the app when a link wants a URL opened.

**Architecture:** The recipe from `2026-10-01-swift-package.md` Appendix A §A.0, with metadata and search as models. A link's target and destination are `epdf-annotation-core` types (`PdfLinkTarget`, `PdfDestination`, `AnnotationRef`), which annotation (plan 05), form and signature return too, so their mirrors go into a new `crates/epdf-ffi/src/annotation.rs`. Link's own records live in `crates/plugins/link/src/ffi.rs` under a `Link` prefix. Without the annotation plugin, link reads a page's annotations itself through the kernel path `page.annotations.list`, which `translate.rs` doesn't map yet; this plan maps it to the wire kind `annotations.listRawPage`. A `goto` link reveals through the stage when stage is installed, so link's resolution record reuses plan 02's `StageRevealOptions`.

**Tech Stack:** Rust 2024, `uniffi =0.32.0`, Swift 6.2 tools, Swift Testing, `swift-custom-dump` 1.3, Xcode 27.0, iPhone 17 simulator (iOS 26.0).

**Spec:** `docs/superpowers/plans/2026-10-06-swift-plugins/00-umbrella.md` and `docs/superpowers/plans/2026-10-01-swift-package.md` (Global Constraints, Decisions, Appendix A §A.0 and §A.2, Appendix C). Plans 01 (`01-interaction.md`, required by link) and 02 (`02-stage.md`, whose `StageRevealOptions` link's resolution carries) must be merged first.

## Global Constraints

Everything in the umbrella's Global Constraints applies. The ones this plan meets first:

- `uniffi` is pinned at `=0.32.0`.
- UniFFI names are unique across the workspace. `PdfLinkTarget`, `PdfDestination`, `AnnotationRef` and `AnnotationRevisionToken` are written once, in `crates/epdf-ffi/src/annotation.rs`, because plan 05 and later plans return them. Link's records start with `Link`.
- No `serde_json::Value` crosses. A link's `target` is JSON in the plugin (`LinkNavItem.target`); the FFI decodes it into `PdfLinkTarget`, and a target that doesn't decode is an `invalid-state` error. A link's `/A` action tree (`LinkNavItem.activate`) stays in Rust: Swift activates a link by its page and id, and the handle looks the link up again so its action tree goes with it.
- `PdfDestination`'s `Nullable<f64>` fields (absent, `null`, or a number) cross as `Option<f64>`. Absent and `null` both mean "keep the current value" to every consumer in this repo (`crates/plugins/stage/src/destination.rs`), so the collapse loses nothing anyone reads. Going back to Rust, `None` becomes absent.
- Engine paths a plan maps are its own (umbrella, Shared files). This plan maps `page.annotations.list` only; plan 05 maps `create`, `update`, `delete` and `annotations.listRawAll`.
- `epdf-ffi` can depend on `epdf-annotation-core` (default features off): annotation-core doesn't depend on `epdf-ffi`, so there's no cycle.

## Decisions

`activate` takes a `LinkInput` that is either a bare target or a link named by page and id, not the plugin's `LinkInput::Link(Box<LinkNavItem>)`. A Swift copy of a link can't carry the action tree, and activation without it would skip a link's JavaScript and named actions. Looking the link up by id on the kernel thread keeps activation identical to the web's.

A `Dispatched` activation crosses without its promise. When a link has an action tree and the actions plugin is installed, the plugin hands the tree to actions and returns at once with an in-flight dispatch. Plan 15 exposes action results through actions' own events; until then Swift learns only that the link dispatched.

`LinkActivateContext` doesn't cross. It lets a caller override a link's action tree, annotation ref or page, which only the annotation plugin's own glue needs (`crates/plugins/link/src/controller.rs` `activate`).

Plan 04 depends on plan 02, not only on plan 01 as the umbrella's waves say. Link's crate depends on stage's unconditionally, and `LinkResolution::Reveal` carries stage's `RevealOptions`, so mirroring it means using `StageRevealOptions` from plan 02. Writing a second mirror of the same type here would collide in `EmbedPDFBindings`. The umbrella's wave table should move 04 after 02; see the report.

## Review Focus

1. A link whose target the engine reports with a `kind` this build doesn't know. Expected: listing the page's links throws `invalid-state` naming the target, rather than dropping the link silently or crashing. Pinned in Task 3 (`a_target_that_does_not_decode_is_invalid_state`).
2. A `uri` link activated. Expected: the activation answers `Uri { uri }` and the `Activated` event carries it, so the app opens it; the plugin itself never opens anything. Pinned in Task 3 (`a_uri_link_is_reported_for_the_host`) and Task 6 (`aURILinkIsReportedToTheApp`).
3. Two overlapping links under one point. Expected: `link_at` answers the smaller one, as the TypeScript does. Pinned in Task 3 (`the_smallest_link_under_a_point_wins`).
4. A page's links read before its annotations load. Expected: `links(page)` answers empty until `ensure_loaded(page)` settles, then the links; `is_loaded` tells the two apart. Pinned in Task 3 (`links_appear_once_the_page_loads`).
5. A `goto` link activated with no stage installed. Expected: the activation answers `Destination { destination }` for the app to handle, not an error. Pinned in Task 3 (`without_a_stage_a_goto_link_reports_its_destination`).

## Cross-plugin tests

- `interaction` (plan 01, required): this plan owns `a_tap_on_a_link_activates_it` (Task 5).
- `stage` (plan 02, optional): this plan owns `with_a_stage_a_goto_link_reveals_its_page` (Task 3) and `activating_a_goto_link_moves_the_stage` (Task 5).
- `annotation` (plan 05, optional): when annotation is installed, link reads links from it instead of the engine. Plan 05 owns a test that `LinkCapability.links(on:)` answers the same links with annotation installed.
- `actions` (plan 15, optional): plan 15 owns a test that activating a link with an action tree answers `dispatched` and runs the tree.

## File Structure

```
crates/epdf-ffi/Cargo.toml              epdf-annotation-core dependency
crates/epdf-ffi/src/annotation.rs       PdfLinkTarget, PdfDestination, AnnotationRef, AnnotationRevisionToken
crates/epdf-ffi/tests/annotation.rs
crates/plugins/link/Cargo.toml          uniffi feature
crates/plugins/link/src/lib.rs          scaffolding, ffi module
crates/plugins/link/src/controller.rs   lifetime()
crates/plugins/link/src/ffi.rs          LinkHandle, records, events
crates/plugins/link/tests/ffi.rs
crates/epdf-viewer-host/src/translate.rs     page.annotations.list
crates/epdf-viewer-host/tests/translate.rs
crates/epdf-viewer-host/{Cargo.toml,src/api.rs,src/session.rs,src/lib.rs}
crates/epdf-viewer-host/tests/viewer_host.rs
platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Link.swift
platforms/apple/EmbedPDF/Tests/EmbedPDFTests/LinkTests.swift
```

---

### Task 1: Annotation-core records in `epdf-ffi`

**Files:**
- Modify: `crates/epdf-ffi/Cargo.toml`, `crates/epdf-ffi/src/lib.rs`
- Create: `crates/epdf-ffi/src/annotation.rs`, `crates/epdf-ffi/tests/annotation.rs`

**Interfaces:**
- Consumes: `epdf_annotation_core::{PageRef, PdfDestination, PdfLinkTarget, AnnotationRef, RevisionToken, Nullable}` (`crates/plugins/annotation-core/src/types.rs`, `js.rs`).
- Produces: `epdf_ffi::{PdfLinkTarget, PdfDestination, AnnotationRef, AnnotationRevisionToken}` with `From<epdf_ffi::PdfLinkTarget> for epdf_annotation_core::PdfLinkTarget`, `From<&epdf_annotation_core::PdfLinkTarget> for epdf_ffi::PdfLinkTarget`, the same pair for `PdfDestination`, `TryFrom<&epdf_annotation_core::AnnotationRef> for epdf_ffi::AnnotationRef` (an object number or index that isn't a whole `u32` is `invalid-state`), `From<epdf_ffi::AnnotationRef> for epdf_annotation_core::AnnotationRef`, and `From` both ways between `epdf_ffi::PageRef` and `epdf_annotation_core::PageRef`. Plan 05 extends this file, and converts owned values, so each conversion also has a by-value form, plus `From` both ways between `AnnotationRevisionToken` and the core's `RevisionToken`. These four names are final: plans 05 to 16 use them as written here.

- [x] **Step 1: Create the worktree**

```bash
git worktree add .claude/worktrees/swift-link -b research/swift-link research/swift-plugins
cd .claude/worktrees/swift-link
```

Fetch PDFium for `darwin-arm64`, `ios-arm64` and `ios-sim-arm64` as `platforms/apple/README.md` describes.

- [x] **Step 2: Confirm nothing has written these records yet**

Run: `grep -n 'pub enum PdfLinkTarget\|pub enum AnnotationRef' crates/epdf-ffi/src/*.rs`
Expected: no output. This plan owns them (umbrella Shared records); plan 05 waits for it.

- [x] **Step 3: Write the failing test**

Add `epdf-annotation-core = { path = "../plugins/annotation-core", default-features = false }` to `[dependencies]` in `crates/epdf-ffi/Cargo.toml`. Then `crates/epdf-ffi/tests/annotation.rs`:

```rust
use epdf_annotation_core as ac;
use epdf_ffi::{AnnotationRef, PageRef, PdfDestination, PdfLinkTarget};

const PAGE: PageRef = PageRef { page_object_number: 7 };

#[test]
fn every_destination_kind_round_trips() {
    for destination in [
        PdfDestination::Xyz { page: PAGE, left: Some(1.0), top: None, zoom: Some(2.0) },
        PdfDestination::Fit { page: PAGE },
        PdfDestination::FitH { page: PAGE, top: Some(3.0) },
        PdfDestination::FitV { page: PAGE, left: None },
        PdfDestination::FitR { page: PAGE, left: 1.0, bottom: 2.0, right: 3.0, top: 4.0 },
        PdfDestination::FitB { page: PAGE },
        PdfDestination::FitBH { page: PAGE, top: None },
        PdfDestination::FitBV { page: PAGE, left: Some(5.0) },
    ] {
        let core = ac::PdfDestination::from(destination.clone());
        assert_eq!(PdfDestination::from(&core), destination);
    }
}

#[test]
fn an_absent_coordinate_goes_back_as_absent() {
    let core = ac::PdfDestination::from(PdfDestination::Xyz { page: PAGE, left: None, top: None, zoom: None });
    let json = serde_json::to_value(&core).unwrap();
    assert_eq!(json, serde_json::json!({ "kind": "xyz", "page": { "kind": "objectNumber", "pageObjectNumber": 7 } }));
}

#[test]
fn a_null_coordinate_arrives_as_none() {
    let core: ac::PdfDestination = serde_json::from_value(serde_json::json!({
        "kind": "xyz", "page": { "kind": "objectNumber", "pageObjectNumber": 7 }, "left": null, "top": 9
    }))
    .unwrap();
    assert_eq!(
        PdfDestination::from(&core),
        PdfDestination::Xyz { page: PAGE, left: None, top: Some(9.0), zoom: None }
    );
}

#[test]
fn every_target_kind_round_trips() {
    for target in [
        PdfLinkTarget::Goto { destination: PdfDestination::Fit { page: PAGE } },
        PdfLinkTarget::Uri { uri: "https://example.invalid".into() },
        PdfLinkTarget::GotoRemote { file: "other.pdf".into() },
        PdfLinkTarget::Launch { path: "app".into() },
        PdfLinkTarget::Javascript,
        PdfLinkTarget::Named { name: "NextPage".into() },
        PdfLinkTarget::Unsupported,
    ] {
        let core = ac::PdfLinkTarget::from(target.clone());
        assert_eq!(PdfLinkTarget::from(&core), target);
    }
}

#[test]
fn an_annotation_ref_with_a_fractional_number_is_invalid_state() {
    let core = ac::AnnotationRef::ObjectNumber { page: ac::PageRef::new(7), annot_object_number: 8.5 };
    let error = AnnotationRef::try_from(&core).unwrap_err();
    assert_eq!(error.code(), Some("invalid-state"));
    let ok = ac::AnnotationRef::ObjectNumber { page: ac::PageRef::new(7), annot_object_number: 8.0 };
    assert_eq!(
        AnnotationRef::try_from(&ok).unwrap(),
        AnnotationRef::ObjectNumber { page: PAGE, annot_object_number: 8 }
    );
}
```

`epdf-ffi`'s dev-dependencies need `serde_json`; add `serde_json = "1"` under `[dev-dependencies]` if it isn't there.

- [x] **Step 4: Run it to verify it fails**

Run: `cargo test -p epdf-ffi --test annotation`
Expected: FAIL to compile, "unresolved imports `epdf_ffi::AnnotationRef`, ...".

- [x] **Step 5: Write `annotation.rs`**

```rust
//! Records from `epdf-annotation-core` that several plugins' exports
//! return: where a link points, an explicit destination, and how the engine
//! addresses an annotation. The `From` impls live here because neither side
//! is a plugin type.
//!
//! A destination's coordinates are `Nullable` in the core (absent, `null`,
//! or a number). Absent and `null` both mean "keep the current value" to
//! every consumer, so both arrive as `None`, and `None` goes back as absent.

use epdf_annotation_core as ac;
use epdf_annotation_core::js::Nullable;

use crate::PageRef;

impl From<ac::PageRef> for PageRef {
    fn from(p: ac::PageRef) -> Self {
        PageRef { page_object_number: p.page_object_number }
    }
}

impl From<PageRef> for ac::PageRef {
    fn from(p: PageRef) -> Self {
        ac::PageRef::new(p.page_object_number)
    }
}

fn opt(n: &Nullable<f64>) -> Option<f64> {
    match n {
        Nullable::Value(v) => Some(*v),
        Nullable::Null | Nullable::Undefined => None,
    }
}

fn nullable(n: Option<f64>) -> Nullable<f64> {
    n.map_or(Nullable::Undefined, Nullable::Value)
}

/// An explicit in-document destination: a page and how to show it.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum PdfDestination {
    Xyz { page: PageRef, left: Option<f64>, top: Option<f64>, zoom: Option<f64> },
    Fit { page: PageRef },
    FitH { page: PageRef, top: Option<f64> },
    FitV { page: PageRef, left: Option<f64> },
    FitR { page: PageRef, left: f64, bottom: f64, right: f64, top: f64 },
    FitB { page: PageRef },
    FitBH { page: PageRef, top: Option<f64> },
    FitBV { page: PageRef, left: Option<f64> },
}

impl From<&ac::PdfDestination> for PdfDestination {
    fn from(d: &ac::PdfDestination) -> Self {
        use ac::PdfDestination as D;
        match d {
            D::Xyz { page, left, top, zoom } => {
                PdfDestination::Xyz { page: (*page).into(), left: opt(left), top: opt(top), zoom: opt(zoom) }
            }
            D::Fit { page } => PdfDestination::Fit { page: (*page).into() },
            D::FitH { page, top } => PdfDestination::FitH { page: (*page).into(), top: opt(top) },
            D::FitV { page, left } => PdfDestination::FitV { page: (*page).into(), left: opt(left) },
            D::FitR { page, left, bottom, right, top } => PdfDestination::FitR {
                page: (*page).into(),
                left: *left,
                bottom: *bottom,
                right: *right,
                top: *top,
            },
            D::FitB { page } => PdfDestination::FitB { page: (*page).into() },
            D::FitBH { page, top } => PdfDestination::FitBH { page: (*page).into(), top: opt(top) },
            D::FitBV { page, left } => PdfDestination::FitBV { page: (*page).into(), left: opt(left) },
        }
    }
}

impl From<PdfDestination> for ac::PdfDestination {
    fn from(d: PdfDestination) -> Self {
        use ac::PdfDestination as D;
        match d {
            PdfDestination::Xyz { page, left, top, zoom } => D::Xyz {
                page: page.into(),
                left: nullable(left),
                top: nullable(top),
                zoom: nullable(zoom),
            },
            PdfDestination::Fit { page } => D::Fit { page: page.into() },
            PdfDestination::FitH { page, top } => D::FitH { page: page.into(), top: nullable(top) },
            PdfDestination::FitV { page, left } => D::FitV { page: page.into(), left: nullable(left) },
            PdfDestination::FitR { page, left, bottom, right, top } => {
                D::FitR { page: page.into(), left, bottom, right, top }
            }
            PdfDestination::FitB { page } => D::FitB { page: page.into() },
            PdfDestination::FitBH { page, top } => D::FitBH { page: page.into(), top: nullable(top) },
            PdfDestination::FitBV { page, left } => D::FitBV { page: page.into(), left: nullable(left) },
        }
    }
}

/// Where a link points, normalized.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum PdfLinkTarget {
    Goto { destination: PdfDestination },
    Uri { uri: String },
    GotoRemote { file: String },
    Launch { path: String },
    Javascript,
    Named { name: String },
    Unsupported,
}

impl From<&ac::PdfLinkTarget> for PdfLinkTarget {
    fn from(t: &ac::PdfLinkTarget) -> Self {
        use ac::PdfLinkTarget as T;
        match t {
            T::Goto { destination } => PdfLinkTarget::Goto { destination: destination.into() },
            T::Uri { uri } => PdfLinkTarget::Uri { uri: uri.clone() },
            T::GotoRemote { file } => PdfLinkTarget::GotoRemote { file: file.clone() },
            T::Launch { path } => PdfLinkTarget::Launch { path: path.clone() },
            T::Javascript => PdfLinkTarget::Javascript,
            T::Named { name } => PdfLinkTarget::Named { name: name.clone() },
            T::Unsupported => PdfLinkTarget::Unsupported,
        }
    }
}

impl From<PdfLinkTarget> for ac::PdfLinkTarget {
    fn from(t: PdfLinkTarget) -> Self {
        use ac::PdfLinkTarget as T;
        match t {
            PdfLinkTarget::Goto { destination } => T::Goto { destination: destination.into() },
            PdfLinkTarget::Uri { uri } => T::Uri { uri },
            PdfLinkTarget::GotoRemote { file } => T::GotoRemote { file },
            PdfLinkTarget::Launch { path } => T::Launch { path },
            PdfLinkTarget::Javascript => T::Javascript,
            PdfLinkTarget::Named { name } => T::Named { name },
            PdfLinkTarget::Unsupported => T::Unsupported,
        }
    }
}

/// The page generation an index-addressed annotation was read at.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationRevisionToken {
    pub doc_session_id: String,
    pub page: PageRef,
    pub generation: f64,
}

/// How the engine addresses an annotation.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum AnnotationRef {
    ObjectNumber { page: PageRef, annot_object_number: u32 },
    Nm { page: PageRef, nm: String },
    Index { page: PageRef, index: u32, revision: AnnotationRevisionToken },
}

fn whole(n: f64, what: &str) -> Result<u32, epdf_kernel::Error> {
    if n >= 0.0 && n.fract() == 0.0 && n <= f64::from(u32::MAX) {
        Ok(n as u32)
    } else {
        Err(epdf_kernel::Error::named("InvalidState", format!("{what} {n} is not a whole u32"))
            .with("code", "invalid-state"))
    }
}

impl TryFrom<&ac::AnnotationRef> for AnnotationRef {
    type Error = epdf_kernel::Error;

    fn try_from(r: &ac::AnnotationRef) -> Result<Self, Self::Error> {
        Ok(match r {
            ac::AnnotationRef::ObjectNumber { page, annot_object_number } => AnnotationRef::ObjectNumber {
                page: (*page).into(),
                annot_object_number: whole(*annot_object_number, "an annotation object number")?,
            },
            ac::AnnotationRef::Nm { page, nm } => AnnotationRef::Nm { page: (*page).into(), nm: nm.clone() },
            ac::AnnotationRef::Index { page, index, revision } => AnnotationRef::Index {
                page: (*page).into(),
                index: whole(*index, "an annotation index")?,
                revision: AnnotationRevisionToken {
                    doc_session_id: revision.doc_session_id.clone(),
                    page: revision.page.into(),
                    generation: revision.generation,
                },
            },
        })
    }
}

impl From<AnnotationRef> for ac::AnnotationRef {
    fn from(r: AnnotationRef) -> Self {
        match r {
            AnnotationRef::ObjectNumber { page, annot_object_number } => ac::AnnotationRef::ObjectNumber {
                page: page.into(),
                annot_object_number: f64::from(annot_object_number),
            },
            AnnotationRef::Nm { page, nm } => ac::AnnotationRef::Nm { page: page.into(), nm },
            AnnotationRef::Index { page, index, revision } => ac::AnnotationRef::Index {
                page: page.into(),
                index: f64::from(index),
                revision: ac::RevisionToken {
                    doc_session_id: revision.doc_session_id,
                    page: revision.page.into(),
                    generation: revision.generation,
                },
            },
        }
    }
}

// By value as well as by reference: plan 05's code converts owned values.

impl From<ac::PdfDestination> for PdfDestination {
    fn from(d: ac::PdfDestination) -> Self {
        PdfDestination::from(&d)
    }
}

impl From<ac::PdfLinkTarget> for PdfLinkTarget {
    fn from(t: ac::PdfLinkTarget) -> Self {
        PdfLinkTarget::from(&t)
    }
}

impl TryFrom<ac::AnnotationRef> for AnnotationRef {
    type Error = epdf_kernel::Error;

    fn try_from(r: ac::AnnotationRef) -> Result<Self, Self::Error> {
        AnnotationRef::try_from(&r)
    }
}

impl From<ac::RevisionToken> for AnnotationRevisionToken {
    fn from(t: ac::RevisionToken) -> Self {
        AnnotationRevisionToken { doc_session_id: t.doc_session_id, page: t.page.into(), generation: t.generation }
    }
}

impl From<AnnotationRevisionToken> for ac::RevisionToken {
    fn from(t: AnnotationRevisionToken) -> Self {
        ac::RevisionToken { doc_session_id: t.doc_session_id, page: t.page.into(), generation: t.generation }
    }
}
```

If `Nullable` isn't reachable as `epdf_annotation_core::js::Nullable`, find its re-export (`grep -n 'pub use.*Nullable' crates/plugins/annotation-core/src/lib.rs`) and import from there.

- [x] **Step 6: Export them**

In `crates/epdf-ffi/src/lib.rs`:

```rust
pub mod annotation;

pub use annotation::{AnnotationRef, AnnotationRevisionToken, PdfDestination, PdfLinkTarget};
```

- [x] **Step 7: Run the tests to verify they pass**

Run: `cargo test -p epdf-ffi`
Expected: PASS, the five `annotation` tests and every existing one.

- [x] **Step 8: Commit**

```bash
git add crates/epdf-ffi
git commit -m "Add the shared annotation-core link records to epdf-ffi"
```

### Task 2: Map `page.annotations.list` to the native engine

**Files:**
- Modify: `crates/epdf-viewer-host/src/translate.rs`, `crates/epdf-viewer-host/tests/translate.rs`

**Interfaces:**
- Consumes: `translate`, `route`, `Decode::Result` (`crates/epdf-viewer-host/src/translate.rs`); the host's `annotations.listRawPage` (`crates/epdf-engine/src/host.rs:477`).
- Produces: the kernel path `page.annotations.list` reaching the wire kind `annotations.listRawPage` with `pageObjectNumber`, answering the page snapshot with every annotation ref nested the kernel's way; `translate::Decode::Annotations`; `pub fn flatten_refs(value: &mut Value)` and `pub fn nest_refs(value: &mut Value)` in `translate.rs`, which plans 05, 06 and 07 call for their own paths; `test_support::FEATURES` (umbrella Task 0f adds it; this task uses it).

The kernel spells an annotation ref `{kind, page: {kind: "objectNumber", pageObjectNumber}, annotObjectNumber}` (the vectors, `annotation-crud.json`). The host spells it `{kind, pageObjectNumber, annotObjectNumber}`. Link reads refs the kernel's way, so the list's answer is nested here. The rewrite recognizes a ref by `kind` in `objectNumber | nm | index` beside one of `annotObjectNumber | nm | index`, so a `FormFieldRef` (`kind: objectNumber` beside `fieldObjectNumber`) is left alone.

- [x] **Step 1: Write the failing translation test**

Append to `crates/epdf-viewer-host/tests/translate.rs`:

```rust
#[test]
fn a_page_annotation_list_carries_its_page_and_answers_the_snapshot() {
    let call = translate("d1", "page.annotations.list", Some(5), &[], 11).unwrap();
    assert_eq!(
        call.request,
        json!({ "kind": "annotations.listRawPage", "jobId": 11, "docId": "d1", "pageObjectNumber": 5 })
    );
    assert!(matches!(call.decode, Decode::Annotations));
    let error = translate("d1", "page.annotations.list", None, &[], 12).err().unwrap();
    assert_eq!(error.code(), Some("invalid-input"));
}

#[test]
fn a_listed_annotations_ref_and_page_arrive_nested() {
    let json = r#"{"result":{"annotations":[{"ref":{"kind":"objectNumber","pageObjectNumber":3,"annotObjectNumber":17},"page":3}]}}"#;
    let call = translate("d1", "page.annotations.list", Some(3), &[], 13).unwrap();
    let decoded = decode(&call.decode, answer(json)).unwrap().value;
    let page3 = json!({ "kind": "objectNumber", "pageObjectNumber": 3 });
    assert_eq!(
        decoded["annotations"][0]["ref"],
        json!({ "kind": "objectNumber", "page": page3.clone(), "annotObjectNumber": 17 })
    );
    assert_eq!(decoded["annotations"][0]["page"], page3);
}

#[test]
fn a_form_field_ref_is_not_mistaken_for_an_annotation_ref() {
    let mut value = json!({ "ref": { "kind": "objectNumber", "fieldObjectNumber": 9 } });
    let before = value.clone();
    epdf_viewer_host::translate::nest_refs(&mut value);
    epdf_viewer_host::translate::flatten_refs(&mut value);
    assert_eq!(value, before);
}
```

`answer` is the existing helper in `tests/translate.rs` that wraps JSON in a `HostResponse`.

- [x] **Step 2: Confirm the fixture constant**

Run: `grep -n 'pub const FEATURES' crates/test-support/src/lib.rs`
Expected: one hit (umbrella Task 0f).

- [x] **Step 3: Run them to verify the translation test fails**

Run: `cargo test -p epdf-viewer-host --test translate annotation`
Expected: FAIL, `unwrap()` on an `unsupported` error.

- [x] **Step 4: Map the path**

In `crates/epdf-viewer-host/src/translate.rs`, add a `Decode` variant:

```rust
    /// A read whose answer carries annotation refs: the result, with every
    /// ref nested the kernel's way.
    Annotations,
```

Add to `route`'s match, after `search.query`:

```rust
        "page.annotations.list" => ("annotations.listRawPage", Decode::Annotations),
```

to the `extra` match in `translate`:

```rust
        "annotations.listRawPage" => vec![("pageObjectNumber", page()?)],
```

to `PATHS` (umbrella Task 0e), one line in plan order:

```rust
    "page.annotations.list",
```

and to `decode`, beside `Decode::Result`:

```rust
        Decode::Annotations => {
            let mut value = Value::Object(result);
            nest_refs(&mut value);
            value
        }
```

Add the helpers below `add_page_ref`:

```rust
fn is_annotation_ref(map: &Map<String, Value>) -> bool {
    matches!(map.get("kind").and_then(Value::as_str), Some("objectNumber" | "nm" | "index"))
        && (map.contains_key("annotObjectNumber") || map.contains_key("nm") || map.contains_key("index"))
}

/// Every annotation ref in `value`, kernel spelling to host spelling:
/// `page: PageRef` becomes `pageObjectNumber`.
pub fn flatten_refs(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if is_annotation_ref(map) {
                if let Some(pon) = map.remove("page").as_ref().and_then(|p| p.get("pageObjectNumber")).cloned() {
                    map.insert("pageObjectNumber".into(), pon);
                }
            }
            map.values_mut().for_each(flatten_refs);
        }
        Value::Array(items) => items.iter_mut().for_each(flatten_refs),
        _ => {}
    }
}

/// Every annotation ref in `value`, host spelling to kernel spelling, and
/// every bare numeric `page` (a DTO's, a page state's) as a `PageRef`.
pub fn nest_refs(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if is_annotation_ref(map) {
                if let Some(pon) = map.remove("pageObjectNumber").and_then(|p| p.as_u64()) {
                    map.insert("page".into(), PageRef::new(pon as u32).to_json());
                }
            }
            if let Some(pon) = map.get("page").and_then(Value::as_u64) {
                map.insert("page".into(), PageRef::new(pon as u32).to_json());
            }
            map.values_mut().for_each(nest_refs);
        }
        Value::Array(items) => items.iter_mut().for_each(nest_refs),
        _ => {}
    }
}
```

- [x] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p epdf-viewer-host --test translate`
Expected: PASS.

- [x] **Step 6: Commit**

```bash
git add crates/epdf-viewer-host
git commit -m "Map page.annotations.list to the native engine's listRawPage, refs nested"
```

### Task 3: `LinkHandle`, records and events

**Files:**
- Modify: `crates/plugins/link/Cargo.toml`, `crates/plugins/link/src/lib.rs`, `crates/plugins/link/src/controller.rs`
- Create: `crates/plugins/link/src/ffi.rs`, `crates/plugins/link/tests/ffi.rs`

**Interfaces:**
- Consumes: Task 1's records; `epdf_plugin_stage::ffi::StageRevealOptions` (plan 02); `LinkCapability`'s members (`crates/plugins/link/src/controller.rs:97-350`); `LinkResolution`, `LinkActivation`, `LinkInput`, `LinkActivatedEvent`, `LinkLoadedEvent`, `LinkNavItem` (`contract.rs`).
- Produces (in `epdf_plugin_link::ffi`): records `LinkItem { id, page, bounds, target: PdfLinkTarget, attached, annotation: Option<AnnotationRef>, has_action, hover: Option<LinkHoverFlags> }`, `LinkHoverFlags { enter, exit }`, `LinkActivatedEvent { target, activation }`, `LinkLoadedEvent { page }`; enums `LinkInput { Target { target }, Link { page, id } }`, `LinkResolution { Reveal { page, page_index: u32, options: StageRevealOptions }, Destination { destination }, Uri { uri }, Named { name }, Reported { target } }`, `LinkActivation { Revealed, Destination { destination }, Uri { uri }, Named { name }, Reported { target }, Dispatched, None }`, `LinkEvent { Activated, Loaded }`; trait `LinkEventListener`; object `LinkHandle` with `LinkHandle::new(cap: LinkCapability, post: KernelPost) -> Arc<Self>` and methods `links(page)`, `link(page, id)`, `link_at(page, point)`, `all_links()`, `ensure_loaded(page)`, `is_loaded(page)`, `resolve(target)`, `activate(input)`, `activate_at(page, point)`, `label(input)`, `is_navigation_engaged`, `subscribe(listener)`.

That's the 13 entries of `crates/plugins/link/src/harness.rs` `MEMBERS`, with `onActivated` and `onLoaded` folded into `subscribe`.

- [x] **Step 1: Add the feature and `lifetime()`**

In `crates/plugins/link/Cargo.toml`:

```toml
# under [features]
# The uniffi exports (`ffi` module), for `epdf-viewer-host`.
uniffi = ["dep:uniffi", "dep:epdf-ffi", "epdf-plugin-stage/uniffi"]

# under [dependencies]
epdf-ffi = { path = "../../epdf-ffi", optional = true }
uniffi = { version = "=0.32.0", optional = true }

[dev-dependencies]
futures = { version = "0.3", default-features = false, features = ["std", "executor"] }
plugin-conformance = { path = "../../plugin-conformance" }
```

Add `[[test]] name = "ffi" required-features = ["uniffi"]` beside the existing `[[test]] name = "conformance"`.

In `crates/plugins/link/src/lib.rs`:

```rust
#[cfg(feature = "uniffi")]
pub mod ffi;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();
```

In `crates/plugins/link/src/controller.rs`, in `impl LinkCapability`:

```rust
    /// The instance lifetime, aborted when the document closes. A foreign
    /// handle outlives the close, checks this before every call, and ends
    /// its event streams when it aborts.
    #[cfg(feature = "uniffi")]
    #[doc(hidden)]
    pub fn lifetime(&self) -> &epdf_kernel::signal::AbortSignal {
        self.0.cx.lifetime()
    }
```

- [x] **Step 2: Write the failing tests**

`crates/plugins/link/tests/ffi.rs`:

```rust
#![cfg(feature = "uniffi")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use epdf_ffi::{KernelThread, PageRef, PdfDestination, PdfLinkTarget, Point, ViewerError};
use epdf_plugin_interaction::{InteractionConfig, interaction_plugin};
use epdf_plugin_link::ffi::{
    LinkActivation, LinkEvent, LinkEventListener, LinkHandle, LinkInput, LinkResolution,
};
use epdf_plugin_link::{LINK, LinkConfig, link_plugin};
use epdf_plugin_stage::{StagePluginOptions, stage_plugin};
use futures::executor::block_on;
use plugin_conformance::test_kernel::{DOCUMENT, Reply, TestKernel, TestSpec};
use serde_json::{Value, json};

const PAGE_1: PageRef = PageRef { page_object_number: 1 };
const PAGE_3: PageRef = PageRef { page_object_number: 3 };

fn page(pon: u32, index: u32) -> Value {
    let box_ = json!({ "left": 0, "bottom": 0, "right": 600, "top": 400 });
    json!({ "index": index, "ref": { "kind": "objectNumber", "pageObjectNumber": pon },
        "label": null, "size": { "width": 600, "height": 400 }, "rotation": 0, "userUnit": 1,
        "boxes": { "media": box_, "crop": box_ } })
}

fn link(object: u32, rect: [f64; 4], target: Value) -> Value {
    json!({
        "subtype": "link",
        "target": target,
        "flags": { "hidden": false, "noView": false },
        "ref": { "kind": "objectNumber", "page": { "kind": "objectNumber", "pageObjectNumber": 1 },
                 "annotObjectNumber": object },
        "rect": { "left": rect[0], "bottom": rect[1], "right": rect[2], "top": rect[3] }
    })
}

/// Page 1 of `features.pdf`, as the native engine lists it, plus a small
/// link nested inside the internal one.
fn spec(extra: Vec<Value>) -> TestSpec {
    let mut annotations = vec![
        link(8, [40.0, 300.0, 320.0, 330.0], json!({ "kind": "goto", "destination":
            { "kind": "xyz", "page": { "kind": "objectNumber", "pageObjectNumber": 3 }, "left": 0, "top": 400, "zoom": 0 } })),
        link(9, [40.0, 240.0, 360.0, 270.0], json!({ "kind": "uri", "uri": "https://example.invalid/target" })),
    ];
    annotations.extend(extra);
    TestSpec::new(json!({ "documents": { DOCUMENT: {
        "id": DOCUMENT, "security": { "allows": "*" },
        "pages": [page(1, 0), page(2, 1), page(3, 2)]
    } } }))
    .respond_page("page.annotations.list", move |pon, _| {
        Ok(Reply::value(json!({ "annotations": if pon == 1 { annotations.clone() } else { vec![] } })))
    })
}

#[derive(Default)]
struct Events(Mutex<Vec<LinkEvent>>, AtomicUsize);

impl LinkEventListener for Events {
    fn on_event(&self, event: LinkEvent) {
        self.0.lock().unwrap().push(event);
    }
    fn on_end(&self) {
        self.1.fetch_add(1, Ordering::SeqCst);
    }
}

fn handle_with(stage: bool, extra: Vec<Value>) -> (KernelThread<TestKernel>, Arc<LinkHandle>) {
    let thread = KernelThread::spawn(move |_| {
        let mut plugins = vec![interaction_plugin(InteractionConfig::default()), link_plugin(LinkConfig::default())];
        if stage {
            plugins.push(stage_plugin(StagePluginOptions::default()));
        }
        TestKernel::open(plugins, spec(extra))
    })
    .unwrap();
    let post = thread.post();
    let handle =
        block_on(thread.call(move |k| LinkHandle::new(k.capability(&LINK).unwrap(), post))).unwrap();
    (thread, handle)
}

fn loaded() -> (KernelThread<TestKernel>, Arc<LinkHandle>) {
    let (thread, link) = handle_with(false, vec![]);
    block_on(link.ensure_loaded(PAGE_1)).unwrap();
    (thread, link)
}

fn code(error: &ViewerError) -> Option<&str> {
    match error {
        ViewerError::Kernel { code, .. } => code.as_deref(),
        ViewerError::Closed => None,
    }
}

#[test]
fn links_appear_once_the_page_loads() {
    let (_thread, link) = handle_with(false, vec![]);
    assert!(!block_on(link.is_loaded(PAGE_1)).unwrap());
    assert!(block_on(link.links(PAGE_1)).unwrap().is_empty());
    block_on(link.ensure_loaded(PAGE_1)).unwrap();
    assert!(block_on(link.is_loaded(PAGE_1)).unwrap());
    let links = block_on(link.links(PAGE_1)).unwrap();
    assert_eq!(links.len(), 2);
    assert_eq!(links[0].id, "obj:8");
    assert_eq!(links[0].page, PAGE_1);
    // y-down page space: 400 - 330 = 70.
    assert_eq!((links[0].bounds.x, links[0].bounds.y, links[0].bounds.width, links[0].bounds.height), (40.0, 70.0, 280.0, 30.0));
    assert!(matches!(&links[1].target, PdfLinkTarget::Uri { uri } if uri == "https://example.invalid/target"));
    assert!(!links[0].has_action);
}

#[test]
fn every_page_s_links_come_back_from_all_links() {
    let (_thread, link) = handle_with(false, vec![]);
    assert_eq!(block_on(link.all_links()).unwrap().len(), 2);
}

#[test]
fn the_smallest_link_under_a_point_wins() {
    let small = link(10, [100.0, 310.0, 120.0, 320.0], json!({ "kind": "named", "name": "NextPage" }));
    let (_thread, link) = handle_with(false, vec![small]);
    block_on(link.ensure_loaded(PAGE_1)).unwrap();
    let hit = block_on(link.link_at(PAGE_1, Point { x: 110.0, y: 85.0 })).unwrap().unwrap();
    assert_eq!(hit.id, "obj:10");
    let outside = block_on(link.link_at(PAGE_1, Point { x: 500.0, y: 300.0 })).unwrap();
    assert!(outside.is_none());
}

#[test]
fn a_target_that_does_not_decode_is_invalid_state() {
    let odd = link(11, [400.0, 10.0, 420.0, 20.0], json!({ "kind": "teleport" }));
    let (_thread, link) = handle_with(false, vec![odd]);
    block_on(link.ensure_loaded(PAGE_1)).unwrap();
    let error = block_on(link.links(PAGE_1)).unwrap_err();
    assert_eq!(code(&error), Some("invalid-state"), "{error:?}");
}

#[test]
fn a_uri_link_is_reported_for_the_host() {
    let (thread, link) = loaded();
    let events = Arc::new(Events::default());
    let _sub = link.subscribe(events.clone());
    let activation = block_on(link.activate(LinkInput::Link { page: PAGE_1, id: "obj:9".into() })).unwrap();
    assert_eq!(activation, LinkActivation::Uri { uri: "https://example.invalid/target".into() });
    block_on(thread.call(|_| ())).unwrap();
    assert!(matches!(events.0.lock().unwrap().first(), Some(LinkEvent::Activated { event })
        if event.activation == activation));
}

#[test]
fn without_a_stage_a_goto_link_reports_its_destination() {
    let (_thread, link) = loaded();
    let activation = block_on(link.activate_at(PAGE_1, Point { x: 100.0, y: 85.0 })).unwrap();
    assert!(matches!(activation,
        Some(LinkActivation::Destination { destination: PdfDestination::Xyz { page, .. } }) if page == PAGE_3));
}

#[test]
fn with_a_stage_a_goto_link_reveals_its_page() {
    let (_thread, link) = handle_with(true, vec![]);
    block_on(link.ensure_loaded(PAGE_1)).unwrap();
    let resolution = block_on(link.resolve(PdfLinkTarget::Goto {
        destination: PdfDestination::Fit { page: PAGE_3 },
    }))
    .unwrap();
    assert!(matches!(resolution, LinkResolution::Reveal { page, page_index: 2, .. } if page == PAGE_3));
    let activation = block_on(link.activate_at(PAGE_1, Point { x: 100.0, y: 85.0 })).unwrap();
    assert_eq!(activation, Some(LinkActivation::Revealed));
}

#[test]
fn a_tap_off_any_link_activates_nothing() {
    let (_thread, link) = loaded();
    assert_eq!(block_on(link.activate_at(PAGE_1, Point { x: 500.0, y: 380.0 })).unwrap(), None);
}

#[test]
fn an_unknown_link_id_is_not_found() {
    let (_thread, link) = loaded();
    let error = block_on(link.activate(LinkInput::Link { page: PAGE_1, id: "obj:404".into() })).unwrap_err();
    assert_eq!(code(&error), Some("not-found"), "{error:?}");
}

#[test]
fn a_label_names_the_target() {
    let (_thread, link) = loaded();
    assert_eq!(
        block_on(link.label(LinkInput::Link { page: PAGE_1, id: "obj:9".into() })).unwrap(),
        "https://example.invalid/target"
    );
    assert_eq!(
        block_on(link.label(LinkInput::Target { target: PdfLinkTarget::Goto { destination: PdfDestination::Fit { page: PAGE_3 } } })).unwrap(),
        "Go to destination"
    );
}

#[test]
fn a_load_is_observed() {
    let (thread, link) = handle_with(false, vec![]);
    let events = Arc::new(Events::default());
    let _sub = link.subscribe(events.clone());
    block_on(link.ensure_loaded(PAGE_1)).unwrap();
    block_on(thread.call(|_| ())).unwrap();
    assert!(matches!(events.0.lock().unwrap().first(), Some(LinkEvent::Loaded { event }) if event.page == PAGE_1));
}

#[test]
fn a_closed_documents_capability_refuses_with_instance_closed() {
    let (thread, link) = loaded();
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    for error in [
        block_on(link.links(PAGE_1)).map(|_| ()).unwrap_err(),
        block_on(link.activate_at(PAGE_1, Point { x: 100.0, y: 85.0 })).map(|_| ()).unwrap_err(),
    ] {
        assert_eq!(code(&error), Some("instance-closed"), "{error:?}");
    }
}

#[test]
fn closing_the_document_ends_link_subscriptions() {
    let (thread, link) = loaded();
    let events = Arc::new(Events::default());
    let _sub = link.subscribe(events.clone());
    block_on(thread.call(|_| ())).unwrap();
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    assert_eq!(events.1.load(Ordering::SeqCst), 1);
}
```

`project_links` computes bounds from the DTO's `rect` and the page's crop box (`crates/plugins/link/src/source.rs`), so the expected `(40, 70, 280, 30)` follows from a 600 by 400 crop. If `pdf_to_content_rect` rounds or offsets differently, read its result once and fix the expectation to it, not the other way round.

- [x] **Step 3: Run them to verify they fail**

Run: `cargo test -p epdf-plugin-link --features uniffi --test ffi`
Expected: FAIL to compile, "unresolved import `epdf_plugin_link::ffi`".

- [x] **Step 4: Write `ffi.rs`**

```rust
//! The link plugin through uniffi: a page's links, what each targets, and
//! activating them, as one handle and one event enum.
//!
//! Targets and destinations are `epdf-ffi`'s annotation-core mirrors. A
//! link's `target` is JSON in the plugin; it decodes here, and a target
//! that does not decode is an `invalid-state` error. A link's action tree
//! stays in Rust: Swift activates a link by page and id, and the handle
//! looks it up again so the tree goes with it. A `goto` resolution carries
//! the stage's reveal options, so this module needs stage's `ffi`.
//!
//! Every method hops to the kernel thread through `KernelBound`.

use std::sync::Arc;

use epdf_ffi::{
    AnnotationRef, KernelBound, KernelPost, PageRef, PdfDestination, PdfLinkTarget, Point, Rect,
    Subscription, ViewerError,
};
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::{Error, Unsubscribe};
use epdf_plugin_stage::ffi::StageRevealOptions;

use crate::{LinkCapability, LinkNavItem};

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct LinkHoverFlags {
    pub enter: bool,
    pub exit: bool,
}

/// One clickable link area.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct LinkItem {
    /// Stable per page: `obj:<n>` for an object-numbered annotation.
    pub id: String,
    pub page: PageRef,
    /// Page space, y-down.
    pub bounds: Rect,
    pub target: PdfLinkTarget,
    /// A reply grouped under another annotation.
    pub attached: bool,
    pub annotation: Option<AnnotationRef>,
    /// The link carries an action tree, which runs instead of the target
    /// when the actions plugin is installed.
    pub has_action: bool,
    pub hover: Option<LinkHoverFlags>,
}

fn invalid(what: String) -> Error {
    Error::named("InvalidState", format!("[link] {what}")).with("code", "invalid-state")
}

fn target_of(item: &LinkNavItem) -> Result<epdf_annotation_core::PdfLinkTarget, Error> {
    serde_json::from_value(item.target.clone())
        .map_err(|e| invalid(format!("link {} has a target that does not decode ({e}): {}", item.id, item.target)))
}

fn item(page: PageRef, link: &LinkNavItem) -> Result<LinkItem, Error> {
    Ok(LinkItem {
        id: link.id.clone(),
        page,
        bounds: link.bounds.into(),
        target: (&target_of(link)?).into(),
        attached: link.attached,
        annotation: link.ref_.as_ref().map(AnnotationRef::try_from).transpose()?,
        has_action: link.activate.is_some(),
        hover: link.hover_events.map(|h| LinkHoverFlags { enter: h.enter, exit: h.exit }),
    })
}

/// A bare target, or a link named by its page and id.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum LinkInput {
    Target { target: PdfLinkTarget },
    Link { page: PageRef, id: String },
}

/// What activating a target would do, with no side effect.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum LinkResolution {
    Reveal { page: PageRef, page_index: u32, options: StageRevealOptions },
    Destination { destination: PdfDestination },
    Uri { uri: String },
    Named { name: String },
    Reported { target: PdfLinkTarget },
}

impl TryFrom<crate::LinkResolution> for LinkResolution {
    type Error = Error;
    fn try_from(r: crate::LinkResolution) -> Result<Self, Error> {
        Ok(match r {
            crate::LinkResolution::Reveal { page, page_index, options } => LinkResolution::Reveal {
                page: page.into(),
                page_index: u32::try_from(page_index)
                    .map_err(|_| invalid(format!("a page index of {page_index}")))?,
                options: options.into(),
            },
            crate::LinkResolution::Destination { destination } => {
                LinkResolution::Destination { destination: (&destination).into() }
            }
            crate::LinkResolution::Uri { uri } => LinkResolution::Uri { uri },
            crate::LinkResolution::Named { name } => LinkResolution::Named { name },
            crate::LinkResolution::Reported { target } => LinkResolution::Reported { target: (&target).into() },
        })
    }
}

/// What an activation did, or hands to the app to do. `Uri` and `Named`
/// are the app's: the plugin never opens a URL. `Dispatched` means the
/// link's action tree went to the actions plugin.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum LinkActivation {
    Revealed,
    Destination { destination: PdfDestination },
    Uri { uri: String },
    Named { name: String },
    Reported { target: PdfLinkTarget },
    Dispatched,
    None,
}

impl From<&crate::LinkActivation> for LinkActivation {
    fn from(a: &crate::LinkActivation) -> Self {
        match a {
            crate::LinkActivation::Revealed => LinkActivation::Revealed,
            crate::LinkActivation::Destination { destination } => {
                LinkActivation::Destination { destination: destination.into() }
            }
            crate::LinkActivation::Uri { uri } => LinkActivation::Uri { uri: uri.clone() },
            crate::LinkActivation::Named { name } => LinkActivation::Named { name: name.clone() },
            crate::LinkActivation::Reported { target } => LinkActivation::Reported { target: target.into() },
            crate::LinkActivation::Dispatched { .. } => LinkActivation::Dispatched,
            crate::LinkActivation::None => LinkActivation::None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct LinkActivatedEvent {
    pub target: PdfLinkTarget,
    pub activation: LinkActivation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct LinkLoadedEvent {
    pub page: PageRef,
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum LinkEvent {
    Activated { event: LinkActivatedEvent },
    Loaded { event: LinkLoadedEvent },
}

#[uniffi::export(with_foreign)]
pub trait LinkEventListener: Send + Sync {
    fn on_event(&self, event: LinkEvent);
    /// The document closed: no event follows.
    fn on_end(&self);
}

fn open(cap: &LinkCapability) -> Result<&LinkCapability, Error> {
    if cap.lifetime().aborted() {
        return Err(plugin_error(
            code::INSTANCE_CLOSED,
            "link",
            "the document this capability belongs to is closed",
            None,
            None,
        ));
    }
    Ok(cap)
}

/// The plugin's input for a foreign one: a link is looked up again so its
/// action tree goes with it.
fn input_of(cap: &LinkCapability, input: LinkInput) -> Result<crate::LinkInput, Error> {
    Ok(match input {
        LinkInput::Target { target } => crate::LinkInput::Target(target.into()),
        LinkInput::Link { page, id } => {
            let link = cap.get_link(&page.into(), &id).ok_or_else(|| {
                plugin_error(code::NOT_FOUND, "link", format!("no link {id} on that page"), None, None)
            })?;
            crate::LinkInput::Link(Box::new(link))
        }
    })
}

#[derive(uniffi::Object)]
pub struct LinkHandle {
    cap: KernelBound<LinkCapability>,
    post: KernelPost,
}

impl LinkHandle {
    /// On the kernel thread.
    pub fn new(cap: LinkCapability, post: KernelPost) -> Arc<Self> {
        Arc::new(LinkHandle { cap: KernelBound::new(cap, post.clone()), post })
    }

    async fn on<T: Send + 'static>(
        &self,
        f: impl FnOnce(&LinkCapability) -> Result<T, Error> + Send + 'static,
    ) -> Result<T, ViewerError> {
        self.cap.call(move |c| f(open(c)?)).await
    }
}

#[uniffi::export]
impl LinkHandle {
    /// The page's links; empty until the page loads (`ensure_loaded`).
    pub async fn links(&self, page: PageRef) -> Result<Vec<LinkItem>, ViewerError> {
        self.on(move |c| c.list_links(&page.into()).iter().map(|l| item(page, l)).collect()).await
    }

    pub async fn link(&self, page: PageRef, id: String) -> Result<Option<LinkItem>, ViewerError> {
        self.on(move |c| c.get_link(&page.into(), &id).as_ref().map(|l| item(page, l)).transpose()).await
    }

    /// The smallest link under a page point.
    pub async fn link_at(&self, page: PageRef, point: Point) -> Result<Option<LinkItem>, ViewerError> {
        self.on(move |c| c.get_link_at(&page.into(), point.into()).as_ref().map(|l| item(page, l)).transpose())
            .await
    }

    /// Every link in the document, loading pages as needed. The plugin's
    /// items don't carry their page, so once every page has loaded this
    /// walks the pages itself.
    pub async fn all_links(&self) -> Result<Vec<LinkItem>, ViewerError> {
        self.cap.call_promise(|c| Ok(open(c)?.list_all_links()), |_| ()).await?;
        self.on(|c| {
            let mut out = Vec::new();
            for page in c.page_refs() {
                let ffi_page: PageRef = page.into();
                for l in c.list_links(&page) {
                    out.push(item(ffi_page, &l)?);
                }
            }
            Ok(out)
        })
        .await
    }

    pub async fn ensure_loaded(&self, page: PageRef) -> Result<(), ViewerError> {
        self.cap.call_promise(move |c| Ok(open(c)?.ensure_loaded(&page.into())), |()| ()).await
    }

    pub async fn is_loaded(&self, page: PageRef) -> Result<bool, ViewerError> {
        self.on(move |c| Ok(c.is_loaded(&page.into()))).await
    }

    pub async fn resolve(&self, target: PdfLinkTarget) -> Result<LinkResolution, ViewerError> {
        self.on(move |c| c.resolve(&target.into())?.try_into()).await
    }

    /// Activate a link or a bare target. A `uri` or `named` outcome is the
    /// app's to carry out.
    pub async fn activate(&self, input: LinkInput) -> Result<LinkActivation, ViewerError> {
        self.on(move |c| Ok((&c.activate(&input_of(c, input)?, None)?).into())).await
    }

    /// Activate the link under a page point; `None` when there is none.
    pub async fn activate_at(&self, page: PageRef, point: Point) -> Result<Option<LinkActivation>, ViewerError> {
        self.on(move |c| Ok(c.activate_at(&page.into(), point.into(), None)?.as_ref().map(Into::into))).await
    }

    /// A human label for a target: the URL, `Go to destination`, ...
    pub async fn label(&self, input: LinkInput) -> Result<String, ViewerError> {
        self.on(move |c| {
            let target = match input_of(c, input)? {
                crate::LinkInput::Target(target) => epdf_kernel::js::to_value(&target),
                crate::LinkInput::Link(link) => link.target.clone(),
            };
            Ok(c.get_label(&target))
        })
        .await
    }

    /// Whether the active tool lets taps follow links (`link-nav`).
    pub async fn is_navigation_engaged(&self) -> Result<bool, ViewerError> {
        self.on(|c| Ok(c.is_navigation_engaged())).await
    }

    /// The listener is registered on the kernel thread after this returns.
    /// It gets `on_end` when the document closes (at once if it already has).
    pub fn subscribe(&self, listener: Arc<dyn LinkEventListener>) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.cap.fire(move |c| {
            let ending = listener.clone();
            slot.fill_until_end(
                c.lifetime(),
                || {
                    let activated = {
                        let listener = listener.clone();
                        c.on_activated(move |e| {
                            listener.on_event(LinkEvent::Activated {
                                event: LinkActivatedEvent {
                                    target: (&e.target).into(),
                                    activation: (&e.activation).into(),
                                },
                            });
                            Ok(())
                        })
                    };
                    let loaded = {
                        let listener = listener.clone();
                        c.on_loaded(move |e| {
                            listener.on_event(LinkEvent::Loaded { event: LinkLoadedEvent { page: e.page.into() } });
                            Ok(())
                        })
                    };
                    let offs: Vec<Unsubscribe> = vec![activated, loaded];
                    Unsubscribe::new(move || offs.iter().for_each(Unsubscribe::call))
                },
                move || ending.on_end(),
            );
        });
        subscription
    }
}
```

`page_refs` is private in `controller.rs`; make it `pub(crate)` so `ffi.rs` can call it. `epdf_annotation_core::PageRef` converts into `epdf_ffi::PageRef` through Task 1's impls.

- [x] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p epdf-plugin-link --features uniffi --test ffi`
Expected: PASS, 13 tests.

Run: `cargo test -p epdf-plugin-link`
Expected: PASS, conformance unchanged.

- [x] **Step 6: Commit**

```bash
git add crates/plugins/link
git commit -m "Export the link plugin through uniffi"
```

### Task 4: Host wiring

**Files:**
- Modify: `crates/epdf-viewer-host/Cargo.toml`, `crates/epdf-viewer-host/src/api.rs`, `crates/epdf-viewer-host/src/session.rs`, `crates/epdf-viewer-host/src/lib.rs`

**Interfaces:**
- Consumes: `LinkHandle::new`.
- Produces: `ViewerConfig.link: bool`; `ViewerHost::link(document_id: String) -> Result<Arc<LinkHandle>, ViewerError>`.

- [x] **Step 1: Wire it**

`crates/epdf-viewer-host/Cargo.toml`, alphabetical:

```toml
epdf-plugin-link = { path = "../plugins/link", default-features = false, features = ["uniffi"] }
```

`crates/epdf-viewer-host/src/api.rs`, field at the end of `ViewerConfig`:

```rust
    /// Install `linkPlugin()`: link areas and activation, one per document.
    /// Requires `interaction`. With `stage`, a `goto` link reveals its page.
    #[uniffi(default = false)]
    pub link: bool,
```

`crates/epdf-viewer-host/src/session.rs`:

```rust
        if config.link {
            plugins.push(epdf_plugin_link::link_plugin(Default::default()));
        }
```

`crates/epdf-viewer-host/src/lib.rs`, `use epdf_plugin_link::ffi::LinkHandle;` and in the exported impl:

```rust
    /// The links of an open document. Needs `ViewerConfig.link` (and
    /// `interaction`).
    pub async fn link(&self, document_id: String) -> Result<Arc<LinkHandle>, ViewerError> {
        let post = self.post.clone();
        self.call(move |s| {
            let cap = s.kernel.capability(&epdf_plugin_link::LINK, Some(&document_id))?;
            Ok(LinkHandle::new(cap, post))
        })
        .await
    }
```

- [x] **Step 2: Build**

Run: `cargo build -p epdf-viewer-host`
Expected: builds. Task 5 tests it.

- [x] **Step 3: Commit**

```bash
git add crates/epdf-viewer-host
git commit -m "Wire the link plugin into the viewer host"
```

### Task 5: Host and cross-plugin tests

**Files:**
- Modify: `crates/epdf-viewer-host/tests/viewer_host.rs`

- [ ] **Step 1: Write the tests**

Append to `crates/epdf-viewer-host/tests/viewer_host.rs`:

```rust
use epdf_ffi::PdfLinkTarget as HostTarget;
use epdf_plugin_link::ffi::{LinkActivation, LinkInput};

fn with_links(stage: bool) -> Arc<ViewerHost> {
    let config = ViewerConfig {
        interaction: true,
        link: true,
        stage: stage.then(|| epdf_plugin_stage::ffi::StageConfig { frames: false, ..Default::default() }),
        ..Default::default()
    };
    let host = ViewerHost::new(config, Arc::new(Delegate)).unwrap();
    block_on(host.start()).unwrap();
    host
}

#[test]
fn the_link_plugin_lists_and_activates_links_through_the_host() {
    let host = with_links(false);
    let doc = block_on(host.open_document(test_support::fixture(test_support::FEATURES), options()))
        .unwrap();
    let pages = block_on(host.pages(doc.id.clone())).unwrap();
    let link = block_on(host.link(doc.id)).unwrap();
    block_on(link.ensure_loaded(pages[0].page)).unwrap();
    let links = block_on(link.links(pages[0].page)).unwrap();
    assert_eq!(links.len(), 2, "{links:?}");
    let external = links.iter().find(|l| matches!(l.target, HostTarget::Uri { .. })).unwrap();
    let activation =
        block_on(link.activate(LinkInput::Link { page: pages[0].page, id: external.id.clone() })).unwrap();
    assert_eq!(activation, LinkActivation::Uri { uri: "https://example.invalid/target".into() });
}

#[test]
fn activating_a_goto_link_moves_the_stage() {
    let host = with_links(true);
    let doc = block_on(host.open_document(test_support::fixture(test_support::FEATURES), options()))
        .unwrap();
    let pages = block_on(host.pages(doc.id.clone())).unwrap();
    let stage = block_on(host.stage(doc.id.clone())).unwrap();
    block_on(stage.set_viewport_size(epdf_ffi::Size { width: 390.0, height: 300.0 })).unwrap();
    let link = block_on(host.link(doc.id)).unwrap();
    block_on(link.ensure_loaded(pages[0].page)).unwrap();
    // Inside the internal link: x 40..320, y 70..100 in page space.
    let activation = block_on(link.activate_at(pages[0].page, epdf_ffi::Point { x: 100.0, y: 85.0 })).unwrap();
    assert_eq!(activation, Some(LinkActivation::Revealed));
    assert!(block_on(stage.is_page_visible(pages[2].page)).unwrap());
}

#[test]
fn a_tap_on_a_link_activates_it() {
    use epdf_ffi::{Modifiers, Point, PointerKind, PointerPageHit, PointerPhase, PointerSample};
    use epdf_plugin_link::ffi::{LinkEvent, LinkEventListener};
    #[derive(Default)]
    struct Seen(Mutex<Vec<LinkEvent>>);
    impl LinkEventListener for Seen {
        fn on_event(&self, event: LinkEvent) {
            self.0.lock().unwrap().push(event);
        }
        fn on_end(&self) {}
    }
    let host = with_links(false);
    let doc = block_on(host.open_document(test_support::fixture(test_support::FEATURES), options()))
        .unwrap();
    let page = block_on(host.pages(doc.id.clone())).unwrap()[0].page;
    let link = block_on(host.link(doc.id.clone())).unwrap();
    let interaction = block_on(host.interaction(doc.id)).unwrap();
    block_on(link.ensure_loaded(page)).unwrap();
    let seen = Arc::new(Seen::default());
    let _sub = link.subscribe(seen.clone());
    // Inside the external link: x 40..360, y 130..160.
    let at = |phase| PointerSample {
        phase,
        viewport: Point { x: 100.0, y: 145.0 },
        page: Some(PointerPageHit { page, point: Point { x: 100.0, y: 145.0 }, scale: None, rotation: None, zoom: None }),
        modifiers: Modifiers::default(),
        click_count: Some(1),
        pointer_type: Some(PointerKind::Touch),
        gesture: None,
        source: None,
    };
    interaction.dispatch_pointer(at(PointerPhase::Down));
    interaction.dispatch_pointer(at(PointerPhase::Up));
    block_on(link.is_loaded(page)).unwrap();
    let seen = seen.0.lock().unwrap();
    assert!(seen.iter().any(|e| matches!(e, LinkEvent::Activated { event }
        if matches!(&event.activation, LinkActivation::Uri { uri } if uri == "https://example.invalid/target"))),
        "{seen:?}");
}

#[test]
fn without_its_flag_the_link_plugin_is_not_installed() {
    let host = with_interaction(Arc::new(Delegate));
    let doc = block_on(host.open_document(test_support::fixture(test_support::FEATURES), options()))
        .unwrap();
    assert!(block_on(host.link(doc.id)).is_err());
}
```

`activating_a_goto_link_moves_the_stage` checks page 3 is visible after the reveal. The destination is `/XYZ 0 400 0` (top of page 3); a 300-point-tall viewport shows at most two 400-point pages at the fitted zoom, so page 3 being visible means the camera moved.

- [ ] **Step 2: Run them**

Run: `cargo test -p epdf-viewer-host`
Expected: PASS, the four new tests and every existing one. If `the_link_plugin_lists_and_activates_links_through_the_host` lists no links, the native engine's `listRawPage` snapshot shape differs from what `project_links` reads (`snap.annotations[*].subtype`, `.target`, `.flags`, `.ref`, `.rect`). Print the raw snapshot from a `translate` test against `features.pdf` before changing either side.

- [ ] **Step 3: Commit**

```bash
git add crates/epdf-viewer-host/tests/viewer_host.rs
git commit -m "Test links through the native engine, the stage and a tap"
```

### Task 6: Swift `LinkCapability` and tests

**Files:**
- Create: `platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Link.swift`, `platforms/apple/EmbedPDF/Tests/EmbedPDFTests/LinkTests.swift`

**Interfaces:**
- Consumes: generated `LinkHandle` and records.
- Produces: `LinkCapability` and `PDFViewer.link(for:)`.

- [ ] **Step 1: Confirm `features.pdf` reaches the test bundles**

Run: `grep -n 'features.pdf' platforms/apple/build-xcframework.sh`
Expected: one hit in `FIXTURES` (umbrella Task 0f put it there).

- [ ] **Step 2: Write `Link.swift`**

```swift
public import EmbedPDFBindings

public typealias PdfLinkTarget = EmbedPDFBindings.PdfLinkTarget
public typealias PdfDestination = EmbedPDFBindings.PdfDestination
public typealias AnnotationRef = EmbedPDFBindings.AnnotationRef
public typealias AnnotationRevisionToken = EmbedPDFBindings.AnnotationRevisionToken
public typealias LinkItem = EmbedPDFBindings.LinkItem
public typealias LinkHoverFlags = EmbedPDFBindings.LinkHoverFlags
public typealias LinkInput = EmbedPDFBindings.LinkInput
public typealias LinkResolution = EmbedPDFBindings.LinkResolution
public typealias LinkActivation = EmbedPDFBindings.LinkActivation
public typealias LinkEvent = EmbedPDFBindings.LinkEvent
public typealias LinkActivatedEvent = EmbedPDFBindings.LinkActivatedEvent
public typealias LinkLoadedEvent = EmbedPDFBindings.LinkLoadedEvent

/// A document's links: where they are, what they target, and activating
/// them. Get one with `PDFViewer.link(for:)`; the viewer needs
/// `ViewerConfig(interaction: true, link: true)`. It keeps the viewer
/// alive. After its document closes, every call throws `instance-closed`.
///
/// The plugin never opens a URL. A `.uri` activation, returned or on
/// `events`, is the app's to open.
public struct LinkCapability: Sendable {
  let handle: LinkHandle
  let viewer: PDFViewer

  /// Empty until the page loads (`ensureLoaded`).
  public func links(on page: PageRef) async throws(PDFError) -> [LinkItem] {
    try await bridged { try await handle.links(page: page) }
  }

  public func link(_ id: String, on page: PageRef) async throws(PDFError) -> LinkItem? {
    try await bridged { try await handle.link(page: page, id: id) }
  }

  /// The smallest link under a page point.
  public func link(at point: Point, on page: PageRef) async throws(PDFError) -> LinkItem? {
    try await bridged { try await handle.linkAt(page: page, point: point) }
  }

  public func allLinks() async throws(PDFError) -> [LinkItem] {
    try await bridged { try await handle.allLinks() }
  }

  public func ensureLoaded(_ page: PageRef) async throws(PDFError) {
    try await bridged { try await handle.ensureLoaded(page: page) }
  }

  public func isLoaded(_ page: PageRef) async throws(PDFError) -> Bool {
    try await bridged { try await handle.isLoaded(page: page) }
  }

  public func resolve(_ target: PdfLinkTarget) async throws(PDFError) -> LinkResolution {
    try await bridged { try await handle.resolve(target: target) }
  }

  public func activate(_ link: LinkItem) async throws(PDFError) -> LinkActivation {
    try await bridged { try await handle.activate(input: .link(page: link.page, id: link.id)) }
  }

  public func activate(_ target: PdfLinkTarget) async throws(PDFError) -> LinkActivation {
    try await bridged { try await handle.activate(input: .target(target: target)) }
  }

  /// `nil` when no link is under the point.
  public func activate(at point: Point, on page: PageRef) async throws(PDFError) -> LinkActivation? {
    try await bridged { try await handle.activateAt(page: page, point: point) }
  }

  public func label(for link: LinkItem) async throws(PDFError) -> String {
    try await bridged { try await handle.label(input: .link(page: link.page, id: link.id)) }
  }

  public func isNavigationEngaged() async throws(PDFError) -> Bool {
    try await bridged { try await handle.isNavigationEngaged() }
  }

  /// Finishes when the document closes (at once if it already has) or the
  /// viewer is destroyed.
  public var events: AsyncStream<LinkEvent> {
    viewer.registry.stream { handle.subscribe(listener: $0) }
  }
}

extension EventRelay: LinkEventListener where Event == LinkEvent {
  func onEvent(event: LinkEvent) {
    continuation.yield(event)
  }

  func onEnd() {
    continuation.finish()
  }
}

extension PDFViewer {
  public func link(for document: PDFDocument) async throws(PDFError) -> LinkCapability {
    let handle = try await bridged { try await host.link(documentId: document.id) }
    return LinkCapability(handle: handle, viewer: self)
  }
}
```

- [ ] **Step 3: Write `LinkTests.swift`**

```swift
import CustomDump
import EmbedPDF
import Testing

extension BaseSuite {
  @Suite struct LinkTests {
    func opened() async throws -> (PDFViewer, PDFDocument, LinkCapability, PageRef) {
      let viewer = try PDFViewer(ViewerConfig(interaction: true, link: true))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("features"))
      let page = try await viewer.pages(of: document)[0].page
      let link = try await viewer.link(for: document)
      try await link.ensureLoaded(page)
      return (viewer, document, link, page)
    }

    @Test func aPagesLinksAreListed() async throws {
      let (_, _, link, page) = try await opened()
      let links = try await link.links(on: page)
      expectNoDifference(links.count, 2)
    }

    @Test func aURILinkIsReportedToTheApp() async throws {
      let (_, _, link, page) = try await opened()
      let events = link.events
      let activation = try await link.activate(at: Point(x: 100, y: 145), on: page)
      expectNoDifference(activation, .uri(uri: "https://example.invalid/target"))
      for await event in events {
        guard case let .activated(event) = event else { continue }
        expectNoDifference(event.activation, .uri(uri: "https://example.invalid/target"))
        break
      }
    }

    @Test func aGotoLinkWithoutAStageReportsItsDestination() async throws {
      let (_, _, link, page) = try await opened()
      let activation = try await link.activate(at: Point(x: 100, y: 85), on: page)
      guard case let .destination(destination) = activation else {
        Issue.record("expected a destination, got \(String(describing: activation))")
        return
      }
      guard case .xyz = destination else {
        Issue.record("expected an xyz destination, got \(destination)")
        return
      }
    }

    @Test func aClosedDocumentsCapabilityThrows() async throws {
      let (viewer, document, link, page) = try await opened()
      try await viewer.close(document)
      await #expect {
        _ = try await link.links(on: page)
      } throws: { error in
        (error as? PDFError)?.code == "instance-closed"
      }
    }

    @Test func droppingTheStreamUnsubscribes() async throws {
      let (_, _, link, page) = try await opened()
      let task = Task { for await _ in link.events {} }
      try await Task.sleep(for: .milliseconds(20))
      task.cancel()
      _ = await task.value
      _ = try await link.activate(at: Point(x: 100, y: 145), on: page)
    }
  }
}
```

No link member takes a `CancelToken`, so there's no cancellation test.

- [ ] **Step 4: Run them**

Run: `bash platforms/apple/build-xcframework.sh && (cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17' -only-testing:EmbedPDFTests)`
Expected: TEST SUCCEEDED, including 5 `LinkTests`.

- [ ] **Step 5: Commit**

```bash
git add platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Link.swift platforms/apple/EmbedPDF/Tests/EmbedPDFTests/LinkTests.swift
git commit -m "Add the Swift link capability"
```

### Task 7: Docs, CI, merge and clean up

**Files:**
- Modify: `.github/workflows/plugin-rust.yml`, `platforms/apple/README.md`, `docs/superpowers/plans/2026-10-01-swift-package.md`

- [ ] **Step 1: Add the crate to CI**

Append `link` to the `crates=( ... )` list in `.github/workflows/plugin-rust.yml`, in plan-number order.

- [ ] **Step 2: Add the README row**

```markdown
| link | `LinkCapability` | `interaction: true, link: true` | `2026-10-06-swift-plugins/04-link.md` |
```

- [ ] **Step 3: Mark the spec entries delivered**

In `2026-10-01-swift-package.md` Appendix A §A.2, change `**link** (D, requires `interaction`, optional `stage`, `annotation`, `actions`).` to `**link** (D, requires `interaction`, optional `stage`, `annotation`, `actions`), delivered by 04-link.md.` In Appendix C, split the `page.annotations.list` row: `page.annotations.list` is mapped by 04-link.md; `create`, `update` and `delete` stay B2.

- [ ] **Step 4: Run the definition of done**

```bash
cargo test -p epdf-plugin-link --features uniffi
cargo test -p epdf-ffi -p epdf-viewer-host
cargo clippy -p epdf-plugin-link -p epdf-viewer-host --all-targets --features epdf-plugin-link/uniffi -- -D warnings
cargo test -p epdf-plugin-link
bash platforms/apple/build-xcframework.sh
(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17')
cargo build -p epdf-viewer-host -p uniffi-bindgen
./target/debug/uniffi-bindgen generate --library target/debug/libepdf_viewer_host.dylib --language kotlin --no-format --out-dir "$TMPDIR/epdf-kotlin-check"
```

Expected: all pass. Note the simulator `libepdf_viewer_host.a` size before and after.

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/plugin-rust.yml platforms/apple/README.md docs/superpowers/plans/2026-10-01-swift-package.md
git commit -m "List link in CI and the Swift README

libepdf_viewer_host.a (simulator, release): <before> -> <after> bytes."
```

- [ ] **Step 6: Merge into the integration branch**

In `.claude/worktrees/swift-plugins-integration`:

```bash
git merge --no-ff research/swift-link
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
git worktree remove .claude/worktrees/swift-link
git branch -d research/swift-link
git worktree prune
```

No `--force`. Tick 04 in the umbrella's list.
