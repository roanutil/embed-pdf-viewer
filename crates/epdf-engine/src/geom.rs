//! G1. Geometry primitives and the coordinate contract.
//!
//! Replaces `core/src/geometry/{primitives,convert}.ts`.
//!
//! # The coordinate contract
//!
//! These are the ONE geometry vocabulary the engine API speaks, and they are
//! PDF user space:
//!
//! - y-UP. The origin is the page box's bottom-left, so `top > bottom`.
//! - Edges, not x/y/width/height.
//! - The page-box origin is preserved, NOT normalized away, so `left` and
//!   `bottom` may be non-zero or negative.
//! - No device scale, no DOM, no device pixels.
//! - Page identity travels with a coordinate as a `PageObjectNumber`, never as
//!   an index: an index is not identity across reordering
//!   (`crate::identity`).
//! - Crop and rotation are presentation transforms applied by the viewer at
//!   display time. `/Rotate` is metadata; content coordinates stay y-up.
//!
//! Viewer-local geometry (content, view and screen space, y-down,
//! x/y/width/height) is produced from these by an explicit transform in the
//! viewer layer, and never appears here.

use serde::{Deserialize, Serialize};

/// A point in PDF user space (y-up).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PdfPoint {
    pub x: f64,
    pub y: f64,
}

/// A rectangle in the file's coordinates, as edges.
///
/// Normalized means `left <= right` and `bottom <= top`. Producers reading a
/// raw box out of PDFium pass it through [`normalize_pdf_rect`] first, so
/// downstream `right - left` is never negative.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PdfRect {
    pub left: f64,
    pub bottom: f64,
    pub right: f64,
    pub top: f64,
}

/// A width/height pair in PDF points.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PdfSize {
    pub width: f64,
    pub height: f64,
}

/// Origin + size form of a [`PdfRect`], still y-up (origin = bottom-left).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PdfOriginSize {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A `/QuadPoints` quad. The four points are POSITIONAL, in PDFium
/// `FS_QUADPOINTSF` order (PDF 32000 §12.5.6.10).
///
/// This type asserts NO corner semantics. Quads can be rotated or skewed, and
/// PDF producers disagree on corner order, so naming corners on the wire would
/// be a false guarantee. Derive named corners with [`pdf_quad_corners`], which
/// is valid only for axis-aligned quads, or take the enclosing box with
/// [`pdf_quad_bounds`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PdfQuad {
    pub p1: PdfPoint,
    pub p2: PdfPoint,
    pub p3: PdfPoint,
    pub p4: PdfPoint,
}

/// Named corners of a quad, y-up. Valid only for upright quads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PdfQuadCorners {
    pub top_left: PdfPoint,
    pub top_right: PdfPoint,
    pub bottom_left: PdfPoint,
    pub bottom_right: PdfPoint,
}

/// A page's rotation in degrees clockwise, the `/Rotate` values PDF permits.
/// Presentation metadata only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(into = "u16", try_from = "u16")]
pub enum PdfRotation {
    None,
    Clockwise90,
    Clockwise180,
    Clockwise270,
}

impl PdfRotation {
    /// The `/Rotate` value divided by ninety, which is what the fork's
    /// page-rotation helper takes. The wire speaks degrees and PDFium
    /// speaks quarter turns; the conversion lives here so the two
    /// vocabularies meet once rather than at every call site.
    pub fn quarter_turns(self) -> i32 {
        i32::from(self.degrees() / 90)
    }

    pub fn degrees(self) -> u16 {
        match self {
            Self::None => 0,
            Self::Clockwise90 => 90,
            Self::Clockwise180 => 180,
            Self::Clockwise270 => 270,
        }
    }
}

impl From<PdfRotation> for u16 {
    fn from(r: PdfRotation) -> u16 {
        r.degrees()
    }
}

impl TryFrom<u16> for PdfRotation {
    type Error = String;

    fn try_from(degrees: u16) -> Result<Self, Self::Error> {
        match degrees {
            0 => Ok(Self::None),
            90 => Ok(Self::Clockwise90),
            180 => Ok(Self::Clockwise180),
            270 => Ok(Self::Clockwise270),
            other => Err(format!("{other} is not a /Rotate value")),
        }
    }
}

/// The two endpoints of a line annotation's `/L` entry.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LinePoints {
    pub start: PdfPoint,
    pub end: PdfPoint,
}

/// One continuous pen path: an ordered point list from one `/InkList`
/// sub-array.
pub type InkStroke = Vec<PdfPoint>;

/// An ink annotation's `/InkList`.
pub type InkList = Vec<InkStroke>;

/// A free-text callout's `/CL` leader line. Two points draw a straight leader,
/// three draw a knee-jointed one. The last point touches the text box; the
/// first is the point being called out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CalloutLine {
    Straight([PdfPoint; 2]),
    Kneed([PdfPoint; 3]),
}

/// Normalizes a rect to the y-up invariant, whatever corner order the file
/// used.
///
/// THE enforcement point for "a `PdfRect` is always normalized". Every producer
/// reading a raw box from PDFium passes it through here, so the invariant holds
/// once, centrally.
pub fn normalize_pdf_rect(r: PdfRect) -> PdfRect {
    PdfRect {
        left: r.left.min(r.right),
        right: r.left.max(r.right),
        bottom: r.top.min(r.bottom),
        top: r.top.max(r.bottom),
    }
}

pub fn pdf_rect_width(r: PdfRect) -> f64 {
    r.right - r.left
}

pub fn pdf_rect_height(r: PdfRect) -> f64 {
    r.top - r.bottom
}

pub fn pdf_rect_size(r: PdfRect) -> PdfSize {
    PdfSize { width: r.right - r.left, height: r.top - r.bottom }
}

