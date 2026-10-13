//! The model shares its snapshot and indexes across updates that leave them
//! alone, the way the TypeScript shares them by reference, so a large form
//! does not pay a full copy per keystroke.

use std::rc::Rc;
use std::time::Instant;

use epdf_plugin_form::{Model, Msg, field_by_key, initial_model, update};
use serde_json::{Value, json};

const FIELDS: usize = 5_000;

fn field(i: usize, value: &str) -> Value {
    json!({
        "fieldObjectNumber": i + 1,
        "name": format!("f{i}"),
        "family": "text",
        "valueEntry": { "kind": "scalar", "value": value },
        "widgets": [{ "annotObjectNumber": 10_000 + i, "page": { "pageObjectNumber": 1 } }],
    })
}

fn loaded() -> Rc<Model> {
    let fields: Vec<Value> = (0..FIELDS).map(|i| field(i, "")).collect();
    let snapshot = json!({ "revision": 1, "fields": fields, "signatures": [] });
    update(&Rc::new(initial_model()), &Msg::Snapshot { snapshot: Rc::new(snapshot) })
}

#[test]
fn updates_that_leave_the_snapshot_alone_share_it() {
    let model = loaded();
    let key = "obj:7".to_string();
    let steps = [
        Msg::WriteStart { key: key.clone() },
        Msg::WriteFailed { key: key.clone() },
        Msg::PageGeom { page_object_number: 1.0, boxes: Default::default() },
        Msg::ClearGeom { page_object_number: Some(1.0) },
        Msg::ClearGeom { page_object_number: None },
    ];
    let mut current = model.clone();
    for msg in &steps {
        let next = update(&current, msg);
        assert!(Rc::ptr_eq(next.snapshot.as_ref().unwrap(), model.snapshot.as_ref().unwrap()));
        assert!(Rc::ptr_eq(&next.by_key, &model.by_key));
        assert!(Rc::ptr_eq(&next.by_widget, &model.by_widget));
        current = next;
    }
}

#[test]
fn a_write_back_replaces_one_field_in_place() {
    let model = loaded();
    let key = "obj:7".to_string();
    let writing = update(&model, &Msg::WriteStart { key: key.clone() });
    let done = update(&writing, &Msg::WriteDone { key: key.clone(), field: field(6, "typed") });
    let snapshot = done.snapshot.as_ref().unwrap();
    // `{ ...snapshot, fields }` keeps the member order.
    let keys: Vec<&String> = snapshot.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["revision", "fields", "signatures"]);
    assert_eq!(field_by_key(&done, &key), Some(field(6, "typed")));
    assert_eq!(field_by_key(&done, "obj:8"), Some(field(7, "")));
    assert!(done.writing.is_empty());
    // The model it came from still holds the old field.
    assert_eq!(field_by_key(&model, &key), Some(field(6, "")));
}

#[test]
fn a_thousand_status_updates_on_a_large_form_stay_fast() {
    let model = loaded();
    let started = Instant::now();
    let mut current = model.clone();
    for i in 0..1_000 {
        let key = format!("obj:{}", i % FIELDS + 1);
        current = update(&current, &Msg::WriteStart { key: key.clone() });
        current = update(&current, &Msg::WriteFailed { key });
    }
    // One write-back still copies the fields array once.
    current = update(&current, &Msg::WriteStart { key: "obj:1".into() });
    current = update(&current, &Msg::WriteDone { key: "obj:1".into(), field: field(0, "x") });
    assert_eq!(field_by_key(&current, "obj:1"), Some(field(0, "x")));
    let elapsed = started.elapsed();
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}
