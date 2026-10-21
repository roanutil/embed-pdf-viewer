//! Page space, and the one boundary between it and the file's coordinates.
//!
//! `core/src/geometry/pageSpace.ts`, `core/src/pageSpace/` and
//! `services/src/worker-host/pageSpaceBoundary.ts`.
//!
//! Page space is where things are on a page as you see it: x runs right from
//! the left edge of the page's visible box, y runs down from its top edge, in
//! PDF units, on the page as stored. A page's own turn (`/Rotate`) never
//! changes a number. The visible box is the crop box inside the media box
//! ([`crate::geom::page_boxes_of`]).
//!
//! The file keeps PDF user space (y up, its origin wherever the file puts it),
//! and so do the readers and writers in [`crate::features`]. Every position
//! the wire hands out or takes is page space, and [`boundary`] converts on
//! the way out and on the way in, once, in [`crate::host::Host::call`]. A
//! family adds its own DTO's conversion here, never inside a feature.
//!
//! The conversion is a shift and a flip, nothing else, so it is exact: a
//! value read from a file (a 32-bit float) converts and converts back to the
//! same number.

use crate::error::EngineError;
use crate::geom::{NamedPdfQuad, PagePoint, PageQuad, PageRect, PdfPoint, PdfQuad, PdfRect};
use crate::identity::PageObjectNumber;

pub mod annotations;
pub mod boundary;
pub mod destinations;
pub mod forms;
pub mod helpers;
pub mod pages;
pub mod rendering;
pub mod text;

/// The types a value's positions take, for a type that exists in both spaces
/// (`Coordinates`): the file's coordinates ([`PdfCoordinates`]) or page space
/// ([`PageCoordinates`]).
pub trait Coordinates: std::fmt::Debug + Clone + Copy + PartialEq + 'static {
    /// A box: [`PdfRect`] or [`PageRect`].
    type Rect: std::fmt::Debug
        + Clone
        + Copy
        + PartialEq
        + serde::Serialize
        + serde::de::DeserializeOwned;
    /// A quad: [`NamedPdfQuad`] or [`PageQuad`].
    type Quad: std::fmt::Debug
        + Clone
        + Copy
        + PartialEq
        + serde::Serialize
        + serde::de::DeserializeOwned;
    /// A destination: [`crate::features::actions::destination::PdfDestination`]
    /// or [`destinations::PageDestination`].
    type Destination: std::fmt::Debug
        + Clone
        + Copy
        + PartialEq
        + serde::Serialize
        + serde::de::DeserializeOwned;
}

/// The file's coordinates (`PdfCoordinates`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PdfCoordinates;

impl Coordinates for PdfCoordinates {
    type Rect = PdfRect;
    type Quad = NamedPdfQuad;
    type Destination = crate::features::actions::destination::PdfDestination;
}

/// Page space (`PageCoordinates`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageCoordinates;

impl Coordinates for PageCoordinates {
    type Rect = PageRect;
    type Quad = PageQuad;
    type Destination = destinations::PageDestination;
}

/// The visible box of any page of the document, in the file's coordinates
/// (`VisibleBoxOf`). A destination is measured on the page it goes to, so
/// converting one needs that page's box, not the box of the page it sits on.
/// A single page's own box is `&mut |_| Ok(box)` (`DestinationBox`).
pub type VisibleBoxOf<'a> = dyn FnMut(PageObjectNumber) -> Result<PdfRect, EngineError> + 'a;

// ── primitives (`geometry/pageSpace.ts`) ──

/// A point in the file's coordinates, in page space (`pagePointOf`).
/// `visible` is the page's visible box.
pub fn page_point_of(point: PdfPoint, visible: PdfRect) -> PagePoint {
    PagePoint { x: point.x - visible.left, y: visible.top - point.y }
}

/// A page-space point in the file's coordinates (`pdfPointOf`).
pub fn pdf_point_of(point: PagePoint, visible: PdfRect) -> PdfPoint {
    PdfPoint { x: point.x + visible.left, y: visible.top - point.y }
}

