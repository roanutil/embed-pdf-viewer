#![cfg(feature = "uniffi")]

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use epdf_ffi::{CancelToken, FormFieldRef, KernelThread, PageRef, ResourceStatus, ViewerError};
use epdf_plugin_form::ffi::{
    FormCommitStatus, FormEvent, FormEventListener, FormFieldDetail, FormFillControl, FormHandle,
    FormKind, FormValueInput,
};
use epdf_plugin_form::{FORM, FormConfig, Validation, form_plugin};
use epdf_plugin_interaction::{InteractionConfig, interaction_plugin};
use futures::executor::block_on;
use plugin_conformance::test_kernel::{Reply, TestKernel, TestSpec};
use serde_json::{Value, json};

fn page() -> Value {
    json!({ "kind": "objectNumber", "pageObjectNumber": 1 })
}

fn widget(annot: u32) -> Value {
    json!({ "ref": { "kind": "objectNumber", "page": page(), "annotObjectNumber": annot },
        "annotObjectNumber": annot, "page": page() })
}

fn flags(read_only: bool) -> Value {
    json!({ "readOnly": read_only, "requiredValue": false, "required": false, "noExport": false, "raw": 0 })
}

/// A text field and a checkbox, in the kernel's spelling
/// (`crates/plugins/form/tests/api.rs`).
fn snapshot(name: &str, checked: bool) -> Value {
    json!({ "formKind": "acroform", "needsAppearances": false, "calculationOrder": [], "fields": [
        { "ref": { "kind": "objectNumber", "fieldObjectNumber": 1 }, "fieldObjectNumber": 1,
          "name": "name", "family": "text", "origin": "acroform", "flags": flags(false),
          "alternateName": "Your name", "mappingName": null,
          "valueEntry": { "kind": "scalar", "value": name }, "defaultValueEntry": { "kind": "none" },
          "value": name, "defaultValue": "", "maxLength": 40, "multiline": false, "password": false,
          "comb": false, "widgets": [widget(101)] },
        { "ref": { "kind": "objectNumber", "fieldObjectNumber": 2 }, "fieldObjectNumber": 2,
          "name": "agree", "family": "checkbox", "origin": "acroform", "flags": flags(false),
          "alternateName": null, "mappingName": null,
          "valueEntry": { "kind": "scalar", "value": if checked { "On" } else { "Off" } },
          "defaultValueEntry": { "kind": "none" },
          "checked": checked, "exportValue": "Yes",
          "widgets": [{ "ref": { "kind": "objectNumber", "page": page(), "annotObjectNumber": 102 },
            "annotObjectNumber": 102, "page": page(), "onState": "On", "exportValue": "Yes",
            "checked": checked }] }
    ] })
}

thread_local! {
    /// What `forms.setValue` was sent, on the kernel thread. `Rc` can't
    /// cross to the test thread, so tests read it with `sent(&thread)`.
    static SENT: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
}

fn sent(thread: &KernelThread<TestKernel>) -> Vec<Value> {
    block_on(thread.call(|_| SENT.with(|s| s.borrow().clone()))).unwrap()
}

fn spec() -> TestSpec {
    let bx = json!({ "left": 0, "bottom": 0, "right": 600, "top": 800 });
    let state = Rc::new(RefCell::new(("Ada".to_string(), false)));
    let (read, write) = (state.clone(), state.clone());
    TestSpec::new(json!({ "documents": { "a": { "id": "a", "security": { "allows": "*" },
        "pages": [{ "index": 0, "ref": page(), "label": null, "size": { "width": 600, "height": 800 },
            "rotation": 0, "userUnit": 1, "boxes": { "media": bx, "crop": bx } }] } } }))
        .respond("forms.list", move |_| {
            let (name, checked) = read.borrow().clone();
            Ok(Reply::value(snapshot(&name, checked)))
        })
        .respond("forms.setValue", move |args| {
            SENT.with(|s| s.borrow_mut().push(json!(args)));
            let mut s = write.borrow_mut();
            match args[1]["type"].as_str() {
                Some("text") => s.0 = args[1]["value"].as_str().unwrap().into(),
                Some("toggle") => s.1 = args[1]["state"] == "On",
                _ => {}
            }
            // A real engine publishes the mutation's event before it answers.
            let field = snapshot(&s.0, s.1)["fields"][0].clone();
            let event = json!({ "type": "form.valueChanged", "origin": { "kind": "local" }, "field": field });
            Ok(Reply { value: json!({ "changedWidgets": [101] }), events: vec![event] })
        })
        // Widget geometry, which `fill_items` projects through.
        .respond_page("page.annotations.list", |_, _| {
            let dto = |annot: u32, top: f64| json!({ "subtype": "widget",
                "ref": { "kind": "objectNumber", "page": page(), "annotObjectNumber": annot },
                "rect": { "left": 10, "bottom": top - 20.0, "right": 200, "top": top } });
            Ok(Reply::value(json!({ "annotations": [dto(101, 700.0), dto(102, 650.0)] })))
        })
        .respond("forms.exportData", |args| {
            Ok(Reply::value(json!({ "format": args[0], "bytes": { "$bytes": "JUZERi0xLjI=" } })))
        })
        .respond("forms.importData", |_| Ok(Reply::value(json!({ "snapshot": snapshot("Imported", true) }))))
}

