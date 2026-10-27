//! What the vectors don't pin on their own: gestures whose record leaves the
//! view mid-way (H3), a record edited before the engine confirms it, a
//! measurement's label and caption following an edit, the kept refusals
//! (an unknown ref kind, malformed kinds and fields), and sharing of
//! untouched records between steps. Records are made the way the plugin
//! makes them: by `update` messages over a session holding object numbers.

use std::rc::Rc;

use epdf_annotation_core::kinds::kind_named;
use epdf_annotation_core::record::{annotation_key, irt_of, shape_of, with_values};
use epdf_annotation_core::update::initial_model;
use epdf_annotation_core::*;
use serde_json::{Value, json};

fn message(value: Value) -> Message {
    Message::read(&value).expect("a message")
}

fn page() -> Value {
    json!({ "kind": "objectNumber", "objectNumber": 1 })
}

fn input(x: f64, y: f64) -> Value {
    json!({ "page": page(), "point": { "x": x, "y": y }, "shift": false })
}

fn edit(phase: &str, x: f64, y: f64) -> Message {
    message(json!({ "type": "editPointer", "phase": phase, "in": input(x, y) }))
}

fn create(subtype: &str, phase: &str, x: f64, y: f64) -> Message {
    message(
        json!({ "type": "createPointer", "phase": phase, "subtype": subtype, "in": input(x, y) }),
    )
}

/// The model after a result, as the plugin lays a change set on its records
/// (`test/support.ts`'s `apply`).
fn apply(model: &Model, result: &UpdateResult) -> Model {
    let mut by_id = (*model.by_id).clone();
    for id in &result.change.drop {
        by_id.shift_remove(id);
    }
    let added: Vec<Id> = result
        .change
        .put
        .iter()
        .map(|r| r.id.clone())
        .filter(|id| !model.by_id.contains_key(id))
        .collect();
    for record in &result.change.put {
        by_id.insert(record.id.clone(), record.clone());
    }
    let mut order: Vec<Id> =
        model.order.iter().filter(|id| by_id.contains_key(*id)).cloned().collect();
    order.extend(added);
    Model::of(result.session.clone(), Rc::new(by_id), Rc::new(order))
}

fn step(model: &Model, msg: &Message) -> (Model, UpdateResult) {
    let result = update(model, msg).expect("an update");
    (apply(model, &result), result)
}

fn run(model: &Model, msgs: &[Message]) -> Model {
    msgs.iter().fold(model.clone(), |m, msg| step(&m, msg).0)
}

fn session_with_numbers() -> Model {
    Model { object_numbers: Rc::new((100..110).map(f64::from).collect()), ..initial_model() }
}

/// A square drawn from (100, 100) to (180, 160), selected.
fn drawn_square() -> (Model, Id) {
    let m = run(
        &session_with_numbers(),
        &[
            create("square", "down", 100.0, 100.0),
            create("square", "move", 180.0, 160.0),
            create("square", "up", 180.0, 160.0),
        ],
    );
    let id = m.order[0].clone();
    (m, id)
}

// ── H3: a record that leaves the view mid-gesture ───────────────────────────

#[test]
fn forgetting_a_record_mid_move_ends_the_gesture_and_its_references() {
    let (m, id) = drawn_square();
    let moving = run(&m, &[edit("down", 100.5, 115.0), edit("move", 110.5, 134.0)]);
    assert!(matches!(moving.draft(), Some(Draft::Move(_))), "a move is in progress");
    let hovered = Model { hovered: Some(id.clone()), editing: Some(id.clone()), ..moving };

    let (gone, result) = step(&hovered, &message(json!({ "type": "forget", "ids": [id] })));
    assert!(gone.draft.is_none(), "the gesture on the record ended");
    assert!(gone.selected.is_empty() && gone.hovered.is_none() && gone.editing.is_none());
    // `forget` changes the session only: the records are the plugin's.
    assert!(result.change.is_empty() && result.effects.is_empty());

    // The release that follows writes nothing for the record that left.
    let release = update(&gone, &edit("up", 120.5, 144.0)).expect("an update");
    assert!(release.effects.is_empty() && release.change.is_empty());
}

