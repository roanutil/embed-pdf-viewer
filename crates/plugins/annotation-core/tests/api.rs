//! The convenience API the vectors don't reach: constructors and helpers
//! a caller of the crate uses directly.

use epdf_annotation_core::props::PropsChanged;
use epdf_annotation_core::*;

#[test]
fn rect_handle_cursor_matches_the_unrotated_table() {
    for h in RECT_HANDLES {
        assert_eq!(Some(rect_handle_cursor(h).as_str()), rotated_handle_cursor(h, 0.0), "{h}");
    }
}

#[test]
fn props_changed_any_is_any_part() {
    assert!(!PropsChanged::default().any());
    for i in 0..5 {
        let mut c = PropsChanged::default();
        match i {
            0 => c.style = true,
            1 => c.geom = true,
            2 => c.text = true,
            3 => c.icon = true,
            _ => c.link = true,
        }
        assert!(c.any(), "{i}");
    }
}

#[test]
fn click_create_segment_is_a_segment_policy() {
    let p = ClickCreate::segment(40.0, Some(90.0));
    assert!(p.is_segment() && !p.has_width());
    assert_eq!((p.length, p.angle_deg), (Some(40.0), Some(90.0)));
    let b = ClickCreate::boxed(10.0, 20.0, None);
    assert!(!b.is_segment() && b.has_width());
}

#[test]
fn same_page_compares_the_object_number() {
    let a = PageRef::new(7);
    assert!(a.same_page(&PageRef::new(7)));
    assert!(!a.same_page(&PageRef::new(8)));
}

#[test]
fn measurement_appearance_is_polygon_only_for_a_polygon_shape() {
    let parse = |intent: &str| -> MeasurementAppearance {
        serde_json::from_value(serde_json::json!({
            "intent": intent,
            "measure": null,
            "caption": { "enabled": true },
            "crop": { "left": 0, "bottom": 0, "right": 612, "top": 792 },
            "text": "",
        }))
        .expect("an appearance")
    };
    assert!(parse("PolygonDimension").is_polygon());
    assert!(!parse("PolyLineDimension").is_polygon());
    assert!(!parse("LineDimension").is_polygon());
}

#[test]
fn draft_is_create_matches_its_tag() {
    let p = Point { x: 0.0, y: 0.0 };
    let page = PageRef::default();
    let r = Rect { x: 0.0, y: 0.0, width: 1.0, height: 1.0 };
    let geom = Geom::Rect { rect: r, ellipse: false, rot: None };
    let drafts = [
        Draft::CreateRect {
            subtype: "square".into(),
            preset: None,
            page,
            from: p,
            to: p,
            ellipse: false,
            display_rotation: None,
            upright: None,
            click_create: None,
            flags: None,
        },
        Draft::CreateLine {
            measure: None,
            capture: None,
            subtype: "line".into(),
            preset: None,
            page,
            from: p,
            to: p,
            click_create: None,
            flags: None,
        },
        Draft::CreatePoly {
            measure: None,
            subtype: "polygon".into(),
            preset: None,
            page,
            points: vec![p],
            cur: p,
            closed: false,
            flags: None,
        },
        Draft::CreateInk {
            subtype: "ink".into(),
            preset: None,
            page,
            strokes: vec![vec![p]],
            intent: None,
            flags: None,
        },
        Draft::CreateCallout {
            subtype: "free-text".into(),
            preset: None,
            page,
            step: CalloutStep::Knee,
            tip: p,
            knee: None,
            cur: p,
            box_from: None,
            box_to: None,
            page_box: None,
            display_rotation: None,
            upright: None,
            flags: None,
        },
        Draft::Move { ids: vec!["a".into()], start: p, delta: p, guides: Vec::new() },
        Draft::Handle {
            id: "a".into(),
            handle: "nw".into(),
            base: geom.clone(),
            cur: geom,
            view: None,
        },
        Draft::Rotate { ids: vec!["a".into()], pivot: p, start: p, cur: p, free: None, view: None },
        Draft::Group {
            op: GroupOp::Resize,
            ids: vec!["a".into()],
            handle: "nw".into(),
            anchor: p,
            base: r,
            cur: r,
            view: None,
        },
        Draft::Caption { id: "a".into(), start: p, delta: p },
        Draft::Leader { id: "a".into(), start: p, delta: 0.0 },
        Draft::Marquee { page, from: p, to: p },
    ];
    for d in &drafts {
        let tag = serde_json::to_value(d).unwrap()["g"].as_str().unwrap().to_string();
        assert_eq!(d.is_create(), tag.starts_with("create-"), "{tag}");
    }
}
