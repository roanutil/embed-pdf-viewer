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
        link(
            8,
            [40.0, 300.0, 320.0, 330.0],
            json!({ "kind": "goto", "destination":
            { "kind": "xyz", "page": { "kind": "objectNumber", "pageObjectNumber": 3 }, "left": 0, "top": 400, "zoom": 0 } }),
        ),
        link(
            9,
            [40.0, 240.0, 360.0, 270.0],
            json!({ "kind": "uri", "uri": "https://example.invalid/target" }),
        ),
    ];
    annotations.extend(extra);
    TestSpec::new(json!({ "documents": { DOCUMENT: {
        "id": DOCUMENT, "security": { "allows": "*" },
        "pages": [page(1, 0), page(2, 1), page(3, 2)]
    } } }))
    .respond_page("page.annotations.list", move |pon, _| {
        Ok(Reply::value(
            json!({ "annotations": if pon == 1 { annotations.clone() } else { vec![] } }),
        ))
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
        let mut plugins = vec![
            interaction_plugin(InteractionConfig::default()),
            link_plugin(LinkConfig::default()),
        ];
        if stage {
            plugins.push(stage_plugin(StagePluginOptions::default()));
        }
        TestKernel::open(plugins, spec(extra))
    })
    .unwrap();
    let post = thread.post();
    let handle =
        block_on(thread.call(move |k| LinkHandle::new(k.capability(&LINK).unwrap(), post)))
            .unwrap();
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
    assert_eq!(
        (links[0].bounds.x, links[0].bounds.y, links[0].bounds.width, links[0].bounds.height),
        (40.0, 70.0, 280.0, 30.0)
    );
    assert!(
        matches!(&links[1].target, PdfLinkTarget::Uri { uri } if uri == "https://example.invalid/target")
    );
    assert!(!links[0].has_action);
}

#[test]
fn every_page_s_links_come_back_from_all_links() {
    let (_thread, link) = handle_with(false, vec![]);
    assert_eq!(block_on(link.all_links()).unwrap().len(), 2);
}

#[test]
fn the_smallest_link_under_a_point_wins() {
    let small =
        link(10, [100.0, 310.0, 120.0, 320.0], json!({ "kind": "named", "name": "NextPage" }));
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
    let activation =
        block_on(link.activate(LinkInput::Link { page: PAGE_1, id: "obj:9".into() })).unwrap();
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
    let resolution = block_on(
        link.resolve(PdfLinkTarget::Goto { destination: PdfDestination::Fit { page: PAGE_3 } }),
    )
    .unwrap();
    assert!(
        matches!(resolution, LinkResolution::Reveal { page, page_index: 2, .. } if page == PAGE_3)
    );
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
    let error = block_on(link.activate(LinkInput::Link { page: PAGE_1, id: "obj:404".into() }))
        .unwrap_err();
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
        block_on(link.label(LinkInput::Target {
            target: PdfLinkTarget::Goto { destination: PdfDestination::Fit { page: PAGE_3 } }
        }))
        .unwrap(),
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
    assert!(
        matches!(events.0.lock().unwrap().first(), Some(LinkEvent::Loaded { event }) if event.page == PAGE_1)
    );
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