/// A normalized rect in the file's coordinates, in page space
/// (`pageBoxOf`).
pub fn page_rect_of(rect: PdfRect, visible: PdfRect) -> PageRect {
    PageRect {
        x: rect.left - visible.left,
        y: visible.top - rect.top,
        width: rect.right - rect.left,
        height: rect.top - rect.bottom,
    }
}

/// A page-space box in the file's coordinates (`pdfRectOf`).
pub fn pdf_rect_of(rect: PageRect, visible: PdfRect) -> PdfRect {
    let left = rect.x + visible.left;
    let top = visible.top - rect.y;
    PdfRect { left, bottom: top - rect.height, right: left + rect.width, top }
}

/// A quad in the file's coordinates, in page space (`pageQuadOf`). This
/// crate's [`PdfQuad`] is positional in `FS_QUADPOINTSF` order, which is the
/// order Acrobat names the corners in: upper-left, upper-right, lower-left,
/// lower-right.
pub fn page_quad_of(quad: PdfQuad, visible: PdfRect) -> PageQuad {
    PageQuad {
        upper_left: page_point_of(quad.p1, visible),
        upper_right: page_point_of(quad.p2, visible),
        lower_left: page_point_of(quad.p3, visible),
        lower_right: page_point_of(quad.p4, visible),
    }
}

/// A page-space quad in the file's coordinates (`pdfQuadOf`).
pub fn pdf_quad_of(quad: PageQuad, visible: PdfRect) -> PdfQuad {
    PdfQuad {
        p1: pdf_point_of(quad.upper_left, visible),
        p2: pdf_point_of(quad.upper_right, visible),
        p3: pdf_point_of(quad.lower_left, visible),
        p4: pdf_point_of(quad.lower_right, visible),
    }
}

// ── mirroring ──
//
// Math written for the file's coordinates (y up) runs unchanged on page-space
// values flipped top to bottom: the flip turns page space into the file's
// coordinates shifted, and none of that math depends on where the origin is.
// Flipping needs no page box and is exact.

/// A page-space point flipped into y-up coordinates (`mirroredPoint`).
pub fn mirrored_point(point: PagePoint) -> PdfPoint {
    PdfPoint { x: point.x, y: -point.y }
}

/// A y-up point flipped back into page space (`unmirroredPoint`).
pub fn unmirrored_point(point: PdfPoint) -> PagePoint {
    PagePoint { x: point.x, y: -point.y }
}

/// A page-space box flipped into a y-up rect (`mirroredRect`).
pub fn mirrored_rect(rect: PageRect) -> PdfRect {
    PdfRect {
        left: rect.x,
        right: rect.x + rect.width,
        bottom: -(rect.y + rect.height),
        top: -rect.y,
    }
}

/// A y-up rect flipped back into a page-space box (`unmirroredBox`).
pub fn unmirrored_rect(rect: PdfRect) -> PageRect {
    PageRect {
        x: rect.left,
        y: -rect.top,
        width: rect.right - rect.left,
        height: rect.top - rect.bottom,
    }
}

/// A page-space quad flipped into y-up coordinates (`mirroredQuad`).
pub fn mirrored_quad(quad: PageQuad) -> PdfQuad {
    PdfQuad {
        p1: mirrored_point(quad.upper_left),
        p2: mirrored_point(quad.upper_right),
        p3: mirrored_point(quad.lower_left),
        p4: mirrored_point(quad.lower_right),
    }
}

/// A y-up quad flipped back into page space (`unmirroredQuad`).
pub fn unmirrored_quad(quad: PdfQuad) -> PageQuad {
    PageQuad {
        upper_left: unmirrored_point(quad.p1),
        upper_right: unmirrored_point(quad.p2),
        lower_left: unmirrored_point(quad.p3),
        lower_right: unmirrored_point(quad.p4),
    }
}

