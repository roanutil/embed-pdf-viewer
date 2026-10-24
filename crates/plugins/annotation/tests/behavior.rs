#![recursion_limit = "256"]

//! Behaviour the vectors don't pin: kept Rust-only divergences and safety
//! guards.
//!
//! H3: a record keeps its key from its first frame (no temp id to rename),
//! and one that leaves the view mid-gesture leaves the session in the same
//! notification, before any later pointer sample can reach it.
//!
//! N6: a ref of a kind no key is defined for is refused, where the
//! TypeScript keys it `"undefined"` and collapses every such record into one.

use std::rc::Rc;

use epdf_geometry::Point;
use epdf_kernel::kernel::OpenSource;
use epdf_kernel::{Kernel, KernelOptions, OpenOptions, rt};
use epdf_plugin_annotation::write::crud::CreateOptions;
use epdf_plugin_annotation::{ANNOTATION, AnnotationCapability, annotation_plugin};
use epdf_plugin_interaction::{InteractionConfig, interaction_plugin};
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use serde_json::{Map, Value, json};

const PAGE: &str = r#"{ "kind": "objectNumber", "objectNumber": 1 }"#;

fn page() -> Value {
    serde_json::from_str(PAGE).unwrap()
}

fn page_row() -> Value {
    json!({
        "index": 0, "ref": page(), "label": null,
        "size": { "width": 600, "height": 800 }, "rotation": 0, "userUnit": 1, "tabOrder": null,
        "pdfCropBox": { "left": 0, "bottom": 0, "right": 600, "top": 800 },
        "boxes": {
            "media": { "x": 0, "y": 0, "width": 600, "height": 800 },
            "crop": { "x": 0, "y": 0, "width": 600, "height": 800 },
            "bleed": { "x": 0, "y": 0, "width": 600, "height": 800 },
            "trim": { "x": 0, "y": 0, "width": 600, "height": 800 },
            "art": { "x": 0, "y": 0, "width": 600, "height": 800 }
        }
    })
}

fn square_ref(object_number: u32) -> Value {
    json!({ "kind": "objectNumber", "page": page(), "objectNumber": object_number })
}

/// A confirmed square at `x, y`, 100 by 100, as the engine reads it.
fn square(object_number: u32, x: f64, y: f64) -> Value {
    json!({
        "subtype": "square", "ref": square_ref(object_number), "page": page(),
        "hasAppearance": true, "appearanceState": null, "nm": null,
        "rect": { "x": x, "y": y, "width": 100, "height": 100 },
        "contents": null, "subject": null, "blendMode": "normal",
        "invisible": false, "hidden": false, "print": true, "noZoom": false, "noRotate": false,
        "noView": false, "readOnly": false, "locked": false, "toggleNoView": false,
        "lockedContents": false, "reply": null, "popup": null, "groupId": null, "author": null,
        "createdAt": null, "modifiedAt": null, "userId": null, "createdBy": null,
        "modifiedBy": null, "importedBy": null, "actions": null, "color": "#ef4444", "opacity": 1,
        "strokeWidth": 2, "borderStyle": "solid", "dashArray": null, "interiorColor": null,
        "box": { "x": x, "y": y, "width": 100, "height": 100 }, "rotation": null,
        "cloudyIntensity": null
    })
}

/// A kernel with the interaction hub and the annotation plugin over the
/// conformance fake engine; the document's annotations are `listed`, and
/// `responses` are tried before the list.
fn kernel(listed: Vec<Value>, responses: Vec<Value>) -> (Kernel, Rc<FakeEngine>) {
    rt::reset();
    let mut all = responses;
    all.push(json!({ "path": "annotations.list", "value": { "annotations": listed } }));
    let fake = FakeEngine::new(json!({
        "documents": { "a": {
            "pages": [page_row()],
            "security": { "allows": "*" },
            "objectNumbers": [10001, 10002, 10003],
            "responses": all
        } }
    }));
    let kernel = Kernel::new(
        Rc::new(AsEngine(fake.clone())),
        vec![interaction_plugin(InteractionConfig::default()), annotation_plugin(None)],
        KernelOptions { report: None, on_dispatch: None, release_closed_sessions: false },
    )
    .unwrap();
    let _ = kernel.start();
    rt::run_until_stalled();
    let _ = kernel.documents().open(
        OpenSource::Input(json!({ "kind": "bytes", "id": "a", "bytes": { "$bytes": "" } })),
        OpenOptions::default(),
    );
    rt::run_until_stalled();
    (kernel, fake)
}

fn annotation(kernel: &Kernel) -> AnnotationCapability {
    kernel.capability(&ANNOTATION, Some("a")).expect("the annotation capability")
}

fn refs(cap: &AnnotationCapability) -> Vec<Value> {
    cap.list(None).iter().map(|a| a["ref"].clone()).collect()
}

fn pointer(cap: &AnnotationCapability, phase: &str, x: f64, y: f64) {
    cap.edit_pointer(phase, &page(), Point { x, y }, false, None, None, None, None)
        .expect("a pointer sample");
}

