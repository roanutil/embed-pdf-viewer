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

/// The most points a cloud's outline may hold (`MAX_OUTLINE_POINTS`).
const OUTLINE_BUDGET: usize = 1_000_000;

fn points(lines: &[Vec<Point>]) -> usize {
    lines.iter().map(Vec::len).sum()
}

#[test]
fn a_hostile_curl_size_flattens_within_the_budget_or_not_at_all() {
    let bx = Rect { x: 0.0, y: 0.0, width: 100.0, height: 100.0 };
    let ring = [
        Point { x: 0.0, y: 0.0 },
        Point { x: 100.0, y: 0.0 },
        Point { x: 100.0, y: 100.0 },
        Point { x: 0.0, y: 100.0 },
    ];
    // (intensity, stroke width): a hostile /BE /I, then a hostile /BS /W.
    let hostile = [(1e4, 1.0), (1e5, 1.0), (f64::INFINITY, 1.0), (1.0, 1e5), (1.0, f64::INFINITY)];
    for (intensity, width) in hostile {
        let started = std::time::Instant::now();
        for ellipse in [false, true] {
            let n = points(&cloudy_outline(bx, ellipse, intensity, width));
            assert!(n <= OUTLINE_BUDGET, "box {ellipse} {intensity} {width}: {n}");
        }
        let n = points(&cloudy_poly_outline(&ring, intensity, width));
        assert!(n <= OUTLINE_BUDGET, "polygon {intensity} {width}: {n}");
        // The path and the bounds trace each curl once, with no flattening.
        cloudy_path(bx, false, intensity, width);
        cloudy_bounds(bx, true, intensity, width);
        cloudy_poly_path(&ring, intensity, width);
        cloudy_poly_bounds(&ring, intensity, width);
        assert!(
            started.elapsed().as_secs_f64() < 5.0,
            "{intensity} {width}: {:?}",
            started.elapsed()
        );
    }
    // An ordinary cloud still flattens.
    assert!(points(&cloudy_outline(bx, false, 1.0, 1.0)) > 1_000);
}
