use epdf_ffi::{
    Camera, PageFrameInsets, StageAlign, StageAlignValue, StageAlignmentValue, ViewAnchor,
    ZoomMode, ZoomSpec, to_u32,
};
use epdf_stage_core as sc;

#[test]
fn a_zoom_spec_round_trips_every_variant() {
    for spec in [
        ZoomSpec::Level { level: 1.5 },
        ZoomSpec::PageWidth { page_width: 0.9 },
        ZoomSpec::PageHeight { page_height: 1.1 },
        ZoomSpec::Mode { mode: ZoomMode::FitWidth },
    ] {
        let core: sc::ZoomSpec = spec.into();
        assert_eq!(ZoomSpec::from(core), spec);
    }
}

#[test]
fn a_camera_and_insets_round_trip() {
    let camera = Camera { x: 10.0, y: -4.0, zoom: 2.0 };
    assert_eq!(Camera::from(sc::Camera::from(camera)), camera);
    let insets = PageFrameInsets { top: 1.0, right: 2.0, bottom: 3.0, left: 4.0 };
    assert_eq!(PageFrameInsets::from(epdf_geometry::PageFrame::from(insets)), insets);
}

#[test]
fn an_alignment_value_keeps_named_stops_and_fractions() {
    let value = StageAlignmentValue {
        x: StageAlignValue::Named { align: StageAlign::Center },
        y: StageAlignValue::Fraction { value: 0.25 },
    };
    assert_eq!(StageAlignmentValue::from(sc::AlignmentValue::from(value)), value);
}

#[test]
fn an_anchor_index_that_does_not_fit_is_invalid_state() {
    let core = sc::Anchor { page_index: usize::MAX, fx: 0.0, fy: 0.0 };
    let error = ViewAnchor::try_from(core).unwrap_err();
    assert_eq!(error.code(), Some("invalid-state"));
    let ok = ViewAnchor::try_from(sc::Anchor { page_index: 3, fx: 0.5, fy: 0.25 }).unwrap();
    assert_eq!(ok, ViewAnchor { page_index: 3, fx: 0.5, fy: 0.25 });
    assert_eq!(to_u32(7, "a page index").unwrap(), 7);
}
