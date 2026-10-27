//! The behaviour Task 13.22 pins beyond the printed traces, against
//! `packages/plugin/measurement/src/{model,write/calibration,write/scale}.ts`
//! and the kernel page mirror the viewports live in
//! (`packages/core/main/src/page-mirror.ts`): the three `pageScaleOf`
//! inputs, a calibration capture taken from the drag's final move (the
//! `up` ends the draft where the last `move` left it, `update/draw.ts`), the
//! finite-positive refusals of `calibrate`, and a close while changes run
//! (each all-pages write reports its page's error, the queued single-page
//! write rejects `instance-closed`, and the closed-state update is reported
//! once).

use std::cell::RefCell;
use std::rc::Rc;

use epdf_geometry::Point as GeometryPoint;
use epdf_kernel::error::code;
use epdf_kernel::geometry::{PageRef, Point};
use epdf_kernel::kernel::{OpenOptions, OpenSource};
use epdf_kernel::mirror::ResourceStatus;
use epdf_kernel::{AnyPlugin, Error, Kernel, KernelOptions, Promise, rt};
use epdf_plugin_annotation::{ANNOTATION, AnnotationCapability, annotation_plugin};
use epdf_plugin_interaction::{INTERACTION, InteractionConfig, interaction_plugin};
use epdf_plugin_measurement::model::{PageScaleInputs, PageSize, page_scale_of};
use epdf_plugin_measurement::*;
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use serde_json::{Value, json};

fn pr(n: u32) -> Value {
    PageRef::new(n).to_json()
}

fn layout(pon: u32, index: u32, width: f64, height: f64) -> Value {
    let b = json!({ "x": 0, "y": 0, "width": width, "height": height });
    json!({
        "index": index,
        "ref": pr(pon),
        "label": null,
        "size": { "width": width, "height": height },
        "rotation": 0,
        "userUnit": 1,
        "tabOrder": null,
        "pdfCropBox": { "left": 0, "bottom": 0, "right": width, "top": height },
        "boxes": { "media": b, "crop": b, "bleed": b, "trim": b, "art": b },
    })
}

fn viewports(pon: u32, list: Value) -> Value {
    json!({ "path": "page.measure.listViewports", "pon": pon, "value": { "viewports": list } })
}

fn metres() -> Value {
    json!({
        "subtype": "rectilinear",
        "ratio": "1 in = 1 m",
        "x": [{ "unit": "m", "conversion": 1.0 / 72.0, "precision": 100 }],
        "distance": [{ "unit": "m", "conversion": 1, "precision": 100 }],
        "area": [{ "unit": "m²", "conversion": 1, "precision": 100 }],
    })
}

fn feet() -> Value {
    json!({
        "subtype": "rectilinear",
        "ratio": "1 in = 1 ft",
        "x": [{ "unit": "ft", "conversion": 1.0 / 72.0, "precision": 100 }],
        "distance": [{ "unit": "ft", "conversion": 1, "precision": 100 }],
        "area": [{ "unit": "ft²", "conversion": 1, "precision": 100 }],
    })
}

struct Rig {
    kernel: Kernel,
    fake: Rc<FakeEngine>,
    measurement: MeasurementCapability,
    annotation: AnnotationCapability,
    reports: Rc<RefCell<Vec<Error>>>,
}

impl Rig {
    /// Document `a`, pages 1 (600x800) and 2 (400x500), with the engine's
    /// page measure service and the given responses ahead of the defaults.
    fn new(responses: Vec<Value>) -> Rig {
        rt::reset();
        let reports: Rc<RefCell<Vec<Error>>> = Rc::default();
        let heard = reports.clone();
        let mut all = responses;
        all.push(json!({ "path": "annotations.list", "value": { "annotations": [] } }));
        all.push(viewports(1, json!([])));
        all.push(viewports(2, json!([])));
        let fake = FakeEngine::new(json!({ "documents": { "a": {
            "pages": [layout(1, 0, 600.0, 800.0), layout(2, 1, 400.0, 500.0)],
            "security": { "allows": "*" },
            "services": ["page.measure"],
            "responses": all,
        } } }));
        let plugins: Vec<AnyPlugin> = vec![
            interaction_plugin(InteractionConfig::default()),
            annotation_plugin(None),
            measurement_plugin(MeasurementConfig::default()),
        ];
        let kernel = Kernel::new(
            Rc::new(AsEngine(fake.clone())),
            plugins,
            KernelOptions {
                report: Some(Rc::new(move |error: &Error| heard.borrow_mut().push(error.clone()))),
                ..KernelOptions::default()
            },
        )
        .expect("kernel");
        kernel.start();
        rt::run_until_stalled();
        kernel.documents().open(
            OpenSource::Input(json!({ "kind": "bytes", "id": "a", "bytes": { "$bytes": "" } })),
            OpenOptions::default(),
        );
        rt::run_until_stalled();
        let measurement = kernel.capability(&MEASUREMENT, Some("a")).expect("measurement");
        let annotation = kernel.capability(&ANNOTATION, Some("a")).expect("annotation");
        Rig { kernel, fake, measurement, annotation, reports }
    }

