//! The brain tests that were `packages/shapes/src/shapes.test.ts` in typescript-baseline.
//!
//! They moved into Rust wholesale, which is the trade this POC is measuring:
//! the logic is now shareable with a Swift or Kotlin binding, and it is no
//! longer testable from TypeScript without going through wasm.

use poc_core::geom::{quad_bounds, rect_quad, Rect, Point};
use poc_core::model::{
    anchor_factors, hit_test, project_quad, scene_buffer, shape_quad, update, Effect, Model, Msg,
    ViewEnv, FLAG_PROJECTED, FLAG_SELECTED, ITEM_STRIDE,
};

fn v(zoom: f64, rotation: f64) -> ViewEnv {
    ViewEnv { zoom, rotation }
}

fn with_one_shape(anchored: bool) -> Model {
    let mut m = Model::default();
    update(
        &mut m,
        Msg::Add {
            at: Point { x: 100.0, y: 100.0 },
            color: "#c00".into(),
        },
    );
    if anchored {
        update(
            &mut m,
            Msg::SetAnchored {
                id: "s1".into(),
                anchored: true,
            },
        );
    }
    m
}

fn many(count: u32, anchored: bool) -> Model {
    let mut m = Model::default();
    for i in 0..count {
        update(
            &mut m,
            Msg::Add {
                at: Point {
                    x: 60.0 + f64::from(i) * 30.0,
                    y: 80.0,
                },
                color: "#0c0".into(),
            },
        );
        if anchored {
            update(
                &mut m,
                Msg::SetAnchored {
                    id: format!("s{}", i + 1),
                    anchored: true,
                },
            );
        }
    }
    // Add selects what it adds, so without this the last shape carries
    // FLAG_SELECTED and the flag assertions below are testing the helper.
    m.selected = None;
    m
}

#[test]
fn anchor_factors_is_none_unless_the_view_actually_projects() {
    assert!(anchor_factors(false, v(4.0, 90.0)).is_none());
    assert!(anchor_factors(true, v(1.0, 0.0)).is_none());
    // Clamped below 100%: an anchored shape scales WITH the page there.
    assert!(anchor_factors(true, v(0.25, 0.0)).is_none());
    assert_eq!(anchor_factors(true, v(3.0, 90.0)), Some((3.0, 90.0)));
}

#[test]
fn projection_holds_the_bounds_top_left_and_shrinks_by_one_over_zoom() {
    let q = rect_quad(
        Rect {
            x: 100.0,
            y: 60.0,
            width: 80.0,
            height: 40.0,
        },
        0.0,
    );
    let out = project_quad(q, true, v(2.0, 0.0));
    assert_eq!(out.0[0].x, 100.0);
    assert_eq!(out.0[0].y, 60.0);
    assert!((out.0[1].x - 140.0).abs() < 1e-9);
    assert!((out.0[2].y - 80.0).abs() < 1e-9);
}

#[test]
fn identity_projection_returns_an_equal_quad() {
    let q = rect_quad(
        Rect {
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 40.0,
        },
        0.0,
    );
    assert_eq!(project_quad(q, true, v(1.0, 0.0)), q);
    assert_eq!(project_quad(q, false, v(4.0, 90.0)), q);
}

#[test]
fn add_selects_the_new_shape_and_asks_the_shell_to_persist() {
    let mut m = Model::default();
    let (changed, effects) = update(
        &mut m,
        Msg::Add {
            at: Point { x: 10.0, y: 10.0 },
            color: "#00f".into(),
        },
    );
    assert!(changed);
    assert_eq!(m.shapes.len(), 1);
    assert_eq!(m.selected, Some(1));
    assert_eq!(effects.len(), 2);
    assert!(matches!(&effects[0], Effect::Log { message } if message == "added s1"));
    assert!(matches!(&effects[1], Effect::Persist { ids } if ids == &["s1".to_string()]));
}

