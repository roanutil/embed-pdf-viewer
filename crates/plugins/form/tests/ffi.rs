#![cfg(feature = "uniffi")]

//! The uniffi handle over the current plugin. Migrated by hand in Task 13.18
//! with `src/ffi.rs`, including the five restored in 13.18's fix round (refresh and status, the widget projection, a closed
//! document's refusal, `on_end` at close, a cancelled write).

use std::cell::RefCell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use epdf_ffi::{CancelToken, FormFieldRef, KernelThread, PageRef, ResourceStatus, ViewerError};
use epdf_plugin_form::ffi::{
    FormConfig, FormEvent, FormEventListener, FormFieldDetail, FormFillControl, FormHandle,
    FormKind, FormSetValueStatus, FormValidation, FormValueInput,
};
use epdf_plugin_form::{FORM, form_plugin};
use epdf_plugin_interaction::{InteractionConfig, interaction_plugin};
use futures::executor::block_on;
use plugin_conformance::test_kernel::{Reply, TestKernel, TestSpec};
use serde_json::{Value, json};

fn page() -> Value {
    json!({ "kind": "objectNumber", "objectNumber": 1 })
}

fn text(name: &str) -> Value {
    json!({
        "ref": { "kind": "objectNumber", "objectNumber": 1 }, "name": "name", "origin": "acroform",
        "readOnly": false, "required": false, "noExport": false,
        "alternateName": "Your name", "mappingName": null,
        "valueEntry": { "kind": "scalar", "value": name }, "defaultValueEntry": { "kind": "none" },
        "groupId": null, "createdBy": null, "createdAt": null, "filledBy": null,
        "filledByName": null, "filledAt": null, "importedBy": null,
        "widgets": [{ "ref": { "kind": "objectNumber", "page": page(), "objectNumber": 101 },
            "objectNumber": 101, "page": page() }],
        "family": "text", "value": name, "defaultValue": "", "maxLength": 40,
        "multiline": false, "password": false, "comb": false
    })
}

fn row() -> Value {
    json!({
        "subtype": "widget", "ref": { "kind": "objectNumber", "page": page(), "objectNumber": 101 },
        "page": page(), "rect": { "x": 10, "y": 20, "width": 100, "height": 24 },
        "hidden": false, "noView": false, "strokeWidth": 1, "borderStyle": "solid",
        "textAlign": "left", "field": { "kind": "objectNumber", "objectNumber": 1 },
        "fieldFamily": "text",
    })
}

thread_local! {
    static NAME: RefCell<String> = RefCell::new("Ada".into());
    /// The ops each `apply` the engine was sent carried, in order.
    static SENT: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
}

fn sent(thread: &KernelThread<TestKernel>) -> Vec<Value> {
    block_on(thread.call(|_| SENT.with(|s| s.borrow().clone()))).unwrap()
}

fn spec() -> TestSpec {
    let bx = json!({ "x": 0, "y": 0, "width": 600, "height": 800 });
    TestSpec::new(json!({ "documents": { "a": { "id": "a", "security": { "allows": "*" },
        "pages": [{ "index": 0, "ref": page(), "label": null, "size": { "width": 600, "height": 800 },
            "rotation": 0, "userUnit": 1, "tabOrder": null,
            "pdfCropBox": { "left": 0, "bottom": 0, "right": 600, "top": 800 },
            "boxes": { "media": bx, "crop": bx, "bleed": bx, "trim": bx, "art": bx } }] } } }))
    .respond("forms.list", |_| {
        let name = NAME.with(|n| n.borrow().clone());
        Ok(Reply::value(json!({
            "formKind": "acroform", "needsAppearances": false, "calculationOrder": [],
            "fields": [text(&name)], "widgets": [row()],
        })))
    })
    .respond("apply", |args| {
        SENT.with(|s| s.borrow_mut().push(args[0]["ops"].clone()));
        let value = args[0]["ops"][0]["value"]["value"].as_str().unwrap_or_default().to_string();
        NAME.with(|n| *n.borrow_mut() = value.clone());
        let field = text(&value);
        let op_id = args[1]["opId"].clone();
        let meta = json!({ "affectedPages": [page()], "cacheDelta": null, "opId": op_id, "undoable": true,
            "changedFields": [field["ref"]], "changedWidgets": field["widgets"], "redrawnWidgets": field["widgets"] });
        let event = json!({ "type": "forms.valueSet", "field": field, "widgets": [], "meta": meta,
            "origin": { "kind": "local", "tx": { "id": op_id, "index": 0, "count": 1 } } });
        Ok(Reply {
            value: json!({ "items": [{ "type": "forms.setValue", "field": field, "widgets": [], "meta": meta }], "meta": meta }),
            events: vec![event],
        })
    })
}

fn handle() -> (KernelThread<TestKernel>, Arc<FormHandle>) {
    let thread = KernelThread::spawn(|_| {
        TestKernel::open(
            vec![
                interaction_plugin(InteractionConfig::default()),
                form_plugin(FormConfig { validation: Some(FormValidation::None) }.into()),
            ],
            spec(),
        )
    })
    .unwrap();
    let post = thread.post();
    let handle =
        block_on(thread.call(move |k| FormHandle::new(k.capability(&FORM).unwrap(), post)))
            .unwrap();
    (thread, handle)
}