    fn calls_to(&self, path: &str) -> usize {
        self.fake.log_since(0).iter().filter(|entry| entry["path"] == path).count()
    }
}

fn settled<T: Clone + 'static>(promise: &Promise<T>) -> Option<Result<T, Error>> {
    rt::run_until_stalled();
    promise.peek()
}

fn failure<T: Clone + 'static>(promise: &Promise<T>) -> Error {
    match settled(promise) {
        Some(Err(error)) => error,
        Some(Ok(_)) => panic!("resolved where the TypeScript rejects"),
        None => panic!("still pending"),
    }
}

fn input(from: (f64, f64), to: (f64, f64), value: f64) -> CalibrateInput {
    CalibrateInput {
        page: PageInput::Ref(PageRef::new(1)),
        from: Point { x: from.0, y: from.1 },
        to: Point { x: to.0, y: to.1 },
        distance: KnownLength { value, unit: "m".into() },
    }
}

#[test]
fn page_scale_of_projects_its_three_read_states() {
    let known = json!([{
        "bbox": { "x": 0, "y": 0, "width": 600, "height": 800 },
        "measure": feet(),
        "owned": true,
        "name": null,
    }]);
    let offline =
        json!({ "name": "EngineError", "code": "RuntimeUnavailable", "message": "offline" });
    let fallback = metres();
    let size = Some(PageSize { width: 600.0, height: 800.0 });

    // Loading, nothing read yet: not ready, no measure, persistence kept.
    let loading = page_scale_of(&PageScaleInputs {
        viewports: None,
        status: ResourceStatus::Loading,
        error: None,
        size,
        fallback: &fallback,
        persistent: true,
    });
    assert_eq!(
        loading.to_json(),
        json!({ "measure": null, "source": "default", "ready": false, "persistent": true })
    );

    // A failed re-read keeps the last confirmed viewports and reports why.
    let failed_known = page_scale_of(&PageScaleInputs {
        viewports: Some(&known),
        status: ResourceStatus::Error,
        error: Some(&offline),
        size,
        fallback: &fallback,
        persistent: true,
    });
    assert_eq!(
        failed_known.to_json(),
        json!({
            "measure": feet(),
            "source": "owned",
            "ready": true,
            "persistent": true,
            "error": offline,
        })
    );

    // A failed first read on a page not in the registry: not ready, with
    // the error.
    let failed_empty = page_scale_of(&PageScaleInputs {
        viewports: None,
        status: ResourceStatus::Error,
        error: Some(&offline),
        size: None,
        fallback: &fallback,
        persistent: false,
    });
    assert_eq!(
        failed_empty.to_json(),
        json!({
            "measure": null,
            "source": "default",
            "ready": false,
            "persistent": false,
            "error": offline,
        })
    );
}

#[test]
fn a_calibration_capture_ends_at_the_drags_final_move() {
    let rig = Rig::new(Vec::new());
    let requested: Rc<RefCell<Vec<CalibrationRequest>>> = Rc::default();
    let heard = requested.clone();
    let _off = rig.measurement.on_calibration_requested(move |event| {
        heard.borrow_mut().push(event.request.clone());
        Ok(())
    });
    rig.measurement.start_calibration().expect("armed");
    let interaction = rig.kernel.capability(&INTERACTION, Some("a")).expect("interaction");
    assert_eq!(interaction.get_active_tool_id(), "calibrate");
    let page = pr(1);
    for (phase, x) in [("down", 100.0), ("move", 150.0), ("move", 260.0), ("up", 244.0)] {
        rig.annotation
            .create_pointer("calibrate", phase, &page, GeometryPoint { x, y: 100.0 }, false, None)
            .expect("pointer");
        // Each pointer event is its own step, drained as a scenario drains.
        rt::run_until_stalled();
    }
    rt::run_until_stalled();
    let request = rig.measurement.get_calibration_request().expect("a request");
    assert_eq!(request.page, PageRef::new(1));
    assert_eq!(request.from, Point { x: 100.0, y: 100.0 });
    assert_eq!(request.to, Point { x: 260.0, y: 100.0 }, "the final move, not the up point");
    assert_eq!(request.user_space_length, 160.0);
    assert_eq!(*requested.borrow(), vec![request]);
    assert_eq!(interaction.get_active_tool_id(), "pointer");
}

