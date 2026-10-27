//! The engine's geometry helpers, for page-space values
//! (`core/src/pageSpace/helpers.ts`, the geometry half).
//!
//! Each flips its values top to bottom, runs the helper written for the
//! file's coordinates (y up), and flips the answer back: exact, and needing no
//! page box, since none of that math depends on where the origin is. The
//! annotation and glyph helpers in the same upstream file arrive with the
//! families that own those DTOs.

use serde::{Deserialize, Serialize};

use super::{mirrored_point, mirrored_quad, unmirrored_point, unmirrored_rect};
use crate::geom::{
    PagePoint, PageQuad, PageRect, PdfPoint, PdfPointTurn, pdf_point_turned, pdf_point_unturned,
    pdf_points_bounds, pdf_quad_bounds, pdf_turn_of_drawn, pdf_turn_of_upright,
};

/// A turn in page space: degrees clockwise, as the page shows it, about
/// `center` (`PagePointTurn`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PagePointTurn {
    pub degrees: f64,
    pub center: PagePoint,
}

fn mirrored_turn(turn: PagePointTurn) -> PdfPointTurn {
    PdfPointTurn { degrees: turn.degrees, center: mirrored_point(turn.center) }
}

fn unmirrored_turn(turn: PdfPointTurn) -> PagePointTurn {
    PagePointTurn { degrees: turn.degrees, center: unmirrored_point(turn.center) }
}

fn mirrored_points(points: &[PagePoint]) -> Vec<PdfPoint> {
    points.iter().copied().map(mirrored_point).collect()
}

/// The box around `points`; a zero box at the origin when there are none
/// (`pagePointsBounds`).
pub fn page_points_bounds(points: &[PagePoint]) -> PageRect {
    unmirrored_rect(pdf_points_bounds(&mirrored_points(points)))
}

/// `point` turned by `turn`; a negative angle turns it back
/// (`pagePointTurned`).
pub fn page_point_turned(point: PagePoint, turn: PagePointTurn) -> PagePoint {
    unmirrored_point(pdf_point_turned(mirrored_point(point), mirrored_turn(turn)))
}

/// `point` turned back by `turn` (`pagePointUnturned`).
pub fn page_point_unturned(point: PagePoint, turn: PagePointTurn) -> PagePoint {
    unmirrored_point(pdf_point_unturned(mirrored_point(point), mirrored_turn(turn)))
}

/// The turn that draws upright `points`: `degrees` about the middle of their
/// box (`pageTurnOfUpright`).
pub fn page_turn_of_upright(points: &[PagePoint], degrees: f64) -> PagePointTurn {
    unmirrored_turn(pdf_turn_of_upright(&mirrored_points(points), degrees))
}

/// The turn drawn `points` were made with: `degrees` about the middle of
/// their upright box (`pageTurnOfDrawn`).
pub fn page_turn_of_drawn(points: &[PagePoint], degrees: f64) -> PagePointTurn {
    unmirrored_turn(pdf_turn_of_drawn(&mirrored_points(points), degrees))
}

/// The upright box around a quad, whatever its turn or skew
/// (`pageQuadBounds`).
pub fn page_quad_bounds(quad: PageQuad) -> PageRect {
    unmirrored_rect(pdf_quad_bounds(mirrored_quad(quad)))
}

#[cfg(test)]
mod tests {
    //! `packages/engine/core/test/pageSpace/helpers.test.ts`: the cases
    //! over points, turns and bounds. The glyph-cell, drawn-points and
    //! appearance-turn cases need DTOs later families port.

    use super::*;
    use crate::geom::{PdfQuad, PdfRect};
    use crate::page_space::{page_point_of, page_quad_of, page_rect_of};

    const VISIBLE: PdfRect = PdfRect { left: -306.0, bottom: -396.0, right: 306.0, top: 396.0 };

    fn to_page(point: PdfPoint) -> PagePoint {
        page_point_of(point, VISIBLE)
    }