#[test]
fn the_snapshot_is_typed() {
    let (_thread, form) = handle();
    let snapshot = block_on(form.snapshot()).unwrap().expect("read");
    let field = &snapshot.fields[0];
    assert_eq!(field.name, "name");
    assert_eq!(field.alternate_name.as_deref(), Some("Your name"));
    assert!(
        matches!(&field.detail, FormFieldDetail::Text { value, max_length: Some(40), .. } if value == "Ada")
    );
    assert_eq!(field.widgets[0].page, Some(PageRef { page_object_number: 1 }));
}

#[derive(Default)]
struct Events(Mutex<Vec<FormEvent>>, AtomicUsize);

impl FormEventListener for Events {
    fn on_event(&self, event: FormEvent) {
        self.0.lock().unwrap().push(event);
    }
    fn on_end(&self) {
        self.1.fetch_add(1, Ordering::SeqCst);
    }
}

fn code(error: &ViewerError) -> Option<&str> {
    match error {
        ViewerError::Kernel { code, .. } => code.as_deref(),
        ViewerError::Closed => None,
    }
}

fn text_ref() -> FormFieldRef {
    FormFieldRef::ObjectNumber { field_object_number: 1 }
}

#[test]
fn a_value_is_written_and_announced() {
    let (_thread, form) = handle();
    let events = Arc::new(Events::default());
    let _subscription = form.subscribe(events.clone());
    let field_ref = FormFieldRef::ObjectNumber { field_object_number: 1 };
    let result = block_on(form.set_value(
        field_ref.clone(),
        FormValueInput::Value { value: Some("Grace".into()) },
        CancelToken::new(),
    ))
    .unwrap();
    assert_eq!(result.status, FormSetValueStatus::Applied);
    assert!(
        matches!(&result.field.detail, FormFieldDetail::Text { value, .. } if value == "Grace")
    );
    let value = block_on(form.value(field_ref)).unwrap();
    assert_eq!(value, Some(FormValueInput::Value { value: Some("Grace".into()) }));
    let events = events.0.lock().unwrap();
    assert!(events.iter().any(|e| matches!(e, FormEvent::ValueChanged { .. })), "{events:?}");
}

#[test]
fn refresh_reads_the_form_again_and_reports_ready() {
    let (_thread, form) = handle();
    block_on(form.refresh(CancelToken::new())).unwrap();
    assert_eq!(block_on(form.status()).unwrap(), ResourceStatus::Ready);
    let snapshot = block_on(form.snapshot()).unwrap().expect("read");
    assert_eq!(snapshot.form_kind, FormKind::Acroform);
    let fields = block_on(form.fields(None)).unwrap();
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].name, "name");
}

#[test]
fn the_widget_projection_draws_each_widget_as_a_control() {
    let (_thread, form) = handle();
    block_on(form.ensure_loaded(PageRef { page_object_number: 1 }, CancelToken::new())).unwrap();
    let items = block_on(form.widgets(PageRef { page_object_number: 1 })).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].annot_object_number, 101);
    assert!(items[0].annotation_ref.is_some());
    assert!(
        matches!(&items[0].control, FormFillControl::Text { value, max_length: Some(40), .. } if value == "Ada"),
        "{:?}",
        items[0]
    );
    assert!(!items[0].disabled);
}

#[test]
fn a_closed_documents_capability_refuses_with_instance_closed() {
    let (thread, form) = handle();
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    for error in [
        block_on(form.fields(None)).map(|_| ()).unwrap_err(),
        block_on(form.can_fill(text_ref())).map(|_| ()).unwrap_err(),
        block_on(form.refresh(CancelToken::new())).unwrap_err(),
    ] {
        assert_eq!(code(&error), Some("instance-closed"), "{error:?}");
    }
}

#[test]
fn closing_the_document_ends_its_subscriptions() {
    let (thread, form) = handle();
    let events = Arc::new(Events::default());
    let _subscription = form.subscribe(events.clone());
    block_on(thread.call(|_| ())).unwrap();
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    assert_eq!(events.1.load(Ordering::SeqCst), 1);
}

#[test]
fn cancelling_a_set_value_before_it_runs_leaves_the_field_alone() {
    let (thread, form) = handle();
    block_on(form.refresh(CancelToken::new())).unwrap();
    let (release, gate) = std::sync::mpsc::channel::<()>();
    let (parked_tx, parked) = std::sync::mpsc::channel::<()>();
    let blocker = thread.call(move |_| {
        parked_tx.send(()).unwrap();
        gate.recv().unwrap();
    });
    parked.recv().unwrap();
    let cancel = CancelToken::new();
    let write = form.set_value(
        text_ref(),
        FormValueInput::Value { value: Some("Grace".into()) },
        cancel.clone(),
    );
    cancel.cancel();
    release.send(()).unwrap();
    block_on(blocker).unwrap();
    let error = block_on(write).unwrap_err();
    assert_eq!(code(&error), Some("operation-cancelled"), "{error:?}");
    assert!(sent(&thread).is_empty());
    let value = block_on(form.value(text_ref())).unwrap();
    assert_eq!(value, Some(FormValueInput::Value { value: Some("Ada".into()) }));
}
