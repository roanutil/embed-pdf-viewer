//! The conformance adapter: the function table the vectors call by name, with
//! arguments and results in canonical JSON.
//!
//! The names are the ones `tooling/plugin-conformance/src/suites/geometry.ts`
//! records. Some keep the case's historical label as the function name
//! (`textQuadEdge` calls `quadEdge`, `applyTextQuad` calls `applyQuad`), and
//! the page suite calls `engine-core`'s page-space conversions through the
//! generator's `enginePageSpace` wrapper.

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

/// The generator's `pageTransformProbe` result. Its method keys keep the
/// names they had before 81bdc82ec renamed content space to page space.
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
        content_to_view: map(PageTransform::page_to_view),
        view_to_content: map(PageTransform::view_to_page),
        content_to_view_rect: map_rect(PageTransform::page_to_view_rect),
        view_to_content_rect: map_rect(PageTransform::view_to_page_rect),
        transform: t,
    })
}

#[derive(Deserialize, Default)]
struct SpaceProbe {
    #[serde(default)]
    points: Option<Vec<Point>>,
    #[serde(default)]
    rects: Option<Vec<PageBox>>,
    #[serde(default)]
    edges: Option<Vec<PdfRect>>,
    #[serde(default)]
    quads: Option<Vec<Quad>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EnginePageSpaceProbe {
    pdf_to_page: Vec<PagePoint>,
    page_to_pdf: Vec<PdfPoint>,
    pdf_rect_to_page: Vec<PageBox>,
    page_rect_to_pdf: Vec<PdfRect>,
    pdf_quad_to_page: Vec<PageQuad>,
    page_quad_to_pdf: Vec<PdfQuad>,
}

/// The suite's `enginePageSpace` wrapper: each `engine-core` page-space
/// conversion on the probes, against one visible box.
fn engine_page_space_probe(a: &[Value]) -> Out {
    let visible: PdfRect = arg(a, 0)?;
    let probe: Option<SpaceProbe> = arg(a, 1)?;
    let probe = probe.unwrap_or_default();
    let points = probe.points.unwrap_or_default();
    let quads = probe.quads.unwrap_or_default();
    ok(EnginePageSpaceProbe {
        pdf_to_page: points.iter().map(|p| page_point_of(*p, visible)).collect(),
        page_to_pdf: points.iter().map(|p| pdf_point_of(*p, visible)).collect(),
        pdf_rect_to_page: probe
            .edges
            .unwrap_or_default()
            .into_iter()
            .map(|e| page_box_of(e, visible))
            .collect(),
        page_rect_to_pdf: probe
            .rects
            .unwrap_or_default()
            .into_iter()
            .map(|r| pdf_rect_of(r, visible))
            .collect(),
        pdf_quad_to_page: quads.iter().map(|q| page_quad_of(*q, visible)).collect(),
        page_quad_to_pdf: quads.iter().map(|q| pdf_quad_of(*q, visible)).collect(),
    })
}

pub fn functions() -> FunctionTable {
    vec![
        // geometry-basics.json
        ("NO_FRAME", |_| ok(NO_FRAME)),
        ("boundsOfRects", |a| ok(bounds_of_rects(&arg::<Vec<Rect>>(a, 0)?))),
        ("intersectRects", |a| ok(intersect_rects(arg(a, 0)?, arg(a, 1)?))),
        ("rectFromCorners", |a| ok(rect_from_corners(arg(a, 0)?, arg(a, 1)?))),
        ("rectContains", |a| ok(rect_contains(arg(a, 0)?, arg(a, 1)?))),
        ("rectsOverlap", |a| ok(rects_overlap(arg(a, 0)?, arg(a, 1)?))),
        ("isQuarterTurn", |a| ok(is_quarter_turn(arg(a, 0)?))),
        ("addRotations", |a| ok(add_rotations(arg(a, 0)?, arg(a, 1)?))),
        ("snapToDevice", |a| ok(snap_to_device(arg(a, 0)?, arg(a, 1)?))),
        ("displaySize", |a| ok(display_size(arg(a, 0)?, arg(a, 1)?))),
        ("deviceHeightForWidth", |a| ok(device_height_for_width(arg(a, 0)?, arg(a, 1)?))),
        // geometry-affine.json
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
        // geometry-page.json (also `rectsOverlap`, above)
        ("rotateScaleMatrix", |a| {
            ok(rotate_scale_matrix(arg(a, 0)?, arg(a, 1)?, arg(a, 2)?, arg(a, 3)?))
        }),
        ("pageTransform", page_transform_probe),
        ("enginePageSpace", engine_page_space_probe),
        ("pdfQuadBounds", |a| ok(pdf_quad_bounds(arg(a, 0)?))),
        // geometry-text-quad.json
        ("textQuadFromRect", |a| ok(quad_from_rect(arg(a, 0)?))),
        ("textQuadPoints", |a| ok(quad_corners(arg(a, 0)?))),
        ("quadCorners", |a| ok(quad_corners(arg(a, 0)?))),
        ("textQuadRing", |a| ok(quad_ring(arg(a, 0)?))),
        ("textQuadEdge", |a| ok(quad_edge(arg(a, 0)?, arg(a, 1)?))),
        ("textQuadEquals", |a| ok(quad_equals(arg(a, 0)?, arg(a, 1)?))),
        ("textQuadBounds", |a| ok(quad_bounds(arg(a, 0)?))),
        ("applyTextQuad", |a| ok(apply_quad(arg(a, 0)?, arg(a, 1)?))),
        ("normalizePdfQuad", |a| ok(normalize_pdf_quad(arg(a, 0)?))),
    ]
}
