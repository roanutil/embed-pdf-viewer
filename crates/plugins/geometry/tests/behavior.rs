//! Behaviour the vectors only sample: singular transforms refuse, and
//! rotation, crop and `userUnit` round trips give back the coordinates they
//! started from.

use epdf_geometry::*;

const ROTATIONS: [PageRotation; 4] =
    [PageRotation::DEG_0, PageRotation::DEG_90, PageRotation::DEG_180, PageRotation::DEG_270];

const CROPS: [PdfRect; 4] = [
    PdfRect { left: 0.0, right: 612.0, bottom: 0.0, top: 792.0 },
    PdfRect { left: 12.0, right: 312.0, bottom: -8.0, top: 392.0 },
    PdfRect { left: -50.0, right: 50.0, bottom: -30.0, top: 170.0 },
    PdfRect { left: 0.25, right: 200.75, bottom: 100.5, top: 400.125 },
];

fn pt(x: f64, y: f64) -> Point {
    Point { x, y }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

fn assert_point_close(got: Point, want: Point, context: &str) {
    assert!(close(got.x, want.x) && close(got.y, want.y), "{context}: {got:?} != {want:?}");
}

fn assert_rect_close(got: Rect, want: Rect, context: &str) {
    assert!(
        close(got.x, want.x)
            && close(got.y, want.y)
            && close(got.width, want.width)
            && close(got.height, want.height),
        "{context}: {got:?} != {want:?}"
    );
}

const SINGULAR: &str = "Mat2D.invert: non-invertible matrix (det = 0)";

#[test]
fn singular_matrices_refuse_to_invert() {
    for m in [
        [0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [1.0, 2.0, 2.0, 4.0, 5.0, 6.0],
        scale(0.0, 3.0),
        rotate_scale_matrix(0.0, 10.0, 20.0, PageRotation::DEG_90),
    ] {
        let err = invert(m).expect_err("a zero determinant has no inverse");
        assert_eq!(err.to_string(), SINGULAR);
    }
    // A NaN determinant is not zero: the TypeScript inverts it to NaNs.
    let nan = invert([f64::NAN, 0.0, 0.0, 1.0, 0.0, 0.0]).expect("NaN is not zero");
    assert!(nan[0].is_nan());
}

#[test]
fn a_singular_page_transform_refuses() {
    // A view scale whose square underflows to zero: the device width clamps
    // to 1, so `viewScale` is 1e-300 and `det` is 0.
    let input = PageTransformInput {
        page_size: Size { width: 1e300, height: 1e300 },
        rotation: PageRotation::DEG_0,
        scale: 1e-300,
        base_scale: None,
        dpr: 1.0,
    };
    let err = page_transform(input).expect_err("singular");
    assert_eq!(err.to_string(), SINGULAR);
    assert_eq!(err, NonInvertible);
}

/// The view a page's own `pageTransform` builds, with `userUnit` and zoom
/// composed into `scale` the way the shell composes them.
fn transform_for(
    crop: PdfRect,
    rotation: PageRotation,
    user_unit: f64,
    zoom: f64,
) -> PageTransform {
    page_transform(PageTransformInput {
        page_size: Size { width: crop.right - crop.left, height: crop.top - crop.bottom },
        rotation,
        scale: zoom * user_unit,
        base_scale: Some(user_unit),
        dpr: 2.0,
    })
    .expect("a positive scale inverts")
}

#[test]
fn rotation_round_trips_preserve_page_coordinates() {
    let points = [pt(0.0, 0.0), pt(10.0, 20.0), pt(137.25, 42.5), pt(-5.0, 3.5)];
    let rects = [
        Rect { x: 0.0, y: 0.0, width: 100.0, height: 200.0 },
        Rect { x: 10.5, y: 20.25, width: 30.0, height: 40.0 },
    ];
    for crop in CROPS {
        for rotation in ROTATIONS {
            for user_unit in [1.0, 1.5, 0.75] {
                let t = transform_for(crop, rotation, user_unit, 1.25);
                assert_eq!(t.rotation, rotation);
                for p in points {
                    let ctx = format!("{crop:?} r{} u{user_unit} {p:?}", rotation.0);
                    assert_point_close(t.view_to_page(t.page_to_view(p)), p, &ctx);
                    assert_point_close(t.from_pixels(t.to_pixels(p)), p, &ctx);
                }
                for r in rects {
                    let ctx = format!("{crop:?} r{} u{user_unit} {r:?}", rotation.0);
                    assert_rect_close(t.view_to_page_rect(t.page_to_view_rect(r)), r, &ctx);
                }
            }
        }
    }
}

#[test]
fn quarter_turns_swap_the_footprint_and_keep_the_page_corner() {
    let crop = CROPS[1];
    for rotation in ROTATIONS {
        let t = transform_for(crop, rotation, 1.5, 1.0);
        if is_quarter_turn(rotation) {
            assert_eq!((t.view_width, t.view_height), (t.content_height, t.content_width));
        } else {
            assert_eq!((t.view_width, t.view_height), (t.content_width, t.content_height));
        }
        // The page's far corner lands on a corner of the display box.
        let far = t.page_to_view(pt(crop.right - crop.left, crop.top - crop.bottom));
        let corner_x = close(far.x, 0.0) || close(far.x, t.view_width);
        let corner_y = close(far.y, 0.0) || close(far.y, t.view_height);
        assert!(corner_x && corner_y, "r{}: {far:?}", rotation.0);
    }
}

#[test]
fn crop_round_trips_are_exact() {
    let points = [pt(0.0, 0.0), pt(10.0, 20.0), pt(-5.0, 3.5), pt(611.0, 791.0)];
    let boxes = [
        PdfRect { left: 10.0, right: 40.0, bottom: 20.0, top: 60.0 },
        PdfRect { left: -3.5, right: 7.25, bottom: 1.0, top: 1.0 },
    ];
    let quad = Quad {
        upper_left: pt(10.0, 30.0),
        upper_right: pt(40.0, 32.0),
        lower_left: pt(9.0, 20.0),
        lower_right: pt(39.0, 22.0),
    };
    for crop in CROPS {
        for p in points {
            assert_eq!(pdf_point_of(page_point_of(p, crop), crop), p, "{crop:?}");
            assert_eq!(page_point_of(pdf_point_of(p, crop), crop), p, "{crop:?}");
        }
        for b in boxes {
            assert_eq!(pdf_rect_of(page_box_of(b, crop), crop), b, "{crop:?}");
        }
        assert_eq!(pdf_quad_of(page_quad_of(quad, crop), crop), quad, "{crop:?}");
        // The crop's own top-left is the page origin, its bottom-right the
        // page's far corner.
        assert_eq!(page_point_of(pt(crop.left, crop.top), crop), pt(0.0, 0.0));
        assert_eq!(
            page_box_of(crop, crop),
            Rect { x: 0.0, y: 0.0, width: crop.right - crop.left, height: crop.top - crop.bottom }
        );
    }
}

#[test]
fn pdf_to_view_and_back_preserves_pdf_coordinates() {
    let pdf_points = [pt(100.0, 700.0), pt(12.5, -3.0), pt(0.0, 0.0)];
    for crop in CROPS {
        for rotation in ROTATIONS {
            for user_unit in [1.0, 2.0] {
                let t = transform_for(crop, rotation, user_unit, 0.8);
                for p in pdf_points {
                    let view = t.page_to_view(page_point_of(p, crop));
                    let back = pdf_point_of(t.view_to_page(view), crop);
                    assert_point_close(back, p, &format!("{crop:?} r{} u{user_unit}", rotation.0));
                }
            }
        }
    }
}

#[test]
fn quads_keep_their_corner_names_through_transforms() {
    let q = quad_from_rect(Rect { x: 100.0, y: 200.0, width: 60.0, height: 16.0 });
    assert_eq!(quad_corners(q), [q.upper_left, q.upper_right, q.lower_left, q.lower_right]);
    assert_eq!(quad_ring(q), [q.upper_left, q.upper_right, q.lower_right, q.lower_left]);
    assert_eq!(quad_edge(q, QuadSide::Left), [q.upper_left, q.lower_left]);
    assert_eq!(quad_edge(q, QuadSide::Right), [q.upper_right, q.lower_right]);
    let turned = apply_quad(rotate_about(pt(130.0, 208.0), std::f64::consts::PI), q);
    // Turned half way round, the upper-left is at the bottom right.
    assert_point_close(turned.upper_left, pt(160.0, 216.0), "half turn");
    assert!(!quad_equals(q, turned));
    assert_eq!(quad_bounds(q), Rect { x: 100.0, y: 200.0, width: 60.0, height: 16.0 });
}

#[test]
fn contained_rect_fits_and_centres() {
    let box_ = Rect { x: 10.0, y: 20.0, width: 200.0, height: 100.0 };
    assert_eq!(
        contained_rect(Size { width: 50.0, height: 50.0 }, box_),
        Rect { x: 60.0, y: 20.0, width: 100.0, height: 100.0 }
    );
    assert_eq!(
        contained_rect(Size { width: 400.0, height: 100.0 }, box_),
        Rect { x: 10.0, y: 45.0, width: 200.0, height: 50.0 }
    );
    // No area: the box itself.
    assert_eq!(contained_rect(Size { width: 0.0, height: 10.0 }, box_), box_);
    assert_eq!(contained_rect(Size { width: 10.0, height: -1.0 }, box_), box_);
}

#[test]
fn same_page_placement_ignores_nothing_it_names() {
    let crop = CROPS[0];
    let a = transform_for(crop, PageRotation::DEG_90, 1.0, 1.0);
    let b = transform_for(crop, PageRotation::DEG_90, 1.0, 1.0);
    assert!(same_page_placement(&a, &b));
    assert!(!same_page_placement(&a, &transform_for(crop, PageRotation::DEG_0, 1.0, 1.0)));
    assert!(!same_page_placement(&a, &transform_for(crop, PageRotation::DEG_90, 1.0, 2.0)));
    // Same view scale, different baseline: the zoom differs.
    assert!(!same_page_placement(&a, &transform_for(crop, PageRotation::DEG_90, 2.0, 0.5)));
}
