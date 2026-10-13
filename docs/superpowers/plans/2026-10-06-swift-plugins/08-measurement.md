# Measurement: Swift Wrapper Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Export `epdf-plugin-measurement` through UniFFI as `MeasurementHandle`, wire it into `ViewerHost`, give the native engine the `page.measure` service it feature-detects, and ship `MeasurementCapability` in the `EmbedPDF` Swift package.

**Architecture:** The recipe in `2026-10-01-swift-package.md` Appendix A §A.0, worked as metadata's Task 6. The plugin keeps a page's scale as a set of measurement viewports. Without an engine `page.measure` service it keeps them in memory only (`crates/plugins/measurement/src/write/scale.rs:89`), so this plan also adds `measure.viewports` and `measure.setScale` to `epdf-engine` over the fork's `epdf_measure.h`, and maps `page.measure.viewports` and `page.measure.setScale` in `translate.rs`. A calibration then persists into the PDF's `/VP` array, as it does on the web.

**Tech Stack:** Rust 2024, `uniffi =0.32.0`, `bindgen` (via `pdfium-sys`), Swift 6.2 tools, Swift Testing, `swift-custom-dump` 1.3, Xcode 27.0, iPhone 17 simulator.

**Spec:** `docs/superpowers/plans/2026-10-06-swift-plugins/00-umbrella.md` (the shared contract), `docs/superpowers/plans/2026-10-01-swift-package.md` (Global Constraints, Appendix A §A.0 and §A.3, Appendix C), and the TypeScript this plan ports: `packages/engine/services/src/features/measure/` and the wire kinds at `packages/engine/core/src/wire/worker-protocol.ts:882-897`.

## Global Constraints

Everything under "Global Constraints" in `00-umbrella.md` applies. The ones this plan is most likely to break:

- `uniffi` is pinned at `=0.32.0`.
- UniFFI names are unique across the workspace. This plan's records are prefixed `Measurement` or `Scale`, never a bare `Readout`, `PageTarget` or `Unavailable`: `epdf-plugin-annotation` has a `RecalibrationReport` and plan 05 may export it.
- No `usize`, `isize`, `Rc` or `serde_json::Value` across the boundary. Every `Value` in `ScaleChangeReport` and `PageScale` decodes into a typed record, and a value that doesn't decode is an `invalid-state` error.
- The kernel is `!Send`; every call goes through `KernelBound`.
- `measurement` requires `annotation` and `interaction` (`crates/plugins/measurement/src/lib.rs:56-57`). This plan starts after plan 05 merges (Wave C).
- Measurement engine paths need the fork's `epdf_measure.h`. The PDFium builds under `packages/engine/runtime/build/libpdfium/` dated 2026-08-30 (`BUILD-METADATA.json` `createdAt`) don't ship it; the fork added it in `6dc31d14e` (2026-09-25), and umbrella Task 0b rebuilds the slices from a fork that has it.
- Swift value assertions use `expectNoDifference`.

## Review Focus

1. A page whose PDF already carries a foreign scale (an Acrobat `/VP` with a non-`RL` or non-EmbedPDF measure). People expect `pageScale` to report `.foreign` with the PDF's measure, and `setScale` to add EmbedPDF's own viewport beside it without deleting the foreign one. Pinned in Task 2 (`set_scale_keeps_a_foreign_viewport`) and Task 5 (`aForeignScaleIsReadAndKept`).
2. `setScale` on `.all` when one page's write fails. People expect a report per page, with the failing page carried as `.scaleFailed` and the other pages written. Pinned in Task 3 (`a_failed_page_write_is_reported_and_the_rest_land`).
3. A measure the engine's own validator refuses (`assertWritableMeasure`: an `RL` measure with no distance format). People expect `invalid-input`, not a corrupt `/VP` in the saved file. Pinned in Task 2 (`set_scale_refuses_an_unwritable_measure`).
4. A calibration whose two points coincide (a zero-length capture). People expect `invalid-input` from `calibrate`, not a scale with an infinite ratio. Pinned in Task 3 (`calibrating_a_zero_length_is_refused`).
5. Swift calling `pageScale` before the page's viewports have loaded. People expect `ready == false` and no measure, then `ready == true` after `ensureLoaded`. Pinned in Task 3 (`a_page_reads_loading_until_ensure_loaded`) and Task 5 (`ensureLoadedMakesThePageReady`).

## Cross-plugin tests

`measurement` has no optional plugins. Its required ones (`annotation`, `interaction`) are already merged when this plan starts, so every test here builds a kernel or host with all three. `createMeasurement` returns an `AnnotationRef` that plan 05's `AnnotationCapability` can read back; this plan owns that round-trip test (Task 5, `aCreatedMeasurementIsAnAnnotation`).

## File Structure

```
crates/pdfium-sys/wrapper.h                       + #include "epdf_measure.h"
crates/pdfium/src/measure.rs                      new: Viewport, Measure, NumberFormat over EPDFPage_*Viewport*, EPDFViewport_*, EPDFMeasure_*
crates/pdfium/src/lib.rs                          + pub mod measure; re-exports
crates/pdfium/tests/measure.rs                    new
crates/epdf-engine/src/features/measure.rs        new: read_viewports, set_scale
crates/epdf-engine/src/features/mod.rs            + pub mod measure;
crates/epdf-engine/src/host.rs                    + "measure.viewports", "measure.setScale"
crates/epdf-engine/tests/measure.rs               new
crates/epdf-viewer-host/src/translate.rs          + page.measure.viewports, page.measure.setScale; PATHS lines
crates/epdf-viewer-host/tests/translate.rs        + two tests
crates/plugins/measurement/Cargo.toml             + uniffi feature
crates/plugins/measurement/src/lib.rs             + scaffolding, pub mod ffi
crates/plugins/measurement/src/controller.rs      + pub(crate) fn lifetime
crates/plugins/measurement/src/ffi.rs             new
crates/plugins/measurement/tests/ffi.rs           new
crates/epdf-viewer-host/{Cargo.toml,src/api.rs,src/session.rs,src/lib.rs}   shared-file additions
crates/epdf-viewer-host/tests/viewer_host.rs      + one test
platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Measurement.swift          new
platforms/apple/EmbedPDF/Tests/EmbedPDFTests/MeasurementTests.swift          new
platforms/apple/build-xcframework.sh              + measure-acrobat-metric fixture
.github/workflows/plugin-rust.yml, platforms/apple/README.md                 shared-file additions
```

---

### Task 1: Worktree and the PDFium prerequisite

**Files:**
- Modify: `crates/pdfium-sys/wrapper.h`

**Interfaces:**
- Consumes: branch `research/swift-plugins` with plans 01 (interaction) and 05 (annotation) merged.
- Produces: `pdfium_sys::EPDFPage_CountViewports` and the rest of `epdf_measure.h`, for Task 2.

- [ ] **Step 1: Create the worktree**

```bash
git worktree add .claude/worktrees/swift-measurement -b research/swift-measurement research/swift-plugins
cd .claude/worktrees/swift-measurement
```

- [ ] **Step 2: Check the PDFium slices ship `epdf_measure.h`**

```bash
for t in darwin-arm64 ios-arm64 ios-sim-arm64; do
  ls packages/engine/runtime/build/libpdfium/$t/include/epdf_measure.h || echo "MISSING $t"
done
```

Expected: three paths, no `MISSING`. If any slice is missing it, this worktree fetched the old slices: rerun `bash packages/engine/runtime/scripts/fetch-libpdfium.sh <slice>`, and if it's still missing, stop and finish umbrella Task 0b. Don't continue on an old slice: `bindgen` would silently generate no viewport functions and Task 2 fails to compile with a misleading error.

- [ ] **Step 3: Include the header**

In `crates/pdfium-sys/wrapper.h`, after `#include "epdf_form.h"`:

```c
#include "epdf_measure.h"
```

- [ ] **Step 4: Confirm the bindings**

Run: `cargo build -p pdfium-sys && grep -c 'pub fn EPDFPage_CountViewports\|pub fn EPDFViewport_GetMeasure\|pub fn EPDFMeasure_GetRatio' target/debug/build/pdfium-sys-*/out/bindings.rs`
Expected: `3`.

- [ ] **Step 5: Commit**

```bash
git add crates/pdfium-sys/wrapper.h
git commit -m "Bind the fork's measurement viewports in pdfium-sys"
```

### Task 2: The `page.measure` service in `pdfium` and `epdf-engine`

**Files:**
- Create: `crates/pdfium/src/measure.rs`, `crates/pdfium/tests/measure.rs`
- Modify: `crates/pdfium/src/lib.rs`
- Create: `crates/epdf-engine/src/features/measure.rs`, `crates/epdf-engine/tests/measure.rs`
- Modify: `crates/epdf-engine/src/features/mod.rs`, `crates/epdf-engine/src/host.rs`

**Interfaces:**
- Consumes: Task 1's bindings.
- Produces: wire kinds `measure.viewports` (`{pageObjectNumber}` → `{viewports: PageMeasurementViewport[]}`) and `measure.setScale` (`{pageObjectNumber, measure: PdfMeasure | null}` → mutation envelope `{result: {}, mutationSequence, event}`), the shapes `worker-protocol.ts:882-897` and `:1400-1406` give. Task 4 maps them.

The TypeScript is three files: `MeasureReader.ts` (22 lines), `MeasureMutator.ts` (85 lines) and `internal/measureCodec.ts` (163 lines), with `internal/readViewports.ts` (32 lines). The Rust follows them function for function. `CALIBRATION_NAME` is `"EmbedPDF"`. A viewport is EmbedPDF's own (`owned`) when its name is `"EmbedPDF"` and it has no measure or an `RL` one. A write never rewrites `/VP`: it updates EmbedPDF's last owned viewport, or adds one over the crop box (falling back to the media box), or, for `null`, removes every owned viewport from the highest index down.

- [ ] **Step 1: Write the failing `pdfium` test**

`packages/engine/main/test/fixtures/measure-acrobat-metric.pdf` is Acrobat's own metric calibration: one page, one viewport with an `RL` measure that isn't named `EmbedPDF`.

```rust
// crates/pdfium/tests/measure.rs
use pdfium::measure::{MeasureWrite, NumberFormatWrite};
use pdfium::{Document, Library};

fn fixture() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../packages/engine/main/test/fixtures/measure-acrobat-metric.pdf"
    ))
    .unwrap()
}

#[test]
fn an_acrobat_viewport_reads_with_its_measure() {
    let library = Library::init();
    let doc = Document::load_mem(&library, fixture(), None).unwrap();
    let page = doc.page(0).unwrap();
    let viewports = page.viewports().unwrap();
    assert_eq!(viewports.len(), 1);
    let vp = &viewports[0];
    assert_ne!(vp.name.as_deref(), Some("EmbedPDF"));
    let measure = vp.measure.as_ref().expect("a measure");
    assert_eq!(measure.subtype, "RL");
    assert!(!measure.distance.is_empty());
}

#[test]
fn a_written_viewport_reads_back_and_removes() {
    let library = Library::init();
    let doc = Document::load_mem(&library, fixture(), None).unwrap();
    let page = doc.page(0).unwrap();
    let bbox = page.crop_box().unwrap();
    let measure = MeasureWrite {
        ratio: "1 in = 1 m".into(),
        x: vec![NumberFormatWrite::unit("m", 1.0 / 72.0, 100.0)],
        distance: vec![NumberFormatWrite::unit("m", 1.0, 100.0)],
        area: vec![NumberFormatWrite::unit("m²", 1.0, 100.0)],
    };
    page.upsert_named_viewport("EmbedPDF", bbox, &measure).unwrap();
    let viewports = page.viewports().unwrap();
    assert_eq!(viewports.len(), 2);
    assert_eq!(viewports[1].name.as_deref(), Some("EmbedPDF"));
    assert_eq!(viewports[1].measure.as_ref().unwrap().ratio.as_deref(), Some("1 in = 1 m"));
    page.remove_named_viewports("EmbedPDF").unwrap();
    assert_eq!(page.viewports().unwrap().len(), 1);
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p pdfium --test measure`
Expected: FAIL, `could not find measure in pdfium`.

- [ ] **Step 3: Write `crates/pdfium/src/measure.rs`**

