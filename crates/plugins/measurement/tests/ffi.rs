#![cfg(feature = "uniffi")]
//! The uniffi handle over a test kernel. Migrated by hand in Task 13.22 to
//! the current plugin (page-space viewports from `page.measure.listViewports`,
//! `rectilinear` measures, the annotation plugin's 13.17 records).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use epdf_ffi::{CancelToken, KernelThread, PageRef, Point, ViewerError};
use epdf_kernel::page_predict::viewports_with_scale;
use epdf_plugin_annotation::annotation_plugin;
use epdf_plugin_interaction::{InteractionConfig, interaction_plugin};
use epdf_plugin_measurement::ffi::{
    MeasurementCalibrateInput, MeasurementCalibrateOptions, MeasurementEvent,
    MeasurementEventListener, MeasurementHandle, MeasurementPageTarget, MeasurementReading,
    MeasurementScaleChangeOptions as ScaleChangeOptions, MeasurementScaleSource, PdfMeasurement,
    PdfNumberFormat, ScaleChangeReport,
};
use epdf_plugin_measurement::{MEASUREMENT, MeasurementConfig, measurement_plugin};
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

fn page_ref(pon: u32) -> Value {
    json!({ "kind": "objectNumber", "objectNumber": pon })
}

fn page(pon: u32, w: f64, h: f64) -> Value {
    let b = json!({ "x": 0, "y": 0, "width": w, "height": h });
    json!({
        "index": pon - 1, "ref": page_ref(pon),
        "label": null, "size": { "width": w, "height": h }, "rotation": 0, "userUnit": 1,
        "tabOrder": null,
        "pdfCropBox": { "left": 0, "bottom": 0, "right": w, "top": h },
        "boxes": { "media": b, "crop": b, "bleed": b, "trim": b, "art": b }
    })
}

/// The rectilinear measure the conformance vectors use: 1 in on paper is
/// 1 m.
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
        subtype: "rectilinear".into(),
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
    // The pages as the engine has them: their viewports ride on the layout.
    let pages: Rc<RefCell<Vec<Value>>> =
        Rc::new(RefCell::new(vec![page(1, 600.0, 800.0), page(2, 400.0, 500.0)]));
    if foreign {
        pages.borrow_mut()[0]["viewports"] = json!([{
            "bbox": { "x": 0, "y": 0, "width": 600, "height": 800 },
            "name": "Acrobat", "owned": false,
            "measure": serde_json::to_value(metric()).unwrap()
        }]);
    }
    let mut engine = json!({
        "documents": { "a": {
            "pages": pages.borrow().clone(),
            "security": { "allows": "*" },
            "responses": [{ "path": "annotations.list", "value": { "annotations": [] } }]
        } }
    });
    if service {
        engine["documents"]["a"]["services"] = json!(["page.measure"]);
    }
    let spec =
        if service { TestSpec::new(engine) } else { TestSpec::new(engine).lacking("page.measure") };
    // A change of `pages.setScale` ops: all of it applies, or a refusal of
    // one page refuses the whole change. The `pages.scaleSet` event carries
    // the new layout, published before the answer.
    spec.respond("apply", move |args| {
        let ops = args.first().and_then(|c| c["ops"].as_array()).cloned().unwrap_or_default();
        let named = |op: &Value| -> Vec<u32> {
            op["pages"].as_array().into_iter().flatten().filter_map(|p| p["objectNumber"].as_u64()).map(|n| n as u32).collect()
        };
        if ops.iter().any(|op| fail_pon.is_some_and(|f| named(op).contains(&f))) {
            return Err(epdf_kernel::Error::engine("Unknown", "Unknown: the write failed"));
        }
        let mut list = pages.borrow_mut();
        let mut events = Vec::new();
        for op in ops.iter().filter(|op| op["type"] == "pages.setScale") {
            for pon in named(op) {
                let layout = &mut list[(pon - 1) as usize];
                let before = layout["viewports"].as_array().cloned().unwrap_or_default();
                let after = viewports_with_scale(&before, &layout["size"], &op["measure"]);
                let fields = layout.as_object_mut().unwrap();
                fields.shift_remove("viewports");
                if !after.is_empty() {
                    fields.insert("viewports".into(), Value::Array(after));
                }
            }
            events.push(json!({
                "type": "pages.scaleSet",
                "pages": op["pages"],
                "layout": { "pages": list.clone(), "labels": [] },
                "meta": { "affectedPages": op["pages"], "cacheDelta": null, "opId": "set", "undoable": true },
                "origin": { "kind": "local", "sessionId": "s1",
                    "subject": null, "at": "1970-01-01T00:00:00.000Z", "serverSequence": null }
            }));
        }
        Ok(Reply {
            value: json!({
                "items": [],
                "meta": { "affectedPages": [], "cacheDelta": null, "opId": "set", "undoable": true }
            }),
            events,
        })
    })
}

