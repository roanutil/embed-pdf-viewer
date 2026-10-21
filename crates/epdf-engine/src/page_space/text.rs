//! A page's text geometry in page space (`core/src/pageSpace/text.ts`,
//! `pageGeometryOf`).

use super::{PageCoordinates, PdfCoordinates, page_quad_of, page_rect_of};
use crate::features::geometry::{
    PageGeometryGlyph, PageGeometryRun, PageGeometrySnapshot, RotatedGeometryGlyph,
};
use crate::geom::{PagePoint, PageQuad, PageRect, PdfRect};

/// A character with no box of its own (`empty`) carries a zeroed box or quad
/// that says so, in either space, and so does a run of nothing but such
/// characters; it is not a place, so it never converts.
const NO_BOX: PageRect = PageRect { x: 0.0, y: 0.0, width: 0.0, height: 0.0 };
const NO_POINT: PagePoint = PagePoint { x: 0.0, y: 0.0 };
const NO_QUAD: PageQuad = PageQuad {
    upper_left: NO_POINT,
    upper_right: NO_POINT,
    lower_left: NO_POINT,
    lower_right: NO_POINT,
};

/// A page's text geometry in page space (`pageGeometryOf`). `visible` is the
/// page's visible box.
pub fn page_geometry_of(
    snapshot: PageGeometrySnapshot<PdfCoordinates>,
    visible: PdfRect,
) -> PageGeometrySnapshot<PageCoordinates> {
    PageGeometrySnapshot {
        runs: snapshot.runs.into_iter().map(|run| page_geometry_run_of(run, visible)).collect(),
    }
}

