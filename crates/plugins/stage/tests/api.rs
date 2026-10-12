//! The typed surface the vectors can't reach: lens identity, a frame that
//! outlives its document, and the small helpers.

use std::rc::Rc;

use epdf_geometry::{Point, Size};
use epdf_kernel::geometry::PageRef;
use epdf_kernel::kernel::{OpenOptions, OpenSource};
use epdf_kernel::{Kernel, KernelOptions, rt};
use epdf_plugin_stage::harness::FrameScheduler;
use epdf_plugin_stage::*;
use epdf_stage_core::Camera;
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use serde_json::json;

fn kernel(scheduler: bool) -> Kernel {
    rt::reset();
    let box_ = json!({ "left": 0, "bottom": 0, "right": 600, "top": 800 });
    let page = |pon: u32, index: u32| {
        json!({ "index": index, "ref": { "kind": "objectNumber", "pageObjectNumber": pon },
            "label": null,
            "size": { "width": 600, "height": 800 }, "rotation": 0, "userUnit": 1,
            "boxes": { "media": box_, "crop": box_ } })
    };
    let fake = FakeEngine::new(json!({
        "documents": { "a": { "pages": [page(1, 0), page(2, 1), page(3, 2)] } }
    }));
    let config = StageConfig {
        scheduler: scheduler.then(|| Rc::new(FrameScheduler::default()) as Rc<dyn Scheduler>),
        ..Default::default()
    };
    let kernel = Kernel::new(
        Rc::new(AsEngine(fake)),
        vec![stage_plugin(StagePluginOptions { config, ..Default::default() })],
        KernelOptions::default(),
    )
    .expect("kernel");
    kernel.start();
    rt::run_until_stalled();
    kernel.documents().open(
        OpenSource::Input(json!({ "kind": "bytes", "id": "a", "bytes": { "$bytes": "" } })),
        OpenOptions::default(),
    );
    rt::run_until_stalled();
    kernel
}

#[test]
fn handles_to_one_lens_are_the_same_lens() {
    let kernel = kernel(false);
    let a = kernel.capability(&STAGE, Some("a")).expect("stage");
    let b = kernel.capability(&STAGE, Some("a")).expect("stage");
    assert!(a.same(&b));
    assert!(a.same(&a.clone()));
    let other = self::kernel(false);
    let c = other.capability(&STAGE, Some("a")).expect("stage");
    assert!(!a.same(&c));
}

#[test]
fn frames_after_close_run_out() {
    let kernel = kernel(true);
    let stage = kernel.capability(&STAGE, Some("a")).expect("stage");
    stage.set_viewport_size(Size { width: 1000.0, height: 700.0 }).expect("viewport");
    stage.go_to_page_index(2, Some(GoToOptions::behavior(ScrollBehaviorKind::Smooth)));
    stage.zoom_in().expect("zoom");
    stage.fling(0.0, -3000.0).expect("fling");
    assert!(stage.is_moving());
    kernel.documents().close("a");
    rt::run_until_stalled();
    // The pending physics and rest frames fire after close. They read the
    // lease's last state, as the TypeScript's do, their dispatches are
    // dropped, and each motion runs to its end.
    rt::advance(10_000.0);
    assert!(rt::pending_timers() == 0);
}

#[test]
fn closing_the_document_releases_providers_and_rule_predicates() {
    let kernel = kernel(false);
    let stage = kernel.capability(&STAGE, Some("a")).expect("stage");
    // Each closure holds the capability (the cycle) and a marker to count.
    let marker = Rc::new(());
    let (held, lens) = (marker.clone(), stage.clone());
    let _withdraw = stage.provide_initial_view(1.0, move || {
        let _ = (&held, &lens);
        None
    });
    let (held, lens) = (marker.clone(), stage.clone());
    stage
        .set_responsive_rules(vec![ResponsiveRule {
            name: Some("held".into()),
            when: RuleWhen::Predicate(Rc::new(move |_| {
                let _ = (&held, &lens);
                false
            })),
            settings: None,
        }])
        .expect("rules");
    assert_eq!(Rc::strong_count(&marker), 3);
    kernel.documents().close("a");
    rt::run_until_stalled();
    assert_eq!(Rc::strong_count(&marker), 1, "close let go of both closures");
}