    /// The test's small, repeatable random source, seeded as upstream's is.
    fn random(seed: u64) -> impl FnMut() -> f64 {
        let mut state = seed;
        move || {
            state = (state * 1_103_515_245 + 12_345) % 2_147_483_648;
            state as f64 / 2_147_483_648.0
        }
    }

    fn close(actual: &[f64], expected: &[f64]) {
        assert_eq!(actual.len(), expected.len());
        for (a, e) in actual.iter().zip(expected) {
            // `toBeCloseTo(e, 9)`.
            assert!((a - e).abs() < 5e-10, "{actual:?} vs {expected:?}");
        }
    }

    fn point_numbers(p: PagePoint) -> [f64; 2] {
        [p.x, p.y]
    }

    fn turn_numbers(t: PagePointTurn) -> [f64; 3] {
        [t.degrees, t.center.x, t.center.y]
    }

    fn rect_numbers(r: PageRect) -> [f64; 4] {
        [r.x, r.y, r.width, r.height]
    }

    #[test]
    fn a_clockwise_turn_looks_clockwise_right_of_the_middle_turns_to_below_it() {
        let turned = page_point_turned(
            PagePoint { x: 10.0, y: 0.0 },
            PagePointTurn { degrees: 90.0, center: PagePoint { x: 0.0, y: 0.0 } },
        );
        close(&point_numbers(turned), &[0.0, 10.0]);
    }

    #[test]
    fn turns_agree_with_the_originals() {
        let mut next = random(7);
        let mut coordinate = move || (next() - 0.5) * 600.0;
        let mut point_at = || PdfPoint { x: coordinate(), y: coordinate() };
        let mut degrees_source = random(11);
        for _ in 0..200 {
            let point = point_at();
            let center = point_at();
            let degrees = degrees_source() * 720.0 - 360.0;
            let turned = page_point_turned(
                to_page(point),
                PagePointTurn { degrees, center: to_page(center) },
            );
            let expected = to_page(pdf_point_turned(point, PdfPointTurn { degrees, center }));
            close(&point_numbers(turned), &point_numbers(expected));
            let back =
                page_point_unturned(turned, PagePointTurn { degrees, center: to_page(center) });
            close(&point_numbers(back), &point_numbers(to_page(point)));

            let points = [point_at(), point_at(), point_at()];
            let page_points: Vec<PagePoint> = points.iter().copied().map(to_page).collect();
            let upright = pdf_turn_of_upright(&points, degrees);
            close(
                &turn_numbers(page_turn_of_upright(&page_points, degrees)),
                &turn_numbers(PagePointTurn { degrees, center: to_page(upright.center) }),
            );
            let drawn = pdf_turn_of_drawn(&points, degrees);
            close(
                &turn_numbers(page_turn_of_drawn(&page_points, degrees)),
                &turn_numbers(PagePointTurn { degrees, center: to_page(drawn.center) }),
            );
        }
    }

    #[test]
    fn bounds_agree_with_the_originals() {
        let mut next = random(7);
        let mut coordinate = move || (next() - 0.5) * 600.0;
        let mut point_at = || PdfPoint { x: coordinate(), y: coordinate() };
        for _ in 0..100 {
            let points = [point_at(), point_at(), point_at(), point_at()];
            let page_points: Vec<PagePoint> = points.iter().copied().map(to_page).collect();
            close(
                &rect_numbers(page_points_bounds(&page_points)),
                &rect_numbers(page_rect_of(pdf_points_bounds(&points), VISIBLE)),
            );
            let quad = PdfQuad { p1: points[0], p2: points[1], p3: points[2], p4: points[3] };
            close(
                &rect_numbers(page_quad_bounds(page_quad_of(quad, VISIBLE))),
                &rect_numbers(page_rect_of(pdf_quad_bounds(quad), VISIBLE)),
            );
        }
        assert_eq!(page_points_bounds(&[]), PageRect { x: 0.0, y: 0.0, width: 0.0, height: 0.0 });
    }
}
