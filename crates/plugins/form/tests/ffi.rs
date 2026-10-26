#![cfg(feature = "uniffi")]

//! The uniffi handle over the current plugin. Migrated by hand in Task 13.18
//! with `src/ffi.rs`; not compiled until `epdf-ffi` builds (Task 13.33).

use std::cell::RefCell;
use std::sync::{Arc, Mutex};

use epdf_ffi::{CancelToken, FormFieldRef, KernelThread, PageRef};
use epdf_plugin_form::ffi::{
    FormConfig, FormEvent, FormEventListener, FormFieldDetail, FormHandle, FormSetValueStatus,
    FormValidation, FormValueInput,
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

thread_local! {
    static NAME: RefCell<String> = RefCell::new("Ada".into());
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
            "fields": [text(&name)], "widgets": [],
        })))
    })
    .respond("apply", |args| {
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

struct Events(Mutex<Vec<FormEvent>>);

impl FormEventListener for Events {
    fn on_event(&self, event: FormEvent) {
        self.0.lock().unwrap().push(event);
    }
    fn on_end(&self) {}
}

#[test]
fn a_value_is_written_and_announced() {
    let (_thread, form) = handle();
    let events = Arc::new(Events(Mutex::new(Vec::new())));
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