#[test]
fn forgetting_a_record_mid_resize_and_mid_turn_ends_each_gesture() {
    let (m, id) = drawn_square();
    // The south-east handle of the box (100, 100, 80, 60).
    let resizing = run(&m, &[edit("down", 180.0, 160.0)]);
    assert!(matches!(resizing.draft(), Some(Draft::Handle(_))));
    let forget = message(json!({ "type": "forget", "ids": [id] }));
    assert!(step(&resizing, &forget).0.draft.is_none());

    // The rotate knob hangs 24 above the top edge's middle.
    let turning = run(&m, &[edit("down", 140.0, 76.0)]);
    assert!(matches!(turning.draft(), Some(Draft::Rotate(_))));
    assert!(step(&turning, &forget).0.draft.is_none());
}

#[test]
fn forgetting_another_record_keeps_the_gesture_and_the_session() {
    let (m, _) = drawn_square();
    let moving = run(&m, &[edit("down", 100.5, 115.0)]);
    let result = update(&moving, &message(json!({ "type": "forget", "ids": ["obj:999"] })))
        .expect("an update");
    assert!(result.session.same(&moving.session()), "nothing to forget is the same session");
}

// ── a record edited before the engine confirms it ───────────────────────────

#[test]
fn a_new_record_is_editable_before_the_engine_confirms_it() {
    let (m, id) = drawn_square();
    let record = m.record(&id).expect("the new square");
    // Its key is final from the start: the object number its create takes.
    assert_eq!(id, "obj:100");
    assert_eq!(record.unconfirmed, Some(true));
    assert_eq!(
        annotation_key(record.annotation.get("ref").expect("a ref")).as_deref(),
        Some("obj:100")
    );

    // Dragged before any answer: the write names the same record.
    let dragged = run(&m, &[edit("down", 100.5, 115.0), edit("move", 110.5, 125.0)]);
    let release = update(&dragged, &edit("up", 110.5, 125.0)).expect("an update");
    let patch = release
        .effects
        .iter()
        .find_map(|e| match e {
            Effect::Patch(p) => Some(p),
            _ => None,
        })
        .expect("a patch");
    assert_eq!(patch.id, id);
    let moved = &release.change.put[0];
    assert_eq!(moved.id, id);
    assert_eq!(moved.unconfirmed, Some(true), "still the unconfirmed record, moved");
    match shape_of(&moved.annotation) {
        Shape::Box(b) => assert_eq!((b.box_.x, b.box_.y), (110.0, 110.0)),
        other => panic!("a box, not {other:?}"),
    }
}

// ── measurements follow their edits ─────────────────────────────────────────

fn measure() -> Value {
    json!({
        "subtype": "rectilinear",
        "ratio": "1 in = 1 m",
        "x": [{ "unit": "m", "conversion": 0.013888888888888888 }],
        "distance": [{ "unit": "m", "conversion": 1 }],
        "area": [{ "unit": "m²", "conversion": 1 }],
    })
}

/// An area measurement over the triangle (100, 100), (172, 100), (100, 172).
fn area() -> (Model, Id) {
    let measure =
        json!({ "intent": "polygon-dimension", "measure": measure(), "captionEnabled": true });
    let poly = |phase: &str, x: f64, y: f64| {
        message(json!({
            "type": "createPointer",
            "phase": phase,
            "subtype": "polygon",
            "measure": measure,
            "in": input(x, y),
        }))
    };
    let m = run(
        &session_with_numbers(),
        &[poly("down", 100.0, 100.0), poly("down", 172.0, 100.0), poly("down", 100.0, 172.0)],
    );
    let m = run(&m, &[message(json!({ "type": "finishCreationDraft" }))]);
    let id = m.order[0].clone();
    (m, id)
}