```rust
//! Measurement viewports (`/VP`) and their `/Measure` dictionaries.
//!
//! Wraps the fork's `epdf_measure.h`: `EPDFPage_*Viewport*`,
//! `EPDFViewport_*`, `EPDFMeasure_*` and `EPDFNumberFormat_*`. The shapes
//! are `PageMeasurementViewport` and `PdfMeasure` as
//! `packages/engine/services/src/features/measure/internal/measureCodec.ts`
//! reads and writes them.

use crate::{PageView, PdfiumError, RectF};

/// One `/NumberFormat` dictionary, as read.
#[derive(Clone, Debug, PartialEq)]
pub struct NumberFormat {
    pub unit: String,
    pub conversion: Option<f64>,
    pub fraction: Option<String>,
    pub precision: Option<f64>,
    pub fixed: Option<bool>,
    pub thousands: Option<String>,
    pub decimal: Option<String>,
    pub prefix_spacing: Option<String>,
    pub suffix_spacing: Option<String>,
    pub label_position: Option<String>,
}

/// One `/Measure` dictionary. A non-`RL` (foreign) measure has only its
/// subtype.
#[derive(Clone, Debug, PartialEq)]
pub struct Measure {
    pub subtype: String,
    pub ratio: Option<String>,
    pub x: Vec<NumberFormat>,
    pub y: Vec<NumberFormat>,
    pub distance: Vec<NumberFormat>,
    pub area: Vec<NumberFormat>,
    pub angle: Vec<NumberFormat>,
    pub slope: Vec<NumberFormat>,
    pub origin: Option<(f64, f64)>,
    pub cyx: Option<f64>,
}

/// One `/VP` entry.
#[derive(Clone, Debug, PartialEq)]
pub struct Viewport {
    pub bbox: RectF,
    pub name: Option<String>,
    pub measure: Option<Measure>,
}

/// What a write sets: an `RL` measure's ratio and its three required
/// format lists. `assertWritableMeasure` in the engine validates first.
#[derive(Clone, Debug, PartialEq)]
pub struct MeasureWrite {
    pub ratio: String,
    pub x: Vec<NumberFormatWrite>,
    pub distance: Vec<NumberFormatWrite>,
    pub area: Vec<NumberFormatWrite>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NumberFormatWrite {
    pub unit: String,
    pub conversion: f64,
    pub precision: Option<f64>,
    pub fraction: Option<String>,
}

impl NumberFormatWrite {
    pub fn unit(unit: &str, conversion: f64, precision: f64) -> Self {
        NumberFormatWrite {
            unit: unit.into(),
            conversion,
            precision: Some(precision),
            fraction: None,
        }
    }
}

/// `EPDF_MEASURE_SUBTYPE`: 1 is `RL`, anything else is foreign and is
/// reported by its name.
const SUBTYPE_RL: i32 = 1;

/// The format lists by `EPDF_MEASURE_FORMAT_*` index, in the header's order.
const LISTS: [&str; 6] = ["x", "y", "d", "a", "t", "s"];

impl PageView<'_> {
    /// Every viewport in drawing order. A viewport whose `/BBox` doesn't
    /// read is skipped, as `readViewports.ts` skips it.
    pub fn viewports(&self) -> Result<Vec<Viewport>, PdfiumError> {
        // SAFETY: the page handle is live for the borrow.
        let page = unsafe { self.as_raw() };
        let count = unsafe { pdfium_sys::EPDFPage_CountViewports(page) };
        let mut out = Vec::with_capacity(count as usize);
        for i in 0..count {
            let vp = unsafe { pdfium_sys::EPDFPage_GetViewport(page, i as i32) };
            if vp.is_null() {
                continue;
            }
            let mut rect = pdfium_sys::FS_RECTF::default();
            if unsafe { pdfium_sys::EPDFViewport_GetBBox(vp, &mut rect) } == 0 {
                continue;
            }
            let name = crate::text::read_utf16_opt(|buf, len| unsafe {
                pdfium_sys::EPDFViewport_GetName(vp, buf, len)
            })?;
            let measure = read_measure(unsafe { pdfium_sys::EPDFViewport_GetMeasure(vp) })?;
            out.push(Viewport { bbox: RectF::from(rect).normalized(), name, measure });
        }
        Ok(out)
    }

    /// Update the last viewport named `name` whose measure is absent or
    /// `RL`, or add one, over `bbox`, carrying `measure`.
    pub fn upsert_named_viewport(
        &self,
        name: &str,
        bbox: RectF,
        measure: &MeasureWrite,
    ) -> Result<(), PdfiumError> {
        let page = unsafe { self.as_raw() };
        let owned = self.owned_indices(name)?;
        let rect: pdfium_sys::FS_RECTF = bbox.into();
        let vp = match owned.last() {
            Some(&i) => unsafe { pdfium_sys::EPDFPage_GetViewport(page, i as i32) },
            None => unsafe { pdfium_sys::EPDFPage_AddViewport(page, &rect) },
        };
        require(!vp.is_null(), "viewport")?;
        require(unsafe { pdfium_sys::EPDFViewport_SetBBox(vp, &rect) } != 0, "viewport bbox")?;
        crate::text::with_utf16(name, |wide| {
            require(unsafe { pdfium_sys::EPDFViewport_SetName(vp, wide) } != 0, "viewport name")
        })?;
        let m = unsafe { pdfium_sys::EPDFViewport_AddMeasure(vp) };
        require(!m.is_null(), "measure")?;
        write_measure(m, measure)
    }

    /// Remove every viewport `upsert_named_viewport` would update, highest
    /// index first so the lower indices stay valid.
    pub fn remove_named_viewports(&self, name: &str) -> Result<(), PdfiumError> {
        let page = unsafe { self.as_raw() };
        for i in self.owned_indices(name)?.into_iter().rev() {
            require(
                unsafe { pdfium_sys::EPDFPage_RemoveViewport(page, i as i32) } != 0,
                "remove viewport",
            )?;
        }
        Ok(())
    }

    fn owned_indices(&self, name: &str) -> Result<Vec<u32>, PdfiumError> {
        Ok(self
            .viewports_with_index()?
            .into_iter()
            .filter(|(_, vp)| {
                vp.name.as_deref() == Some(name)
                    && vp.measure.as_ref().is_none_or(|m| m.subtype == "RL")
            })
            .map(|(i, _)| i)
            .collect())
    }

    fn viewports_with_index(&self) -> Result<Vec<(u32, Viewport)>, PdfiumError> {
        let page = unsafe { self.as_raw() };
        let count = unsafe { pdfium_sys::EPDFPage_CountViewports(page) };
        let mut out = Vec::new();
        for i in 0..count {
            let vp = unsafe { pdfium_sys::EPDFPage_GetViewport(page, i as i32) };
            if vp.is_null() {
                continue;
            }
            let name = crate::text::read_utf16_opt(|buf, len| unsafe {
                pdfium_sys::EPDFViewport_GetName(vp, buf, len)
            })?;
            let measure = read_measure(unsafe { pdfium_sys::EPDFViewport_GetMeasure(vp) })?;
            let mut rect = pdfium_sys::FS_RECTF::default();
            unsafe { pdfium_sys::EPDFViewport_GetBBox(vp, &mut rect) };
            out.push((i, Viewport { bbox: RectF::from(rect).normalized(), name, measure }));
        }
        Ok(out)
    }
}

fn require(ok: bool, what: &'static str) -> Result<(), PdfiumError> {
    if ok { Ok(()) } else { Err(PdfiumError::WriteFailed(what)) }
}

fn read_measure(m: pdfium_sys::EPDF_MEASURE) -> Result<Option<Measure>, PdfiumError> {
    if m.is_null() {
        return Ok(None);
    }
    let subtype = unsafe { pdfium_sys::EPDFMeasure_GetSubtype(m) };
    if subtype != SUBTYPE_RL as _ {
        // `measureCodec.ts` `readMeasure`: a foreign measure is its subtype
        // name and nothing else. `GEO` is the only other the header names.
        return Ok(Some(Measure { subtype: "GEO".into(), ..Measure::empty() }));
    }
    let ratio = crate::text::read_utf16_opt(|buf, len| unsafe {
        pdfium_sys::EPDFMeasure_GetRatio(m, buf, len)
    })?;
    let list = |which: usize| read_formats(m, which as i32);
    let mut origin = pdfium_sys::FS_POINTF::default();
    let has_origin = unsafe { pdfium_sys::EPDFMeasure_GetOrigin(m, &mut origin) } != 0;
    let mut cyx = 0f32;
    let has_cyx = unsafe { pdfium_sys::EPDFMeasure_GetCYX(m, &mut cyx) } != 0;
    Ok(Some(Measure {
        subtype: "RL".into(),
        ratio,
        x: list(0)?,
        y: list(1)?,
        distance: list(2)?,
        area: list(3)?,
        angle: list(4)?,
        slope: list(5)?,
        origin: has_origin.then_some((origin.x as f64, origin.y as f64)),
        cyx: has_cyx.then_some(cyx as f64),
    }))
}

impl Measure {
    fn empty() -> Self {
        Measure {
            subtype: String::new(),
            ratio: None,
            x: Vec::new(),
            y: Vec::new(),
            distance: Vec::new(),
            area: Vec::new(),
            angle: Vec::new(),
            slope: Vec::new(),
            origin: None,
            cyx: None,
        }
    }
}
```

`read_formats` and `write_measure` transcribe `measureCodec.ts`'s `readFormats` and `writeMeasure`. Each format field has one getter and one setter in `epdf_measure.h` (lines 200-266): `EPDFMeasure_CountFormats(m, list)`, `EPDFMeasure_GetFormat(m, list, i)`, then on the `EPDF_NUMBERFORMAT` `EPDFNumberFormat_GetUnit`/`SetUnit` (UTF-16), `GetConversion`/`SetConversion` (`f32`), `GetPrecision`, `GetFraction` (an `EPDF_FRACTION_*` code: 0 decimal, 1 fraction, 2 round, 3 truncate), `GetFixed`, `GetThousands`, `GetDecimal`, `GetPrefixSpacing`, `GetSuffixSpacing`, `GetLabelPosition` (0 suffix, 1 prefix). Write the two functions field by field against that list, in `measureCodec.ts`'s order; `write_measure` calls `EPDFMeasure_RemoveFormats(m, list)` before `EPDFMeasure_AddFormat` for each of `x`, `d`, `a`, as the TypeScript does, so a rewrite never appends to an old list. `LISTS` names the index order for the doc comment on each.

Add `pub mod measure;` to `crates/pdfium/src/lib.rs`, and `read_utf16_opt` / `with_utf16` to `crates/pdfium/src/text.rs` if they aren't there under those names (`action.rs:379` has a private `read_utf16` of the same shape: move it to `text.rs` as `pub(crate) fn read_utf16_opt` and call it from both). Add `PdfiumError::WriteFailed(&'static str)` if `error.rs` has no variant for a refused write.

- [ ] **Step 4: Run the `pdfium` test**

Run: `cargo test -p pdfium --test measure`
Expected: PASS, 2 tests.

- [ ] **Step 5: Write the failing engine test**

```rust
// crates/epdf-engine/tests/measure.rs
use epdf_engine::host::Host;
use serde_json::{Value, json};

mod common;
use common::{call, open_fixture};

const METRIC: &str = "packages/engine/main/test/fixtures/measure-acrobat-metric.pdf";

fn rl_measure() -> Value {
    json!({
        "subtype": "RL",
        "ratio": "1 in = 1 m",
        "x": [{ "unit": "m", "conversion": 0.013888888888888888, "precision": 100 }],
        "distance": [{ "unit": "m", "conversion": 1, "precision": 100 }],
        "area": [{ "unit": "m²", "conversion": 1, "precision": 100 }]
    })
}

#[test]
fn viewports_read_the_acrobat_scale_as_foreign() {
    let mut host = Host::new();
    let (doc, pon) = open_fixture(&mut host, METRIC);
    let result = call(&mut host, json!({ "kind": "measure.viewports", "docId": doc, "pageObjectNumber": pon }));
    let viewports = result["viewports"].as_array().unwrap();
    assert_eq!(viewports.len(), 1);
    assert_eq!(viewports[0]["owned"], json!(false));
    assert_eq!(viewports[0]["measure"]["subtype"], json!("RL"));
}

#[test]
fn set_scale_keeps_a_foreign_viewport() {
    let mut host = Host::new();
    let (doc, pon) = open_fixture(&mut host, METRIC);
    let written = call(&mut host, json!({
        "kind": "measure.setScale", "docId": doc, "pageObjectNumber": pon, "measure": rl_measure()
    }));
    assert!(written.get("event").is_some(), "a mutation publishes its event: {written}");
    let result = call(&mut host, json!({ "kind": "measure.viewports", "docId": doc, "pageObjectNumber": pon }));
    let viewports = result["viewports"].as_array().unwrap();
    assert_eq!(viewports.len(), 2);
    assert_eq!(viewports[1]["name"], json!("EmbedPDF"));
    assert_eq!(viewports[1]["owned"], json!(true));
    call(&mut host, json!({
        "kind": "measure.setScale", "docId": doc, "pageObjectNumber": pon, "measure": null
    }));
    let result = call(&mut host, json!({ "kind": "measure.viewports", "docId": doc, "pageObjectNumber": pon }));
    assert_eq!(result["viewports"].as_array().unwrap().len(), 1);
}

#[test]
fn set_scale_refuses_an_unwritable_measure() {
    let mut host = Host::new();
    let (doc, pon) = open_fixture(&mut host, METRIC);
    let response = host.call(
        &json!({
            "kind": "measure.setScale", "jobId": 1, "docId": doc, "pageObjectNumber": pon,
            "measure": { "subtype": "RL", "ratio": "1 in = 1 m", "x": [] }
        })
        .to_string(),
        Vec::new(),
    );
    let envelope: Value = serde_json::from_str(&response.json).unwrap();
    assert_eq!(envelope["error"]["code"], json!("InvalidArg"), "{envelope}");
}
```

If `crates/epdf-engine/tests/common/mod.rs` has no `open_fixture` and `call`, add them there: `open_fixture` reads the file relative to the workspace root, sends `open.fatMem` with the bytes as the payload, then `pages.list`, and returns the doc id and the first page's `pageObjectNumber`; `call` adds a `jobId`, sends, panics on an `error` envelope, and returns `result`.

- [ ] **Step 6: Run it to verify it fails**

Run: `cargo test -p epdf-engine --test measure`
Expected: FAIL, the envelope's error is `unknown request kind "measure.viewports"`.

- [ ] **Step 7: Write `crates/epdf-engine/src/features/measure.rs`**

```rust
//! `measure.viewports` and `measure.setScale`: a page's measurement
//! viewports, and EmbedPDF's full-page calibration among them.
//!
//! `MeasureReader.ts` and `MeasureMutator.ts`. Other producers' viewports
//! survive every write; `/VP` itself is never rewritten.