/// Edges to origin+size, staying y-up (origin at bottom-left).
pub fn pdf_rect_to_origin_size(r: PdfRect) -> PdfOriginSize {
    PdfOriginSize { x: r.left, y: r.bottom, width: r.right - r.left, height: r.top - r.bottom }
}

/// y-up origin+size back to edges.
pub fn pdf_rect_from_origin_size(o: PdfOriginSize) -> PdfRect {
    PdfRect { left: o.x, bottom: o.y, right: o.x + o.width, top: o.y + o.height }
}

/// Enclosing axis-aligned box of a quad. Orientation agnostic, so it is
/// correct for rotated and skewed quads.
pub fn pdf_quad_bounds(q: PdfQuad) -> PdfRect {
    let xs = [q.p1.x, q.p2.x, q.p3.x, q.p4.x];
    let ys = [q.p1.y, q.p2.y, q.p3.y, q.p4.y];
    PdfRect {
        left: xs.iter().copied().fold(f64::INFINITY, f64::min),
        right: xs.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        bottom: ys.iter().copied().fold(f64::INFINITY, f64::min),
        top: ys.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    }
}

/// Axis-aligned named-corner reading of a quad.
///
/// VALID ONLY for upright quads: it is derived from the enclosing bounds, so
/// for a rotated or skewed quad it returns the bounding box's corners, not the
/// quad's. For arbitrary quads use `p1..p4` or [`pdf_quad_bounds`].
pub fn pdf_quad_corners(q: PdfQuad) -> PdfQuadCorners {
    let b = pdf_quad_bounds(q);
    PdfQuadCorners {
        top_left: PdfPoint { x: b.left, y: b.top },
        top_right: PdfPoint { x: b.right, y: b.top },
        bottom_left: PdfPoint { x: b.left, y: b.bottom },
        bottom_right: PdfPoint { x: b.right, y: b.bottom },
    }
}

/// Builds a quad from named corners in PDFium slot order (`p1` = top-left,
/// `p2` = top-right, `p3` = bottom-left, `p4` = bottom-right). The inverse of
/// [`pdf_quad_corners`] for upright quads.
pub fn pdf_quad_from_corners(c: PdfQuadCorners) -> PdfQuad {
    PdfQuad { p1: c.top_left, p2: c.top_right, p3: c.bottom_left, p4: c.bottom_right }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: f64, bottom: f64, right: f64, top: f64) -> PdfRect {
        PdfRect { left, bottom, right, top }
    }

    fn point(x: f64, y: f64) -> PdfPoint {
        PdfPoint { x, y }
    }

    #[test]
    fn normalizing_orders_edges_whatever_order_the_file_used() {
        let normalized = rect(10.0, 20.0, 30.0, 40.0);
        assert_eq!(normalize_pdf_rect(normalized), normalized);
        // Swapped on both axes, which is what an FS_RECTF from a rotated page
        // can look like.
        assert_eq!(normalize_pdf_rect(rect(30.0, 40.0, 10.0, 20.0)), normalized);
        assert_eq!(pdf_rect_width(normalized), 20.0);
        assert_eq!(pdf_rect_height(normalized), 20.0);
    }

    #[test]
    fn negative_origins_survive_normalization() {
        // The page box's origin is preserved, so a rect below and left of it
        // stays where it is rather than being shifted to the first quadrant.
        let r = normalize_pdf_rect(rect(-30.0, -5.0, -10.0, 15.0));
        assert_eq!(r, rect(-30.0, -5.0, -10.0, 15.0));
        assert_eq!(pdf_rect_size(r), PdfSize { width: 20.0, height: 20.0 });
    }

    #[test]
    fn origin_size_round_trips() {
        let r = rect(10.0, 20.0, 30.0, 45.0);
        assert_eq!(
            pdf_rect_to_origin_size(r),
            PdfOriginSize { x: 10.0, y: 20.0, width: 20.0, height: 25.0 }
        );
        assert_eq!(pdf_rect_from_origin_size(pdf_rect_to_origin_size(r)), r);
    }

    #[test]
    fn quad_bounds_enclose_a_rotated_quad() {
        // A square rotated 45 degrees about the origin.
        let q = PdfQuad {
            p1: point(0.0, 1.0),
            p2: point(1.0, 0.0),
            p3: point(-1.0, 0.0),
            p4: point(0.0, -1.0),
        };
        assert_eq!(pdf_quad_bounds(q), rect(-1.0, -1.0, 1.0, 1.0));
    }

    #[test]
    fn named_corners_round_trip_only_for_upright_quads() {
        let upright = PdfQuad {
            p1: point(1.0, 4.0),
            p2: point(3.0, 4.0),
            p3: point(1.0, 2.0),
            p4: point(3.0, 2.0),
        };
        assert_eq!(pdf_quad_from_corners(pdf_quad_corners(upright)), upright);

        // For a rotated quad the named corners are the bounding box's, which
        // is why the helper is documented as upright-only.
        let rotated = PdfQuad {
            p1: point(0.0, 1.0),
            p2: point(1.0, 0.0),
            p3: point(-1.0, 0.0),
            p4: point(0.0, -1.0),
        };
        assert_ne!(pdf_quad_from_corners(pdf_quad_corners(rotated)), rotated);
    }

    #[test]
    fn rotation_accepts_only_the_four_rotate_values() {
        for degrees in [0u16, 90, 180, 270] {
            assert_eq!(PdfRotation::try_from(degrees).unwrap().degrees(), degrees);
        }
        assert!(PdfRotation::try_from(45).is_err());
        assert_eq!(serde_json::to_value(PdfRotation::Clockwise90).unwrap(), serde_json::json!(90));
    }
}
