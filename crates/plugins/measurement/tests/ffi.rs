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
use epdf_plugin_measurement::{
    MEASUREMENT, MeasurementConfig, ScaleChangeOptions, measurement_plugin,
};
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
    // The plugin loads every page's viewports when it connects, so page 2's
    // read is held open by the fake engine (`defer`) until the test settles
    // it; until then the page reads as loading.
    let (thread, m) = handle_over(|| {
        TestSpec::new(json!({
            "documents": { "a": {
                "pages": [page(1, 600.0, 800.0), page(2, 400.0, 500.0)],
                "security": { "allows": "*" },
                "services": ["page.measure"],
                "responses": [
                    { "path": "annotations.listRawAll", "value": { "pages": [] } },
                    { "path": "page.measure.viewports", "pon": 1, "value": [] },
                    { "path": "page.measure.viewports", "pon": 2, "defer": "page-2" }
                ]
            } }
        }))
    });
    let before = block_on(m.page_scale(P2)).unwrap();
    assert!(!before.ready);
    assert_eq!(before.measure, None);
    let loaded = m.ensure_loaded(P2, token());
    let settled = block_on(thread.call(|k| k.engine.settle("page-2", Ok(json!([]))))).unwrap();
    assert!(settled, "page 2's viewport read was not pending");
    block_on(loaded).unwrap();
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
    assert!(
        reports
            .iter()
            .any(|r| matches!(r, ScaleChangeReport::Recalibrated { report } if report.page == P1))
    );
    assert!(reports.iter().any(|r| matches!(r, ScaleChangeReport::ScaleFailed { page, error } if *page == P2 && error.code == "Unknown")));
    assert_eq!(block_on(m.page_scale(P1)).unwrap().source, MeasurementScaleSource::Owned);
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
