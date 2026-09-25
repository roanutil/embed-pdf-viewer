//! `geometry.ts`: the plugin's coordinate seam, and nothing else. It converts
//! between the viewer's content space (y-down, crop-relative) and the
//! engine's PDF space (y-up): pointer points inward, canonical segments
//! outward. The y-flip is `epdf_geometry::page_geometry`'s.

use epdf_geometry::{
    CropBox, Mat2D, NonInvertible, PageGeometryInput, PageRotation, Point, Rect, TextQuad,
    apply_point, invert, page_geometry, text_quad_bounds,
};
use serde::{Deserialize, Serialize};

use crate::text_layout::{
    PageGeometrySnapshot, PageTextLayout, PdfQuad, PdfRect, PdfTextSegment, build_page_text_layout,
};

/// One merged visual line of a selection, in content space.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SelectionSegment {
    pub quad: TextQuad,
    pub rect: Rect,
    /// Reading direction along the baseline: 1 toward `end`, -1 back.
    pub advance: i32,
}

/// A page's cached selection geometry: the canonical layout plus the seam.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionPageGeometry {
    pub layout: PageTextLayout,
    pub to_content: Mat2D,
    pub from_content: Mat2D,
}

/// `buildSelectionPageGeometry`. Display rotation does not enter the seam;
/// it is carried for the signature. `Err` is where the TypeScript's
/// `pageGeometry` throws: a crop box or `userUnit` with no area.
pub fn build_selection_page_geometry(
    snapshot: &PageGeometrySnapshot,
    crop: PdfRect,
    rotation: PageRotation,
    user_unit: f64,
) -> Result<SelectionPageGeometry, NonInvertible> {
    let crop = CropBox { left: crop.left, bottom: crop.bottom, right: crop.right, top: crop.top };
    let pdf_to_content =
        page_geometry(PageGeometryInput { crop, rotation, user_unit }, 1.0)?.pdf_to_content;
    Ok(SelectionPageGeometry {
        layout: build_page_text_layout(snapshot),
        to_content: pdf_to_content,
        from_content: invert(pdf_to_content)?,
    })
}

/// A content-space pointer point in the layout's PDF space.
pub fn content_point_to_pdf(geom: &SelectionPageGeometry, p: Point) -> Point {
    apply_point(geom.from_content, p)
}

/// A canonical PDF quad in content space; the y-flip maps corners straight
/// onto their names.
pub fn to_content_text_quad(geom: &SelectionPageGeometry, quad: &PdfQuad) -> TextQuad {
    let pt = |p: Point| apply_point(geom.to_content, p);
    TextQuad {
        upper_start: pt(quad.p1),
        upper_end: pt(quad.p2),
        lower_start: pt(quad.p3),
        lower_end: pt(quad.p4),
    }
}

/// A canonical segment in content space, `rect` recomputed from the quad.
pub fn to_content_segment(
    geom: &SelectionPageGeometry,
    segment: &PdfTextSegment,
) -> SelectionSegment {
    let quad = to_content_text_quad(geom, &segment.quad);
    SelectionSegment { quad, rect: text_quad_bounds(quad), advance: segment.advance }
}
