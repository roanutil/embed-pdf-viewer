//! The conformance adapter: the function table the vectors call by the
//! TypeScript names, with arguments and results in canonical JSON.

use epdf_kernel::Error;
use plugin_conformance::FunctionTable;
use plugin_conformance::js::{arg, to_value};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::*;

type Out = Result<Value, Error>;

fn ok<T: Serialize>(value: T) -> Out {
    Ok(to_value(&value))
}

fn thrown(e: NonInvertible) -> Error {
    Error::new(e.to_string())
}

#[derive(Deserialize, Default)]
struct Probe {
    #[serde(default)]
    points: Option<Vec<Point>>,
    #[serde(default)]
    rects: Option<Vec<Rect>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PageTransformProbe {
    #[serde(flatten)]
    transform: PageTransform,
    to_pixels: Vec<Point>,
    from_pixels: Vec<Point>,
    content_to_view: Vec<Point>,
    view_to_content: Vec<Point>,
    content_to_view_rect: Vec<Rect>,
    view_to_content_rect: Vec<Rect>,
}

/// The suite's `pageTransform` wrapper: build the transform, then call each
/// method on the probe points and rects.
fn page_transform_probe(a: &[Value]) -> Out {
    let t = page_transform(arg(a, 0)?).map_err(thrown)?;
    let probe: Option<Probe> = arg(a, 1)?;
    let probe = probe.unwrap_or_default();
    let points = probe.points.unwrap_or_default();
    let rects = probe.rects.unwrap_or_default();
    let map = |f: fn(&PageTransform, Point) -> Point| points.iter().map(|p| f(&t, *p)).collect();
    let map_rect = |f: fn(&PageTransform, Rect) -> Rect| rects.iter().map(|r| f(&t, *r)).collect();
    ok(PageTransformProbe {
        to_pixels: map(PageTransform::to_pixels),
        from_pixels: map(PageTransform::from_pixels),
        content_to_view: map(PageTransform::content_to_view),
        view_to_content: map(PageTransform::view_to_content),
        content_to_view_rect: map_rect(PageTransform::content_to_view_rect),
        view_to_content_rect: map_rect(PageTransform::view_to_content_rect),
        transform: t,
    })
}

#[derive(Deserialize)]
struct LeftTop {
    left: f64,
    top: f64,
}

pub fn functions() -> FunctionTable {
    vec![
        ("NO_FRAME", |_| ok(NO_FRAME)),
        ("boundsOfRects", |a| ok(bounds_of_rects(&arg::<Vec<Rect>>(a, 0)?))),
        ("intersectRects", |a| ok(intersect_rects(arg(a, 0)?, arg(a, 1)?))),
        ("isQuarterTurn", |a| ok(is_quarter_turn(arg(a, 0)?))),
        ("addRotations", |a| ok(add_rotations(arg(a, 0)?, arg(a, 1)?))),
        ("snapToDevice", |a| ok(snap_to_device(arg(a, 0)?, arg(a, 1)?))),
        ("displaySize", |a| ok(display_size(arg(a, 0)?, arg(a, 1)?))),
        ("deviceHeightForWidth", |a| ok(device_height_for_width(arg(a, 0)?, arg(a, 1)?))),
        ("identity", |_| ok(identity())),
        ("compose", |a| ok(compose(arg(a, 0)?, arg(a, 1)?))),
        ("invert", |a| ok(invert(arg(a, 0)?).map_err(thrown)?)),
        ("applyPoint", |a| ok(apply_point(arg(a, 0)?, arg(a, 1)?))),
        ("applyRect", |a| ok(apply_rect(arg(a, 0)?, arg(a, 1)?))),
        ("applyQuad", |a| ok(apply_quad(arg(a, 0)?, arg(a, 1)?))),
        ("matrixToCss", |a| ok(matrix_to_css(arg(a, 0)?))),
        ("translate", |a| ok(translate(arg(a, 0)?, arg(a, 1)?))),
        ("scale", |a| ok(scale(arg(a, 0)?, arg(a, 1)?))),
        ("rotate", |a| ok(rotate(arg(a, 0)?))),
        ("rotateAbout", |a| ok(rotate_about(arg(a, 0)?, arg(a, 1)?))),
        ("scaleAbout", |a| ok(scale_about(arg(a, 0)?, arg(a, 1)?, arg(a, 2)?))),
        ("angleOf", |a| ok(angle_of(arg(a, 0)?))),
        ("rotateScaleMatrix", |a| {
            ok(rotate_scale_matrix(arg(a, 0)?, arg(a, 1)?, arg(a, 2)?, arg(a, 3)?))
        }),
        ("pdfToContentMatrix", |a| {
            let LeftTop { left, top } = arg(a, 0)?;
            ok(pdf_to_content_matrix(left, top))
        }),
        ("pageGeometry", |a| ok(page_geometry(arg(a, 0)?, arg(a, 1)?).map_err(thrown)?)),
        ("pageTransform", page_transform_probe),
        ("textQuadFromRect", |a| ok(text_quad_from_rect(arg(a, 0)?))),
        ("textQuadPoints", |a| ok(text_quad_points(arg(a, 0)?))),
        ("textQuadRing", |a| ok(text_quad_ring(arg(a, 0)?))),
        ("textQuadEdge", |a| ok(text_quad_edge(arg(a, 0)?, arg(a, 1)?))),
        ("textQuadEquals", |a| ok(text_quad_equals(arg(a, 0)?, arg(a, 1)?))),
        ("textQuadBounds", |a| ok(text_quad_bounds(arg(a, 0)?))),
        ("applyTextQuad", |a| ok(apply_text_quad(arg(a, 0)?, arg(a, 1)?))),
        ("positionalQuad", |a| ok(positional_quad(arg(a, 0)?))),
        ("normalizeQuad", |a| ok(normalize_quad(arg(a, 0)?))),
    ]
}