#[test]
fn an_area_measurement_states_its_label_and_relabels_when_a_vertex_moves() {
    let (m, id) = area();
    let record = m.record(&id).expect("the area");
    // 72 points is one inch, a metre at this scale: half a square metre.
    assert_eq!(record.annotation.get("contents"), Some(&json!("0.50 m²")));

    // Drag the third vertex down by an inch: the area doubles to a square metre.
    let dragged = run(&m, &[edit("down", 100.0, 172.0), edit("move", 100.0, 244.0)]);
    let release = update(&dragged, &edit("up", 100.0, 244.0)).expect("an update");
    let relabelled = &release.change.put[0];
    assert_eq!(relabelled.annotation.get("contents"), Some(&json!("1.00 m²")));
}

#[test]
fn a_dragged_caption_moves_with_its_shape_and_the_label_stays() {
    let (m, id) = area();
    let record = m.record(&id).expect("the area");
    let style = record::style_of(&record.annotation);
    let shape = shape_of(&record.annotation);
    let appearance = measurement::measurement_of(&record.annotation).expect("a measurement");
    let caption = measurement_shape::shape_measurement_layout(&shape, &appearance, &style)
        .and_then(|l| l.caption)
        .expect("a caption");

    let at = caption.center;
    let dragged = run(&m, &[edit("down", at.x, at.y), edit("move", at.x + 10.0, at.y + 5.0)]);
    assert!(matches!(dragged.draft(), Some(Draft::Caption(_))), "the caption is grabbed");
    let release = update(&dragged, &edit("up", at.x + 10.0, at.y + 5.0)).expect("an update");
    let moved = &release.change.put[0];
    let center = moved.annotation.get("captionCenter").expect("a caption center");
    assert_eq!(center, &json!({ "x": at.x + 10.0, "y": at.y + 5.0 }));
    assert_eq!(moved.annotation.get("contents"), Some(&json!("0.50 m²")));
}

// ── kept refusals ───────────────────────────────────────────────────────────

#[test]
fn a_ref_of_unknown_kind_has_no_key() {
    // N6: the TypeScript keys it "undefined", which would make every such
    // record one entry. Here it has no key, and nothing answers it.
    assert_eq!(annotation_key(&json!({ "kind": "nm", "page": page(), "nm": "x" })), None);
    assert_eq!(annotation_key(&json!({ "objectNumber": 3 })), None);
    assert_eq!(
        annotation_key(&json!({ "kind": "baseIndex", "page": page(), "baseIndex": 2 })).as_deref(),
        Some("base:1:2")
    );
    let reply = json!({ "subtype": "text", "reply": { "to": { "kind": "nm", "nm": "x" }, "type": "reply" } });
    assert_eq!(irt_of(reply.as_object().expect("an object")), None);
    assert!(
        record::from_dto(Rc::new(
            json!({ "subtype": "square", "ref": { "kind": "nm" } }).as_object().cloned().unwrap()
        ))
        .is_none()
    );
}

#[test]
fn a_name_object_prototype_has_is_a_kind_only_shown() {
    // The TypeScript finds `Object.prototype`'s member under these names, and
    // reading it as a kind throws; the core reads them as unknown kinds.
    for name in ["constructor", "toString", "__proto__", "hasOwnProperty"] {
        let kind = kind_named(name);
        assert_eq!(kind.name, name);
        assert!(kind.caps.raster_only && !kind.caps.selectable);
        assert!(props::properties_of(&kind).iter().all(|p| p["control"] == "flag"));
        assert!(!props::kind_takes_link(&kind));
    }
}