#[test]
fn add_centres_the_shape_on_the_click_point() {
    let m = with_one_shape(false);
    let r = m.shapes[0].rect;
    assert_eq!(r.x + r.width / 2.0, 100.0);
    assert_eq!(r.y + r.height / 2.0, 100.0);
}

#[test]
fn drag_is_absolute_from_the_down_origin_not_cumulative() {
    let mut m = with_one_shape(false);
    update(
        &mut m,
        Msg::PointerDown {
            at: Point { x: 100.0, y: 100.0 },
            view: v(1.0, 0.0),
        },
    );
    update(
        &mut m,
        Msg::PointerMove {
            at: Point { x: 150.0, y: 100.0 },
        },
    );
    update(
        &mut m,
        Msg::PointerMove {
            at: Point { x: 120.0, y: 100.0 },
        },
    );
    assert!((m.shapes[0].rect.x - 75.0).abs() < 1e-9);
}

#[test]
fn an_anchored_drag_tracks_the_pointer_one_to_one() {
    let mut m = with_one_shape(true);
    let view = v(2.0, 0.0);
    update(
        &mut m,
        Msg::PointerDown {
            at: Point { x: 70.0, y: 76.0 },
            view,
        },
    );
    assert!(m.drag.is_some(), "the down should have hit the projected quad");
    update(
        &mut m,
        Msg::PointerMove {
            at: Point { x: 80.0, y: 76.0 },
        },
    );
    // The projection holds the unprojected bounds top-left fixed, so the
    // projected quad already sits at the stored rect position: 10 pointer units
    // is 10 stored units. Scaling the delta by max(zoom, 1) sent it 20 and left
    // the cursor behind the shape.
    assert!((m.shapes[0].rect.x - 65.0).abs() < 1e-9);
    assert_eq!(
        hit_test(&m, Point { x: 80.0, y: 76.0 }, view),
        Some(1),
        "the cursor must still be on the shape it is dragging"
    );
}

#[test]
fn deleting_a_shape_mid_gesture_drops_the_drag_and_the_selection() {
    let mut m = with_one_shape(false);
    update(
        &mut m,
        Msg::PointerDown {
            at: Point { x: 100.0, y: 100.0 },
            view: v(1.0, 0.0),
        },
    );
    assert!(m.drag.is_some());
    update(&mut m, Msg::Delete { id: "s1".into() });
    assert!(m.drag.is_none());
    assert!(m.selected.is_none());
}

#[test]
fn update_reports_no_change_for_a_no_op() {
    let mut m = Model::default();
    let (changed, effects) = update(&mut m, Msg::Delete { id: "s99".into() });
    assert!(!changed);
    assert!(effects.is_empty());
}

#[test]
fn hit_test_follows_the_projection() {
    let m = with_one_shape(true);
    let q = shape_quad(&m.shapes[0], v(4.0, 0.0));
    let b = quad_bounds(q);
    let centre = poc_core::geom::Point {
        x: b.x + b.width / 2.0,
        y: b.y + b.height / 2.0,
    };
    assert_eq!(hit_test(&m, centre, v(4.0, 0.0)), Some(1));
    // The unprojected centre is outside the shrunken shape.
    assert_eq!(
        hit_test(&m, poc_core::geom::Point { x: 100.0, y: 100.0 }, v(4.0, 0.0)),
        None
    );
}

#[test]
fn hit_test_returns_the_topmost_of_two_overlapping_shapes() {
    let mut m = with_one_shape(false);
    update(
        &mut m,
        Msg::Add {
            at: Point { x: 100.0, y: 100.0 },
            color: "#0c0".into(),
        },
    );
    assert_eq!(
        hit_test(&m, poc_core::geom::Point { x: 100.0, y: 100.0 }, v(1.0, 0.0)),
        Some(2)
    );
}

