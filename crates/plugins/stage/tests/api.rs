//! The typed surface the vectors can't reach: lens identity, a frame that
//! outlives its document, and the small helpers.

use std::rc::Rc;

use epdf_geometry::Size;
use epdf_kernel::geometry::PageRef;
use epdf_kernel::kernel::{OpenOptions, OpenSource};
use epdf_kernel::{Kernel, KernelOptions, rt};
use epdf_plugin_stage::harness::FrameScheduler;
use epdf_plugin_stage::*;
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
    stage.set_viewport_size(Size { width: 1000.0, height: 700.0 });
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
