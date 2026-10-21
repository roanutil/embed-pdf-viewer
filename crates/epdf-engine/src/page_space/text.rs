//! A page's text geometry and layout in page space
//! (`core/src/pageSpace/text.ts`: `pageGeometryOf`, `mirroredRun`,
//! `createTextLayout`).

use super::{
    PageCoordinates, PdfCoordinates, mirrored_point, mirrored_quad, mirrored_rect, page_quad_of,
    page_rect_of, unmirrored_quad, unmirrored_rect,
};
use crate::features::geometry::{
    PageGeometryGlyph, PageGeometryRun, PageGeometrySnapshot, RotatedGeometryGlyph,
};
use crate::features::text::TextRange;
use crate::features::text::layout::{PdfTextLayout, PdfTextSegment, TextAt};
use crate::geom::{NamedPdfQuad, PagePoint, PageQuad, PageRect, PdfPoint, PdfRect};

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

const NO_RECT: PdfRect = PdfRect { left: 0.0, bottom: 0.0, right: 0.0, top: 0.0 };
const NO_PDF_POINT: PdfPoint = PdfPoint { x: 0.0, y: 0.0 };
const NO_PDF_QUAD: NamedPdfQuad = NamedPdfQuad {
    upper_left: NO_PDF_POINT,
    upper_right: NO_PDF_POINT,
    lower_left: NO_PDF_POINT,
    lower_right: NO_PDF_POINT,
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

/// A page-space run flipped top to bottom, for the y-up math of the text
/// layout (`mirroredRun`).
pub fn mirrored_run(run: &PageGeometryRun<PageCoordinates>) -> PageGeometryRun<PdfCoordinates> {
    let rect = if run.has_box() { mirrored_rect(run.rect()) } else { NO_RECT };
    match run {
        PageGeometryRun::Rotated { start, glyphs, rotation, ascent_flip, font_size, .. } => {
            PageGeometryRun::Rotated {
                rect,
                start: *start,
                glyphs: glyphs
                    .iter()
                    .map(|glyph| RotatedGeometryGlyph {
                        loose: if glyph.empty {
                            NO_PDF_QUAD
                        } else {
                            mirrored_quad(glyph.loose).into()
                        },
                        tight: glyph.tight.map(|tight| mirrored_quad(tight).into()),
                        space: glyph.space,
                        empty: glyph.empty,
                    })
                    .collect(),
                rotation: *rotation,
                ascent_flip: *ascent_flip,
                font_size: *font_size,
            }
        }
        PageGeometryRun::Upright { start, glyphs, font_size, .. } => PageGeometryRun::Upright {
            rect,
            start: *start,
            glyphs: glyphs
                .iter()
                .map(|glyph| PageGeometryGlyph {
                    loose: if glyph.empty { NO_RECT } else { mirrored_rect(glyph.loose) },
                    tight: glyph.tight.map(mirrored_rect),
                    space: glyph.space,
                    empty: glyph.empty,
                })
                .collect(),
            font_size: *font_size,
        },
    }
}

/// One merged visual line of a text range, in page space
/// (`PdfTextSegment<PageCoordinates>`). `quad` names its corners in the
/// line's own frame; `rect` is its axis-aligned bounds; `advance` is the
/// reading direction along the baseline.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PageTextSegment {
    pub quad: PageQuad,
    pub rect: PageRect,
    pub advance: i8,
}

impl From<PagePoint> for TextAt<PagePoint> {
    fn from(point: PagePoint) -> Self {
        Self::Point(point)
    }
}

/// Where a page's text is, in page space (`TextLayout`, as
/// `page.text.layout()` returns it): points from the top-left of the page,
/// y down. Read it once per page and keep it; every method answers right
/// away.
#[derive(Debug, Clone, PartialEq)]
pub struct TextLayout {
    runs: Vec<PageGeometryRun<PageCoordinates>>,
    layout: PdfTextLayout,
}

