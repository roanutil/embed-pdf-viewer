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

// ── the tap handler: a native reader's version of the web's link anchors ──

fn pointer(
    phase: epdf_plugin_interaction::Phase,
    at: Point,
) -> epdf_plugin_interaction::PointerSample {
    let hit = epdf_plugin_interaction::PageHit {
        page: epdf_kernel::geometry::PageRef::new(1),
        point: epdf_geometry::Point { x: at.x, y: at.y },
        scale: None,
        rotation: None,
        zoom: None,
    };
    epdf_plugin_interaction::PointerSample::new(
        phase,
        epdf_geometry::Point { x: at.x, y: at.y },
        Some(hit),
    )
}

/// A down at `down` and an up at `up`, through the document's pointer hub.
fn tap(thread: &KernelThread<TestKernel>, down: Point, up: Point) {
    use epdf_plugin_interaction::{INTERACTION, Phase};
    block_on(thread.call(move |k| {
        let hub = k.capability(&INTERACTION).unwrap();
        hub.dispatch_pointer(&pointer(Phase::Down, down));
        hub.dispatch_pointer(&pointer(Phase::Up, up));
    }))
    .unwrap();
}

fn activations(events: &Events) -> Vec<LinkActivation> {
    events
        .0
        .lock()
        .unwrap()
        .iter()
        .filter_map(|e| match e {
            LinkEvent::Activated { event } => Some(event.activation.clone()),
            LinkEvent::Loaded { .. } => None,
        })
        .collect()
}

const ON_URI: Point = Point { x: 100.0, y: 145.0 };

#[test]
fn a_tap_on_a_link_activates_it_once_the_tap_handler_is_registered() {
    let (thread, link) = loaded();
    let events = Arc::new(Events::default());
    let _sub = link.subscribe(events.clone());
    tap(&thread, ON_URI, ON_URI);
    assert!(activations(&events).is_empty(), "nothing listens before the registration");
    let registration = block_on(link.register_tap_handler()).unwrap();
    tap(&thread, ON_URI, ON_URI);
    assert_eq!(
        activations(&events),
        [LinkActivation::Uri { uri: "https://example.invalid/target".into() }]
    );
    registration.unregister();
    tap(&thread, ON_URI, ON_URI);
    assert_eq!(activations(&events).len(), 1, "a tap after unregister activated a link");
}

#[test]
fn a_tap_that_lifts_off_the_link_activates_nothing() {
    let (thread, link) = loaded();
    let events = Arc::new(Events::default());
    let _sub = link.subscribe(events.clone());
    let _registration = block_on(link.register_tap_handler()).unwrap();
    tap(&thread, ON_URI, Point { x: 500.0, y: 380.0 });
    assert!(activations(&events).is_empty());
}

#[test]
fn a_tool_without_link_nav_does_not_follow_links() {
    let (thread, link) = loaded();
    let events = Arc::new(Events::default());
    let _sub = link.subscribe(events.clone());
    let _registration = block_on(link.register_tap_handler()).unwrap();
    block_on(thread.call(|k| {
        use epdf_plugin_interaction::{INTERACTION, RegisterToolOptions, Tool};
        let hub = k.capability(&INTERACTION).unwrap();
        let _ = hub.register_tool(
            Tool::new("ink", "crosshair", ["annotation-draw"]),
            RegisterToolOptions::default(),
        );
        hub.activate_tool("ink", None).unwrap();
    }))
    .unwrap();
    assert!(!block_on(link.is_navigation_engaged()).unwrap());
    tap(&thread, ON_URI, ON_URI);
    assert!(activations(&events).is_empty());
}

#[test]
fn a_closed_documents_tap_handler_refuses_with_instance_closed() {
    let (thread, link) = loaded();
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    let error = block_on(link.register_tap_handler()).map(|_| ()).unwrap_err();
    assert_eq!(code(&error), Some("instance-closed"), "{error:?}");
}

/// A link activated by page and id carries its page into the action
/// context, as the web's link layer does (`link.tsx` passes `page.ref`).
#[test]
fn a_link_activated_by_id_dispatches_with_its_page() {
    use epdf_plugin_actions::{
        ACTIONS, ActionExecutorResult, ActionSource, ActionsConfig, OpenSequence, actions_plugin,
    };
    let mut scripted =
        link(12, [400.0, 10.0, 420.0, 20.0], json!({ "kind": "named", "name": "Mark" }));
    scripted["actions"] = json!({ "activate": { "root": { "type": "named", "subtype": "Named",
        "name": "Mark", "next": [] }, "incomplete": false, "warningFlags": 0, "warnings": [] } });
    let thread = KernelThread::spawn(move |_| {
        let actions =
            ActionsConfig { open_sequence: Some(OpenSequence::Off), ..Default::default() };
        let plugins = vec![
            interaction_plugin(InteractionConfig::default()),
            link_plugin(LinkConfig::default()),
            actions_plugin(actions),
        ];
        TestKernel::open(plugins, spec(vec![scripted]))
    })
    .unwrap();
    let post = thread.post();
    let link = block_on(thread.call(move |k| LinkHandle::new(k.capability(&LINK).unwrap(), post)))
        .unwrap();
    block_on(link.ensure_loaded(PAGE_1)).unwrap();
    thread_local! {
        static SEEN: std::cell::RefCell<Vec<Option<u32>>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    block_on(thread.call(|k| {
        let actions = k.capability(&ACTIONS).unwrap();
        let _ = actions.register_executor(
            "named",
            std::rc::Rc::new(|_, cx| {
                if let ActionSource::Link { page, .. } = &cx.source {
                    SEEN.with(|s| s.borrow_mut().push(page.map(|p| p.page_object_number)));
                }
                Ok(epdf_kernel::rt::Promise::resolved(ActionExecutorResult::Executed))
            }),
        );
    }))
    .unwrap();
    let activation =
        block_on(link.activate(LinkInput::Link { page: PAGE_1, id: "obj:12".into() })).unwrap();
    assert_eq!(activation, LinkActivation::Dispatched);
    let seen = block_on(thread.call(|_| SEEN.with(|s| s.borrow().clone()))).unwrap();
    assert_eq!(seen, [Some(1)]);
}