#[cfg(test)]
mod tests {
    //! `packages/engine/core/test/pageSpace/primitives.test.ts`.

    use super::*;
    use crate::geom::normalize_pdf_rect;

    /// A value as a file holds it: a 32-bit float.
    fn f32_of(value: f64) -> f64 {
        f64::from(value as f32)
    }

    /// The test's small, repeatable random source.
    fn random(seed: u64) -> impl FnMut() -> f64 {
        let mut state = seed;
        move || {
            state = (state * 1_103_515_245 + 12_345) % 2_147_483_648;
            state as f64 / 2_147_483_648.0
        }
    }

    fn rect(left: f64, bottom: f64, right: f64, top: f64) -> PdfRect {
        PdfRect { left, bottom, right, top }
    }

    fn visible_boxes() -> [PdfRect; 5] {
        [
            rect(0.0, 0.0, 612.0, 792.0),
            rect(-306.0, -396.0, 306.0, 396.0),
            rect(-1685.0, -1192.0, 1685.0, 1192.0),
            rect(50.0, 60.0, 562.0, 732.0),
            rect(f32_of(12.345), f32_of(-7.25), f32_of(600.125), f32_of(781.5)),
        ]
    }

    #[test]
    fn measure_from_the_top_left_of_the_visible_box_y_down() {
        let visible = rect(50.0, 60.0, 562.0, 732.0);
        assert_eq!(
            page_point_of(PdfPoint { x: 50.0, y: 732.0 }, visible),
            PagePoint { x: 0.0, y: 0.0 }
        );
        assert_eq!(
            page_point_of(PdfPoint { x: 150.0, y: 632.0 }, visible),
            PagePoint { x: 100.0, y: 100.0 }
        );
        assert_eq!(
            page_rect_of(rect(100.0, 600.0, 150.0, 650.0), visible),
            PageRect { x: 50.0, y: 82.0, width: 50.0, height: 50.0 }
        );
    }

    #[test]
    fn every_value_a_file_can_hold_comes_back_exactly() {
        let same = |a: f64, b: f64| a.to_bits() == b.to_bits();
        for (i, visible) in visible_boxes().into_iter().enumerate() {
            let mut next = random(i as u64 + 1);
            let mut value = || f32_of((next() - 0.5) * 20000.0);
            for _ in 0..2000 {
                let point = PdfPoint { x: value(), y: value() };
                let back = pdf_point_of(page_point_of(point, visible), visible);
                assert!(same(back.x, point.x) && same(back.y, point.y), "box {i}: {point:?}");

                let (a, b) = (value(), value());
                let (c, d) = (value(), value());
                let r = rect(a.min(b), c.min(d), a.max(b), c.max(d));
                assert_eq!(
                    pdf_rect_of(page_rect_of(r, visible), visible),
                    normalize_pdf_rect(r),
                    "box {i}"
                );

                let quad = PdfQuad {
                    p1: point,
                    p2: PdfPoint { x: a, y: c },
                    p3: PdfPoint { x: b, y: d },
                    p4: PdfPoint { x: a, y: d },
                };
                assert_eq!(pdf_quad_of(page_quad_of(quad, visible), visible), quad, "box {i}");
            }
        }
    }

    #[test]
    fn a_mirror_is_its_own_inverse_and_needs_no_box() {
        let page = PageRect { x: 10.0, y: 20.0, width: 30.0, height: 40.0 };
        assert_eq!(mirrored_rect(page), rect(10.0, -60.0, 40.0, -20.0));
        assert_eq!(unmirrored_rect(mirrored_rect(page)), page);
        let quad = PageQuad {
            upper_left: PagePoint { x: 1.0, y: 2.0 },
            upper_right: PagePoint { x: 3.0, y: 2.0 },
            lower_left: PagePoint { x: 1.0, y: 5.0 },
            lower_right: PagePoint { x: 3.0, y: 5.0 },
        };
        assert_eq!(unmirrored_quad(mirrored_quad(quad)), quad);
    }
}
