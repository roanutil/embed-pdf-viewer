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