use pdfium::measure::{Measure, MeasureWrite, NumberFormat, NumberFormatWrite, Viewport};
use pdfium::{PageBoxType, PageView};
use serde_json::{Value, json};

use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};

pub const CALIBRATION_NAME: &str = "EmbedPDF";

pub fn read_viewports(page: &PageView<'_>, cancel: &CancelToken) -> Result<Value, EngineError> {
    cancel.check()?;
    let viewports = page.viewports().map_err(EngineError::from)?;
    Ok(json!({ "viewports": viewports.iter().map(viewport_json).collect::<Vec<_>>() }))
}

pub fn set_scale(
    page: &PageView<'_>,
    measure: Option<&Value>,
    cancel: &CancelToken,
) -> Result<Value, EngineError> {
    cancel.check()?;
    let Some(measure) = measure.filter(|m| !m.is_null()) else {
        page.remove_named_viewports(CALIBRATION_NAME).map_err(EngineError::from)?;
        return Ok(json!({}));
    };
    let write = writable(measure)?;
    let bbox = page
        .box_rect(PageBoxType::Crop)
        .or_else(|| page.box_rect(PageBoxType::Media))
        .filter(|r| [r.left, r.right, r.bottom, r.top].iter().all(|v| v.is_finite()))
        .ok_or_else(|| invalid("Page has no valid calibration box"))?;
    cancel.check()?;
    page.upsert_named_viewport(CALIBRATION_NAME, bbox.normalized(), &write)
        .map_err(EngineError::from)?;
    Ok(json!({}))
}

fn invalid(message: &str) -> EngineError {
    EngineError::with_message(EngineErrorCode::InvalidArg, message)
}

/// `assertWritableMeasure`: an `RL` measure with a ratio and non-empty
/// `x`, `distance` and `area` lists, each format with a unit and a finite
/// positive conversion.
fn writable(measure: &Value) -> Result<MeasureWrite, EngineError> {
    if measure.get("subtype").and_then(Value::as_str) != Some("RL") {
        return Err(invalid("only an RL measure can be written"));
    }
    let ratio = measure
        .get("ratio")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("an RL measure needs a ratio"))?;
    let list = |key: &str| -> Result<Vec<NumberFormatWrite>, EngineError> {
        let items = measure
            .get(key)
            .and_then(Value::as_array)
            .filter(|a| !a.is_empty())
            .ok_or_else(|| invalid(&format!("an RL measure needs a non-empty {key} list")))?;
        items
            .iter()
            .map(|f| {
                let unit = f.get("unit").and_then(Value::as_str).filter(|u| !u.is_empty());
                let conversion =
                    f.get("conversion").and_then(Value::as_f64).filter(|c| c.is_finite() && *c > 0.0);
                match (unit, conversion) {
                    (Some(unit), Some(conversion)) => Ok(NumberFormatWrite {
                        unit: unit.into(),
                        conversion,
                        precision: f.get("precision").and_then(Value::as_f64),
                        fraction: f.get("fraction").and_then(Value::as_str).map(Into::into),
                    }),
                    _ => Err(invalid(&format!("a {key} format needs a unit and a positive conversion"))),
                }
            })
            .collect()
    };
    Ok(MeasureWrite {
        ratio: ratio.into(),
        x: list("x")?,
        distance: list("distance")?,
        area: list("area")?,
    })
}

fn viewport_json(vp: &Viewport) -> Value {
    let owned = vp.name.as_deref() == Some(CALIBRATION_NAME)
        && vp.measure.as_ref().is_none_or(|m| m.subtype == "RL");
    let mut out = json!({
        "bbox": { "left": vp.bbox.left, "bottom": vp.bbox.bottom, "right": vp.bbox.right, "top": vp.bbox.top },
        "owned": owned,
    });
    if let Some(name) = &vp.name {
        out["name"] = json!(name);
    }
    if let Some(measure) = &vp.measure {
        out["measure"] = measure_json(measure);
    }
    out
}

fn measure_json(m: &Measure) -> Value {
    if m.subtype != "RL" {
        return json!({ "subtype": m.subtype });
    }
    let mut out = json!({ "subtype": "RL" });
    let mut put = |key: &str, list: &[NumberFormat]| {
        if !list.is_empty() {
            out[key] = Value::Array(list.iter().map(format_json).collect());
        }
    };
    put("x", &m.x);
    put("y", &m.y);
    put("distance", &m.distance);
    put("area", &m.area);
    put("angle", &m.angle);
    put("slope", &m.slope);
    if let Some(ratio) = &m.ratio {
        out["ratio"] = json!(ratio);
    }
    if let Some((x, y)) = m.origin {
        out["origin"] = json!({ "x": x, "y": y });
    }
    if let Some(cyx) = m.cyx {
        out["cyx"] = json!(cyx);
    }
    out
}

fn format_json(f: &NumberFormat) -> Value {
    let mut out = json!({ "unit": f.unit });
    let fields: [(&str, Option<Value>); 9] = [
        ("conversion", f.conversion.map(Value::from)),
        ("fraction", f.fraction.clone().map(Value::from)),
        ("precision", f.precision.map(Value::from)),
        ("fixed", f.fixed.map(Value::from)),
        ("thousands", f.thousands.clone().map(Value::from)),
        ("decimal", f.decimal.clone().map(Value::from)),
        ("prefixSpacing", f.prefix_spacing.clone().map(Value::from)),
        ("suffixSpacing", f.suffix_spacing.clone().map(Value::from)),
        ("labelPosition", f.label_position.clone().map(Value::from)),
    ];
    for (key, value) in fields {
        if let Some(value) = value {
            out[key] = value;
        }
    }
    out
}
```

Route both kinds in `crates/epdf-engine/src/host.rs`. `measure.viewports` goes with the per-page reads:

```rust
            "measure.viewports" => self
                .on_page(&key, body, |page| f::measure::read_viewports(page, cancel)),
```

`measure.setScale` goes with the mutations, through the same helper `pages.rotate` uses so it carries a `mutationSequence` and publishes a `pages.changed`-style document event. Use the event kind `measure.changed` with `{pageObjectNumber}`; the measurement plugin re-reads on its own after a write (`write/scale.rs:93`), so the event exists for other listeners and isn't load-bearing here:

```rust
            "measure.setScale" => self.mutate_page(&key, body, "measure.changed", |page| {
                f::measure::set_scale(page, body.get("measure"), cancel)
            }),
```

If `host.rs` has no `mutate_page` helper, follow the shape of the `"pages.rotate"` arm (`host.rs:821`) exactly and name the event the same way. Add `pub mod measure;` to `features/mod.rs`. Add both kinds to the wire-contract list `crates/epdf-engine/tests/wire_contract.rs` checks, if it enumerates kinds.

- [ ] **Step 8: Run the engine tests**

Run: `cargo test -p epdf-engine --test measure && cargo test -p epdf-engine`
Expected: PASS, 3 new tests, nothing else broken.

- [ ] **Step 9: Commit**

```bash
git add crates/pdfium crates/epdf-engine
git commit -m "Read and write measurement viewports in the Rust engine"
```

### Task 3: The measurement FFI module

**Files:**
- Modify: `crates/plugins/measurement/Cargo.toml`, `crates/plugins/measurement/src/lib.rs`, `crates/plugins/measurement/src/controller.rs`, `crates/plugins/measurement/src/contract.rs`
- Create: `crates/plugins/measurement/src/ffi.rs`, `crates/plugins/measurement/tests/ffi.rs`

**Interfaces:**
- Consumes: `epdf_ffi::{CancelToken, KernelBound, KernelPost, PageRef, Point, Subscription, ViewerError}`; `epdf_ffi::AnnotationRef` with `From<epdf_annotation_core::AnnotationRef>` and back, from plan 05 (`crates/epdf-ffi/src/annotation.rs`).
- Produces (Rust path `epdf_plugin_measurement::ffi`): `MeasurementHandle` with `new(cap: MeasurementCapability, post: KernelPost) -> Arc<Self>`; records `MeasurementConfig`, `MeasurementDefaultScale`, `PdfMeasurement`, `PdfNumberFormat`, `MeasureFraction`, `MeasureLabelPosition`, `MeasurementPageTarget`, `MeasurementPageScale`, `MeasurementScaleSource`, `MeasurementReading`, `MeasurementReadout`, `MeasurementReadoutKind`, `MeasurementUnavailable`, `MeasurementCalibrationRequest`, `MeasurementCalibrateInput`, `MeasurementCalibrateOptions`, `MeasurementKind`, `CreateMeasurementInput`, `ScaleChangeReport`, `ScaleRecalibration`, `ScaleSkippedAnnotation`, `ScaleSkipReason`, `ScaleFailedAnnotation`, `MeasurementErrorInfo`, `MeasurementEvent`; derived in place: `ScalePreset`, `ScaleChangeOptions`; trait `MeasurementEventListener`.

Before writing a record, run `grep -rn 'pub struct PdfMeasurement\|pub struct PdfNumberFormat\|pub enum MeasureFraction' crates/epdf-ffi/src crates/plugins/*/src/ffi.rs`. If plan 05 already exported any of them, import that one and leave its mirror out of this file.

- [ ] **Step 1: Add the feature and scaffolding**

`crates/plugins/measurement/Cargo.toml`:

```toml
[features]
default = ["harness"]
harness = [
  "dep:plugin-conformance",
  "epdf-plugin-annotation/harness",
  "epdf-plugin-interaction/harness",
]
# The uniffi exports (`ffi` module), for `epdf-viewer-host`.
uniffi = ["dep:uniffi", "dep:epdf-ffi"]

[dependencies]
# (existing lines unchanged)
epdf-ffi = { path = "../../epdf-ffi", optional = true }
uniffi = { version = "=0.32.0", optional = true }

[dev-dependencies]
futures = { version = "0.3", default-features = false, features = ["std", "executor"] }
plugin-conformance = { path = "../../plugin-conformance" }

[[test]]
name = "ffi"
required-features = ["uniffi"]
```

`crates/plugins/measurement/src/lib.rs`, after the `harness` module:

```rust
#[cfg(feature = "uniffi")]
pub mod ffi;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();
```

`crates/plugins/measurement/src/controller.rs`, in `impl MeasurementCapability`:

```rust
    /// The instance lifetime, aborted when the document closes. Not in the
    /// TypeScript: the foreign handle's `instance-closed` check reads it.
    pub(crate) fn lifetime(&self) -> &epdf_kernel::signal::AbortSignal {
        self.s().cx.lifetime()
    }
```

In `crates/plugins/measurement/src/contract.rs`, derive in place on the two records whose fields are already UniFFI types:

```rust
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScalePreset { /* unchanged */ }

#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct ScaleChangeOptions {
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    #[serde(default)]
    pub recalculate: Option<bool>,
}
```

- [ ] **Step 2: Write the failing tests**

```rust
// crates/plugins/measurement/tests/ffi.rs
#![cfg(feature = "uniffi")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use epdf_ffi::{CancelToken, KernelThread, PageRef, Point, ViewerError};
use epdf_plugin_annotation::{AnnotationConfig, annotation_plugin};
use epdf_plugin_interaction::{InteractionConfig, interaction_plugin};
use epdf_plugin_measurement::ffi::{
    MeasurementCalibrateInput, MeasurementCalibrateOptions, MeasurementEvent,
    MeasurementEventListener, MeasurementHandle, MeasurementPageTarget, MeasurementReading,
    MeasurementScaleSource, PdfMeasurement, PdfNumberFormat, ScaleChangeReport,
};
use epdf_plugin_measurement::{MEASUREMENT, MeasurementConfig, ScaleChangeOptions, measurement_plugin};
use futures::executor::block_on;
use plugin_conformance::test_kernel::{Reply, TestKernel, TestSpec};
use serde_json::{Value, json};

#[derive(Default)]
struct Events(Mutex<Vec<MeasurementEvent>>, AtomicUsize);

impl MeasurementEventListener for Events {
    fn on_event(&self, event: MeasurementEvent) {
        self.0.lock().unwrap().push(event);
    }
    fn on_end(&self) {
        self.1.fetch_add(1, Ordering::SeqCst);
    }
}

const P1: PageRef = PageRef { page_object_number: 1 };
const P2: PageRef = PageRef { page_object_number: 2 };

fn page(pon: u32, w: f64, h: f64) -> Value {
    json!({
        "index": pon - 1, "ref": { "kind": "objectNumber", "pageObjectNumber": pon },
        "label": null, "size": { "width": w, "height": h }, "rotation": 0, "userUnit": 1,
        "boxes": {
            "media": { "left": 0, "bottom": 0, "right": w, "top": h },
            "crop": { "left": 0, "bottom": 0, "right": w, "top": h }
        }
    })
}