#[test]
fn rule_predicates_debug_print_without_the_closure() {
    let rule = RuleWhen::Predicate(Rc::new(|_| true));
    assert_eq!(format!("{rule:?}"), "Predicate(fn)");
    let query = RuleWhen::Query(BoxQuery { max_width: Some(600.0), ..Default::default() });
    assert!(format!("{query:?}").starts_with("Query(BoxQuery"));
}

#[test]
fn every_destination_names_its_page() {
    let page = json!({ "kind": "objectNumber", "pageObjectNumber": 7 });
    let kinds = [
        json!({ "kind": "xyz", "page": page }),
        json!({ "kind": "fit", "page": page }),
        json!({ "kind": "fitH", "page": page }),
        json!({ "kind": "fitV", "page": page }),
        json!({ "kind": "fitR", "page": page, "left": 0, "bottom": 0, "right": 1, "top": 1 }),
        json!({ "kind": "fitB", "page": page }),
        json!({ "kind": "fitBH", "page": page }),
        json!({ "kind": "fitBV", "page": page }),
    ];
    for kind in kinds {
        let dest: PdfDestination = serde_json::from_value(kind).expect("destination");
        assert_eq!(dest.page(), PageRef::new(7));
    }
}

fn grid(columns: GridColumns) -> (Kernel, StageCapability) {
    let kernel = kernel(false);
    let stage = kernel.capability(&STAGE, Some("a")).expect("stage");
    stage.set_viewport_size(Size { width: 1000.0, height: 700.0 }).expect("viewport");
    stage
        .update_settings(PartialSettings {
            layout: Some(LayoutKind::Grid),
            columns: Some(columns),
            ..Default::default()
        })
        .expect("grid");
    (kernel, stage)
}

#[test]
fn non_finite_camera_input_on_a_grid_is_an_error() {
    let (_kernel, stage) = grid(GridColumns::Count(2.0));
    let nan = Camera { x: f64::NAN, y: 0.0, zoom: 1.0 };
    let error = stage.set_camera(nan).expect_err("no item under a NaN camera");
    assert_eq!(error.name, "TypeError");
    assert!(stage.get_viewpoint().is_err());
    assert!(stage.set_viewport_size(Size { width: 900.0, height: 700.0 }).is_err());
    stage.set_camera(Camera { x: 0.0, y: 0.0, zoom: 1.0 }).expect("a finite camera recovers");
    assert!(stage.zoom_by(f64::NAN, None).is_err());
    assert!(stage.pan_by(f64::NAN, 0.0).is_err());
    assert!(stage.double_tap_zoom(Point { x: f64::NAN, y: 0.0 }).is_err());
}

#[test]
fn a_nan_column_count_empties_the_grid_instead_of_panicking() {
    let (_kernel, stage) = grid(GridColumns::Count(f64::NAN));
    assert!(stage.list_visible_pages().is_empty());
    stage.set_camera(Camera { x: 0.0, y: 0.0, zoom: 1.0 }).expect("camera");
    stage.zoom_by(2.0, None).expect("zoom");
    stage.double_tap_zoom(Point { x: 10.0, y: 10.0 }).expect("double tap");
    stage.set_viewport_size(Size { width: 900.0, height: 700.0 }).expect("viewport");
    stage.get_viewpoint().expect("viewpoint");
    stage.go_to_page_index(2, None);
    stage.next_page(None);
    stage.fit_page().expect("fit");
    stage
        .update_settings(PartialSettings {
            columns: Some(GridColumns::Count(2.0)),
            ..Default::default()
        })
        .expect("a whole count re-lays the grid");
    assert!(!stage.list_visible_pages().is_empty());
}

#[test]
fn a_nan_zoom_on_an_auto_wrapped_grid_does_not_panic() {
    let (_kernel, stage) = grid(GridColumns::Policy(ColumnsPolicy::Auto));
    let _ = stage.zoom_by(f64::NAN, None);
    let _ = stage.set_camera(Camera { x: f64::NAN, y: f64::NAN, zoom: f64::NAN });
    let _ = stage.get_viewpoint();
    let _ = stage.list_visible_pages();
    let _ = stage.set_viewport_size(Size { width: 800.0, height: 600.0 });
}