/// The layout of a page's text geometry in page space (`createTextLayout`).
/// It runs the same layout as the file's coordinates do, on the geometry
/// flipped top to bottom, and flips each answer back: the flip is exact and
/// the layout doesn't depend on where the origin is.
pub fn create_text_layout(snapshot: PageGeometrySnapshot<PageCoordinates>) -> TextLayout {
    let layout = PdfTextLayout::new(PageGeometrySnapshot {
        runs: snapshot.runs.iter().map(mirrored_run).collect(),
    });
    TextLayout { runs: snapshot.runs, layout }
}

impl TextLayout {
    /// How many characters the page has: they are numbered 0 to
    /// `char_count - 1`.
    pub fn char_count(&self) -> u32 {
        self.layout.char_count()
    }

    /// The raw geometry: characters grouped by line and font, with their
    /// boxes.
    pub fn runs(&self) -> &[PageGeometryRun<PageCoordinates>] {
        &self.runs
    }

    /// The y-up layout this one flips, frames and all.
    pub fn pdf_layout(&self) -> &PdfTextLayout {
        &self.layout
    }

    /// The character at a point, or `None` when there's no text near it.
    /// The tight box is tried first, then a near miss within 1.5 times the
    /// average glyph height.
    pub fn char_at(&self, point: PagePoint) -> Option<u32> {
        self.layout.char_at(mirrored_point(point))
    }

    /// The word at a point or around a character, or `None` when there is
    /// none.
    pub fn word_at(&self, at: impl Into<TextAt<PagePoint>>) -> Option<TextRange> {
        self.layout.word_at(mirrored_at(at.into()))
    }

    /// The visual line at a point or around a character, or `None` when there
    /// is none.
    pub fn line_at(&self, at: impl Into<TextAt<PagePoint>>) -> Option<TextRange> {
        self.layout.line_at(mirrored_at(at.into()))
    }

    /// One segment per visual line the range covers: what to draw, or to
    /// highlight.
    pub fn segments(&self, range: TextRange) -> Vec<PageTextSegment> {
        self.layout.segments(range).into_iter().map(unmirrored_segment).collect()
    }

    /// One character's loose cell, as a quad, or `None` for a character with
    /// no box.
    pub fn char_quad(&self, index: u32) -> Option<PageQuad> {
        self.layout.char_quad(index).map(unmirrored_quad)
    }
}

fn mirrored_at(at: TextAt<PagePoint>) -> TextAt<PdfPoint> {
    match at {
        TextAt::Point(point) => TextAt::Point(mirrored_point(point)),
        TextAt::Char(index) => TextAt::Char(index),
    }
}