/// The `RL` measure the conformance vectors use: 1 in on paper is 1 m.
fn metric() -> PdfMeasurement {
    let format = |unit: &str, conversion: f64| PdfNumberFormat {
        unit: unit.into(),
        conversion: Some(conversion),
        precision: Some(100.0),
        fraction: None,
        fixed: None,
        thousands: None,
        decimal: None,
        prefix_spacing: None,
        suffix_spacing: None,
        label_position: None,
    };
    PdfMeasurement {
        subtype: "RL".into(),
        ratio: Some("1 in = 1 m".into()),
        x: Some(vec![format("m", 1.0 / 72.0)]),
        y: None,
        distance: Some(vec![format("m", 1.0)]),
        area: Some(vec![format("m²", 1.0)]),
        angle: None,
        slope: None,
        origin: None,
        cyx: None,
    }
}

/// Two pages, an empty annotation list, and, when `service` is set, a
/// `page.measure` service that keeps viewports per page in memory and
/// fails writes to `fail_pon`.
fn spec(service: bool, foreign: bool, fail_pon: Option<u32>) -> TestSpec {
    let viewports: std::rc::Rc<std::cell::RefCell<std::collections::HashMap<u32, Value>>> =
        Default::default();
    if foreign {
        viewports.borrow_mut().insert(1, json!([{
            "bbox": { "left": 0, "bottom": 0, "right": 600, "top": 800 },
            "name": "Acrobat", "owned": false,
            "measure": serde_json::to_value(epdf_annotation_core::measure::PdfMeasurement::from(metric())).unwrap()
        }]));
    }
    let mut engine = json!({
        "documents": { "a": {
            "pages": [page(1, 600.0, 800.0), page(2, 400.0, 500.0)],
            "security": { "allows": "*" },
            "responses": [{ "path": "annotations.listRawAll", "value": { "pages": [] } }]
        } }
    });
    if service {
        engine["documents"]["a"]["services"] = json!(["page.measure"]);
    }
    let read = viewports.clone();
    let write = viewports.clone();
    TestSpec::new(engine)
        .respond_page("page.measure.viewports", move |pon, _| {
            Ok(Reply::value(read.borrow().get(&pon).cloned().unwrap_or(json!([]))))
        })
        .respond_page("page.measure.setScale", move |pon, args| {
            if Some(pon) == fail_pon {
                return Err(epdf_kernel::Error::engine("Unknown", "Unknown: the write failed"));
            }
            let mut all = write.borrow_mut();
            let mut list: Vec<Value> = all
                .get(&pon)
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|v| v["owned"] != json!(true))
                .collect();
            if let Some(measure) = args.first().filter(|m| !m.is_null()) {
                list.push(json!({
                    "bbox": { "left": 0, "bottom": 0, "right": 600, "top": 800 },
                    "name": "EmbedPDF", "owned": true, "measure": measure
                }));
            }
            all.insert(pon, Value::Array(list));
            Ok(Reply::value(json!({})))
        })
}

fn handle_over(
    spec: impl FnOnce() -> TestSpec + Send + 'static,
) -> (KernelThread<TestKernel>, Arc<MeasurementHandle>) {
    let thread = KernelThread::spawn(|_| {
        TestKernel::open(
            vec![
                interaction_plugin(InteractionConfig::default()),
                annotation_plugin(AnnotationConfig::default()),
                measurement_plugin(MeasurementConfig::default()),
            ],
            spec(),
        )
    })
    .unwrap();
    let post = thread.post();
    let handle = block_on(
        thread.call(move |k| MeasurementHandle::new(k.capability(&MEASUREMENT).unwrap(), post)),
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

#[test]
fn a_page_reads_loading_until_ensure_loaded() {
    let (_thread, m) = handle_over(|| spec(true, false, None));
    let before = block_on(m.page_scale(P2)).unwrap();
    assert!(!before.ready);
    block_on(m.ensure_loaded(P2, token())).unwrap();
    let after = block_on(m.page_scale(P2)).unwrap();
    assert!(after.ready);
    assert_eq!(after.source, MeasurementScaleSource::Default);
    assert!(after.persistent);
}

#[test]
fn set_scale_writes_one_page_and_reports_it() {
    let (_thread, m) = handle_over(|| spec(true, false, None));
    let events = Arc::new(Events::default());
    let _sub = m.subscribe(events.clone());
    let reports = block_on(m.set_scale(
        MeasurementPageTarget::Page { page: P1 },
        metric(),
        ScaleChangeOptions::default(),
        token(),
    ))
    .unwrap();
    assert_eq!(reports.len(), 1);
    assert!(matches!(&reports[0], ScaleChangeReport::Recalibrated { report } if report.page == P1));
    let scale = block_on(m.page_scale(P1)).unwrap();
    assert_eq!(scale.source, MeasurementScaleSource::Owned);
    assert_eq!(scale.measure.unwrap().ratio.as_deref(), Some("1 in = 1 m"));
    let seen = events.0.lock().unwrap();
    assert!(
        seen.iter().any(|e| matches!(e, MeasurementEvent::ScaleChanged { page, .. } if *page == P1)),
        "{seen:?}"
    );
}

#[test]
fn set_scale_keeps_a_foreign_viewport() {
    let (_thread, m) = handle_over(|| spec(true, true, None));
    block_on(m.ensure_loaded(P1, token())).unwrap();
    assert_eq!(block_on(m.page_scale(P1)).unwrap().source, MeasurementScaleSource::Foreign);
    block_on(m.set_scale(
        MeasurementPageTarget::Page { page: P1 },
        metric(),
        ScaleChangeOptions::default(),
        token(),
    ))
    .unwrap();
    // Last containing viewport wins, so the scale is ours; the foreign one is
    // still in the list the service holds.
    assert_eq!(block_on(m.page_scale(P1)).unwrap().source, MeasurementScaleSource::Owned);
    block_on(m.clear_scale(MeasurementPageTarget::Page { page: P1 }, ScaleChangeOptions::default(), token()))
        .unwrap();
    assert_eq!(block_on(m.page_scale(P1)).unwrap().source, MeasurementScaleSource::Foreign);
}

#[test]
fn a_failed_page_write_is_reported_and_the_rest_land() {
    let (_thread, m) = handle_over(|| spec(true, false, Some(2)));
    let reports = block_on(m.set_scale(
        MeasurementPageTarget::All,
        metric(),
        ScaleChangeOptions::default(),
        token(),
    ))
    .unwrap();
    assert_eq!(reports.len(), 2);
    assert!(reports.iter().any(|r| matches!(r, ScaleChangeReport::Recalibrated { report } if report.page == P1)));
    assert!(reports.iter().any(|r| matches!(r, ScaleChangeReport::ScaleFailed { page, error } if *page == P2 && error.code == "Unknown")));
    assert_eq!(block_on(m.page_scale(P1)).unwrap().source, MeasurementScaleSource::Owned);
}

#[test]
fn measure_distance_reads_in_the_page_scale() {
    let (_thread, m) = handle_over(|| spec(true, false, None));
    block_on(m.set_scale(MeasurementPageTarget::Page { page: P1 }, metric(), ScaleChangeOptions::default(), token()))
        .unwrap();
    let reading = block_on(m.measure_distance(P1, Point { x: 0.0, y: 0.0 }, Point { x: 72.0, y: 0.0 })).unwrap();
    match reading {
        MeasurementReading::Value { readout } => {
            assert!((readout.value - 1.0).abs() < 1e-9, "{readout:?}");
            assert_eq!(readout.label, "1 m");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn calibrating_a_zero_length_is_refused() {
    let (_thread, m) = handle_over(|| spec(true, false, None));
    let input = MeasurementCalibrateInput {
        page: P1,
        from: Point { x: 10.0, y: 10.0 },
        to: Point { x: 10.0, y: 10.0 },
        value: 1.0,
        unit: "m".into(),
    };
    let error = block_on(m.calibrate(input, MeasurementCalibrateOptions::default(), token())).unwrap_err();
    assert_eq!(code(&error), Some("invalid-input"), "{error:?}");
}

#[test]
fn calibrating_writes_the_scale_and_completes() {
    let (_thread, m) = handle_over(|| spec(true, false, None));
    let events = Arc::new(Events::default());
    let _sub = m.subscribe(events.clone());
    let input = MeasurementCalibrateInput {
        page: P1,
        from: Point { x: 0.0, y: 0.0 },
        to: Point { x: 144.0, y: 0.0 },
        value: 4.0,
        unit: "m".into(),
    };
    block_on(m.calibrate(input, MeasurementCalibrateOptions::default(), token())).unwrap();
    let reading = block_on(m.measure_distance(P1, Point { x: 0.0, y: 0.0 }, Point { x: 72.0, y: 0.0 })).unwrap();
    assert!(matches!(reading, MeasurementReading::Value { readout } if (readout.value - 2.0).abs() < 1e-6));
    assert!(events.0.lock().unwrap().iter().any(|e| matches!(e, MeasurementEvent::CalibrationCompleted { page } if *page == P1)));
}

#[test]
fn without_a_service_the_scale_is_in_memory() {
    let (_thread, m) = handle_over(|| spec(false, false, None));
    block_on(m.set_scale(MeasurementPageTarget::Page { page: P1 }, metric(), ScaleChangeOptions::default(), token()))
        .unwrap();
    let scale = block_on(m.page_scale(P1)).unwrap();
    assert_eq!(scale.source, MeasurementScaleSource::Owned);
    assert!(!scale.persistent);
}

#[test]
fn units_and_presets_are_listed() {
    let (_thread, m) = handle_over(|| spec(true, false, None));
    assert!(block_on(m.units()).unwrap().contains(&"m".to_string()));
    assert!(block_on(m.area_units()).unwrap().contains(&"m²".to_string()));
    assert!(!block_on(m.presets()).unwrap().is_empty());
}

#[test]
fn cancelling_a_scale_write_before_it_runs_rejects() {
    let (thread, m) = handle_over(|| spec(true, false, None));
    let (release, gate) = std::sync::mpsc::channel::<()>();
    let (parked_tx, parked) = std::sync::mpsc::channel::<()>();
    let blocker = thread.call(move |_| {
        parked_tx.send(()).unwrap();
        gate.recv().unwrap();
    });
    parked.recv().unwrap();
    let cancel = token();
    let write = m.set_scale(MeasurementPageTarget::Page { page: P1 }, metric(), ScaleChangeOptions::default(), cancel.clone());
    cancel.cancel();
    release.send(()).unwrap();
    block_on(blocker).unwrap();
    let error = block_on(write).unwrap_err();
    assert_eq!(code(&error), Some("operation-cancelled"), "{error:?}");
}

#[test]
fn a_closed_documents_capability_refuses_with_instance_closed() {
    let (thread, m) = handle_over(|| spec(true, false, None));
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    for error in [
        block_on(m.page_scale(P1)).map(|_| ()).unwrap_err(),
        block_on(m.is_busy()).map(|_| ()).unwrap_err(),
        block_on(m.ensure_loaded(P1, token())).unwrap_err(),
    ] {
        assert_eq!(code(&error), Some("instance-closed"), "{error:?}");
    }
}

#[test]
fn closing_the_document_ends_its_subscriptions() {
    let (thread, m) = handle_over(|| spec(true, false, None));
    let events = Arc::new(Events::default());
    let _sub = m.subscribe(events.clone());
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    assert_eq!(events.1.load(Ordering::SeqCst), 1);
}
```

Add `epdf-annotation-core`, `epdf-plugin-annotation` and `epdf-plugin-interaction` to `[dev-dependencies]` with `default-features = false` if the test can't see them through the normal dependencies. Add `epdf-kernel` too, for `Error::engine`.

- [ ] **Step 3: Run them to verify they fail**

Run: `cargo test -p epdf-plugin-measurement --features uniffi --test ffi`
Expected: FAIL to compile, `could not find ffi in epdf_plugin_measurement`.

- [ ] **Step 4: Write `crates/plugins/measurement/src/ffi.rs`**

```rust
//! The measurement plugin through uniffi: typed records, one handle, one
//! event enum. Every method hops to the kernel thread through
//! `KernelBound`.
//!
//! The plugin keeps a measure and engine errors as JSON (`PageScale.error`,
//! `RecalibrationReport.scale`), so the records here decode them. A value
//! that does not decode is an `invalid-state` error: the record and the
//! engine disagree, and a default would hide it.

use std::sync::Arc;

use epdf_annotation_core::measure as core;
use epdf_ffi::{
    AnnotationRef, CancelToken, KernelBound, KernelPost, PageRef, Point, Subscription, ViewerError,
};
use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error};
use serde_json::Value;

use crate::{
    CalibrateInput, CalibrateOptions, CreateMeasurementInput as CoreCreate, DefaultScale,
    KnownDistance, MeasurementCapability, PageTarget, ScaleChangeOptions, ScalePreset,
};

// ── measures ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum MeasureFraction {
    Decimal,
    Fraction,
    Round,
    Truncate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum MeasureLabelPosition {
    Suffix,
    Prefix,
}

/// One ISO 32000 number format (`/NumberFormat`).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PdfNumberFormat {
    pub unit: String,
    #[uniffi(default = None)]
    pub conversion: Option<f64>,
    #[uniffi(default = None)]
    pub fraction: Option<MeasureFraction>,
    #[uniffi(default = None)]
    pub precision: Option<f64>,
    #[uniffi(default = None)]
    pub fixed: Option<bool>,
    #[uniffi(default = None)]
    pub thousands: Option<String>,
    #[uniffi(default = None)]
    pub decimal: Option<String>,
    #[uniffi(default = None)]
    pub prefix_spacing: Option<String>,
    #[uniffi(default = None)]
    pub suffix_spacing: Option<String>,
    #[uniffi(default = None)]
    pub label_position: Option<MeasureLabelPosition>,
}

/// A `/Measure` dictionary. `subtype` is `RL` for a rectilinear measure
/// (the only kind EmbedPDF writes); any other subtype is foreign and has
/// no format lists.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PdfMeasurement {
    pub subtype: String,
    #[uniffi(default = None)]
    pub ratio: Option<String>,
    #[uniffi(default = None)]
    pub x: Option<Vec<PdfNumberFormat>>,
    #[uniffi(default = None)]
    pub y: Option<Vec<PdfNumberFormat>>,
    #[uniffi(default = None)]
    pub distance: Option<Vec<PdfNumberFormat>>,
    #[uniffi(default = None)]
    pub area: Option<Vec<PdfNumberFormat>>,
    #[uniffi(default = None)]
    pub angle: Option<Vec<PdfNumberFormat>>,
    #[uniffi(default = None)]
    pub slope: Option<Vec<PdfNumberFormat>>,
    #[uniffi(default = None)]
    pub origin: Option<Point>,
    #[uniffi(default = None)]
    pub cyx: Option<f64>,
}

impl From<core::MeasureFraction> for MeasureFraction {
    fn from(f: core::MeasureFraction) -> Self {
        match f {
            core::MeasureFraction::Decimal => MeasureFraction::Decimal,
            core::MeasureFraction::Fraction => MeasureFraction::Fraction,
            core::MeasureFraction::Round => MeasureFraction::Round,
            core::MeasureFraction::Truncate => MeasureFraction::Truncate,
        }
    }
}

impl From<MeasureFraction> for core::MeasureFraction {
    fn from(f: MeasureFraction) -> Self {
        match f {
            MeasureFraction::Decimal => core::MeasureFraction::Decimal,
            MeasureFraction::Fraction => core::MeasureFraction::Fraction,
            MeasureFraction::Round => core::MeasureFraction::Round,
            MeasureFraction::Truncate => core::MeasureFraction::Truncate,
        }
    }
}

impl From<core::LabelPosition> for MeasureLabelPosition {
    fn from(p: core::LabelPosition) -> Self {
        match p {
            core::LabelPosition::Suffix => MeasureLabelPosition::Suffix,
            core::LabelPosition::Prefix => MeasureLabelPosition::Prefix,
        }
    }
}

impl From<MeasureLabelPosition> for core::LabelPosition {
    fn from(p: MeasureLabelPosition) -> Self {
        match p {
            MeasureLabelPosition::Suffix => core::LabelPosition::Suffix,
            MeasureLabelPosition::Prefix => core::LabelPosition::Prefix,
        }
    }
}

impl From<core::PdfNumberFormat> for PdfNumberFormat {
    fn from(f: core::PdfNumberFormat) -> Self {
        PdfNumberFormat {
            unit: f.unit,
            conversion: f.conversion,
            fraction: f.fraction.map(Into::into),
            precision: f.precision,
            fixed: f.fixed,
            thousands: f.thousands,
            decimal: f.decimal,
            prefix_spacing: f.prefix_spacing,
            suffix_spacing: f.suffix_spacing,
            label_position: f.label_position.map(Into::into),
        }
    }
}

impl From<PdfNumberFormat> for core::PdfNumberFormat {
    fn from(f: PdfNumberFormat) -> Self {
        core::PdfNumberFormat {
            unit: f.unit,
            conversion: f.conversion,
            fraction: f.fraction.map(Into::into),
            precision: f.precision,
            fixed: f.fixed,
            thousands: f.thousands,
            decimal: f.decimal,
            prefix_spacing: f.prefix_spacing,
            suffix_spacing: f.suffix_spacing,
            label_position: f.label_position.map(Into::into),
        }
    }
}

fn formats_in(list: Option<Vec<core::PdfNumberFormat>>) -> Option<Vec<PdfNumberFormat>> {
    list.map(|l| l.into_iter().map(Into::into).collect())
}

fn formats_out(list: Option<Vec<PdfNumberFormat>>) -> Option<Vec<core::PdfNumberFormat>> {
    list.map(|l| l.into_iter().map(Into::into).collect())
}

impl From<core::PdfMeasurement> for PdfMeasurement {
    fn from(m: core::PdfMeasurement) -> Self {
        PdfMeasurement {
            subtype: m.subtype,
            ratio: m.ratio,
            x: formats_in(m.x),
            y: formats_in(m.y),
            distance: formats_in(m.distance),
            area: formats_in(m.area),
            angle: formats_in(m.angle),
            slope: formats_in(m.slope),
            origin: m.origin.map(|p| Point { x: p.x, y: p.y }),
            cyx: m.cyx,
        }
    }
}

impl From<PdfMeasurement> for core::PdfMeasurement {
    fn from(m: PdfMeasurement) -> Self {
        core::PdfMeasurement {
            subtype: m.subtype,
            ratio: m.ratio,
            x: formats_out(m.x),
            y: formats_out(m.y),
            distance: formats_out(m.distance),
            area: formats_out(m.area),
            angle: formats_out(m.angle),
            slope: formats_out(m.slope),
            origin: m.origin.map(|p| epdf_annotation_core::PdfPoint { x: p.x, y: p.y }),
            cyx: m.cyx,
        }
    }
}

fn invalid_state(what: &str, e: impl std::fmt::Display) -> Error {
    Error::named("InvalidState", format!("[measurement] {what} does not decode: {e}"))
        .with("code", "invalid-state")
}

fn decode_measure(value: &Value) -> Result<Option<PdfMeasurement>, Error> {
    if value.is_null() {
        return Ok(None);
    }
    serde_json::from_value::<core::PdfMeasurement>(value.clone())
        .map(|m| Some(m.into()))
        .map_err(|e| invalid_state("a measure", e))
}

// ── config ──────────────────────────────────────────────────────────────────

/// The fallback scale for a page with no viewport.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum MeasurementDefaultScale {
    Metric,
    Imperial,
    Measure { measure: PdfMeasurement },
}

#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct MeasurementConfig {
    /// `None` is metric.
    #[uniffi(default = None)]
    pub default_scale: Option<MeasurementDefaultScale>,
    /// `None` is the default picker: metric 1:1 to 1:200, imperial 1/4 and
    /// 1/8 in.
    #[uniffi(default = None)]
    pub presets: Option<Vec<ScalePreset>>,
}

