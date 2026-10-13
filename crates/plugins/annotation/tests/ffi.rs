#![cfg(feature = "uniffi")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use epdf_ffi::{
    AnnotationPropsPatch, AnnotationRef, CancelToken, KernelThread, PageRef, Rect, ViewerError,
};
use epdf_plugin_annotation::ffi::{
    AnnotationEvent, AnnotationEventListener, AnnotationFilter, AnnotationHandle,
    AnnotationHydration, AnnotationPatch, CreateAnnotationInput,
};
use epdf_plugin_annotation::{ANNOTATION, AnnotationConfig, annotation_plugin};
use epdf_plugin_interaction::{InteractionConfig, interaction_plugin};
use futures::executor::block_on;
use plugin_conformance::test_kernel::{Reply, TestKernel, TestSpec};
use serde_json::{Value, json};

fn page_json(pon: u32) -> Value {
    json!({ "kind": "objectNumber", "pageObjectNumber": pon })
}

fn ref_json(pon: u32, annot: u32) -> Value {
    json!({ "kind": "objectNumber", "page": page_json(pon), "annotObjectNumber": annot })
}

/// A square annotation DTO as the kernel reads one (the vectors' shape,
/// `tooling/plugin-conformance/vectors/plugins/annotation-crud.json`).
fn square(annot: u32, extra_props: Value) -> Value {
    let mut dto = json!({
        "ref": ref_json(1, annot), "page": page_json(1), "index": annot,
        "identityQuality": "durable", "nm": null,
        "flags": { "invisible": false, "hidden": false, "print": true, "noZoom": false,
            "noRotate": false, "noView": false, "readOnly": false, "locked": false,
            "toggleNoView": false, "lockedContents": false },
        "rect": { "left": 100, "bottom": 600, "right": 200, "top": 700 },
        "contents": "Note", "subject": null, "author": "Ada", "created": null, "modified": null,
        "blendMode": "normal", "inReplyTo": null, "replyType": null, "subtype": "square",
        "color": { "r": 239, "g": 68, "b": 68 }, "interiorColor": null, "opacity": 1,
        "strokeWidth": 2, "borderStyle": "solid", "cloudyIntensity": null, "rectDifferences": null
    });
    if let (Value::Object(dto), Value::Object(extra)) = (&mut dto, extra_props) {
        dto.extend(extra);
    }
    dto
}

fn spec(annotations: Vec<Value>) -> TestSpec {
    let bx = json!({ "left": 0, "bottom": 0, "right": 600, "top": 800 });
    let pages = json!([{ "index": 0, "ref": page_json(1), "label": null,
        "size": { "width": 600, "height": 800 }, "rotation": 0, "userUnit": 1,
        "boxes": { "media": bx, "crop": bx } }]);
    let snapshot = json!({ "pages": [{
        "pageState": { "page": page_json(1) }, "annotations": annotations }] });
    TestSpec::new(json!({ "documents": { "a": {
        "id": "a", "security": { "allows": "*" }, "pages": pages } } }))
    .respond("annotations.listRawAll", move |_| Ok(Reply::value(snapshot.clone())))
    .respond_page("page.annotations.create", |_, args| {
        let mut created = square(10, json!({}));
        created["subtype"] = args[0]["subtype"].clone();
        Ok(Reply::value(json!({ "created": created })))
    })
    .respond_page("page.annotations.update", |_, args| {
        // Object numbers travel as JavaScript numbers (`5.0`).
        let annot = args[0]["annotObjectNumber"].as_f64().unwrap() as u32;
        Ok(Reply::value(json!({ "updated": square(annot, json!({ "contents": "Edited" })) })))
    })
    .respond_page("page.annotations.delete", |_, args| {
        if args[0]["annotObjectNumber"].as_f64() == Some(99.0) {
            return Err(epdf_kernel::Error::engine("NotFound", "no annotation 99"));
        }
        Ok(Reply::value(json!({})))
    })
}

