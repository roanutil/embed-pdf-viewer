//! Geometry from a hostile PDF cannot make a cloudy border allocate without
//! bound or loop forever, and ordinary geometry still draws.

use epdf_annotation_core::*;

#[test]
fn a_page_sized_cloudy_border_still_draws() {
    let page = Rect { x: 0.0, y: 0.0, width: 14_400.0, height: 14_400.0 };
    for ellipse in [false, true] {
        let d = cloudy_path(page, ellipse, 1.0, 1.0);
        assert!(d.starts_with("M ") && d.ends_with(" Z"), "{ellipse}");
    }
    let ring = [
        Point { x: 0.0, y: 0.0 },
        Point { x: 14_400.0, y: 0.0 },
        Point { x: 14_400.0, y: 14_400.0 },
    ];
    assert!(cloudy_poly_path(&ring, 1.0, 1.0).starts_with("M "));
}

#[test]
fn an_absurd_cloudy_border_draws_nothing() {
    for size in [1e12, f64::INFINITY] {
        let bx = Rect { x: 0.0, y: 0.0, width: size, height: size };
        for ellipse in [false, true] {
            assert_eq!(cloudy_path(bx, ellipse, 1.0, 1.0), "", "{size} {ellipse}");
        }
        let ring =
            [Point { x: 0.0, y: 0.0 }, Point { x: size, y: 0.0 }, Point { x: size, y: size }];
        assert_eq!(cloudy_poly_path(&ring, 1.0, 1.0), "", "{size}");
    }
    // A huge intensity cancelled by a negative width shrinks the curls to
    // their minimum while the inset blows the ring up.
    let bx = Rect { x: 0.0, y: 0.0, width: 100.0, height: 100.0 };
    assert_eq!(cloudy_path(bx, false, 1e12, -8e12), "");
}