impl From<MeasurementConfig> for crate::MeasurementConfig {
    fn from(config: MeasurementConfig) -> Self {
        crate::MeasurementConfig {
            default_scale: config.default_scale.map(|s| match s {
                MeasurementDefaultScale::Metric => DefaultScale::Metric,
                MeasurementDefaultScale::Imperial => DefaultScale::Imperial,
                MeasurementDefaultScale::Measure { measure } => {
                    DefaultScale::Measure(Box::new(measure.into()))
                }
            }),
            presets: config.presets,
            host: crate::MeasurementHost::default(),
        }
    }
}

// ── targets, scales, readouts ───────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum MeasurementPageTarget {
    Page { page: PageRef },
    Pages { pages: Vec<PageRef> },
    All,
}

fn core_page(page: PageRef) -> epdf_annotation_core::PageRef {
    epdf_annotation_core::PageRef::new(page.page_object_number)
}

fn ffi_page(page: epdf_annotation_core::PageRef) -> PageRef {
    PageRef { page_object_number: page.page_object_number }
}

fn core_point(p: Point) -> epdf_annotation_core::Point {
    epdf_annotation_core::Point { x: p.x, y: p.y }
}

impl From<MeasurementPageTarget> for PageTarget {
    fn from(t: MeasurementPageTarget) -> Self {
        match t {
            MeasurementPageTarget::Page { page } => PageTarget::Page(core_page(page)),
            MeasurementPageTarget::Pages { pages } => {
                PageTarget::Pages(pages.into_iter().map(core_page).collect())
            }
            MeasurementPageTarget::All => PageTarget::All,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum MeasurementScaleSource {
    /// EmbedPDF's own viewport.
    Owned,
    /// Another producer's viewport.
    Foreign,
    /// No viewport: the configured default.
    Default,
}

/// An engine error as the plugin keeps it (`SerializedEngineError`).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct MeasurementErrorInfo {
    pub code: String,
    pub detail: String,
}

fn decode_error(value: &Value) -> Result<MeasurementErrorInfo, Error> {
    let code = value.get("code").and_then(Value::as_str);
    let message = value.get("message").and_then(Value::as_str);
    match (code, message) {
        (Some(code), Some(message)) => {
            Ok(MeasurementErrorInfo { code: code.into(), detail: message.into() })
        }
        _ => Err(invalid_state("an engine error", value)),
    }
}

/// A page's resolved scale.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct MeasurementPageScale {
    pub measure: Option<PdfMeasurement>,
    pub source: MeasurementScaleSource,
    /// The page's viewports are known.
    pub ready: bool,
    /// The engine can persist a scale into the document.
    pub persistent: bool,
    pub error: Option<MeasurementErrorInfo>,
}

impl TryFrom<crate::PageScale> for MeasurementPageScale {
    type Error = Error;