#[test]
fn a_write_takes_only_the_fields_its_kind_has_and_refuses_a_rect_with_a_new_shape() {
    let (m, id) = drawn_square();
    let record = m.record(&id).expect("the square");
    let fields = |v: Value| v.as_object().cloned().expect("fields");
    // A field the kind doesn't declare, and `subtype`, are ignored.
    assert_eq!(
        with_values(record, &fields(json!({ "bogus": 1, "subtype": "circle" }))).expect("no error"),
        None
    );
    // A rect is a command: with a changed box it would be a guess.
    let refused = with_values(
        record,
        &fields(json!({ "rect": { "x": 0, "y": 0, "width": 10, "height": 10 }, "box": { "x": 1, "y": 1, "width": 5, "height": 5 } })),
    )
    .expect_err("refused");
    assert_eq!((refused.name, refused.code), ("EngineError", Some("InvalidArg")));
    // A place not in page space is refused before it reaches the file's coordinates.
    let nan = engine::space::to_pdf(&fields(json!({ "subtype": "square", "box": { "x": { "$number": "NaN" }, "y": 0, "width": 1, "height": 1 } })))
        .expect_err("refused");
    assert_eq!(
        nan.message,
        "field 'box': expected page space, { x, y, width, height } from the page's top-left"
    );
}

// ── sharing ─────────────────────────────────────────────────────────────────

#[test]
fn a_message_that_leaves_the_records_alone_changes_none() {
    let (m, id) = drawn_square();
    let result = update(&m, &message(json!({ "type": "hover", "id": id }))).expect("an update");
    assert!(result.change.is_empty());
    let next = apply(&m, &result);
    assert!(Rc::ptr_eq(
        m.by_id.values().next().expect("one"),
        next.by_id.values().next().expect("one")
    ));
    // The same hover again is the same session.
    let again =
        update(&next, &message(json!({ "type": "hover", "id": next.hovered }))).expect("an update");
    assert!(again.session.same(&next.session()));
}

// ── a gesture whose record is gone without a forget ─────────────────────────

/// The model with `id` gone from the records but not from the session, as a
/// host that drops a record without sending `forget` leaves it.
fn without_record(model: &Model, id: &str) -> Model {
    let mut by_id = (*model.by_id).clone();
    by_id.shift_remove(id);
    let order: Vec<Id> = model.order.iter().filter(|o| *o != id).cloned().collect();
    Model::of(model.session(), Rc::new(by_id), Rc::new(order))
}

fn assert_type_error(result: CoreResult<UpdateResult>) {
    let Err(error) = result else { panic!("the TypeScript throws") };
    assert_eq!(error.name, "TypeError");
    assert_eq!(error.message, "Cannot read properties of undefined (reading 'annotation')");
}

#[test]
fn releasing_a_move_on_a_record_that_left_throws_the_typescript_error() {
    let (m, id) = drawn_square();
    let moving = run(&m, &[edit("down", 100.5, 115.0), edit("move", 110.5, 134.0)]);
    assert!(matches!(moving.draft(), Some(Draft::Move(_))));
    assert_type_error(update(&without_record(&moving, &id), &edit("up", 110.5, 134.0)));
}

#[test]
fn releasing_a_resize_on_a_record_that_left_throws_the_typescript_error() {
    let (m, id) = drawn_square();
    let resizing = run(&m, &[edit("down", 180.0, 160.0), edit("move", 200.0, 190.0)]);
    assert!(matches!(resizing.draft(), Some(Draft::Handle(_))));
    assert_type_error(update(&without_record(&resizing, &id), &edit("up", 200.0, 190.0)));
}

// ── malformed messages and kinds ────────────────────────────────────────────

#[test]
fn a_message_of_unknown_type_or_phase_does_not_read() {
    assert!(Message::read(&json!({ "type": "bogus" })).is_err());
    assert!(
        Message::read(
            &json!({ "type": "editPointer", "phase": "sideways", "in": input(1.0, 1.0) })
        )
        .is_err()
    );
    assert!(Message::read(&json!("hover")).is_err());
}

