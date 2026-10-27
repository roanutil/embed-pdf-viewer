//! The layout keeps a snapshot's `start`s as given, as the TypeScript does
//! (the vectors pin that). Runs that don't tile the glyphs, from a host
//! other than PDFium, used to index past them and panic (S2); the glyphs a
//! run claims past the end are now skipped where the TypeScript throws.

use epdf_geometry::Point;
use epdf_plugin_selection::text_layout::{PageGeometrySnapshot, TextRange, create_text_layout};
use serde_json::json;

fn snapshot(starts: [u64; 2]) -> PageGeometrySnapshot {
    let glyph = |x: f64| json!({ "loose": { "x": x, "y": 0, "width": 6, "height": 10 } });
    serde_json::from_value(json!({ "runs": [
        { "rect": { "x": 0, "y": 0, "width": 12, "height": 10 }, "start": starts[0],
          "glyphs": [glyph(0.0), glyph(6.0)] },
        { "rect": { "x": 20, "y": 0, "width": 12, "height": 10 }, "start": starts[1],
          "glyphs": [glyph(20.0), glyph(26.0)] },
    ] }))
    .unwrap()
}

const IN_FIRST_RUN: Point = Point { x: 8.0, y: 5.0 };
const IN_SECOND_RUN: Point = Point { x: 28.0, y: 5.0 };

#[test]
fn tiling_runs_hit_every_glyph() {
    let layout = create_text_layout(snapshot([0, 2]));
    assert_eq!(layout.char_at(IN_FIRST_RUN), Some(1));
    assert_eq!(layout.char_at(IN_SECOND_RUN), Some(3));
    assert_eq!(layout.line_at(3), Some(TextRange { start: 0, count: 4 }));
}

#[test]
fn runs_past_the_glyphs_do_not_panic() {
    // A gap after the first run, a run starting past the glyphs, and one
    // at the end of the character space.
    for starts in [[0, 5], [3, 1], [0, u64::MAX - 1]] {
        let layout = create_text_layout(snapshot(starts));
        for point in [IN_FIRST_RUN, IN_SECOND_RUN] {
            let _ = layout.char_at(point);
        }
        for glyph in 0..4 {
            let _ = layout.line_at(glyph);
            let _ = layout.word_at(glyph);
            let _ = layout.char_quad(glyph);
        }
        let _ = layout.segments(TextRange { start: 0, count: 4 });
    }
    // The second run claims glyphs 5 and 6 of four; its hit misses.
    let layout = create_text_layout(snapshot([0, 5]));
    assert_eq!(layout.char_at(IN_SECOND_RUN), None);
}