    fn try_from(s: crate::PageScale) -> Result<Self, Error> {
        Ok(MeasurementPageScale {
            measure: s.measure.map(Into::into),
            source: match s.source {
                crate::ScaleSource::Owned => MeasurementScaleSource::Owned,
                crate::ScaleSource::Foreign => MeasurementScaleSource::Foreign,
                crate::ScaleSource::Default => MeasurementScaleSource::Default,
            },
            ready: s.ready,
            persistent: s.persistent,
            error: s.error.as_ref().map(decode_error).transpose()?,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum MeasurementReadoutKind {
    Distance,
    Perimeter,
    Area,
}

/// A value in the scale's first format unit, and its label.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct MeasurementReadout {
    pub kind: MeasurementReadoutKind,
    pub label: String,
    pub value: f64,
    pub perimeter: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum MeasurementUnavailable {
    NotDimension,
    NoMeasure,
    ForeignMeasure,
    NoScale,
    NoDistanceFormat,
    NoAreaFormat,
    InvalidFormat,
    InvalidGeometry,
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum MeasurementReading {
    Value { readout: MeasurementReadout },
    Unavailable { reason: MeasurementUnavailable },
}

impl From<core::Readout> for MeasurementReading {
    fn from(r: core::Readout) -> Self {
        match r {
            core::Readout::Readout(r) => MeasurementReading::Value {
                readout: MeasurementReadout {
                    kind: match r.kind {
                        core::ReadoutKind::Distance => MeasurementReadoutKind::Distance,
                        core::ReadoutKind::Perimeter => MeasurementReadoutKind::Perimeter,
                        core::ReadoutKind::Area => MeasurementReadoutKind::Area,
                    },
                    label: r.label,
                    value: r.value,
                    perimeter: r.perimeter,
                },
            },
            core::Readout::Unavailable { unavailable } => MeasurementReading::Unavailable {
                reason: match unavailable {
                    core::Unavailable::NotDimension => MeasurementUnavailable::NotDimension,
                    core::Unavailable::NoMeasure => MeasurementUnavailable::NoMeasure,
                    core::Unavailable::ForeignMeasure => MeasurementUnavailable::ForeignMeasure,
                    core::Unavailable::NoScale => MeasurementUnavailable::NoScale,
                    core::Unavailable::NoDistanceFormat => MeasurementUnavailable::NoDistanceFormat,
                    core::Unavailable::NoAreaFormat => MeasurementUnavailable::NoAreaFormat,
                    core::Unavailable::InvalidFormat => MeasurementUnavailable::InvalidFormat,
                    core::Unavailable::InvalidGeometry => MeasurementUnavailable::InvalidGeometry,
                },
            },
        }
    }
}

// ── calibration and creation ────────────────────────────────────────────────

/// Two points the calibrate tool captured, awaiting the real length. Page
/// space.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct MeasurementCalibrationRequest {
    pub page: PageRef,
    pub from: Point,
    pub to: Point,
    /// The captured length in PDF user space.
    pub user_space_length: f64,
}

impl From<crate::CalibrationRequest> for MeasurementCalibrationRequest {
    fn from(r: crate::CalibrationRequest) -> Self {
        MeasurementCalibrationRequest {
            page: ffi_page(r.page),
            from: Point { x: r.from.x, y: r.from.y },
            to: Point { x: r.to.x, y: r.to.y },
            user_space_length: r.user_space_length,
        }
    }
}

/// Two page points and the real length between them, `value` in `unit`.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct MeasurementCalibrateInput {
    pub page: PageRef,
    pub from: Point,
    pub to: Point,
    pub value: f64,
    pub unit: String,
}

#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct MeasurementCalibrateOptions {
    /// Re-measure the pages' measurement annotations. `None` is true.
    #[uniffi(default = None)]
    pub recalculate: Option<bool>,
    /// Write the scale to these pages. `None` is the input's page.
    #[uniffi(default = None)]
    pub apply_to: Option<MeasurementPageTarget>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum MeasurementKind {
    Distance,
    Perimeter,
    Area,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct CreateMeasurementInput {
    pub kind: MeasurementKind,
    pub page: PageRef,
    /// Page-space points: two for a distance, the vertices otherwise.
    pub points: Vec<Point>,
    /// The authoring tool whose defaults apply. `None` is the kind's own.
    #[uniffi(default = None)]
    pub tool: Option<String>,
}

// ── reports ─────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ScaleSkipReason {
    NoAuthority,
    Locked,
    ForeignMeasure,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ScaleSkippedAnnotation {
    pub annotation: AnnotationRef,
    pub reason: ScaleSkipReason,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ScaleFailedAnnotation {
    pub annotation: AnnotationRef,
    pub error: MeasurementErrorInfo,
}

/// One page's re-measurement after a scale change.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ScaleRecalibration {
    pub page: PageRef,
    pub scale: Option<PdfMeasurement>,
    pub error: Option<MeasurementErrorInfo>,
    pub updated: Vec<AnnotationRef>,
    pub skipped: Vec<ScaleSkippedAnnotation>,
    pub failed: Vec<ScaleFailedAnnotation>,
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum ScaleChangeReport {
    Recalibrated { report: ScaleRecalibration },
    /// The page's viewport write failed; nothing on it was re-measured.
    ScaleFailed { page: PageRef, error: MeasurementErrorInfo },
}

impl TryFrom<crate::ScaleChangeReport> for ScaleChangeReport {
    type Error = Error;

    fn try_from(r: crate::ScaleChangeReport) -> Result<Self, Error> {
        use epdf_plugin_annotation::SkipReason;
        Ok(match r {
            crate::ScaleChangeReport::ScaleError { page, scale_error } => {
                ScaleChangeReport::ScaleFailed { page: ffi_page(page), error: decode_error(&scale_error)? }
            }
            crate::ScaleChangeReport::Recalibrated(r) => ScaleChangeReport::Recalibrated {
                report: ScaleRecalibration {
                    page: ffi_page(r.page),
                    scale: decode_measure(&r.scale)?,
                    error: r.error.as_ref().map(decode_error).transpose()?,
                    updated: r.updated.into_iter().map(Into::into).collect(),
                    skipped: r
                        .skipped
                        .into_iter()
                        .map(|(annotation, reason)| ScaleSkippedAnnotation {
                            annotation: annotation.into(),
                            reason: match reason {
                                SkipReason::NoAuthority => ScaleSkipReason::NoAuthority,
                                SkipReason::Locked => ScaleSkipReason::Locked,
                                SkipReason::ForeignMeasure => ScaleSkipReason::ForeignMeasure,
                                SkipReason::Unavailable => ScaleSkipReason::Unavailable,
                            },
                        })
                        .collect(),
                    failed: r
                        .failed
                        .into_iter()
                        .map(|(annotation, error)| {
                            Ok(ScaleFailedAnnotation {
                                annotation: annotation.into(),
                                error: decode_error(&error)?,
                            })
                        })
                        .collect::<Result<_, Error>>()?,
                },
            },
        })
    }
}

fn reports(list: Vec<crate::ScaleChangeReport>) -> Result<Vec<ScaleChangeReport>, ViewerError> {
    list.into_iter()
        .map(ScaleChangeReport::try_from)
        .collect::<Result<_, _>>()
        .map_err(Into::into)
}

// ── events ──────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum MeasurementEvent {
    ScaleChanged { page: PageRef, report: ScaleChangeReport },
    CalibrationRequested { request: MeasurementCalibrationRequest },
    CalibrationCompleted { page: PageRef },
    CalibrationDismissed,
}

#[uniffi::export(with_foreign)]
pub trait MeasurementEventListener: Send + Sync {
    fn on_event(&self, event: MeasurementEvent);
    /// The document closed: no event follows. Called once, and at once for a
    /// subscription made after the close.
    fn on_end(&self);
}

// ── the handle ──────────────────────────────────────────────────────────────

/// The capability, if its document is still open.
fn open(cap: &MeasurementCapability) -> Result<&MeasurementCapability, Error> {
    if cap.lifetime().aborted() {
        return Err(plugin_error(
            code::INSTANCE_CLOSED,
            "measurement",
            "the document this capability belongs to is closed",
            None,
            None,
        ));
    }
    Ok(cap)
}

fn opts(o: ScaleChangeOptions) -> ScaleChangeOptions {
    o
}

#[derive(uniffi::Object)]
pub struct MeasurementHandle {
    cap: KernelBound<MeasurementCapability>,
    post: KernelPost,
}

impl MeasurementHandle {
    /// On the kernel thread.
    pub fn new(cap: MeasurementCapability, post: KernelPost) -> Arc<Self> {
        Arc::new(MeasurementHandle { cap: KernelBound::new(cap, post.clone()), post })
    }
}

#[uniffi::export]
impl MeasurementHandle {
    /// Whether this caller may measure `page` (`doc.annotations.read`).
    pub async fn can_measure(&self, page: PageRef) -> Result<bool, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.can_measure(&core_page(page)))).await
    }

    /// `ready` is false until the page's viewports load (`ensure_loaded`).
    pub async fn page_scale(&self, page: PageRef) -> Result<MeasurementPageScale, ViewerError> {
        self.cap.call(move |c| open(c)?.get_page_scale(&core_page(page)).try_into()).await
    }

    pub async fn is_busy(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.is_busy())).await
    }

    /// The reports of the last scale change.
    pub async fn last_reports(&self) -> Result<Vec<ScaleChangeReport>, ViewerError> {
        self.cap
            .call(|c| {
                open(c)?
                    .list_last_reports()
                    .into_iter()
                    .map(ScaleChangeReport::try_from)
                    .collect()
            })
            .await
    }

    pub async fn presets(&self) -> Result<Vec<ScalePreset>, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.list_presets())).await
    }

    pub async fn units(&self) -> Result<Vec<String>, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.list_units().into_iter().map(String::from).collect())).await
    }

    pub async fn area_units(&self) -> Result<Vec<String>, ViewerError> {
        self.cap
            .call(|c| Ok(open(c)?.list_area_units().into_iter().map(String::from).collect()))
            .await
    }

    /// A measurement annotation's value in its own scale.
    pub async fn readout(&self, annotation: AnnotationRef) -> Result<MeasurementReading, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.get_readout(&annotation.into()).into())).await
    }

    /// A distance in the page's scale, with no annotation. `not-found` for
    /// a page not in the document.
    pub async fn measure_distance(
        &self,
        page: PageRef,
        from: Point,
        to: Point,
    ) -> Result<MeasurementReading, ViewerError> {
        self.cap
            .call(move |c| {
                Ok(open(c)?
                    .measure_distance(&core_page(page), core_point(from), core_point(to))?
                    .into())
            })
            .await
    }

    pub async fn measure_area(
        &self,
        page: PageRef,
        vertices: Vec<Point>,
    ) -> Result<MeasurementReading, ViewerError> {
        self.cap
            .call(move |c| {
                let vertices: Vec<_> = vertices.into_iter().map(core_point).collect();
                Ok(open(c)?.measure_area(&core_page(page), &vertices)?.into())
            })
            .await
    }

    pub async fn calibration_request(
        &self,
    ) -> Result<Option<MeasurementCalibrationRequest>, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.get_calibration_request().map(Into::into))).await
    }

    /// Write `measure` to `pages` and re-measure their dimensions. One
    /// report per page, in page order. Cancelling stops waiting; a page
    /// write already sent still lands.
    pub async fn set_scale(
        &self,
        pages: MeasurementPageTarget,
        measure: PdfMeasurement,
        options: ScaleChangeOptions,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<ScaleChangeReport>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let measure = serde_json::to_value(core::PdfMeasurement::from(measure))
                        .map_err(|e| Error::new(e.to_string()))?;
                    Ok(open(c)?.set_scale(pages.into(), measure, opts(options)))
                },
                reports,
            )
            .await?
    }

    /// Derive a scale from a known length and write it.
    pub async fn calibrate(
        &self,
        input: MeasurementCalibrateInput,
        options: MeasurementCalibrateOptions,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<ScaleChangeReport>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let input = CalibrateInput {
                        page: core_page(input.page),
                        from: core_point(input.from),
                        to: core_point(input.to),
                        distance: KnownDistance { value: input.value, unit: input.unit },
                    };
                    let options = CalibrateOptions {
                        recalculate: options.recalculate,
                        apply_to: options.apply_to.map(Into::into),
                    };
                    Ok(open(c)?.calibrate(input, options))
                },
                reports,
            )
            .await?
    }

    pub async fn set_unit(
        &self,
        pages: MeasurementPageTarget,
        unit: String,
        options: ScaleChangeOptions,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<ScaleChangeReport>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.set_unit(pages.into(), &unit, options)),
                reports,
            )
            .await?
    }

    pub async fn set_area_unit(
        &self,
        pages: MeasurementPageTarget,
        unit: String,
        options: ScaleChangeOptions,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<ScaleChangeReport>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.set_area_unit(pages.into(), &unit, options)),
                reports,
            )
            .await?
    }

    pub async fn set_precision(
        &self,
        pages: MeasurementPageTarget,
        precision: f64,
        options: ScaleChangeOptions,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<ScaleChangeReport>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.set_precision(pages.into(), precision, options)),
                reports,
            )
            .await?
    }

    /// `not-found` for a preset id `presets()` doesn't list.
    pub async fn set_preset(
        &self,
        pages: MeasurementPageTarget,
        preset_id: String,
        options: ScaleChangeOptions,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<ScaleChangeReport>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.set_preset(pages.into(), &preset_id, options)),
                reports,
            )
            .await?
    }

    /// Back to the default scale: EmbedPDF's viewport is removed, another
    /// producer's stays.
    pub async fn clear_scale(
        &self,
        pages: MeasurementPageTarget,
        options: ScaleChangeOptions,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<ScaleChangeReport>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.clear_scale(pages.into(), options)),
                reports,
            )
            .await?
    }

    /// Arm the calibrate tool; its two-point capture raises
    /// `CalibrationRequested`.
    pub async fn start_calibration(&self) -> Result<(), ViewerError> {
        self.cap.call(|c| open(c)?.start_calibration()).await
    }

    pub async fn dismiss_calibration(&self) -> Result<(), ViewerError> {
        self.cap.call(|c| {
            open(c)?.dismiss_calibration();
            Ok(())
        })
        .await
    }

    /// Create a measurement annotation through the annotation plugin.
    pub async fn create_measurement(
        &self,
        input: CreateMeasurementInput,
        cancel: Arc<CancelToken>,
    ) -> Result<AnnotationRef, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let input = CoreCreate {
                        kind: match input.kind {
                            MeasurementKind::Distance => crate::MeasurementKind::Distance,
                            MeasurementKind::Perimeter => crate::MeasurementKind::Perimeter,
                            MeasurementKind::Area => crate::MeasurementKind::Area,
                        },
                        page: core_page(input.page),
                        points: input.points.into_iter().map(core_point).collect(),
                        tool: input.tool,
                    };
                    Ok(open(c)?.create_measurement(input))
                },
                AnnotationRef::from,
            )
            .await
    }

    pub async fn can_calibrate(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.can_calibrate())).await
    }

    /// Load the page's viewports. Shares a read in flight.
    pub async fn ensure_loaded(
        &self,
        page: PageRef,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| open(c)?.ensure_loaded(&core_page(page)),
                |()| (),
            )
            .await
    }

    /// Every hook in one registration. Gets `on_end` when the document
    /// closes (at once if it already has). An event that does not decode
    /// is reported through the kernel's report sink, not delivered.
    pub fn subscribe(&self, listener: Arc<dyn MeasurementEventListener>) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.cap.fire(move |c| {
            let ending = listener.clone();
            slot.fill_until_end(
                c.lifetime(),
                || {
                    let l = listener.clone();
                    let scale = c.on_scale_changed(move |e| {
                        let report = ScaleChangeReport::try_from(e.report.clone())?;
                        l.on_event(MeasurementEvent::ScaleChanged { page: ffi_page(e.page), report });
                        Ok(())
                    });
                    let l = listener.clone();
                    let requested = c.on_calibration_requested(move |e| {
                        l.on_event(MeasurementEvent::CalibrationRequested {
                            request: e.request.clone().into(),
                        });
                        Ok(())
                    });
                    let l = listener.clone();
                    let completed = c.on_calibration_completed(move |e| {
                        l.on_event(MeasurementEvent::CalibrationCompleted { page: ffi_page(e.page) });
                        Ok(())
                    });
                    let l = listener;
                    let dismissed = c.on_calibration_dismissed(move |_| {
                        l.on_event(MeasurementEvent::CalibrationDismissed);
                        Ok(())
                    });
                    epdf_kernel::Unsubscribe::new(move || {
                        for off in [&scale, &requested, &completed, &dismissed] {
                            off.call();
                        }
                    })
                },
                move || ending.on_end(),
            );
        });
        subscription
    }
}
```

`create_measurement`'s promise resolves an `epdf_annotation_core::AnnotationRef`; if `MeasurementCapability::create_measurement` returns `Promise<epdf_plugin_annotation::AnnotationRef>` under another path, convert through whatever `From` plan 05 provides. Delete the `opts` identity helper if clippy flags it; it's there so the `set_scale` closure reads the same as its siblings.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p epdf-plugin-measurement --features uniffi --test ffi`
Expected: PASS, 12 tests. If `measure_distance_reads_in_the_page_scale` reports a label other than `"1 m"`, read it from `cargo test -p epdf-plugin-measurement` vector `units-precision-and-presets` and match that; the plugin, not this test, owns label formatting.

