//! Geometry from a hostile PDF cannot make a cloudy border allocate without
//! bound or loop forever (review finding M20, a kept Rust-only bound), and
//! ordinary geometry still draws.

use epdf_annotation_core::cloudy::{
    cloudy_bounds, cloudy_outline, cloudy_path, cloudy_poly_bounds, cloudy_poly_outline,
    cloudy_poly_path,
};
use epdf_annotation_core::*;

#[test]
fn a_page_sized_cloudy_border_still_draws() {
    let page = Rect { x: 0.0, y: 0.0, width: 14_400.0, height: 14_400.0 };
    for ellipse in [false, true] {
        let d = cloudy_path(page, ellipse, 1.0, 1.0);
        assert!(d.starts_with("M ") && d.ends_with(" Z"), "{ellipse}");
        assert!(!cloudy_outline(page, ellipse, 1.0, 1.0).is_empty());
        assert!(cloudy_bounds(page, ellipse, 1.0, 1.0).width >= 14_400.0);
    }
    let ring = [
        Point { x: 0.0, y: 0.0 },
        Point { x: 14_400.0, y: 0.0 },
        Point { x: 14_400.0, y: 14_400.0 },
    ];
    assert!(cloudy_poly_path(&ring, 1.0, 1.0).starts_with("M "));
    assert!(!cloudy_poly_outline(&ring, 1.0, 1.0).is_empty());
}

#[test]
fn an_absurd_cloudy_border_draws_nothing() {
    for size in [1e12, f64::INFINITY] {
        let bx = Rect { x: 0.0, y: 0.0, width: size, height: size };
        for ellipse in [false, true] {
            assert_eq!(cloudy_path(bx, ellipse, 1.0, 1.0), "", "{size} {ellipse}");
            assert!(cloudy_outline(bx, ellipse, 1.0, 1.0).is_empty(), "{size} {ellipse}");
            // The box itself, grown by half the stroke: nothing traced.
            assert_eq!(cloudy_bounds(bx, ellipse, 1.0, 2.0).x, -1.0, "{size} {ellipse}");
        }
        let ring =
            [Point { x: 0.0, y: 0.0 }, Point { x: size, y: 0.0 }, Point { x: size, y: size }];
        assert_eq!(cloudy_poly_path(&ring, 1.0, 1.0), "", "{size}");
        assert!(cloudy_poly_outline(&ring, 1.0, 1.0).is_empty(), "{size}");
        assert_eq!(cloudy_poly_bounds(&ring, 1.0, 2.0).x, -1.0, "{size}");
    }
}