#[test]
fn the_scene_buffer_is_one_flat_allocation_for_the_whole_frame() {
    let m = many(10, true);
    let buf = scene_buffer(&m, v(3.0, 90.0));

    // Header plus one fixed-stride record per shape, and nothing else. Growing
    // the scene grows the BUFFER, never the number of crossings.
    assert_eq!(buf.len(), 1 + 10 * ITEM_STRIDE);
    assert_eq!(buf[0], 10.0);

    // Every shape is anchored and the view projects, so every flags word says so.
    for i in 0..10 {
        let flags = buf[1 + i * ITEM_STRIDE + 9];
        assert_eq!(flags, FLAG_PROJECTED, "shape {i} should be projected only");
    }

    // 100 shapes: 100x the floats, still one call.
    let big = scene_buffer(&many(100, true), v(3.0, 90.0));
    assert_eq!(big.len(), 1 + 100 * ITEM_STRIDE);
}

#[test]
fn the_scene_buffer_marks_the_selection() {
    let m = with_one_shape(false);
    let buf = scene_buffer(&m, v(1.0, 0.0));
    assert_eq!(buf[1 + 9], FLAG_SELECTED);
}

#[test]
fn an_unprojecting_view_leaves_the_flags_clear() {
    let m = many(3, false);
    let buf = scene_buffer(&m, v(4.0, 90.0));
    for i in 0..3 {
        assert_eq!(buf[1 + i * ITEM_STRIDE + 9], 0.0);
    }
}

#[test]
fn messages_deserialize_from_the_typescript_shapes() {
    // The exact JSON the TypeScript union produces. A tag mismatch here is the
    // failure mode that argues for generating the TS types from these enums.
    let cases = [
        r##"{"t":"add","at":{"x":1,"y":2},"color":"#c00"}"##,
        r##"{"t":"select","id":"s1"}"##,
        r##"{"t":"select","id":null}"##,
        r##"{"t":"pointerDown","at":{"x":1,"y":2},"view":{"zoom":2,"rotation":90}}"##,
        r##"{"t":"pointerMove","at":{"x":3,"y":4}}"##,
        r##"{"t":"pointerUp"}"##,
        r##"{"t":"setAnchored","id":"s1","anchored":true}"##,
        r##"{"t":"setRot","id":"s1","rot":45}"##,
        r##"{"t":"delete","id":"s1"}"##,
    ];
    for raw in cases {
        serde_json::from_str::<Msg>(raw).unwrap_or_else(|e| panic!("{raw} failed: {e}"));
    }
}

#[test]
fn effects_serialize_to_the_typescript_shapes() {
    let log = serde_json::to_string(&Effect::Log {
        message: "hi".into(),
    })
    .unwrap();
    assert_eq!(log, r##"{"t":"log","message":"hi"}"##);

    let persist = serde_json::to_string(&Effect::Persist {
        ids: vec!["s1".into()],
    })
    .unwrap();
    assert_eq!(persist, r##"{"t":"persist","ids":["s1"]}"##);
}

/// typescript-baseline stores any string handed to `select`. This model keeps a
/// `u32` handle, so an id it cannot represent must not be silently rewritten
/// into a deselect: leave the selection alone and say so through an effect.
#[test]
fn select_with_a_malformed_id_is_a_logged_no_op() {
    let mut m = with_one_shape(false);
    assert_eq!(m.selected, Some(1));

    let (changed, effects) = update(
        &mut m,
        Msg::Select {
            id: Some("not-a-handle".into()),
        },
    );

    assert!(!changed);
    assert_eq!(m.selected, Some(1));
    assert_eq!(
        effects,
        vec![Effect::Log {
            message: "select: ignored malformed id \"not-a-handle\"".into(),
        }]
    );

    // The well-formed shapes of the message still behave as before.
    let (changed, effects) = update(&mut m, Msg::Select { id: None });
    assert!(changed);
    assert!(effects.is_empty());
    assert_eq!(m.selected, None);
}