- [ ] **Step 6: Run the conformance vectors without `uniffi`**

Run: `cargo test -p epdf-plugin-measurement`
Expected: PASS. The `cfg_attr` derives don't change the default build.

- [ ] **Step 7: Commit**

```bash
git add crates/plugins/measurement
git commit -m "Export the measurement plugin through uniffi"
```

### Task 4: Engine paths and host wiring

**Files:**
- Modify: `crates/epdf-viewer-host/src/translate.rs`, `crates/epdf-viewer-host/tests/translate.rs`
- Modify: `crates/epdf-viewer-host/Cargo.toml`, `crates/epdf-viewer-host/src/api.rs`, `crates/epdf-viewer-host/src/session.rs`, `crates/epdf-viewer-host/src/lib.rs`, `crates/epdf-viewer-host/tests/viewer_host.rs`

**Interfaces:**
- Consumes: Task 2's wire kinds; Task 3's `MeasurementHandle`, `ffi::MeasurementConfig`.
- Produces: `ViewerConfig.measurement: Option<MeasurementConfig>`; `ViewerHost::measurement(document_id: String) -> Result<Arc<MeasurementHandle>, ViewerError>`.

- [ ] **Step 1: Write the failing translation tests**

Append to `crates/epdf-viewer-host/tests/translate.rs`:

```rust
#[test]
fn page_measure_paths_go_to_the_measure_kinds() {
    assert!(translate::maps("page.measure"));
    let read = translate::translate("doc-1", "page.measure.viewports", Some(7), &[], 3).unwrap();
    assert_eq!(read.request["kind"], json!("measure.viewports"));
    assert_eq!(read.request["pageObjectNumber"], json!(7));
    let measure = json!({ "subtype": "RL", "ratio": "1 in = 1 m" });
    let write = translate::translate("doc-1", "page.measure.setScale", Some(7), &[measure.clone()], 4).unwrap();
    assert_eq!(write.request["kind"], json!("measure.setScale"));
    assert_eq!(write.request["measure"], measure);
}

#[test]
fn clearing_a_scale_sends_an_explicit_null() {
    let write = translate::translate("doc-1", "page.measure.setScale", Some(7), &[Value::Null], 4).unwrap();
    assert!(write.request.as_object().unwrap().contains_key("measure"));
    assert_eq!(write.request["measure"], Value::Null);
}

#[test]
fn viewports_decode_to_the_list() {
    let response = HostResponse {
        json: r#"{"result":{"viewports":[{"bbox":{"left":0,"bottom":0,"right":1,"top":1},"owned":true}]}}"#.into(),
        payloads: Vec::new(),
    };
    let decoded = translate::decode(&translate::Decode::Field("viewports"), response).unwrap();
    assert_eq!(decoded.value.as_array().unwrap().len(), 1);
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p epdf-viewer-host --test translate`
Expected: FAIL, `maps("page.measure")` is false.

- [ ] **Step 3: Map the paths**

In `translate.rs`, `route`:

```rust
        "page.measure.viewports" => ("measure.viewports", Decode::Field("viewports")),
        "page.measure.setScale" => ("measure.setScale", Decode::Mutation),
```

Add both to `PATHS` (umbrella Task 0e), one line each in plan order:

```rust
    "page.measure.viewports",
    "page.measure.setScale",
```

`maps("page.measure")`, the namespace the plugin feature-detects (`doc.has("page.measure")`), then answers through the namespace rule.

In `translate`, the `extra` match:

```rust
        "measure.viewports" => vec![("pageObjectNumber", page()?)],
        "measure.setScale" => vec![("pageObjectNumber", page()?), ("measure", arg(0))],
```

and, because the request loop drops `null` values and a cleared scale is an explicit `null`, insert `measure` after the loop:

```rust
    if kind == "measure.setScale" {
        request.insert("measure".into(), arg(0));
    }
```

Update the Appendix C row in `docs/superpowers/plans/2026-10-01-swift-package.md` for `page.measure.setScale`, `page.measure.viewports` to `mapped (08-measurement.md)`.

- [ ] **Step 4: Run the translation tests**

Run: `cargo test -p epdf-viewer-host --test translate`
Expected: PASS.

- [ ] **Step 5: Write the failing host test**

Append to `crates/epdf-viewer-host/tests/viewer_host.rs`, following the file's existing `report.pdf` helpers:

```rust
#[test]
fn a_measurement_scale_persists_through_the_native_engine() {
    let host = ViewerHost::new(
        ViewerConfig {
            interaction: true,
            annotation: Some(Default::default()),
            measurement: Some(Default::default()),
            ..Default::default()
        },
        Arc::new(NullDelegate),
    )
    .unwrap();
    block_on(host.start()).unwrap();
    let doc = block_on(host.open_document(fixture("report"), OpenDocumentOptions::default())).unwrap();
    let page = block_on(host.pages(doc.id.clone())).unwrap()[0].page;
    let m = block_on(host.measurement(doc.id.clone())).unwrap();
    block_on(m.ensure_loaded(page, CancelToken::new())).unwrap();
    assert!(block_on(m.page_scale(page)).unwrap().persistent);
    let reports = block_on(m.set_preset(
        MeasurementPageTarget::Page { page },
        "metric-1-100".into(),
        Default::default(),
        CancelToken::new(),
    ))
    .unwrap();
    assert_eq!(reports.len(), 1);
    assert_eq!(block_on(m.page_scale(page)).unwrap().source, MeasurementScaleSource::Owned);
}
```

Use the first id `presets()` returns if `"metric-1-100"` isn't one (`crates/plugins/measurement/src/scale.rs:19` lists them). `NullDelegate` and `fixture` are the helpers the file already defines; if they're named differently, use those names.

- [ ] **Step 6: Run it to verify it fails**

Run: `cargo test -p epdf-viewer-host --test viewer_host a_measurement_scale`
Expected: FAIL to compile, `no field measurement on ViewerConfig`.

- [ ] **Step 7: Wire the host**

`crates/epdf-viewer-host/Cargo.toml`, alphabetical among the plugin lines:

```toml
epdf-plugin-measurement = { path = "../plugins/measurement", default-features = false, features = ["uniffi"] }
```

`api.rs`, in `ViewerConfig`, in plan-number order after plan 07's field:

```rust
    /// Install `measurementPlugin(config)`: page scale, calibration and
    /// readouts. Needs `interaction` and `annotation`.
    #[uniffi(default = None)]
    pub measurement: Option<epdf_plugin_measurement::ffi::MeasurementConfig>,
```

`session.rs`, in `Session::build`, in plan-number order:

```rust
        if let Some(measurement) = config.measurement {
            plugins.push(epdf_plugin_measurement::measurement_plugin(measurement.into()));
        }
```

`lib.rs`: `use epdf_plugin_measurement::ffi::MeasurementHandle;` and, in the `#[uniffi::export] impl ViewerHost` block, in plan-number order:

```rust
    /// The measurement capability of an open document. Needs
    /// `ViewerConfig.measurement`; without it the kernel refuses the token.
    pub async fn measurement(
        &self,
        document_id: String,
    ) -> Result<Arc<MeasurementHandle>, ViewerError> {
        let post = self.post.clone();
        self.call(move |s| {
            let cap = s
                .kernel
                .capability(&epdf_plugin_measurement::MEASUREMENT, Some(&document_id))?;
            Ok(MeasurementHandle::new(cap, post))
        })
        .await
    }
```

- [ ] **Step 8: Run the host tests**

Run: `cargo test -p epdf-ffi -p epdf-viewer-host`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add crates/epdf-viewer-host docs/superpowers/plans/2026-10-01-swift-package.md
git commit -m "Map page.measure and add measurement to ViewerHost"
```

### Task 5: `MeasurementCapability` in Swift

**Files:**
- Create: `platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Measurement.swift`
- Create: `platforms/apple/EmbedPDF/Tests/EmbedPDFTests/MeasurementTests.swift`
- Modify: `platforms/apple/build-xcframework.sh`

**Interfaces:**
- Consumes: Task 4's generated `MeasurementHandle`, `ViewerHost.measurement(documentId:)`; plan 05's `PDFViewer.annotations(for:)` and `AnnotationCapability` for the round-trip test.
- Produces: `MeasurementCapability`, `PDFViewer.measurement(for:)`.

- [ ] **Step 1: Add the fixture to the build script**

In `platforms/apple/build-xcframework.sh`, add to the `FIXTURES` list (umbrella Task 0f), one line each, after the last plan's lines:

```bash
  packages/engine/main/test/fixtures/measure-acrobat-metric.pdf
```

- [ ] **Step 2: Write the failing Swift tests**

```swift
// platforms/apple/EmbedPDF/Tests/EmbedPDFTests/MeasurementTests.swift
import CustomDump
import EmbedPDF
import Testing

extension BaseSuite {
  @Suite struct MeasurementTests {
    func opened(_ fixture: String = "report") async throws
      -> (PDFViewer, PDFDocument, MeasurementCapability, PageRef)
    {
      let viewer = try PDFViewer(
        ViewerConfig(interaction: true, annotation: AnnotationConfig(), measurement: MeasurementConfig()))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data(fixture))
      let page = try await viewer.pages(of: document)[0].page
      return (viewer, document, try await viewer.measurement(for: document), page)
    }

    @Test func ensureLoadedMakesThePageReady() async throws {
      let (_, _, measurement, page) = try await opened()
      try await measurement.ensureLoaded(page)
      let scale = try await measurement.pageScale(page)
      expectNoDifference(scale.ready, true)
      expectNoDifference(scale.source, .default)
      expectNoDifference(scale.persistent, true)
    }

    @Test func aPresetIsWrittenAndReadsBack() async throws {
      let (_, _, measurement, page) = try await opened()
      let preset = try #require(try await measurement.presets().first)
      let reports = try await measurement.setPreset(.page(page: page), presetId: preset.id)
      expectNoDifference(reports.count, 1)
      let scale = try await measurement.pageScale(page)
      expectNoDifference(scale.source, .owned)
    }

    @Test func aForeignScaleIsReadAndKept() async throws {
      let (_, _, measurement, page) = try await opened("measure-acrobat-metric")
      try await measurement.ensureLoaded(page)
      expectNoDifference(try await measurement.pageScale(page).source, .foreign)
      let preset = try #require(try await measurement.presets().first)
      _ = try await measurement.setPreset(.page(page: page), presetId: preset.id)
      expectNoDifference(try await measurement.pageScale(page).source, .owned)
      _ = try await measurement.clearScale(.page(page: page))
      expectNoDifference(try await measurement.pageScale(page).source, .foreign)
    }

    @Test func measureDistanceUsesThePageScale() async throws {
      let (_, _, measurement, page) = try await opened("measure-acrobat-metric")
      try await measurement.ensureLoaded(page)
      let reading = try await measurement.measureDistance(
        on: page, from: Point(x: 0, y: 0), to: Point(x: 72, y: 0))
      guard case let .value(readout) = reading else {
        Issue.record("expected a readout, got \(reading)")
        return
      }
      expectNoDifference(readout.kind, .distance)
      #expect(readout.value > 0)
    }