#[test]
fn a_kind_no_table_has_is_only_shown() {
    let kind = kind_named("bogus");
    assert_eq!(kind.name, "bogus");
    assert!(kind.caps.raster_only && !kind.caps.selectable && !kind.caps.movable);
    assert!(!props::kind_takes_link(&kind));
}

// ── an edit leaves the old model's records as they were ─────────────────────

#[test]
fn an_edit_leaves_the_old_models_records_intact() {
    let (m, id) = drawn_square();
    let before = m.by_id.get(&id).cloned().expect("the square");
    let dragged = run(&m, &[edit("down", 100.5, 115.0), edit("move", 110.5, 125.0)]);
    let (after, result) = step(&dragged, &edit("up", 110.5, 125.0));
    assert!(!result.change.is_empty());
    assert!(
        Rc::ptr_eq(m.by_id.get(&id).expect("still there"), &before),
        "the old model still holds the old record"
    );
    assert!(!Rc::ptr_eq(after.by_id.get(&id).expect("moved"), &before));
    assert_eq!(shape_of(&before.annotation), shape_of(&m.record(&id).expect("old").annotation));
}

// ── geometry equality is JSON.stringify's ───────────────────────────────────

#[test]
fn minus_zero_and_zero_are_the_same_geometry() {
    use epdf_annotation_core::update::changes::geom_equal;
    let shape = |x: f64| {
        Shape::read(&json!({ "kind": "box", "box": { "x": x, "y": 0, "width": 1, "height": 1 }, "rotation": 0, "ellipse": false })).expect("a shape")
    };
    assert!(geom_equal(&shape(-0.0), &shape(0.0)));
}

// ── N10: a draft says whether it creates, from its tag ──────────────────────

#[test]
fn draft_is_create_matches_its_tag() {
    let base = session_with_numbers();
    let mut drafts: Vec<Rc<Draft>> = Vec::new();
    for subtype in ["square", "line", "polygon", "ink", "freeText"] {
        let m = run(
            &base,
            &[create(subtype, "down", 100.0, 100.0), create(subtype, "move", 150.0, 150.0)],
        );
        drafts.extend(m.draft.clone());
    }
    let (m, _) = drawn_square();
    for gesture in [
        vec![edit("down", 100.5, 115.0)],
        vec![edit("down", 180.0, 160.0)],
        vec![edit("down", 140.0, 76.0)],
        vec![edit("down", 400.0, 400.0), edit("move", 420.0, 420.0)],
    ] {
        drafts.extend(run(&m, &gesture).draft.clone());
    }
    assert!(drafts.len() >= 7, "{}", drafts.len());
    let mut seen = std::collections::BTreeSet::new();
    for d in &drafts {
        let tag = d.tag();
        seen.insert(tag);
        assert_eq!(d.is_create(), tag.starts_with("create-"), "{tag}");
    }
    assert!(
        seen.iter().any(|t| t.starts_with("create-"))
            && seen.iter().any(|t| !t.starts_with("create-")),
        "{seen:?}"
    );
}

// ── a snap patch the settings can't hold ────────────────────────────────────

#[test]
fn a_snap_patch_that_does_not_read_changes_nothing() {
    let m = session_with_numbers();
    let bad = update(&m, &message(json!({ "type": "setSnap", "patch": { "alignment": "yes" } })))
        .expect("an update");
    assert!(bad.session.same(&m.session()), "the same session: no change");
    let good = update(&m, &message(json!({ "type": "setSnap", "patch": { "alignment": false } })))
        .expect("an update");
    assert!(!good.session.same(&m.session()) && !good.session.snap.alignment);
}

// ── a foreign measure marker sent back ──────────────────────────────────────