fn page_geometry_run_of(
    run: PageGeometryRun<PdfCoordinates>,
    visible: PdfRect,
) -> PageGeometryRun<PageCoordinates> {
    let rect = if run.has_box() { page_rect_of(run.rect(), visible) } else { NO_BOX };
    match run {
        PageGeometryRun::Rotated { start, glyphs, rotation, ascent_flip, font_size, .. } => {
            PageGeometryRun::Rotated {
                rect,
                start,
                glyphs: glyphs
                    .into_iter()
                    .map(|glyph| RotatedGeometryGlyph {
                        loose: if glyph.empty {
                            NO_QUAD
                        } else {
                            page_quad_of(glyph.loose.into(), visible)
                        },
                        tight: glyph.tight.map(|tight| page_quad_of(tight.into(), visible)),
                        space: glyph.space,
                        empty: glyph.empty,
                    })
                    .collect(),
                rotation,
                ascent_flip,
                font_size,
            }
        }
        PageGeometryRun::Upright { start, glyphs, font_size, .. } => PageGeometryRun::Upright {
            rect,
            start,
            glyphs: glyphs
                .into_iter()
                .map(|glyph| PageGeometryGlyph {
                    loose: if glyph.empty { NO_BOX } else { page_rect_of(glyph.loose, visible) },
                    tight: glyph.tight.map(|tight| page_rect_of(tight, visible)),
                    space: glyph.space,
                    empty: glyph.empty,
                })
                .collect(),
            font_size,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::{PdfPoint, PdfQuad};

    fn rect(left: f64, bottom: f64, right: f64, top: f64) -> PdfRect {
        PdfRect { left, bottom, right, top }
    }

    const VISIBLE: PdfRect = PdfRect { left: 10.0, bottom: 20.0, right: 610.0, top: 820.0 };
    const ZERO: PdfRect = PdfRect { left: 0.0, bottom: 0.0, right: 0.0, top: 0.0 };

    #[test]
    fn an_upright_run_measures_from_the_top_left_of_the_visible_box() {
        let snapshot = PageGeometrySnapshot::<PdfCoordinates> {
            runs: vec![PageGeometryRun::Upright {
                rect: rect(110.0, 700.0, 130.0, 712.0),
                start: 4,
                glyphs: vec![
                    PageGeometryGlyph {
                        loose: rect(110.0, 700.0, 120.0, 712.0),
                        tight: Some(rect(111.0, 701.0, 119.0, 711.0)),
                        space: false,
                        empty: false,
                    },
                    PageGeometryGlyph { loose: ZERO, tight: None, space: true, empty: true },
                ],
                font_size: Some(12.0),
            }],
        };
        let page = page_geometry_of(snapshot, VISIBLE);
        let PageGeometryRun::Upright { rect, start, glyphs, font_size } = &page.runs[0] else {
            panic!("expected an upright run");
        };
        assert_eq!(*rect, PageRect { x: 100.0, y: 108.0, width: 20.0, height: 12.0 });
        assert_eq!(*start, 4);
        assert_eq!(*font_size, Some(12.0));
        assert_eq!(glyphs[0].loose, PageRect { x: 100.0, y: 108.0, width: 10.0, height: 12.0 });
        assert_eq!(
            glyphs[0].tight,
            Some(PageRect { x: 101.0, y: 109.0, width: 8.0, height: 10.0 })
        );
        // A glyph with no box stays the zeroed box, not the page's corner.
        assert_eq!(glyphs[1].loose, NO_BOX);
        assert!(glyphs[1].space && glyphs[1].empty);
    }

    #[test]
    fn a_run_with_no_box_keeps_the_zeroed_box() {
        let snapshot = PageGeometrySnapshot::<PdfCoordinates> {
            runs: vec![PageGeometryRun::Upright {
                rect: ZERO,
                start: 0,
                glyphs: vec![PageGeometryGlyph {
                    loose: ZERO,
                    tight: None,
                    space: false,
                    empty: true,
                }],
                font_size: None,
            }],
        };
        assert_eq!(page_geometry_of(snapshot, VISIBLE).runs[0].rect(), NO_BOX);
    }

    #[test]
    fn a_rotated_run_keeps_its_turn_and_names_its_corners_in_page_space() {
        let point = |x, y| PdfPoint { x, y };
        let quad = PdfQuad {
            p1: point(60.0, 700.0),
            p2: point(60.0, 710.0),
            p3: point(50.0, 700.0),
            p4: point(50.0, 710.0),
        };
        let zero = PdfQuad {
            p1: point(0.0, 0.0),
            p2: point(0.0, 0.0),
            p3: point(0.0, 0.0),
            p4: point(0.0, 0.0),
        };
        let snapshot = PageGeometrySnapshot::<PdfCoordinates> {
            runs: vec![PageGeometryRun::Rotated {
                rect: rect(50.0, 700.0, 60.0, 710.0),
                start: 0,
                glyphs: vec![
                    RotatedGeometryGlyph {
                        loose: quad.into(),
                        tight: Some(quad.into()),
                        space: false,
                        empty: false,
                    },
                    RotatedGeometryGlyph {
                        loose: zero.into(),
                        tight: None,
                        space: false,
                        empty: true,
                    },
                ],
                rotation: 270.0,
                ascent_flip: true,
                font_size: None,
            }],
        };
        let page = page_geometry_of(snapshot, VISIBLE);
        let PageGeometryRun::Rotated { rect, glyphs, rotation, ascent_flip, .. } = &page.runs[0]
        else {
            panic!("expected a rotated run");
        };
        assert_eq!(*rect, PageRect { x: 40.0, y: 110.0, width: 10.0, height: 10.0 });
        assert_eq!(*rotation, 270.0);
        assert!(*ascent_flip);
        assert_eq!(glyphs[0].loose.upper_left, PagePoint { x: 50.0, y: 120.0 });
        assert_eq!(glyphs[0].loose.lower_right, PagePoint { x: 40.0, y: 110.0 });
        assert_eq!(glyphs[0].tight, Some(glyphs[0].loose));
        assert_eq!(glyphs[1].loose, NO_QUAD);
    }
}
