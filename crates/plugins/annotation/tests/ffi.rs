#![recursion_limit = "256"]
#![cfg(feature = "uniffi")]
//! The uniffi handles over a kernel thread, on the re-ported capability.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use epdf_ffi::{AnnotationRef, CancelToken, KernelThread, PageRef, ResourceStatus, ViewerError};
use epdf_plugin_annotation::ffi::{
    AnnotationEvent, AnnotationEventListener, AnnotationFilter, AnnotationHandle,
    AnnotationStampPlacement, AnnotationStampSource, AnnotationTextFormat, AnnotationThreadStatus,
};
use epdf_plugin_annotation::{ANNOTATION, annotation_plugin};
use epdf_plugin_interaction::{InteractionConfig, interaction_plugin};
use futures::executor::block_on;
use plugin_conformance::test_kernel::{Reply, TestKernel, TestSpec};
use serde_json::{Value, json};

fn page_json() -> Value {
    json!({ "kind": "objectNumber", "objectNumber": 1 })
}

fn ref_json(annot: u32) -> Value {
    json!({ "kind": "objectNumber", "page": page_json(), "objectNumber": annot })
}

/// A square as the engine reads it (the vectors' shape,
/// `tooling/plugin-conformance/vectors/plugins/annotation-crud.json`).
fn square(annot: u32, extra: Value) -> Value {
    let mut dto = json!({
        "subtype": "square", "ref": ref_json(annot), "page": page_json(),
        "hasAppearance": true, "appearanceState": null, "nm": null,
        "rect": { "x": 100, "y": 100, "width": 100, "height": 100 },
        "contents": "Note", "subject": null, "blendMode": "normal",
        "invisible": false, "hidden": false, "print": true, "noZoom": false, "noRotate": false,
        "noView": false, "readOnly": false, "locked": false, "toggleNoView": false,
        "lockedContents": false, "reply": null, "popup": null, "groupId": null, "author": "Ada",
        "createdAt": null, "modifiedAt": null, "userId": null, "createdBy": null,
        "modifiedBy": null, "importedBy": null, "actions": null, "color": "#ef4444", "opacity": 1,
        "strokeWidth": 2, "borderStyle": "solid", "dashArray": null, "interiorColor": null,
        "box": { "x": 100, "y": 100, "width": 100, "height": 100 }, "rotation": null,
        "cloudyIntensity": null
    });
    if let (Value::Object(dto), Value::Object(extra)) = (&mut dto, extra) {
        dto.extend(extra);
    }
    dto
}