/// A remote session deleted these records.
fn deleted(refs: Vec<Value>) -> Value {
    json!({
        "type": "annotations.deleted", "page": page(), "deleted": refs,
        "origin": { "kind": "remote", "sessionId": "s2", "sub": "u-9", "ts": 0, "serverId": null },
        "meta": { "affectedPages": [page()], "cacheDelta": null, "changed": [], "opId": "remote-delete", "undoable": true }
    })
}

/// A remote session created this record.
fn created(record: Value) -> Value {
    json!({
        "type": "annotations.created", "page": page(), "annotation": record,
        "origin": { "kind": "remote", "sessionId": "s2", "sub": "u-9", "ts": 0, "serverId": null },
        "meta": { "affectedPages": [page()], "cacheDelta": null, "changed": [], "opId": "remote-create", "undoable": true }
    })
}

// ── H3 ──────────────────────────────────────────────────────────────────────

#[test]
fn h3_a_created_record_keeps_its_key_before_and_after_the_engine_confirms_it() {
    let op_id = "00000000-0000-7000-8000-000000000001";
    // The engine's answer, and the event it publishes with it.
    let event = json!({
        "type": "annotations.created", "page": page(), "annotation": square(10001, 100.0, 100.0),
        "meta": { "affectedPages": [page()], "cacheDelta": null, "opId": op_id, "undoable": true,
                  "changed": [square_ref(10001)] },
        "origin": { "kind": "local", "sessionId": "s1", "sub": null, "ts": 0, "serverId": null,
                    "tx": { "id": op_id, "index": 0, "count": 1 } }
    });
    let (kernel, fake) = kernel(
        Vec::new(),
        vec![json!({ "path": "apply", "times": 1, "defer": "born", "events": [event] })],
    );
    let cap = annotation(&kernel);
    let mut fields = Map::new();
    fields.insert("subtype".into(), json!("square"));
    fields.insert("box".into(), json!({ "x": 100, "y": 100, "width": 100, "height": 100 }));
    let made = cap.create(&page(), &fields, None, CreateOptions::default());
    rt::run_until_stalled();
    // Shown at once, under the object number the engine handed out.
    assert_eq!(refs(&cap), vec![square_ref(10001)]);
    let confirmed = json!({
        "items": [{
            "type": "annotations.create", "page": page(), "annotation": square(10001, 100.0, 100.0),
            "meta": { "affectedPages": [page()], "cacheDelta": null, "opId": op_id, "undoable": true,
                      "changed": [square_ref(10001)] }
        }],
        "meta": { "affectedPages": [page()], "cacheDelta": null, "opId": op_id, "undoable": true }
    });
    assert!(fake.settle("born", Ok(confirmed)));
    rt::run_until_stalled();
    // The same key once confirmed: nothing to rename.
    assert_eq!(refs(&cap), vec![square_ref(10001)]);
    assert_eq!(
        made.peek().expect("settled").expect("created")["annotation"]["ref"],
        square_ref(10001)
    );
}

#[test]
fn h3_a_record_deleted_mid_drag_leaves_the_session_before_the_next_sample() {
    let (kernel, fake) = kernel(vec![square(7, 100.0, 100.0)], Vec::new());
    let cap = annotation(&kernel);
    cap.selection_set(&[square_ref(7)]);
    // On its left edge: a square with no fill is hit on its stroke.
    pointer(&cap, "down", 100.0, 150.0);
    pointer(&cap, "move", 110.0, 160.0);
    assert!(cap.model().draft().is_some(), "dragging");
    fake.emit("a", &deleted(vec![square_ref(7)])).expect("the event");
    // No turn has run: the forget is part of the notification itself.
    assert!(cap.model().draft().is_none(), "the drag went with its record");
    assert!(cap.list_selected().is_empty());
    // The rest of the gesture finds nothing to move and writes nothing.
    let before = fake.log_len();
    pointer(&cap, "move", 130.0, 180.0);
    pointer(&cap, "up", 130.0, 180.0);
    rt::run_until_stalled();
    assert!(refs(&cap).is_empty());
    let applied = fake.log_since(before).iter().filter(|call| call["path"] == "apply").count();
    assert_eq!(applied, 0, "no write for a record that is gone");
}

// ── N6 ──────────────────────────────────────────────────────────────────────

#[test]
fn n6_a_record_whose_ref_has_no_key_is_refused_and_the_others_stay() {
    let (kernel, fake) = kernel(vec![square(7, 100.0, 100.0)], Vec::new());
    let cap = annotation(&kernel);
    let mut stray = square(8, 300.0, 300.0);
    stray["ref"] = json!({ "kind": "bogus", "page": page(), "objectNumber": 8 });
    let mut other = square(9, 400.0, 400.0);
    other["ref"] = json!({ "kind": "bogus", "page": page(), "objectNumber": 9 });
    let _ = fake.emit("a", &created(stray));
    let _ = fake.emit("a", &created(other));
    rt::run_until_stalled();
    // TypeScript keys both "undefined", so the second replaces the first.
    // Rust keys neither: the list holds only the record it could key.
    assert_eq!(refs(&cap), vec![square_ref(7)]);
    assert!(cap.get(&json!({ "kind": "bogus", "page": page(), "objectNumber": 8 })).is_none());
    assert!(cap.get(&square_ref(7)).is_some());
}
