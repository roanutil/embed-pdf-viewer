//! `getLabel` takes a bare target or a whole link (`Link | PdfLinkTarget`);
//! a link answers for its target, outside the harness too.

use std::rc::Rc;

use epdf_kernel::kernel::OpenSource;
use epdf_kernel::{Kernel, KernelOptions, OpenOptions, rt};
use epdf_plugin_interaction::{InteractionConfig, interaction_plugin};
use epdf_plugin_link::*;
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use serde_json::json;

fn kernel() -> Kernel {
    rt::reset();
    let box_ = json!({ "left": 0, "bottom": 0, "right": 600, "top": 800 });
    let fake = FakeEngine::new(json!({
        "documents": { "a": {
            "pages": [{
                "index": 0, "ref": { "kind": "objectNumber", "pageObjectNumber": 1 }, "label": null,
                "size": { "width": 600, "height": 800 }, "rotation": 0, "userUnit": 1,
                "boxes": { "media": box_, "crop": box_ }
            }]
        } }
    }));
    let kernel = Kernel::new(
        Rc::new(AsEngine(fake)),
        vec![interaction_plugin(InteractionConfig::default()), link_plugin(LinkConfig::default())],
        KernelOptions { report: None, on_dispatch: None, release_closed_sessions: false },
    )
    .unwrap();
    let _ = kernel.start();
    rt::run_until_stalled();
    let _ = kernel
        .documents()
        .open(OpenSource::Input(json!({ "kind": "bytes", "id": "a" })), OpenOptions::default());
    rt::run_until_stalled();
    kernel
}

#[test]
fn a_link_answers_its_targets_label() {
    let kernel = kernel();
    let link = kernel.capability(&LINK, Some("a")).unwrap();
    let target = json!({ "kind": "uri", "uri": "https://example.com" });
    let whole = json!({
        "id": "l1", "bounds": { "x": 0, "y": 0, "width": 10, "height": 10 },
        "target": target, "attached": true
    });
    assert_eq!(link.get_label(&target), "https://example.com");
    assert_eq!(link.get_label(&whole), "https://example.com");

    let typed: Link = serde_json::from_value(whole).unwrap();
    assert_eq!(link.get_input_label(&LinkInput::Link(Box::new(typed))), "https://example.com");
    let named = PdfLinkTarget::Named { name: "NextPage".into() };
    assert_eq!(link.get_input_label(&LinkInput::Target(named)), "NextPage");
}