thread_local! {
    /// Every op `apply` was sent, on the kernel thread.
    static OPS: std::cell::RefCell<Vec<Value>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn local_origin(op_id: &str) -> Value {
    json!({ "kind": "local", "sessionId": "s1", "sub": null, "ts": 0, "serverId": null,
            "tx": { "id": op_id, "index": 0, "count": 1 } })
}

/// The engine's answer to one op, and the event it publishes for it.
fn answer(op: &Value, op_id: &str) -> Result<(Value, Value), epdf_kernel::Error> {
    let meta = |changed: Value| {
        json!({ "affectedPages": [page_json()], "cacheDelta": null, "opId": op_id,
                "undoable": true, "changed": changed })
    };
    match op["type"].as_str() {
        Some("annotations.create") => {
            let n = op["objectNumber"].as_u64().unwrap_or(10) as u32;
            let mut made = square(n, json!({}));
            made["subtype"] = op["data"]["subtype"].clone();
            let item = json!({ "type": "annotations.create", "page": page_json(), "annotation": made,
                               "meta": meta(json!([ref_json(n)])) });
            let event = json!({ "type": "annotations.created", "page": page_json(), "annotation": made,
                                "meta": meta(json!([ref_json(n)])), "origin": local_origin(op_id) });
            Ok((item, event))
        }
        Some("annotations.update") => {
            let n = op["ref"]["objectNumber"].as_u64().unwrap_or(0) as u32;
            let updated = square(n, op["patch"].clone());
            let item = json!({ "type": "annotations.update", "annotation": updated,
                               "meta": meta(json!([ref_json(n)])) });
            let event = json!({ "type": "annotations.updated", "page": page_json(), "annotation": updated,
                                "meta": meta(json!([ref_json(n)])), "origin": local_origin(op_id) });
            Ok((item, event))
        }
        Some("annotations.delete") => {
            let n = op["ref"]["objectNumber"].as_u64().unwrap_or(0) as u32;
            if n == 99 {
                return Err(epdf_kernel::Error::engine("NotFound", "no annotation 99"));
            }
            let item = json!({ "type": "annotations.delete", "deleted": [ref_json(n)],
                               "meta": meta(json!([ref_json(n)])) });
            let event = json!({ "type": "annotations.deleted", "page": page_json(), "deleted": [ref_json(n)],
                                "meta": meta(json!([ref_json(n)])), "origin": local_origin(op_id) });
            Ok((item, event))
        }
        _ => Ok((json!({ "type": op["type"], "meta": meta(json!([])) }), Value::Null)),
    }
}

fn spec(annotations: Vec<Value>) -> TestSpec {
    let page = json!({
        "index": 0, "ref": page_json(), "label": null, "size": { "width": 600, "height": 800 },
        "rotation": 0, "userUnit": 1, "tabOrder": null,
        "pdfCropBox": { "left": 0, "bottom": 0, "right": 600, "top": 800 },
        "boxes": { "media": { "x": 0, "y": 0, "width": 600, "height": 800 },
                   "crop": { "x": 0, "y": 0, "width": 600, "height": 800 } }
    });
    let listed = json!({ "annotations": annotations });
    TestSpec::new(json!({ "documents": { "a": {
        "id": "a", "security": { "allows": "*" }, "pages": [page],
        "objectNumbers": [10, 11, 12, 13, 14, 15]
    } } }))
    .respond("annotations.list", move |_| Ok(Reply::value(listed.clone())))
    .respond("apply", |args| {
        let ops = args[0]["ops"].as_array().cloned().unwrap_or_default();
        let op_id = "00000000-0000-7000-8000-000000000001";
        let mut items = Vec::new();
        let mut events = Vec::new();
        for op in &ops {
            OPS.with(|o| o.borrow_mut().push(op.clone()));
            let (item, event) = answer(op, op_id)?;
            items.push(item);
            if !event.is_null() {
                events.push(event);
            }
        }
        let meta = json!({ "affectedPages": [page_json()], "cacheDelta": null, "opId": op_id, "undoable": true });
        Ok(Reply { value: json!({ "items": items, "meta": meta }), events })
    })
}

fn handle_over(annotations: Vec<Value>) -> (KernelThread<TestKernel>, Arc<AnnotationHandle>) {
    let thread = KernelThread::spawn(move |_| {
        TestKernel::open(
            vec![interaction_plugin(InteractionConfig::default()), annotation_plugin(None)],
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

fn sent_ops(thread: &KernelThread<TestKernel>) -> Vec<Value> {
    block_on(thread.call(|_| OPS.with(|o| o.borrow().clone()))).unwrap()
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
fn the_records_read_as_the_engine_wrote_them() {
    let (_thread, annotations) = handle_over(vec![square(5, json!({}))]);
    block_on(annotations.refresh(token())).unwrap();
    assert_eq!(block_on(annotations.status()).unwrap(), ResourceStatus::Ready);
    let all = block_on(annotations.list(None)).unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].annotation_ref, obj(5));
    assert_eq!(all[0].subtype, "square");
    assert_eq!(all[0].contents.as_deref(), Some("Note"));
    assert_eq!(all[0].author.as_deref(), Some("Ada"));
    let fields: Value = serde_json::from_str(&all[0].fields_json).unwrap();
    assert_eq!(fields["strokeWidth"], 2);
    let filtered = block_on(
        annotations
            .list(Some(AnnotationFilter { subtype: Some("ink".into()), ..Default::default() })),
    )
    .unwrap();
    assert!(filtered.is_empty());
}

#[test]
fn create_then_update_then_delete_is_observed() {
    let (thread, annotations) = handle_over(vec![]);
    block_on(annotations.refresh(token())).unwrap();
    let events = Arc::new(Events::default());
    let _sub = annotations.subscribe(events.clone());
    let fields =
        json!({ "subtype": "square", "box": { "x": 100, "y": 100, "width": 100, "height": 100 } });
    let created = block_on(annotations.create(
        PageRef { page_object_number: 1 },
        fields.to_string(),
        None,
        None,
        token(),
    ))
    .unwrap();
    assert_eq!(created, obj(10));
    block_on(annotations.update(
        created.clone(),
        json!({ "contents": "Edited" }).to_string(),
        token(),
    ))
    .unwrap();
    block_on(annotations.delete(created, token())).unwrap();
    let kinds: Vec<_> = sent_ops(&thread).iter().map(|op| op["type"].clone()).collect();
    assert_eq!(
        kinds,
        vec![json!("annotations.create"), json!("annotations.update"), json!("annotations.delete")]
    );
    let seen = events.0.lock().unwrap();
    assert!(seen.iter().any(|e| matches!(e, AnnotationEvent::Created { .. })), "{seen:?}");
    assert!(seen.iter().any(|e| matches!(e, AnnotationEvent::Updated { .. })), "{seen:?}");
    assert!(seen.iter().any(|e| matches!(e, AnnotationEvent::Deleted { .. })), "{seen:?}");
}

#[test]
fn a_refused_delete_rejects_and_the_record_stays() {
    let (_thread, annotations) = handle_over(vec![square(99, json!({}))]);
    block_on(annotations.refresh(token())).unwrap();
    assert!(block_on(annotations.delete(obj(99), token())).is_err());
    assert_eq!(block_on(annotations.list(None)).unwrap().len(), 1);
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
    let fields =
        json!({ "subtype": "square", "box": { "x": 0, "y": 0, "width": 10, "height": 10 } });
    let create = annotations.create(
        PageRef { page_object_number: 1 },
        fields.to_string(),
        None,
        None,
        cancel.clone(),
    );
    cancel.cancel();
    release.send(()).unwrap();
    block_on(blocker).unwrap();
    let error = block_on(create).unwrap_err();
    assert_eq!(code(&error), Some("operation-cancelled"), "{error:?}");
    assert!(block_on(annotations.list(None)).unwrap().is_empty());
}

#[test]
fn selecting_two_annotations_and_grouping_them() {
    let (_thread, annotations) = handle_over(vec![
        square(5, json!({})),
        square(6, json!({ "rect": { "x": 300, "y": 300, "width": 50, "height": 50 } })),
    ]);
    block_on(annotations.refresh(token())).unwrap();
    let selection = annotations.selection();
    block_on(selection.select(vec![obj(5), obj(6)], None)).unwrap();
    assert_eq!(block_on(selection.refs()).unwrap(), vec![obj(5), obj(6)]);
    assert!(block_on(selection.can_group()).unwrap());
    block_on(selection.clear()).unwrap();
    assert!(block_on(selection.refs()).unwrap().is_empty());
}

#[test]
fn a_selection_properties_read_names_what_is_mixed() {
    let (_thread, annotations) =
        handle_over(vec![square(5, json!({ "opacity": 1 })), square(6, json!({ "opacity": 0.5 }))]);
    block_on(annotations.refresh(token())).unwrap();
    let selection = annotations.selection();
    block_on(selection.select(vec![obj(5), obj(6)], None)).unwrap();
    let props = block_on(selection.props()).unwrap();
    assert!(props.mixed.iter().any(|k| k == "opacity"), "{props:?}");
}

#[test]
fn toggling_bold_without_a_free_text_selection_writes_nothing() {
    let (thread, annotations) = handle_over(vec![square(5, json!({}))]);
    block_on(annotations.refresh(token())).unwrap();
    let selection = annotations.selection();
    block_on(selection.select(vec![obj(5)], None)).unwrap();
    block_on(selection.toggle_text_format(AnnotationTextFormat::Bold, token())).unwrap();
    assert!(sent_ops(&thread).is_empty());
}

#[test]
fn the_default_tools_are_listed_and_their_defaults_change() {
    let (_thread, annotations) = handle_over(vec![]);
    let tools = annotations.tools();
    let all = block_on(tools.tools()).unwrap();
    let ink = all.iter().find(|t| t.subtype == "ink").expect("an ink tool");
    let properties = block_on(tools.properties(ink.id.clone())).unwrap();
    assert!(properties.properties.iter().any(|p| p.key == "strokeWidth"), "{properties:?}");
    block_on(tools.update_defaults(ink.id.clone(), json!({ "strokeWidth": 4 }).to_string()))
        .unwrap();
    let defaults: Value =
        serde_json::from_str(&block_on(tools.defaults(ink.id.clone())).unwrap()).unwrap();
    assert_eq!(defaults["strokeWidth"], 4);
}

#[test]
fn a_stamp_placed_from_png_bytes_sends_its_bytes() {
    let png = include_bytes!("../../../../tooling/engine-conformance/fixtures/tiny.png").to_vec();
    let (thread, annotations) = handle_over(vec![]);
    block_on(annotations.refresh(token())).unwrap();
    block_on(annotations.drafts().place_stamp(
        AnnotationStampSource {
            bytes: png,
            mime_type: Some("image/png".into()),
            name: None,
            subject: None,
        },
        AnnotationStampPlacement {
            page: PageRef { page_object_number: 1 },
            center: epdf_ffi::Point { x: 300.0, y: 400.0 },
            rotation: None,
        },
        Some(120.0),
        token(),
    ))
    .unwrap();
    let ops = sent_ops(&thread);
    let create = ops.iter().find(|op| op["type"] == "annotations.create").expect("a create");
    assert_eq!(create["data"]["subtype"], "stamp", "{create}");
}

#[test]
fn a_reply_lands_in_its_thread() {
    let (_thread, annotations) = handle_over(vec![square(5, json!({}))]);
    block_on(annotations.refresh(token())).unwrap();
    let threads = annotations.threads();
    let reply = block_on(threads.reply(obj(5), "Agreed".into(), token())).unwrap();
    assert_eq!(reply, obj(10));
    assert!(block_on(threads.permissions(obj(5))).unwrap().can_reply);
    block_on(threads.set_status(obj(5), AnnotationThreadStatus::Accepted, token())).unwrap();
}

#[test]
fn a_polygon_draft_has_nothing_to_finish_before_drawing() {
    let (_thread, annotations) = handle_over(vec![]);
    let drafts = annotations.drafts();
    assert!(block_on(drafts.creation_draft()).unwrap().is_none());
    assert_eq!(block_on(drafts.finish_creation_draft(token())).unwrap(), None);
}

use epdf_plugin_annotation::ffi::{
    AnnotationFilePicker, AnnotationFilePromptRequest, AnnotationPickedFile,
};

/// Make `id` the hub's active tool, as a toolbar tap would.
fn activate(thread: &KernelThread<TestKernel>, id: &'static str) {
    block_on(thread.call(move |k| {
        k.capability(&epdf_plugin_interaction::INTERACTION).unwrap().activate_tool(id)
    }))
    .unwrap()
    .unwrap();
}

fn tiny_png() -> AnnotationPickedFile {
    AnnotationPickedFile {
        bytes: include_bytes!("../../../../tooling/engine-conformance/fixtures/tiny.png").to_vec(),
        name: Some("tiny.png".into()),
        mime_type: Some("image/png".into()),
    }
}

struct OnePng(Mutex<Vec<AnnotationFilePromptRequest>>);

#[async_trait::async_trait]
impl AnnotationFilePicker for OnePng {
    async fn pick(&self, request: AnnotationFilePromptRequest) -> Option<AnnotationPickedFile> {
        self.0.lock().unwrap().push(request);
        Some(tiny_png())
    }
}

struct Cancels;

#[async_trait::async_trait]
impl AnnotationFilePicker for Cancels {
    async fn pick(&self, _request: AnnotationFilePromptRequest) -> Option<AnnotationPickedFile> {
        None
    }
}

/// Wait (bounded) for the picked file's create to reach the engine.
fn wait_for_create(thread: &KernelThread<TestKernel>) -> Option<Value> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        if let Some(op) = sent_ops(thread).into_iter().find(|op| op["type"] == "annotations.create")
        {
            return Some(op);
        }
        if std::time::Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn a_requested_stamp_asks_the_picker_and_places_what_it_returns() {
    let (thread, annotations) = handle_over(vec![]);
    block_on(annotations.refresh(token())).unwrap();
    activate(&thread, "stamp");
    let picker = Arc::new(OnePng(Mutex::new(Vec::new())));
    block_on(annotations.set_file_picker(Some(picker.clone()))).unwrap();
    let page = PageRef { page_object_number: 1 };
    assert!(
        block_on(annotations.drafts().request_stamp_at(
            page,
            epdf_ffi::Point { x: 100.0, y: 100.0 },
            None
        ))
        .unwrap()
    );
    let create = wait_for_create(&thread).expect("a stamp was created");
    assert_eq!(create["data"]["subtype"], "stamp", "{create}");
    assert_eq!(picker.0.lock().unwrap().len(), 1);
}

#[test]
fn a_cancelled_pick_places_nothing() {
    let (thread, annotations) = handle_over(vec![]);
    block_on(annotations.refresh(token())).unwrap();
    activate(&thread, "stamp");
    block_on(annotations.set_file_picker(Some(Arc::new(Cancels)))).unwrap();
    let page = PageRef { page_object_number: 1 };
    assert!(
        block_on(annotations.drafts().request_stamp_at(
            page,
            epdf_ffi::Point { x: 100.0, y: 100.0 },
            None
        ))
        .unwrap()
    );
    std::thread::sleep(std::time::Duration::from_millis(100));
    block_on(thread.call(|_| ())).unwrap();
    assert!(block_on(annotations.list(None)).unwrap().is_empty());
    assert!(sent_ops(&thread).is_empty());
}

#[test]
fn a_pick_that_lands_after_a_tool_change_places_nothing() {
    struct Slow;
    #[async_trait::async_trait]
    impl AnnotationFilePicker for Slow {
        async fn pick(&self, _r: AnnotationFilePromptRequest) -> Option<AnnotationPickedFile> {
            std::thread::sleep(std::time::Duration::from_millis(150));
            Some(tiny_png())
        }
    }
    let (thread, annotations) = handle_over(vec![]);
    block_on(annotations.refresh(token())).unwrap();
    activate(&thread, "stamp");
    block_on(annotations.set_file_picker(Some(Arc::new(Slow)))).unwrap();
    let page = PageRef { page_object_number: 1 };
    assert!(
        block_on(annotations.drafts().request_stamp_at(
            page,
            epdf_ffi::Point { x: 100.0, y: 100.0 },
            None
        ))
        .unwrap()
    );
    // The user switches tools while the picker is still open.
    activate(&thread, "note");
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(block_on(annotations.list(None)).unwrap().is_empty());
    assert!(sent_ops(&thread).is_empty());
}

/// `createFromSelection` resolves `{ annotations }` (`write/markup.ts`
/// `createFromSelection`); the handle answers the created annotations'
/// refs. Over the engine of the `annotation-markup.json` vector
/// `create-from-selection-anchors-at-the-end-glyph`, which has the text a
/// selection needs, with that case's selection and tool: its tables answer
/// the creates the case recorded.
#[test]
fn creating_from_the_selection_answers_the_created_refs() {
    use epdf_ffi::KernelBound;
    use epdf_plugin_selection::{SELECTION, SelectionRangeInput, selection_plugin};
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../tooling/plugin-conformance/vectors/plugins/annotation-markup.json"
    );
    let vectors: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let engine = vectors["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "create-from-selection-anchors-at-the-end-glyph")
        .expect("the vector case")["engine"]
        .clone();
    let thread = KernelThread::spawn(move |_| {
        TestKernel::open(
            vec![
                interaction_plugin(InteractionConfig::default()),
                selection_plugin(Default::default()),
                annotation_plugin(None),
            ],
            TestSpec::new(engine),
        )
    })
    .unwrap();
    let post = thread.post();
    let (annotations, selection) = block_on(thread.call(move |k| {
        (
            AnnotationHandle::new(k.capability(&ANNOTATION).unwrap(), post.clone()),
            KernelBound::new(k.capability(&SELECTION).unwrap(), post),
        )
    }))
    .unwrap();
    block_on(annotations.refresh(token())).unwrap();
    let page = epdf_kernel::geometry::PageRef::new(1);
    block_on(selection.call_promise(move |s| Ok(s.ensure_loaded(page)), |_| ())).unwrap();
    block_on(
        selection.call(move |s| s.select(SelectionRangeInput::Span { page, start: 0, count: 3 })),
    )
    .unwrap();
    let before = block_on(annotations.list(None)).unwrap().len();
    let refs =
        block_on(annotations.drafts().create_from_selection("replace-text".into(), None, token()))
            .unwrap();
    assert!(!refs.is_empty(), "no refs answered");
    let listed = block_on(annotations.list(None)).unwrap();
    assert!(listed.len() > before, "{listed:?}");
    for made in &refs {
        assert!(listed.iter().any(|a| &a.annotation_ref == made), "{made:?} is not listed");
    }
}