fn handle_over(annotations: Vec<Value>) -> (KernelThread<TestKernel>, Arc<AnnotationHandle>) {
    let thread = KernelThread::spawn(move |_| {
        TestKernel::open(
            vec![
                interaction_plugin(InteractionConfig::default()),
                annotation_plugin(AnnotationConfig::default()),
            ],
            spec(annotations),
        )
    })
    .unwrap();
    let post = thread.post();
    let handle = block_on(
        thread.call(move |k| AnnotationHandle::new(k.capability(&ANNOTATION).unwrap(), post)),
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

fn obj(annot: u32) -> AnnotationRef {
    AnnotationRef::ObjectNumber {
        page: PageRef { page_object_number: 1 },
        annot_object_number: annot,
    }
}

#[derive(Default)]
struct Events(Mutex<Vec<AnnotationEvent>>, AtomicUsize);

impl AnnotationEventListener for Events {
    fn on_event(&self, event: AnnotationEvent) {
        self.0.lock().unwrap().push(event);
    }
    fn on_end(&self) {
        self.1.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn hydration_reads_every_annotation_as_a_typed_record() {
    let (_thread, annotations) = handle_over(vec![square(5, json!({}))]);
    block_on(annotations.refresh(token())).unwrap();
    assert_eq!(block_on(annotations.hydration()).unwrap(), AnnotationHydration::Complete);
    let all = block_on(annotations.list(None)).unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].annotation_ref, obj(5));
    assert_eq!(all[0].subtype, "square");
    assert_eq!(all[0].contents.as_deref(), Some("Note"));
    assert_eq!(all[0].author.as_deref(), Some("Ada"));
    assert_eq!(all[0].props.stroke_width, Some(2.0));
    assert_eq!(all[0].props.font_size, None);
    assert!(all[0].flags.print);
    let filtered = block_on(
        annotations
            .list(Some(AnnotationFilter { subtype: Some("ink".into()), ..Default::default() })),
    )
    .unwrap();
    assert!(filtered.is_empty());
}

#[test]
fn unknown_prop_keys_are_dropped_not_fatal() {
    let (_thread, annotations) =
        handle_over(vec![square(5, json!({ "vendorThing": { "nested": true } }))]);
    block_on(annotations.refresh(token())).unwrap();
    let got = block_on(annotations.get(obj(5))).unwrap().unwrap();
    assert_eq!(got.subtype, "square");
}

#[test]
fn create_then_update_then_delete_is_observed() {
    let (_thread, annotations) = handle_over(vec![]);
    block_on(annotations.refresh(token())).unwrap();
    let events = Arc::new(Events::default());
    let _sub = annotations.subscribe(events.clone());
    let created = block_on(annotations.create(
        CreateAnnotationInput {
            page: PageRef { page_object_number: 1 },
            subtype: "square".into(),
            bounds: Some(Rect { x: 100.0, y: 100.0, width: 100.0, height: 100.0 }),
            ..Default::default()
        },
        token(),
    ))
    .unwrap();
    assert_eq!(created, obj(10));
    block_on(annotations.update(
        created.clone(),
        AnnotationPatch { contents: Some("Edited".into()), ..Default::default() },
        token(),
    ))
    .unwrap();
    block_on(annotations.delete(created, token())).unwrap();
    let seen = events.0.lock().unwrap();
    assert!(seen.iter().any(|e| matches!(e, AnnotationEvent::Created { .. })), "{seen:?}");
    assert!(seen.iter().any(|e| matches!(e, AnnotationEvent::Updated { .. })), "{seen:?}");
    assert!(seen.iter().any(|e| matches!(e, AnnotationEvent::Deleted { .. })), "{seen:?}");
}

#[test]
fn a_props_patch_reaches_the_engine() {
    let (_thread, annotations) = handle_over(vec![square(5, json!({}))]);
    block_on(annotations.refresh(token())).unwrap();
    block_on(annotations.update(
        obj(5),
        AnnotationPatch {
            props: Some(AnnotationPropsPatch { opacity: Some(0.5), ..Default::default() }),
            ..Default::default()
        },
        token(),
    ))
    .unwrap();
}

#[test]
fn a_batch_reports_each_failure_beside_what_applied() {
    let (_thread, annotations) = handle_over(vec![square(5, json!({})), square(6, json!({}))]);
    block_on(annotations.refresh(token())).unwrap();
    let result = block_on(annotations.delete_many(vec![obj(5), obj(99)], token())).unwrap();
    assert_eq!(result.applied, vec![obj(5)]);
    assert_eq!(result.failed.len(), 1);
    assert_eq!(result.failed[0].annotation_ref, obj(99));
}

#[test]
fn a_closed_documents_capability_refuses_with_instance_closed() {
    let (thread, annotations) = handle_over(vec![]);
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    for error in [
        block_on(annotations.list(None)).map(|_| ()).unwrap_err(),
        block_on(annotations.can_create()).map(|_| ()).unwrap_err(),
        block_on(annotations.refresh(token())).unwrap_err(),
    ] {
        assert_eq!(code(&error), Some("instance-closed"), "{error:?}");
    }
}

#[test]
fn closing_the_document_ends_its_subscriptions() {
    let (thread, annotations) = handle_over(vec![]);
    let events = Arc::new(Events::default());
    let _sub = annotations.subscribe(events.clone());
    block_on(thread.call(|_| ())).unwrap();
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    assert_eq!(events.1.load(Ordering::SeqCst), 1);
}

#[test]
fn cancelling_a_create_before_it_runs_creates_nothing() {
    let (thread, annotations) = handle_over(vec![]);
    block_on(annotations.refresh(token())).unwrap();
    let (release, gate) = std::sync::mpsc::channel::<()>();
    let (parked_tx, parked) = std::sync::mpsc::channel::<()>();
    let blocker = thread.call(move |_| {
        parked_tx.send(()).unwrap();
        gate.recv().unwrap();
    });
    parked.recv().unwrap();
    let cancel = token();
    let create = annotations.create(
        CreateAnnotationInput {
            page: PageRef { page_object_number: 1 },
            subtype: "square".into(),
            bounds: Some(Rect { x: 0.0, y: 0.0, width: 10.0, height: 10.0 }),
            ..Default::default()
        },
        cancel.clone(),
    );
    cancel.cancel();
    release.send(()).unwrap();
    block_on(blocker).unwrap();
    let error = block_on(create).unwrap_err();
    assert_eq!(code(&error), Some("operation-cancelled"), "{error:?}");
    assert!(block_on(annotations.list(None)).unwrap().is_empty());
}

use epdf_plugin_annotation::ffi::AnnotationTextFormat;

#[test]
fn selecting_two_annotations_and_grouping_them() {
    let (_thread, annotations) = handle_over(vec![square(5, json!({})), square(6, json!({}))]);
    block_on(annotations.refresh(token())).unwrap();
    let selection = annotations.selection();
    block_on(selection.select(vec![obj(5), obj(6)], None)).unwrap();
    assert_eq!(block_on(selection.refs()).unwrap(), vec![obj(5), obj(6)]);
    assert!(block_on(selection.can_group()).unwrap());
    block_on(selection.clear()).unwrap();
    assert!(block_on(selection.refs()).unwrap().is_empty());
}

#[test]
fn a_selection_props_read_names_what_is_mixed() {
    let (_thread, annotations) =
        handle_over(vec![square(5, json!({ "opacity": 1 })), square(6, json!({ "opacity": 0.5 }))]);
    block_on(annotations.refresh(token())).unwrap();
    let selection = annotations.selection();
    block_on(selection.select(vec![obj(5), obj(6)], None)).unwrap();
    let props = block_on(selection.props()).unwrap();
    assert!(props.mixed.contains(&epdf_ffi::AnnotationPropKey::Opacity), "{props:?}");
}

#[test]
fn toggling_bold_without_a_free_text_selection_is_a_no_op() {
    let (_thread, annotations) = handle_over(vec![square(5, json!({}))]);
    block_on(annotations.refresh(token())).unwrap();
    let selection = annotations.selection();
    block_on(selection.select(vec![obj(5)], None)).unwrap();
    block_on(selection.toggle_text_format(AnnotationTextFormat::Bold, token())).unwrap();
}

#[test]
fn the_default_tools_are_listed_with_their_specs() {
    let (_thread, annotations) = handle_over(vec![]);
    let tools = annotations.tools();
    let all = block_on(tools.tools()).unwrap();
    let ink = all.iter().find(|t| t.subtype == "ink").expect("an ink tool");
    let specs = block_on(tools.prop_specs(ink.id.clone())).unwrap();
    assert!(specs.iter().any(|s| s.key == epdf_ffi::AnnotationPropKey::StrokeWidth), "{specs:?}");
    block_on(tools.set_defaults(
        ink.id.clone(),
        epdf_ffi::AnnotationPropsPatch { stroke_width: Some(4.0), ..Default::default() },
    ))
    .unwrap();
    assert_eq!(block_on(tools.defaults(ink.id.clone())).unwrap().stroke_width, Some(4.0));
}
