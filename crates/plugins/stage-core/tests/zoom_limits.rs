//! A view's zoom range (upstream `e2ee5215f`): `resolveZoomLimits`,
//! `clampZoom`, and the `limits` argument of `zoomAround` and `resolveZoom`.
//! No core vector passes limits, so these pin the TypeScript by hand.

use epdf_geometry::{Point, Size};
use epdf_stage_core::*;

#[test]
fn the_limits_stay_inside_the_widest_range_and_min_never_passes_max() {
    assert_eq!(resolve_zoom_limits(0.5, 4.0), ZoomLimits { min: 0.5, max: 4.0 });
    assert_eq!(resolve_zoom_limits(0.0, 1000.0), ZoomLimits { min: ZOOM_MIN, max: ZOOM_MAX });
    // A minZoom above maxZoom counts as maxZoom.
    assert_eq!(resolve_zoom_limits(8.0, 2.0), ZoomLimits { min: 2.0, max: 2.0 });
}

#[test]
fn clamp_zoom_bounds_a_level_by_the_limits() {
    let limits = ZoomLimits { min: 0.5, max: 4.0 };
    assert_eq!(clamp_zoom(0.1, limits), 0.5);
    assert_eq!(clamp_zoom(9.0, limits), 4.0);
    assert_eq!(clamp_zoom(2.0, limits), 2.0);
    assert_eq!(clamp_zoom(100.0, ALL_ZOOMS), ZOOM_MAX);
}

#[test]
fn zoom_around_within_holds_the_point_and_stops_at_the_limit() {
    let camera = Camera { x: 10.0, y: 20.0, zoom: 1.0 };
    let point = Point { x: 100.0, y: 50.0 };
    let limited = zoom_around_within(camera, point, 10.0, ZoomLimits { min: 0.5, max: 2.0 });
    assert_eq!(limited.zoom, 2.0);
    assert_eq!(limited.x, 10.0 + 100.0 - 100.0 / 2.0);
    assert_eq!(
        zoom_around(camera, point, 10.0),
        zoom_around_within(camera, point, 10.0, ALL_ZOOMS)
    );
}

#[test]
fn resolve_zoom_within_stops_a_fit_at_the_nearest_limit() {
    let page = Size { width: 100.0, height: 100.0 };
    let vp = Size { width: 1000.0, height: 1000.0 };
    let fit_width = ZoomSpec::Mode { mode: ZoomMode::FitWidth };
    assert_eq!(resolve_zoom(fit_width, page, vp, 0.0), 10.0);
    let limits = ZoomLimits { min: 0.25, max: 3.0 };
    assert_eq!(resolve_zoom_within(fit_width, page, vp, 0.0, limits), 3.0);
    assert_eq!(resolve_zoom_within(ZoomSpec::Level { level: 0.1 }, page, vp, 0.0, limits), 0.25);
}
