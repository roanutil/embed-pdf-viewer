//! The typed API a Rust handler sees, beyond what the vectors reach: the
//! trait's default members (a Rust handler that omits `on_cancel` or
//! `claims_touch` gets the hub's behaviour for an absent member), and the
//! release of the registries when the document closes.

use std::cell::RefCell;
use std::rc::Rc;

use epdf_geometry::Point;
use epdf_kernel::geometry::PageRef;
use epdf_kernel::kernel::OpenSource;
use epdf_kernel::{Kernel, KernelOptions, OpenOptions, rt};
use epdf_plugin_interaction::*;
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use serde_json::json;

fn kernel() -> Kernel {
    rt::reset();
    let box_ = json!({ "left": 0, "bottom": 0, "right": 600, "top": 800 });
    let fake = FakeEngine::new(json!({
        "documents": { "a": { "pages": [{
            "index": 0, "pageObjectNumber": 1, "label": null,
            "size": { "width": 600, "height": 800 }, "rotation": 0, "userUnit": 1,
            "boxes": { "media": box_, "crop": box_ }
        }] } }
    }));
    let kernel = Kernel::new(
        Rc::new(AsEngine(fake)),
        vec![interaction_plugin(InteractionConfig::default())],
        KernelOptions { report: None, on_dispatch: None },
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

/// A handler that implements only the required members and `on_up`.
struct UpOnly(Rc<RefCell<Vec<&'static str>>>);

impl InteractionHandler for UpOnly {
    fn id(&self) -> &str {
        "up-only"
    }
    fn priority(&self) -> f64 {
        1.0
    }
    fn enabled_for(&self, tool: &Tool) -> bool {
        tool.enables("text-select")
    }
    fn on_down(&self, _: &PointerSample) -> bool {
        self.0.borrow_mut().push("down");
        true
    }
    fn on_up(&self, _: &PointerSample) {
        self.0.borrow_mut().push("up");
    }
}

fn sample(phase: Phase) -> PointerSample {
    PointerSample::new(
        phase,
        Point { x: 0.0, y: 0.0 },
        Some(PageHit {
            page: PageRef::new(1),
            point: Point { x: 1.0, y: 1.0 },
            scale: None,
            rotation: None,
            zoom: None,
        }),
    )
}

#[test]
fn trait_defaults_are_the_absent_member_behaviour() {
    let kernel = kernel();
    let hub = kernel.capability(&INTERACTION, Some("a")).unwrap();
    let log = Rc::new(RefCell::new(Vec::new()));
    hub.register_handler(Rc::new(UpOnly(log.clone())), RegisterHandlerOptions::default());
    assert!(!hub.would_claim_touch(&sample(Phase::Down)));
    hub.dispatch_pointer(&sample(Phase::Down));
    hub.dispatch_pointer(&sample(Phase::Move));
    hub.dispatch_pointer(&sample(Phase::Cancel));
    hub.dispatch_pointer(&sample(Phase::Up));
    assert_eq!(*log.borrow(), ["down", "up"]);
    hub.activate_tool("pan", None).unwrap();
    hub.dispatch_pointer(&sample(Phase::Down));
    assert_eq!(*log.borrow(), ["down", "up"]);
}

#[test]
fn closing_the_document_releases_the_handlers() {
    let kernel = kernel();
    let hub = kernel.capability(&INTERACTION, Some("a")).unwrap();
    let handler: Rc<dyn InteractionHandler> = Rc::new(UpOnly(Rc::default()));
    hub.register_handler(handler.clone(), RegisterHandlerOptions { source: Some("lens".into()) });
    hub.dispatch_pointer(&sample(Phase::Down));
    let seen = Rc::new(RefCell::new(0));
    let s = seen.clone();
    hub.on_tool_changed(move |_| {
        *s.borrow_mut() += 1;
        Ok(())
    });
    assert!(Rc::strong_count(&handler) > 1);
    let _ = kernel.documents().close("a");
    rt::run_until_stalled();
    assert_eq!(Rc::strong_count(&handler), 1);
}

#[test]
fn a_sample_debug_prints_without_its_function() {
    let mut s = sample(Phase::Move);
    s.project = Some(Rc::new(|_| None));
    let text = format!("{s:?}");
    assert!(text.starts_with("PointerSample { phase: Move"), "{text}");
    assert!(text.ends_with("project: Some(\"fn\") }"), "{text}");
}

#[test]
fn tool_tags_are_a_set() {
    let tool = Tool::new("t", "default", ["a", "b", "a"]);
    assert_eq!(tool.enables, ["a", "b"]);
    assert!(tool.enables("b"));
    assert!(!tool.enables("c"));
}

/// `page_ref_json::nullable` reads back what it writes. Only `GestureEvent`
/// uses it, and that type is serialize-only, so no vector decodes one.
#[test]
fn a_nullable_page_ref_round_trips() {
    let page = json!({ "kind": "objectNumber", "pageObjectNumber": 7 });
    assert_eq!(page_ref_json::nullable::deserialize(page).unwrap(), Some(PageRef::new(7)));
    assert_eq!(page_ref_json::nullable::deserialize(json!(null)).unwrap(), None);
    assert!(page_ref_json::nullable::deserialize(json!({ "kind": "index" })).is_err());
}