    @Test func calibratingAZeroLengthThrows() async throws {
      let (_, _, measurement, page) = try await opened()
      await #expect {
        _ = try await measurement.calibrate(
          MeasurementCalibrateInput(page: page, from: Point(x: 5, y: 5), to: Point(x: 5, y: 5), value: 1, unit: "m"))
      } throws: { error in
        (error as? PDFError)?.code == "invalid-input"
      }
    }

    @Test func aCreatedMeasurementIsAnAnnotation() async throws {
      let (viewer, document, measurement, page) = try await opened()
      let ref = try await measurement.createMeasurement(
        CreateMeasurementInput(kind: .distance, page: page, points: [Point(x: 10, y: 10), Point(x: 82, y: 10)]))
      let annotations = try await viewer.annotations(for: document)
      let found = try await annotations.get(ref)
      #expect(found != nil)
      let reading = try await measurement.readout(ref)
      if case .unavailable(let reason) = reading { Issue.record("no readout: \(reason)") }
    }

    @Test func aClosedDocumentsCapabilityThrows() async throws {
      let (viewer, document, measurement, page) = try await opened()
      try await viewer.close(document)
      await #expect {
        _ = try await measurement.pageScale(page)
      } throws: { error in
        let error = error as? PDFError
        return error?.code == "instance-closed" && error?.name == "PluginError"
      }
    }

    @Test func droppingTheStreamUnsubscribes() async throws {
      let (_, _, measurement, page) = try await opened()
      let task = Task { for await _ in measurement.events {} }
      try await Task.sleep(for: .milliseconds(20))
      task.cancel()
      _ = await task.value
      let preset = try #require(try await measurement.presets().first)
      _ = try await measurement.setPreset(.page(page: page), presetId: preset.id)
    }

    @Test func setScaleCancelsWhenTheTaskIsCancelled() async throws {
      let (_, _, measurement, page) = try await opened()
      let preset = try #require(try await measurement.presets().first)
      let task = Task { try await measurement.setPreset(.page(page: page), presetId: preset.id) }
      task.cancel()
      do {
        _ = try await task.value
      } catch {
        expectNoDifference((error as? PDFError)?.code, "operation-cancelled")
      }
      _ = try await measurement.pageScale(page)
    }

    @Test func ensureLoadedCancelsWhenTheTaskIsCancelled() async throws {
      let (_, _, measurement, page) = try await opened()
      let task = Task { try await measurement.ensureLoaded(page) }
      task.cancel()
      do {
        try await task.value
      } catch {
        expectNoDifference((error as? PDFError)?.code, "operation-cancelled")
      }
    }

    @Test func aScaleChangeIsObserved() async throws {
      let (_, _, measurement, page) = try await opened()
      let events = measurement.events
      let preset = try #require(try await measurement.presets().first)
      _ = try await measurement.setPreset(.page(page: page), presetId: preset.id)
      for await event in events {
        guard case let .scaleChanged(changed, _) = event else { continue }
        expectNoDifference(changed, page)
        break
      }
    }
  }
}
```

The cancellation tests accept either outcome (a cancel can land after the call settled), and both then make one more call so a hung kernel fails the suite's one-minute limit. The same shape is `MetadataTests.cancellingARefreshDoesNotHang`.

- [ ] **Step 3: Run them to verify they fail**

Run: `bash platforms/apple/build-xcframework.sh && (cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17' -only-testing:EmbedPDFTests/BaseSuite/MeasurementTests)`
Expected: FAIL to compile, `value of type PDFViewer has no member measurement`.

- [ ] **Step 4: Write `Measurement.swift`**

```swift
public import EmbedPDFBindings

public typealias MeasurementConfig = EmbedPDFBindings.MeasurementConfig
public typealias MeasurementDefaultScale = EmbedPDFBindings.MeasurementDefaultScale
public typealias PdfMeasurement = EmbedPDFBindings.PdfMeasurement
public typealias PdfNumberFormat = EmbedPDFBindings.PdfNumberFormat
public typealias MeasureFraction = EmbedPDFBindings.MeasureFraction
public typealias MeasureLabelPosition = EmbedPDFBindings.MeasureLabelPosition
public typealias ScalePreset = EmbedPDFBindings.ScalePreset
public typealias ScaleChangeOptions = EmbedPDFBindings.ScaleChangeOptions
public typealias MeasurementPageTarget = EmbedPDFBindings.MeasurementPageTarget
public typealias MeasurementPageScale = EmbedPDFBindings.MeasurementPageScale
public typealias MeasurementScaleSource = EmbedPDFBindings.MeasurementScaleSource
public typealias MeasurementErrorInfo = EmbedPDFBindings.MeasurementErrorInfo
public typealias MeasurementReading = EmbedPDFBindings.MeasurementReading
public typealias MeasurementReadout = EmbedPDFBindings.MeasurementReadout
public typealias MeasurementReadoutKind = EmbedPDFBindings.MeasurementReadoutKind
public typealias MeasurementUnavailable = EmbedPDFBindings.MeasurementUnavailable
public typealias MeasurementCalibrationRequest = EmbedPDFBindings.MeasurementCalibrationRequest
public typealias MeasurementCalibrateInput = EmbedPDFBindings.MeasurementCalibrateInput
public typealias MeasurementCalibrateOptions = EmbedPDFBindings.MeasurementCalibrateOptions
public typealias MeasurementKind = EmbedPDFBindings.MeasurementKind
public typealias CreateMeasurementInput = EmbedPDFBindings.CreateMeasurementInput
public typealias ScaleChangeReport = EmbedPDFBindings.ScaleChangeReport
public typealias ScaleRecalibration = EmbedPDFBindings.ScaleRecalibration
public typealias ScaleSkippedAnnotation = EmbedPDFBindings.ScaleSkippedAnnotation
public typealias ScaleSkipReason = EmbedPDFBindings.ScaleSkipReason
public typealias ScaleFailedAnnotation = EmbedPDFBindings.ScaleFailedAnnotation
public typealias MeasurementEvent = EmbedPDFBindings.MeasurementEvent

/// A document's page scales, calibration and measurement readouts. Get one
/// with `PDFViewer.measurement(for:)`; the viewer needs
/// `ViewerConfig(interaction: true, annotation: AnnotationConfig(),
/// measurement: MeasurementConfig())`. It keeps the viewer alive. After its
/// document closes, every call throws `instance-closed`.
public struct MeasurementCapability: Sendable {
  let handle: MeasurementHandle
  let viewer: PDFViewer

  public func canMeasure(_ page: PageRef) async throws(PDFError) -> Bool {
    try await bridged { try await handle.canMeasure(page: page) }
  }

  /// `ready` is false until `ensureLoaded(_:)` has read the page's viewports.
  public func pageScale(_ page: PageRef) async throws(PDFError) -> MeasurementPageScale {
    try await bridged { try await handle.pageScale(page: page) }
  }

  public func isBusy() async throws(PDFError) -> Bool {
    try await bridged { try await handle.isBusy() }
  }

  public func lastReports() async throws(PDFError) -> [ScaleChangeReport] {
    try await bridged { try await handle.lastReports() }
  }

  public func presets() async throws(PDFError) -> [ScalePreset] {
    try await bridged { try await handle.presets() }
  }

  public func units() async throws(PDFError) -> [String] {
    try await bridged { try await handle.units() }
  }

  public func areaUnits() async throws(PDFError) -> [String] {
    try await bridged { try await handle.areaUnits() }
  }

  public func readout(_ annotation: AnnotationRef) async throws(PDFError) -> MeasurementReading {
    try await bridged { try await handle.readout(annotation: annotation) }
  }

  public func measureDistance(on page: PageRef, from: Point, to: Point) async throws(PDFError)
    -> MeasurementReading
  {
    try await bridged { try await handle.measureDistance(page: page, from: from, to: to) }
  }

  public func measureArea(on page: PageRef, vertices: [Point]) async throws(PDFError)
    -> MeasurementReading
  {
    try await bridged { try await handle.measureArea(page: page, vertices: vertices) }
  }

  public func calibrationRequest() async throws(PDFError) -> MeasurementCalibrationRequest? {
    try await bridged { try await handle.calibrationRequest() }
  }

  /// One report per page. Cancelling the task stops waiting; a page write
  /// already sent still lands.
  public func setScale(
    _ pages: MeasurementPageTarget, measure: PdfMeasurement,
    options: ScaleChangeOptions = ScaleChangeOptions()
  ) async throws(PDFError) -> [ScaleChangeReport] {
    try await bridgedCancellable { token in
      try await handle.setScale(pages: pages, measure: measure, options: options, cancel: token)
    }
  }

  public func calibrate(
    _ input: MeasurementCalibrateInput,
    options: MeasurementCalibrateOptions = MeasurementCalibrateOptions()
  ) async throws(PDFError) -> [ScaleChangeReport] {
    try await bridgedCancellable { token in
      try await handle.calibrate(input: input, options: options, cancel: token)
    }
  }

  public func setUnit(
    _ pages: MeasurementPageTarget, unit: String, options: ScaleChangeOptions = ScaleChangeOptions()
  ) async throws(PDFError) -> [ScaleChangeReport] {
    try await bridgedCancellable { token in
      try await handle.setUnit(pages: pages, unit: unit, options: options, cancel: token)
    }
  }

  public func setAreaUnit(
    _ pages: MeasurementPageTarget, unit: String, options: ScaleChangeOptions = ScaleChangeOptions()
  ) async throws(PDFError) -> [ScaleChangeReport] {
    try await bridgedCancellable { token in
      try await handle.setAreaUnit(pages: pages, unit: unit, options: options, cancel: token)
    }
  }

  public func setPrecision(
    _ pages: MeasurementPageTarget, precision: Double,
    options: ScaleChangeOptions = ScaleChangeOptions()
  ) async throws(PDFError) -> [ScaleChangeReport] {
    try await bridgedCancellable { token in
      try await handle.setPrecision(pages: pages, precision: precision, options: options, cancel: token)
    }
  }

  public func setPreset(
    _ pages: MeasurementPageTarget, presetId: String,
    options: ScaleChangeOptions = ScaleChangeOptions()
  ) async throws(PDFError) -> [ScaleChangeReport] {
    try await bridgedCancellable { token in
      try await handle.setPreset(pages: pages, presetId: presetId, options: options, cancel: token)
    }
  }

  /// Removes EmbedPDF's scale; another producer's stays.
  public func clearScale(
    _ pages: MeasurementPageTarget, options: ScaleChangeOptions = ScaleChangeOptions()
  ) async throws(PDFError) -> [ScaleChangeReport] {
    try await bridgedCancellable { token in
      try await handle.clearScale(pages: pages, options: options, cancel: token)
    }
  }

  public func startCalibration() async throws(PDFError) {
    try await bridged { try await handle.startCalibration() }
  }

  public func dismissCalibration() async throws(PDFError) {
    try await bridged { try await handle.dismissCalibration() }
  }

  public func createMeasurement(_ input: CreateMeasurementInput) async throws(PDFError) -> AnnotationRef {
    try await bridgedCancellable { token in
      try await handle.createMeasurement(input: input, cancel: token)
    }
  }

  public func canCalibrate() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canCalibrate() }
  }

  public func ensureLoaded(_ page: PageRef) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.ensureLoaded(page: page, cancel: token) }
  }

  /// Finishes when the document closes (at once if it already has) or the
  /// viewer is destroyed.
  public var events: AsyncStream<MeasurementEvent> {
    viewer.registry.stream { handle.subscribe(listener: $0) }
  }
}

extension EventRelay: MeasurementEventListener where Event == MeasurementEvent {
  func onEvent(event: MeasurementEvent) {
    continuation.yield(event)
  }

  func onEnd() {
    continuation.finish()
  }
}

extension PDFViewer {
  public func measurement(for document: PDFDocument) async throws(PDFError) -> MeasurementCapability {
    let handle = try await bridged { try await host.measurement(documentId: document.id) }
    return MeasurementCapability(handle: handle, viewer: self)
  }
}
```

- [ ] **Step 5: Run the Swift tests**

Run: `bash platforms/apple/build-xcframework.sh && (cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17')`
Expected: PASS, including the 11 new `MeasurementTests`.

- [ ] **Step 6: Commit**

```bash
git add platforms/apple
git commit -m "Add MeasurementCapability to the Swift package"
```

### Task 6: CI, docs, merge and clean up

**Files:**
- Modify: `.github/workflows/plugin-rust.yml`, `platforms/apple/README.md`, `docs/superpowers/plans/2026-10-01-swift-package.md`

- [ ] **Step 1: Add the crate to CI**

In `.github/workflows/plugin-rust.yml`, append `measurement` to the `crates=( ... )` list in plan-number order.

- [ ] **Step 2: Add the README row**

```markdown
| measurement | `MeasurementCapability` | `measurement: MeasurementConfig()` (with `interaction`, `annotation`) | `2026-10-06-swift-plugins/08-measurement.md` |
```

- [ ] **Step 3: Mark Appendix A delivered**

In `2026-10-01-swift-package.md` §A.3, change `**measurement** (D, requires ...)` to `**measurement** (D), delivered by 08-measurement.md (requires ...)`.

- [ ] **Step 4: Run the definition of done**

```bash
cargo test -p epdf-plugin-measurement --features uniffi
cargo test -p epdf-ffi -p epdf-viewer-host
cargo clippy -p epdf-plugin-measurement -p epdf-viewer-host --all-targets --features epdf-plugin-measurement/uniffi -- -D warnings
cargo test -p epdf-plugin-measurement
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
git commit -m "Test the measurement exports in CI and list them in the README

libepdf_viewer_host.a (ios-sim, release): <before> -> <after> bytes"
```

Take `<before>` from the same `ls` on `research/swift-plugins` before Task 1, and fill both numbers in.

- [ ] **Step 6: Merge, rerun, clean up**

From the integration worktree:

```bash
git merge --no-ff research/swift-measurement
bash platforms/apple/build-xcframework.sh
(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17')
cargo build -p epdf-viewer-host -p uniffi-bindgen
./target/debug/uniffi-bindgen generate --library target/debug/libepdf_viewer_host.dylib --language kotlin --no-format --out-dir "$TMPDIR/epdf-kotlin-check"
git worktree remove .claude/worktrees/swift-measurement
git branch -d research/swift-measurement
git worktree prune
```

Expected: the rerun passes, and both removals succeed without `--force`. Tick 08 in `00-umbrella.md`.