fn handle_over(
    spec: impl FnOnce() -> TestSpec + Send + 'static,
) -> (KernelThread<TestKernel>, Arc<MeasurementHandle>) {
    let thread = KernelThread::spawn(|_| {
        TestKernel::open(
            vec![
                interaction_plugin(InteractionConfig::default()),
                annotation_plugin(None),
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
fn a_page_scale_is_known_once_the_document_opens() {
    // A page's viewports come with its layout: no read of them, and the
    // page is ready at once; `ensure_loaded` resolves for a page that is
    // there.
    let (_thread, m) = handle_over(|| spec(true, false, None));
    let scale = block_on(m.page_scale(P2)).unwrap();
    assert!(scale.ready);
    assert_eq!(scale.source, MeasurementScaleSource::Default);
    assert!(scale.persistent);
    block_on(m.ensure_loaded(P2, token())).unwrap();
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
        seen.iter()
            .any(|e| matches!(e, MeasurementEvent::ScaleChanged { page, .. } if *page == P1)),
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
    block_on(m.clear_scale(
        MeasurementPageTarget::Page { page: P1 },
        ScaleChangeOptions::default(),
        token(),
    ))
    .unwrap();
    assert_eq!(block_on(m.page_scale(P1)).unwrap().source, MeasurementScaleSource::Foreign);
}

#[test]
fn a_refused_change_fails_every_page_of_it() {
    // A scale change is one change: the engine refusing page 2 refuses page
    // 1 with it, and each page reports why.
    let (_thread, m) = handle_over(|| spec(true, false, Some(2)));
    let reports = block_on(m.set_scale(
        MeasurementPageTarget::All,
        metric(),
        ScaleChangeOptions::default(),
        token(),
    ))
    .unwrap();
    assert_eq!(reports.len(), 2);
    for (report, page) in reports.iter().zip([P1, P2]) {
        assert!(
            matches!(report, ScaleChangeReport::ScaleFailed { page: p, error } if *p == page && error.code == "Unknown"),
            "{report:?}"
        );
    }
    assert_eq!(block_on(m.page_scale(P1)).unwrap().source, MeasurementScaleSource::Default);
}

#[test]
fn measure_distance_reads_in_the_page_scale() {
    let (_thread, m) = handle_over(|| spec(true, false, None));
    block_on(m.set_scale(
        MeasurementPageTarget::Page { page: P1 },
        metric(),
        ScaleChangeOptions::default(),
        token(),
    ))
    .unwrap();
    let reading =
        block_on(m.measure_distance(P1, Point { x: 0.0, y: 0.0 }, Point { x: 72.0, y: 0.0 }))
            .unwrap();
    match reading {
        MeasurementReading::Value { readout } => {
            // `1 / 72` goes through `Math.fround`, as the TypeScript does.
            assert!((readout.value - 1.0).abs() < 1e-6, "{readout:?}");
            assert_eq!(readout.label, "1.00 m");
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
    let error =
        block_on(m.calibrate(input, MeasurementCalibrateOptions::default(), token())).unwrap_err();
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
    let reading =
        block_on(m.measure_distance(P1, Point { x: 0.0, y: 0.0 }, Point { x: 72.0, y: 0.0 }))
            .unwrap();
    assert!(
        matches!(reading, MeasurementReading::Value { readout } if (readout.value - 2.0).abs() < 1e-6)
    );
    assert!(
        events
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, MeasurementEvent::CalibrationCompleted { page } if *page == P1))
    );
}

#[test]
fn without_a_service_the_scale_is_in_memory() {
    // The defensive absent-service branch (`sync/viewports.ts:47`,
    // `write/scale.ts:81`): a scale kept in memory, not persistent, on a
    // handle whose `has("page.measure")` is false (`TestSpec::lacking`).
    let (_thread, m) = handle_over(|| spec(false, false, None));
    block_on(m.set_scale(
        MeasurementPageTarget::Page { page: P1 },
        metric(),
        ScaleChangeOptions::default(),
        token(),
    ))
    .unwrap();
    let scale = block_on(m.page_scale(P1)).unwrap();
    assert_eq!(scale.source, MeasurementScaleSource::Owned);
    assert!(!scale.persistent);
}

#[test]
fn units_and_presets_are_listed() {
    let (_thread, m) = handle_over(|| spec(true, false, None));
    assert!(block_on(m.units()).unwrap().contains(&"m".to_string()));
    assert!(block_on(m.area_units()).unwrap().contains(&"m2".to_string()));
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
    let write = m.set_scale(
        MeasurementPageTarget::Page { page: P1 },
        metric(),
        ScaleChangeOptions::default(),
        cancel.clone(),
    );
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
