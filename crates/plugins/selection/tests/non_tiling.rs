//! The layout keeps a snapshot's `charStart`s as given, as the TypeScript
//! does (the vectors pin that). Runs that don't tile the glyphs, from a
//! host other than PDFium, used to index past them and panic; the glyphs a
//! run claims past the end are now skipped where the TypeScript throws.

use epdf_plugin_selection::text_layout::{
    DEFAULT_TOLERANCE_FACTOR, PageGeometrySnapshot, PdfPoint, build_page_text_layout,
    expand_text_range_to_line, text_glyph_at, text_segments_for_range,
};
use serde_json::json;

fn snapshot(starts: [u64; 2]) -> PageGeometrySnapshot {
    let glyph = |left: f64| json!({ "looseBox": { "left": left, "bottom": 0, "right": left + 6.0, "top": 10 }, "flags": 0 });
    serde_json::from_value(json!({ "runs": [
        { "rect": { "left": 0, "bottom": 0, "right": 12, "top": 10 }, "charStart": starts[0],
          "glyphs": [glyph(0.0), glyph(6.0)] },
        { "rect": { "left": 20, "bottom": 0, "right": 32, "top": 10 }, "charStart": starts[1],
          "glyphs": [glyph(20.0), glyph(26.0)] },
    ] }))
    .unwrap()
}

const IN_FIRST_RUN: PdfPoint = PdfPoint { x: 8.0, y: 5.0 };
const IN_SECOND_RUN: PdfPoint = PdfPoint { x: 28.0, y: 5.0 };

#[test]
fn tiling_runs_hit_every_glyph() {
    let layout = build_page_text_layout(&snapshot([0, 2]));
    assert_eq!(text_glyph_at(&layout, IN_FIRST_RUN, DEFAULT_TOLERANCE_FACTOR), Some(1));
    assert_eq!(text_glyph_at(&layout, IN_SECOND_RUN, DEFAULT_TOLERANCE_FACTOR), Some(3));
    assert_eq!(expand_text_range_to_line(&layout, 3), (0, 3));
}

#[test]
fn runs_past_the_glyphs_do_not_panic() {
    // A gap after the first run, a run starting past the glyphs, and one
    // at the end of the character space.
    for starts in [[0, 5], [3, 1], [0, u64::MAX - 1]] {
        let layout = build_page_text_layout(&snapshot(starts));
        for point in [IN_FIRST_RUN, IN_SECOND_RUN] {
            for tolerance in [0.0, DEFAULT_TOLERANCE_FACTOR] {
                let _ = text_glyph_at(&layout, point, tolerance);
            }
        }
        for glyph in 0..4 {
            let _ = expand_text_range_to_line(&layout, glyph);
        }
        let _ = text_segments_for_range(&layout, 0, 4);
    }
    // The second run claims glyphs 5 and 6 of four; its hit misses.
    let layout = build_page_text_layout(&snapshot([0, 5]));
    assert_eq!(text_glyph_at(&layout, IN_SECOND_RUN, 0.0), None);
}
