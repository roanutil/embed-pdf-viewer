//! The typed surface the vectors cannot see.

use std::rc::Rc;

use epdf_annotation_core::PageRef;
use epdf_kernel::kernel::{OpenOptions, OpenSource};
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{Kernel, KernelOptions, rt};
use epdf_plugin_annotation::*;
use epdf_plugin_interaction::{InteractionConfig, interaction_plugin};
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use serde_json::json;

fn kernel() -> Kernel {
    rt::reset();
    let box_ = json!({ "left": 0, "bottom": 0, "right": 612, "top": 792 });
    let fake = FakeEngine::new(json!({
        "documents": { "a": {
            "pages": [{ "index": 0, "ref": { "kind": "objectNumber", "pageObjectNumber": 11 },
                "label": null,
                "size": { "width": 612, "height": 792 }, "rotation": 0, "userUnit": 1,
                "boxes": { "media": box_, "crop": box_ } }],
            "responses": [
                { "path": "annotations.listRawAll", "value": { "pages": [] } },
                { "path": "page.annotations.renderAppearanceImages",
                  "value": { "appearances": [] } }
            ]
        } }
    }));
    let kernel = Kernel::new(
        Rc::new(AsEngine(fake)),
        vec![
            interaction_plugin(InteractionConfig::default()),
            annotation_plugin(AnnotationConfig::default()),
        ],
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
fn render_appearances_lets_go_of_the_signal_once_it_settles() {
    let kernel = kernel();
    let annotation = kernel.capability(&ANNOTATION, Some("a")).expect("annotation");
    let signal = AbortSignal::new();
    for _ in 0..3 {
        let images = annotation
            .render_appearances(PageRef::new(11), 1.0, Some(signal.clone()))
            .expect("a known page");
        assert_eq!(signal.listener_count(), 1);
        rt::run_until_stalled();
        assert_eq!(images.peek().expect("settled").expect("resolved"), json!([]));
        assert_eq!(signal.listener_count(), 0);
    }
}
