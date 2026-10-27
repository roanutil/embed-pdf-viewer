//! Page space: the unrotated page's own frame, origin at the top-left of the
//! visible page, y down, in points. The engine gives and takes every position
//! in it. `core-geometry`'s `page-space.ts` names its values; `engine-core`'s
//! `geometry/pageSpace.ts` converts them to and from the file's coordinates.
//!
//! The conversion is a shift and a flip given the page's visible box in PDF
//! space, nothing else, so it is exact: a value converts and converts back to
//! the same number. A page's own turn (`/Rotate`) never changes a number, and
//! `userUnit` is applied by the view ([`crate::page_transform`]), not here.

use crate::{PdfPoint, PdfQuad, PdfRect};
use crate::{Point, Quad, Rect};

/// A point in page space: x from the visible page's left edge, y down from
/// its top edge.
pub type PagePoint = Point;

/// A rect in page space: `x, y` is its top-left corner.
pub type PageRect = Rect;

/// `engine-core`'s name for a [`PageRect`].
pub type PageBox = Rect;

/// A quad in page space, with [`PdfQuad`]'s corner names: they belong to the
/// text's frame, so the flip keeps them.
pub type PageQuad = Quad;

/// A point in the file's coordinates, in page space. `visible` is the page's
/// visible box.
pub fn page_point_of(point: PdfPoint, visible: PdfRect) -> PagePoint {
    Point { x: point.x - visible.left, y: visible.top - point.y }
}

/// A page-space point in the file's coordinates.
pub fn pdf_point_of(point: PagePoint, visible: PdfRect) -> PdfPoint {
    Point { x: point.x + visible.left, y: visible.top - point.y }
}

/// A normalized rect in the file's coordinates, in page space.
pub fn page_box_of(rect: PdfRect, visible: PdfRect) -> PageBox {
    Rect {
        x: rect.left - visible.left,
        y: visible.top - rect.top,
        width: rect.right - rect.left,
        height: rect.top - rect.bottom,
    }
}

/// A page-space box in the file's coordinates.
pub fn pdf_rect_of(rect: PageBox, visible: PdfRect) -> PdfRect {
    let left = rect.x + visible.left;
    let top = visible.top - rect.y;
    PdfRect { left, right: left + rect.width, bottom: top - rect.height, top }
}

/// A quad in the file's coordinates, in page space.
pub fn page_quad_of(quad: PdfQuad, visible: PdfRect) -> PageQuad {
    Quad {
        upper_left: page_point_of(quad.upper_left, visible),
        upper_right: page_point_of(quad.upper_right, visible),
        lower_left: page_point_of(quad.lower_left, visible),
        lower_right: page_point_of(quad.lower_right, visible),
    }
}

/// A page-space quad in the file's coordinates.
pub fn pdf_quad_of(quad: PageQuad, visible: PdfRect) -> PdfQuad {
    Quad {
        upper_left: pdf_point_of(quad.upper_left, visible),
        upper_right: pdf_point_of(quad.upper_right, visible),
        lower_left: pdf_point_of(quad.lower_left, visible),
        lower_right: pdf_point_of(quad.lower_right, visible),
    }
}