#[test]
fn a_measure_marker_without_a_subtype_is_sent_back_as_the_typescript_compares_it() {
    use epdf_annotation_core::engine::resolve::measurement_follows;
    let obj = |v: Value| v.as_object().cloned().expect("an object");
    let line = |measure: Value| obj(json!({ "subtype": "line", "measure": measure }));
    // `undefined !== undefined` is false: the marker is the current one, dropped.
    let next =
        measurement_follows(&line(json!({})), &obj(json!({ "measure": {} }))).expect("taken");
    assert!(!next.contains_key("measure"), "{next:?}");
    // `null !== undefined`, both ways round: refused.
    for (current, sent) in
        [(json!({ "subtype": null }), json!({})), (json!({}), json!({ "subtype": null }))]
    {
        let error = measurement_follows(&line(current.clone()), &obj(json!({ "measure": sent })))
            .expect_err("refused");
        assert_eq!(
            (error.code, error.message.as_str()),
            (Some("InvalidArg"), "A foreign measure can't be written; leave measure out"),
            "{current} {sent}"
        );
    }
}

// ── a handle on a box turned by a non-finite angle has no cursor ────────────

#[test]
fn a_handle_at_a_non_finite_rotation_has_no_cursor_member() {
    use epdf_annotation_core::geometry::geom_handles;
    for rotation in [json!({ "$number": "NaN" }), json!({ "$number": "Infinity" })] {
        let shape = Shape::read(&json!({
            "kind": "box", "box": { "x": 0, "y": 0, "width": 10, "height": 10 }, "rotation": rotation, "ellipse": false,
        }))
        .expect("a shape");
        let handles = geom_handles(&shape);
        assert_eq!(handles.len(), 8);
        for handle in handles {
            let printed = handle.json();
            assert!(printed.get("cursor").is_none(), "{printed}");
        }
    }
    let upright = Shape::read(&json!({ "kind": "box", "box": { "x": 0, "y": 0, "width": 10, "height": 10 }, "rotation": 0, "ellipse": false })).expect("a shape");
    assert_eq!(geom_handles(&upright)[0].json()["cursor"], "nwse-resize");
}

// ── a destination not given in page space ───────────────────────────────────

#[test]
fn a_page_space_fit_rect_destination_without_its_size_is_refused() {
    use epdf_annotation_core::engine::space::to_pdf;
    let obj = |v: Value| v.as_object().cloned().expect("an object");
    let fit_r = |dest: Value| {
        json!({ "kind": "fitR", "page": page() })
            .as_object()
            .cloned()
            .map(|mut d| {
                d.extend(dest.as_object().cloned().expect("members"));
                Value::Object(d)
            })
            .expect("a destination")
    };
    let whole = fit_r(json!({ "x": 1, "y": 2, "width": 3, "height": 4 }));
    let sizeless = fit_r(json!({ "x": 1, "y": 2 }));
    // On a link target.
    assert!(
        to_pdf(&obj(
            json!({ "subtype": "link", "target": { "kind": "goto", "destination": whole } })
        ))
        .is_ok()
    );
    let error = to_pdf(&obj(
        json!({ "subtype": "link", "target": { "kind": "goto", "destination": sizeless } }),
    ))
    .expect_err("refused");
    assert_eq!(
        error.message,
        "field 'target': expected page space, points as { x, y } from the page's top-left"
    );
    // Inside an action tree, a level down.
    let tree = json!({ "activate": { "root": { "type": "uri", "uri": "x", "next": [{ "type": "goto", "destination": sizeless, "next": [] }] } } });
    let error = to_pdf(&obj(json!({ "subtype": "link", "actions": tree }))).expect_err("refused");
    assert_eq!(
        error.message,
        "field 'actions': expected page space, points as { x, y } from the page's top-left"
    );
    // An xyz destination may leave out either axis.
    let xyz =
        json!({ "kind": "goto", "destination": { "kind": "xyz", "page": page(), "zoom": 1 } });
    assert!(to_pdf(&obj(json!({ "subtype": "link", "target": xyz }))).is_ok());
}