fn unmirrored_segment(segment: PdfTextSegment) -> PageTextSegment {
    PageTextSegment {
        quad: unmirrored_quad(segment.quad),
        rect: unmirrored_rect(segment.rect),
        advance: segment.advance,
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

    // ── text in page space (`test/pageSpace/text.test.ts`) ──

    mod layout {
        use super::super::*;
        use crate::features::text::layout::PdfTextLayout;
        use crate::page_space::page_point_of;

        const VISIBLE: PdfRect = PdfRect { left: 50.0, bottom: 60.0, right: 562.0, top: 732.0 };

        fn upright(start: u32, x: f64, bottom: f64, count: u32) -> PageGeometryRun {
            let glyphs = (0..count)
                .map(|i| {
                    let left = x + f64::from(i) * 10.0;
                    PageGeometryGlyph {
                        loose: PdfRect { left, right: left + 10.0, bottom, top: bottom + 12.0 },
                        tight: None,
                        space: i == 3,
                        empty: false,
                    }
                })
                .collect();
            PageGeometryRun::Upright {
                rect: PdfRect {
                    left: x,
                    right: x + f64::from(count) * 10.0,
                    bottom,
                    top: bottom + 12.0,
                },
                start,
                glyphs,
                font_size: None,
            }
        }

        /// A run along a baseline `u` with ascent `n`, starting at `origin`.
        fn oriented(
            start: u32,
            origin: PdfPoint,
            u: PdfPoint,
            n: PdfPoint,
            rotation: f64,
            ascent_flip: bool,
        ) -> PageGeometryRun {
            let at = |t: f64, up: f64| PdfPoint {
                x: origin.x + u.x * t + n.x * up,
                y: origin.y + u.y * t + n.y * up,
            };
            let glyphs: Vec<RotatedGeometryGlyph> = (0..4)
                .map(|i| {
                    let t = f64::from(i) * 8.0;
                    RotatedGeometryGlyph {
                        loose: NamedPdfQuad {
                            upper_left: at(t, 12.0),
                            upper_right: at(t + 8.0, 12.0),
                            lower_left: at(t, 0.0),
                            lower_right: at(t + 8.0, 0.0),
                        },
                        tight: None,
                        space: false,
                        empty: false,
                    }
                })
                .collect();
            let corners: Vec<PdfPoint> = glyphs
                .iter()
                .flat_map(|g| {
                    [
                        g.loose.upper_left,
                        g.loose.upper_right,
                        g.loose.lower_left,
                        g.loose.lower_right,
                    ]
                })
                .collect();
            let rect = PdfRect {
                left: corners.iter().map(|p| p.x).fold(f64::INFINITY, f64::min),
                right: corners.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max),
                bottom: corners.iter().map(|p| p.y).fold(f64::INFINITY, f64::min),
                top: corners.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max),
            };
            PageGeometryRun::Rotated { rect, start, glyphs, rotation, ascent_flip, font_size: None }
        }

        fn point(x: f64, y: f64) -> PdfPoint {
            PdfPoint { x, y }
        }

        fn snapshot() -> PageGeometrySnapshot {
            let s = std::f64::consts::FRAC_1_SQRT_2;
            PageGeometrySnapshot {
                runs: vec![
                    upright(0, 100.0, 600.0, 8),
                    upright(8, 100.0, 580.0, 6),
                    oriented(
                        14,
                        point(300.0, 300.0),
                        point(0.0, 1.0),
                        point(-1.0, 0.0),
                        270.0,
                        false,
                    ),
                    oriented(18, point(400.0, 200.0), point(s, -s), point(s, s), 45.0, false),
                    oriented(22, point(200.0, 150.0), point(1.0, 0.0), point(0.0, -1.0), 0.0, true),
                ],
            }
        }

        fn close(actual: f64, expected: f64) {
            assert!((actual - expected).abs() < 5e-10, "{actual} against {expected}");
        }

        fn close_quad(actual: PageQuad, expected: PageQuad) {
            for (a, e) in [
                (actual.upper_left, expected.upper_left),
                (actual.upper_right, expected.upper_right),
                (actual.lower_left, expected.lower_left),
                (actual.lower_right, expected.lower_right),
            ] {
                close(a.x, e.x);
                close(a.y, e.y);
            }
        }

        #[test]
        fn a_character_with_no_box_keeps_its_zeroed_box_in_both_directions() {
            let zero = PdfRect { left: 0.0, bottom: 0.0, right: 0.0, top: 0.0 };
            let run = PageGeometryRun::Upright {
                rect: PdfRect { left: 100.0, bottom: 600.0, right: 110.0, top: 612.0 },
                start: 0,
                glyphs: vec![
                    PageGeometryGlyph { loose: zero, tight: None, space: false, empty: true },
                    PageGeometryGlyph {
                        loose: PdfRect { left: 100.0, bottom: 600.0, right: 110.0, top: 612.0 },
                        tight: None,
                        space: false,
                        empty: false,
                    },
                ],
                font_size: None,
            };
            let page = page_geometry_of(PageGeometrySnapshot { runs: vec![run.clone()] }, VISIBLE)
                .runs
                .remove(0);
            let PageGeometryRun::Upright { glyphs, .. } = &page else { panic!("upright") };
            assert_eq!(glyphs[0].loose, NO_BOX);
            assert!(glyphs[0].empty);
            assert_eq!(glyphs[1].loose, PageRect { x: 50.0, y: 120.0, width: 10.0, height: 12.0 });
            assert_eq!(
                create_text_layout(PageGeometrySnapshot { runs: vec![page.clone()] }).char_quad(0),
                None
            );

            let blank = PageGeometryRun::Upright {
                rect: zero,
                start: 2,
                glyphs: vec![PageGeometryGlyph {
                    loose: zero,
                    tight: None,
                    space: false,
                    empty: true,
                }],
                font_size: None,
            };
            let mut both =
                page_geometry_of(PageGeometrySnapshot { runs: vec![run, blank] }, VISIBLE).runs;
            let blank_page = both.remove(1);
            assert_eq!(blank_page.rect(), NO_BOX);
            // A triple-click on the real run doesn't reach into the blank one.
            assert_eq!(
                create_text_layout(PageGeometrySnapshot { runs: vec![page, blank_page] })
                    .line_at(1),
                Some(TextRange { start: 0, count: 2 })
            );
        }

        #[test]
        fn the_geometry_is_measured_from_the_top_left_of_the_visible_box() {
            let pdf = snapshot();
            let page = page_geometry_of(pdf.clone(), VISIBLE);
            assert_eq!(
                page.runs[0].rect(),
                PageRect { x: 50.0, y: 120.0, width: 80.0, height: 12.0 }
            );
            let PageGeometryRun::Upright { glyphs, .. } = &page.runs[0] else { panic!("upright") };
            assert_eq!(glyphs[3].loose, PageRect { x: 80.0, y: 120.0, width: 10.0, height: 12.0 });
            assert!(glyphs[3].space);
            let (
                PageGeometryRun::Rotated { glyphs: turned, .. },
                PageGeometryRun::Rotated { glyphs, rotation, ascent_flip, .. },
            ) = (&pdf.runs[2], &page.runs[2])
            else {
                panic!("rotated");
            };
            assert_eq!((*rotation, *ascent_flip), (270.0, false));
            assert_eq!(glyphs[0].loose, page_quad_of(turned[0].loose.into(), VISIBLE));
        }

        #[test]
        fn every_answer_is_the_file_coordinates_answer_measured_on_the_page() {
            let pdf_layout = PdfTextLayout::new(snapshot());
            let page_layout = create_text_layout(page_geometry_of(snapshot(), VISIBLE));
            assert_eq!(page_layout.char_count(), pdf_layout.char_count());
            let mut hits = 0usize;
            for x in (60..560).step_by(13) {
                for y in (70..730).step_by(11) {
                    let at = point(f64::from(x), f64::from(y));
                    let page = page_point_of(at, VISIBLE);
                    assert_eq!(page_layout.char_at(page), pdf_layout.char_at(at), "at {at:?}");
                    assert_eq!(page_layout.word_at(page), pdf_layout.word_at(at), "at {at:?}");
                    assert_eq!(page_layout.line_at(page), pdf_layout.line_at(at), "at {at:?}");
                    hits += usize::from(pdf_layout.char_at(at).is_some());
                }
            }
            assert!(hits > 0, "the sweep never lands on text, so it compares only misses");

            let count = pdf_layout.char_count();
            for start in 0..count {
                for wanted in [1, 3, 9, 26] {
                    let range = TextRange { start, count: wanted.min(count - start) };
                    let actual = page_layout.segments(range);
                    let expected = pdf_layout.segments(range);
                    assert_eq!(actual.len(), expected.len(), "{range:?}");
                    for (a, e) in actual.iter().zip(&expected) {
                        assert_eq!(a.advance, e.advance, "{range:?}");
                        close_quad(a.quad, page_quad_of(e.quad, VISIBLE));
                        let r = page_rect_of(e.rect, VISIBLE);
                        close(a.rect.x, r.x);
                        close(a.rect.y, r.y);
                        close(a.rect.width, r.width);
                        close(a.rect.height, r.height);
                    }
                }
                match (page_layout.char_quad(start), pdf_layout.char_quad(start)) {
                    (Some(a), Some(e)) => close_quad(a, page_quad_of(e, VISIBLE)),
                    (None, None) => {}
                    (a, e) => panic!("char {start}: {a:?} against {e:?}"),
                }
            }
        }
    }
}