fn handle() -> (KernelThread<TestKernel>, Arc<FormHandle>) {
    let thread = KernelThread::spawn(|_| {
        TestKernel::open(
            vec![
                interaction_plugin(InteractionConfig::default()),
                // No actions plugin here, so commits go straight to the engine.
                form_plugin(FormConfig {
                    validation: Some(Validation::None),
                    ..Default::default()
                }),
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

fn code(error: &ViewerError) -> Option<&str> {
    match error {
        ViewerError::Kernel { code, .. } => code.as_deref(),
        ViewerError::Closed => None,
    }
}

fn token() -> Arc<CancelToken> {
    CancelToken::new()
}

fn text_ref() -> FormFieldRef {
    FormFieldRef::ObjectNumber { field_object_number: 1 }
}

fn box_ref() -> FormFieldRef {
    FormFieldRef::ObjectNumber { field_object_number: 2 }
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

#[test]
fn refresh_reads_the_fields_as_typed_records() {
    let (_thread, form) = handle();
    block_on(form.refresh(token())).unwrap();
    assert_eq!(block_on(form.status()).unwrap(), ResourceStatus::Ready);
    let snapshot = block_on(form.snapshot()).unwrap().unwrap();
    assert_eq!(snapshot.form_kind, FormKind::Acroform);
    let fields = block_on(form.fields(None)).unwrap();
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].name, "name");
    assert_eq!(fields[0].alternate_name.as_deref(), Some("Your name"));
    match &fields[0].detail {
        FormFieldDetail::Text { value, max_length, .. } => {
            assert_eq!(value, "Ada");
            assert_eq!(*max_length, Some(40));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(fields[0].widgets[0].annot_object_number, 101);
}

#[test]
fn fill_items_project_each_widget_as_a_control() {
    let (_thread, form) = handle();
    block_on(form.refresh(token())).unwrap();
    block_on(form.ensure_loaded(PageRef { page_object_number: 1 })).unwrap();
    let items = block_on(form.fill_items(PageRef { page_object_number: 1 })).unwrap();
    assert_eq!(items.len(), 2);
    assert!(matches!(items[0].control, FormFillControl::Text { .. }), "{:?}", items[0]);
    assert!(matches!(items[1].control, FormFillControl::Toggle { .. }), "{:?}", items[1]);
    assert!(items[0].annotation_ref.is_some());
}

#[test]
fn setting_text_is_observed() {
    let (_thread, form) = handle();
    block_on(form.refresh(token())).unwrap();
    let events = Arc::new(Events::default());
    let _sub = form.subscribe(events.clone());
    let result = block_on(form.set_text(text_ref(), "Grace".into(), token())).unwrap();
    assert_eq!(result.status, FormCommitStatus::Applied);
    assert_eq!(
        block_on(form.value(text_ref())).unwrap(),
        Some(FormValueInput::Text { value: "Grace".into() })
    );
    let seen = events.0.lock().unwrap();
    assert!(seen.iter().any(|e| matches!(e, FormEvent::ValueChanged { .. })), "{seen:?}");
}

#[test]
fn checking_a_box_by_its_on_state_reports_applied() {
    let (thread, form) = handle();
    block_on(form.refresh(token())).unwrap();
    let result = block_on(form.set_checked(box_ref(), Some("On".into()), token())).unwrap();
    assert_eq!(result.status, FormCommitStatus::Applied);
    assert_eq!(sent(&thread).last().unwrap()[1], json!({ "type": "toggle", "state": "On" }));
}

#[test]
fn an_export_answers_its_bytes() {
    let (_thread, form) = handle();
    block_on(form.refresh(token())).unwrap();
    let export =
        block_on(form.export_data(Some(epdf_plugin_form::ffi::FormDataFormat::Fdf), token()))
            .unwrap();
    assert_eq!(export.format, epdf_plugin_form::ffi::FormDataFormat::Fdf);
    assert_eq!(export.bytes, b"%FDF-1.2".to_vec());
}

#[test]
fn an_import_replaces_the_snapshot() {
    let (_thread, form) = handle();
    block_on(form.refresh(token())).unwrap();
    block_on(form.import_data(b"%FDF-1.2".to_vec(), None, token())).unwrap();
    let fields = block_on(form.fields(None)).unwrap();
    assert!(
        matches!(&fields[0].detail, FormFieldDetail::Text { value, .. } if value == "Imported")
    );
}

#[test]
fn a_closed_documents_capability_refuses_with_instance_closed() {
    let (thread, form) = handle();
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    for error in [
        block_on(form.fields(None)).map(|_| ()).unwrap_err(),
        block_on(form.can_fill()).map(|_| ()).unwrap_err(),
        block_on(form.refresh(token())).unwrap_err(),
    ] {
        assert_eq!(code(&error), Some("instance-closed"), "{error:?}");
    }
}

#[test]
fn closing_the_document_ends_its_subscriptions() {
    let (thread, form) = handle();
    let events = Arc::new(Events::default());
    let _sub = form.subscribe(events.clone());
    block_on(thread.call(|_| ())).unwrap();
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    assert_eq!(events.1.load(Ordering::SeqCst), 1);
}

#[test]
fn cancelling_a_set_value_before_it_runs_leaves_the_field_alone() {
    let (thread, form) = handle();
    block_on(form.refresh(token())).unwrap();
    let (release, gate) = std::sync::mpsc::channel::<()>();
    let (parked_tx, parked) = std::sync::mpsc::channel::<()>();
    let blocker = thread.call(move |_| {
        parked_tx.send(()).unwrap();
        gate.recv().unwrap();
    });
    parked.recv().unwrap();
    let cancel = token();
    let write = form.set_text(text_ref(), "Grace".into(), cancel.clone());
    cancel.cancel();
    release.send(()).unwrap();
    block_on(blocker).unwrap();
    let error = block_on(write).unwrap_err();
    assert_eq!(code(&error), Some("operation-cancelled"), "{error:?}");
    assert!(sent(&thread).is_empty());
}