#[test]
fn calibrate_refuses_lengths_that_are_not_finite_and_positive() {
    let rig = Rig::new(Vec::new());
    let before = rig.measurement.get_page_scale(PageRef::new(1)).expect("scale");
    let cases = [
        input((5.0, 5.0), (5.0, 5.0), 1.0),
        input((5.0, 5.0), (50.0, 5.0), -1.0),
        input((5.0, 5.0), (50.0, 5.0), 0.0),
        input((5.0, 5.0), (50.0, 5.0), f64::NAN),
        input((5.0, 5.0), (50.0, 5.0), f64::INFINITY),
        input((5.0, 5.0), (f64::INFINITY, 5.0), 1.0),
    ];
    for case in cases {
        let error = failure(&rig.measurement.calibrate(case.clone(), CalibrateOptions::default()));
        assert_eq!(error.plugin_code(), Some(code::INVALID_INPUT), "{case:?}");
        assert_eq!(error.message, "Calibration requires finite positive lengths and scales");
        assert_eq!(error.member("cause"), None, "the refusal carries no cause");
    }
    assert_eq!(rig.calls_to("page.measure.setScale"), 0, "nothing reached the engine");
    assert!(!rig.measurement.is_busy());
    assert!(Rc::ptr_eq(&before, &rig.measurement.get_page_scale(PageRef::new(1)).unwrap()));
}

#[test]
fn closing_mid_change_reports_each_page_and_refuses_the_queued_write() {
    let scale_set = |pon: u32| {
        json!({
            "type": "pages.scaleSet",
            "page": pr(pon),
            "meta": { "affectedPages": [pr(pon)], "cacheDelta": null, "opId": "set", "undoable": true },
            "origin": { "kind": "local", "sessionId": "s1", "sub": null, "ts": 0, "serverId": null },
        })
    };
    let rig = Rig::new(vec![
        json!({ "path": "page.measure.setScale", "pon": 1, "defer": "set1" }),
        json!({
            "path": "page.measure.setScale",
            "pon": 2,
            "value": { "page": pr(2), "meta": { "affectedPages": [pr(2)], "cacheDelta": null, "opId": "set2", "undoable": true } },
            "events": [scale_set(2)],
            "defer": "set2",
        }),
    ]);
    let options =
        || ScaleChangeOptions { recalculate: Some(false), ..ScaleChangeOptions::default() };
    let every = rig.measurement.set_scale(PageTarget::All, feet(), options());
    let queued = rig.measurement.set_scale(PageRef::new(2), metres(), options());
    rt::run_until_stalled();
    assert!(rig.measurement.is_busy());
    assert_eq!(rig.calls_to("page.measure.setScale"), 2);

    let closing = rig.kernel.documents().close("a");
    rt::run_until_stalled();
    assert!(matches!(closing.peek(), Some(Ok(()))));

    let reports = match settled(&every) {
        Some(Ok(reports)) => reports,
        other => panic!("the all-pages change resolves: {:?}", other.map(|r| r.map(|_| ()))),
    };
    let closed = json!({
        "name": "EngineError",
        "code": "Unknown",
        "message": "document instance a#1 was closed",
    });
    assert_eq!(
        *reports,
        vec![
            ScaleChangeReport::ScaleError { page: PageRef::new(1), scale_error: closed.clone() },
            ScaleChangeReport::ScaleError { page: PageRef::new(2), scale_error: closed },
        ]
    );

    let refused = failure(&queued);
    assert_eq!(refused.plugin_code(), Some(code::INSTANCE_CLOSED));
    assert_eq!(refused.message, "document instance a#1 was closed");

    let closed_updates: Vec<String> = rig
        .reports
        .borrow()
        .iter()
        .filter(|e| e.plugin_code() == Some(code::INSTANCE_CLOSED))
        .map(|e| e.message.clone())
        .collect();
    assert_eq!(closed_updates, vec!["a state update after the instance closed (a#1); dropped"]);
}

/// `write/scale.ts:61`: `ctx.onSettle` makes a download wait for the scale
/// changes on their way (`Promise.all` over each page queue's `idle()`), so
/// the file has the scale in it.
#[test]
fn a_download_waits_for_a_scale_change_on_its_way() {
    let rig = Rig::new(vec![
        json!({ "path": "page.measure.setScale", "pon": 1, "defer": "set1" }),
        json!({ "path": "download", "value": { "$bytes": "" } }),
    ]);
    let options = ScaleChangeOptions { recalculate: Some(false), ..ScaleChangeOptions::default() };
    let change = rig.measurement.set_scale(PageRef::new(1), feet(), options);
    rt::run_until_stalled();
    assert_eq!(rig.calls_to("page.measure.setScale"), 1);
    let saved = rig.kernel.documents().save(Some("a"), None);
    rt::run_until_stalled();
    assert_eq!(rig.calls_to("download"), 0, "the download waits for the scale change");
    assert!(saved.peek().is_none());
    let answer = json!({ "page": pr(1), "meta": { "affectedPages": [pr(1)], "cacheDelta": null, "opId": "set1", "undoable": true } });
    assert!(rig.fake.settle("set1", Ok(answer)));
    rt::run_until_stalled();
    assert!(change.peek().is_some());
    assert_eq!(rig.calls_to("download"), 1, "then downloads");
    assert!(saved.peek().is_some());
}
