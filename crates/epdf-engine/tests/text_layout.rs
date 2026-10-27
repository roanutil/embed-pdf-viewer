//! G5: the shared text-layout contract.
//!
//! `core/src/text/layout.ts` is a deliberate duplicate. Web selection plugins
//! consume it, so it stays in TypeScript and the native copy is pinned to it
//! rather than replacing it. `vectors/text/layout.json` is that pin, and this
//! is what reads it.
//!
//! # Two kinds of test, both worth keeping
//!
//! The parity tests compare the Rust layout against the TypeScript one. The
//! invariant tests check the vectors' own shapes: that a segment's rect bounds
//! its quad, that an empty range yields no segment, that runs tile the glyphs.
//! Parity says the two implementations agree; the invariants say what they
//! agree ON, so a corpus regenerated from a broken TypeScript fails here
//! rather than being adopted by Rust as the new truth.
//!
//! # Page space
//!
//! The vectors are page space (`schemaVersion` 2): the generator feeds the
//! page-space geometry vectors to upstream's `createTextLayout`, which mirrors
//! the runs top to bottom, runs the y-up layout, and flips each answer back.
//! Quads name their corners, segment rects are page boxes, and hit probes are
//! page-space points. The parity tests drive
//! [`epdf_engine::page_space::text::create_text_layout`] the same way.
//!
//! Regenerate with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run layout

use serde::Deserialize;

const SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VectorFile {
    schema_version: u32,
    kind: String,
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct Case {
    #[serde(rename = "case")]
    name: String,
    covers: String,
    pages: Vec<Page>,
}

#[derive(Debug, Deserialize)]
struct Page {
    index: u32,
    layout: Layout,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Layout {
    glyph_count: u32,
    frame_count: u32,
    run_count: u32,
    glyphs: Vec<GlyphProbe>,
    segments: Vec<SegmentProbe>,
    hits: Vec<HitProbe>,
}

#[derive(Debug, Deserialize)]
struct GlyphProbe {
    glyph: u32,
    quad: Option<Quad>,
    /// INCLUSIVE `[from, to]` glyph range of the word this glyph belongs to.
    ///
    /// Inclusive, unlike everything else here. `expandTextRangeToWord` and
    /// its line counterpart return the first and last glyph rather than a
    /// half-open pair, and a reader that assumed otherwise drops the last
    /// glyph of every word.
    word: (u32, u32),
    line: (u32, u32),
}

#[derive(Debug, Deserialize, Clone, Copy)]
struct Point {
    x: f64,
    y: f64,
}

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
struct Quad {
    upper_left: Point,
    upper_right: Point,
    lower_left: Point,
    lower_right: Point,
}

impl Quad {
    fn corners(&self) -> [Point; 4] {
        [self.upper_left, self.upper_right, self.lower_left, self.lower_right]
    }
}

#[derive(Debug, Deserialize)]
struct SegmentProbe {
    start: u32,
    count: u32,
    segments: Vec<Segment>,
}

#[derive(Debug, Deserialize)]
struct Segment {
    quad: Quad,
    rect: Rect,
    advance: i32,
}

/// A page box: `x, y` is its top-left corner.
#[derive(Debug, Deserialize, Clone, Copy)]
struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[derive(Debug, Deserialize)]
struct HitProbe {
    x: f64,
    y: f64,
    glyph: Option<u32>,
}

fn load() -> VectorFile {
    let path = test_support::corpus::vectors_dir("text").join("layout.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: VectorFile = serde_json::from_str(&text).expect("layout.json");
    assert_eq!(file.schema_version, SCHEMA_VERSION, "vector envelope moved");
    assert_eq!(file.kind, "text.layout");
    assert!(!file.cases.is_empty(), "no layout cases");
    file
}

fn pages(file: &VectorFile) -> impl Iterator<Item = (&Case, &Page)> {
    file.cases.iter().flat_map(|case| case.pages.iter().map(move |page| (case, page)))
}

#[test]
fn the_corpus_covers_more_than_one_orientation_frame() {
    // Frame 0 is page space and takes a fast path; every rotated cluster gets
    // its own. A corpus with only frame 0 would leave the entire orientation
    // model untested, which is most of what layout.ts does.
    let file = load();
    let max_frames = pages(&file).map(|(_, page)| page.layout.frame_count).max().unwrap_or(0);
    assert!(
        max_frames >= 2,
        "every page in the corpus has one frame, so nothing exercises the orientation model",
    );
}

#[test]
fn word_and_line_ranges_contain_their_glyph_and_stay_on_the_page() {
    let file = load();
    for (case, page) in pages(&file) {
        let count = page.layout.glyph_count;
        for probe in &page.layout.glyphs {
            let where_ = format!("{} page {} glyph {}", case.name, page.index, probe.glyph);
            for (label, (from, to)) in [("word", probe.word), ("line", probe.line)] {
                assert!(from <= to, "{where_}: {label} range [{from}, {to}] inverts");
                assert!(to < count, "{where_}: {label} range ends past {count} glyphs");
                assert!(
                    from <= probe.glyph && probe.glyph <= to,
                    "{where_}: {label} range [{from}, {to}] excludes its own glyph",
                );
            }
            // Deliberately NOT asserting that the word sits inside the line.
            // See `a_word_can_cross_a_line_boundary_between_abutting_runs`.
        }
    }
}

#[test]
fn a_degenerate_glyph_has_no_quad_and_a_real_one_does() {
    // `textGlyphQuad` returns null for an empty glyph, which is the anchor a
    // caret would otherwise be placed at. font-size-zero.pdf has no runs any
    // more (PDFium drops non-finite page objects), so the degenerate probes
    // are the empty glyphs of report, reverse-and-gapped-text and
    // embedded-orientations.
    let file = load();
    let mut with_quad = 0usize;
    let mut without = 0usize;
    for (_, page) in pages(&file) {
        for probe in &page.layout.glyphs {
            if probe.quad.is_some() { with_quad += 1 } else { without += 1 }
        }
    }
    assert!(with_quad > 0, "no probe glyph has a quad, so the corpus has no real text");
    assert!(without > 0, "no probe glyph is degenerate, so caret anchoring goes untested");
}

#[test]
fn every_segment_rect_bounds_its_own_quad() {
    // `rect` is documented as the quad's axis-aligned bounds, produced by the
    // same constructor. A consumer that highlights with the rect and hit-tests
    // with the quad depends on that being true.
    let file = load();
    let mut checked = 0usize;
    for (case, page) in pages(&file) {
        for probe in &page.layout.segments {
            for (i, segment) in probe.segments.iter().enumerate() {
                let xs = segment.quad.corners().map(|p| p.x);
                let ys = segment.quad.corners().map(|p| p.y);
                let r = segment.rect;
                let where_ = format!(
                    "{} page {} range [{}, +{}) segment {i}",
                    case.name, page.index, probe.start, probe.count,
                );
                // A tolerance of one quantum, since both sides are rounded to
                // 1e-4 before they are compared.
                let eps = 1e-4;
                assert!(
                    r.x <= xs.iter().cloned().fold(f64::INFINITY, f64::min) + eps,
                    "{where_}: the rect's left edge is inside the quad",
                );
                assert!(
                    r.x + r.width >= xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max) - eps,
                    "{where_}: the rect's right edge is inside the quad",
                );
                // Page space runs y down, so `y` is the top edge.
                assert!(
                    r.y <= ys.iter().cloned().fold(f64::INFINITY, f64::min) + eps,
                    "{where_}: the rect's top edge is inside the quad",
                );
                assert!(
                    r.y + r.height >= ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max) - eps,
                    "{where_}: the rect's bottom edge is inside the quad",
                );
                assert!(
                    segment.advance == 1 || segment.advance == -1,
                    "{where_}: advance is {}, which is neither direction",
                    segment.advance,
                );
                checked += 1;
            }
        }
    }
    assert!(checked >= 100, "only {checked} segments checked");
}

#[test]
fn an_empty_range_produces_no_segments() {
    // Zero-count ranges appear in the corpus wherever a probe glyph is itself
    // a boundary, and they must not produce a degenerate segment for a
    // consumer to draw.
    let file = load();
    for (case, page) in pages(&file) {
        for probe in &page.layout.segments {
            if probe.count == 0 {
                assert!(
                    probe.segments.is_empty(),
                    "{} page {}: an empty range produced {} segments",
                    case.name,
                    page.index,
                    probe.segments.len(),
                );
            }
        }
    }
}

#[test]
fn a_hit_at_a_glyphs_own_centre_finds_a_glyph() {
    // Hit testing mirrors PDFium's GetIndexAtPos: exact tight box first, then
    // a tolerance pass. A point at a glyph's own centre is the easiest
    // possible case, so a miss there means the index is not being consulted at
    // all.
    //
    // Which glyph it finds is not asserted: overlapping glyphs are legal and
    // the tolerance pass may prefer a neighbour. What must not happen is
    // nothing.
    let file = load();
    for (case, page) in pages(&file) {
        if page.layout.glyph_count == 0 {
            continue;
        }
        let interior: Vec<&HitProbe> =
            page.layout.hits.iter().filter(|h| h.x > -1_000.0 && h.y > -1_000.0).collect();
        let found = interior.iter().filter(|h| h.glyph.is_some()).count();
        assert!(
            found * 2 >= interior.len(),
            "{} page {}: only {found} of {} centre hits found a glyph",
            case.name,
            page.index,
            interior.len(),
        );
    }
}

#[test]
fn a_hit_far_outside_the_page_finds_nothing() {
    let file = load();
    for (case, page) in pages(&file) {
        let outside =
            page.layout.hits.iter().find(|h| h.x < -1_000.0).unwrap_or_else(|| {
                panic!("{} page {} has no outside probe", case.name, page.index)
            });
        assert_eq!(
            outside.glyph, None,
            "{} page {}: a point far off the page hit glyph {:?} ({})",
            case.name, page.index, outside.glyph, case.covers,
        );
    }
}

#[test]
fn the_run_and_glyph_counts_are_consistent() {
    // Runs tile the page, so a page with glyphs has runs and a page without
    // has neither.
    let file = load();
    for (case, page) in pages(&file) {
        let where_ = format!("{} page {}", case.name, page.index);
        if page.layout.glyph_count == 0 {
            assert_eq!(page.layout.run_count, 0, "{where_}: runs without glyphs");
        } else {
            assert!(page.layout.run_count > 0, "{where_}: glyphs without runs");
            assert!(
                page.layout.run_count <= page.layout.glyph_count,
                "{where_}: more runs than glyphs",
            );
        }
        // Frame 0 always exists, even on an empty page.
        assert!(page.layout.frame_count >= 1, "{where_}: no frames at all");
    }
}

#[test]
fn a_word_can_cross_a_line_boundary_between_abutting_runs() {
    // Not a bug, and not obvious. The two expansions walk different things:
    // the word walk follows glyph flags and stops at a space or an empty
    // glyph, while the line walk follows runs and stops at a differently
    // oriented one. Two text objects that abut with no space between them
    // therefore give a word that extends past its own line.
    //
    // `embedded_orientations.pdf` has exactly that: glyph 28 is an empty
    // glyph in a run of two empties after the 330 degree run, and reports
    // word [25, 28] and line [28, 29]. The word walk starts on the empty
    // glyph and runs back into the rotated run, since only a neighbour can
    // stop it; the line is the empty run alone.
    //
    // Recorded rather than asserted away, because a Rust port that "fixed" it
    // would disagree with the TypeScript on every such document, and because
    // a caller double-clicking there selects text in two orientations. That
    // is worth knowing before anybody treats a word as a single visual run.
    let file = load();
    let crossing: Vec<String> = file
        .cases
        .iter()
        .flat_map(|case| case.pages.iter().map(move |page| (case, page)))
        .flat_map(|(case, page)| {
            page.layout
                .glyphs
                .iter()
                .filter(|probe| probe.word.0 < probe.line.0 || probe.word.1 > probe.line.1)
                .map(move |probe| {
                    format!(
                        "{} page {} glyph {}: word {:?} line {:?}",
                        case.name, page.index, probe.glyph, probe.word, probe.line,
                    )
                })
        })
        .collect();

    assert!(
        !crossing.is_empty(),
        "no probe crosses a line boundary any more; if the corpus lost its \
         multi-orientation page this test is no longer covering anything",
    );
}

// --- G5: the Rust layout against the TypeScript one.

use epdf_engine::features::geometry::PageGeometrySnapshot;
use epdf_engine::features::text::TextRange;
use epdf_engine::geom::{PagePoint, PageQuad};
use epdf_engine::page_space::PageCoordinates;
use epdf_engine::page_space::text::{TextLayout, create_text_layout, mirrored_run};

/// The page-space geometry snapshot each layout case was built from.
fn snapshot_for(case_name: &str, page_index: u32) -> PageGeometrySnapshot<PageCoordinates> {
    let path =
        test_support::corpus::vectors_dir("pages.geometry").join(format!("{case_name}.json"));
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: serde_json::Value = serde_json::from_str(&text).expect("geometry vector");
    let page = file["pages"]
        .as_array()
        .expect("pages")
        .iter()
        .find(|p| p["index"] == page_index)
        .unwrap_or_else(|| panic!("{case_name} has no page {page_index}"));
    serde_json::from_value(page["snapshot"].clone()).expect("snapshot")
}

fn layout_for(case_name: &str, page_index: u32) -> TextLayout {
    create_text_layout(snapshot_for(case_name, page_index))
}

/// Round to the corpus quantum, as the generator does with `Math.round`,
/// which rounds a half up rather than away from zero.
fn q(value: f64) -> f64 {
    (value * 10_000.0 + 0.5).floor() / 10_000.0
}

fn same_point(actual: PagePoint, expected: Point) -> bool {
    q(actual.x) == expected.x && q(actual.y) == expected.y
}

fn same_quad(actual: &PageQuad, expected: &Quad) -> bool {
    same_point(actual.upper_left, expected.upper_left)
        && same_point(actual.upper_right, expected.upper_right)
        && same_point(actual.lower_left, expected.lower_left)
        && same_point(actual.lower_right, expected.lower_right)
}

/// `wordAt`/`lineAt` answer a range or nothing; the vectors keep an inclusive
/// `[from, to]` and record nothing as the glyph alone, as the generator does.
fn inclusive(range: Option<TextRange>, glyph: u32) -> (u32, u32) {
    range.map_or((glyph, glyph), |r| (r.start, r.start + r.count - 1))
}

#[test]
fn the_layout_shape_agrees_with_typescript() {
    use epdf_engine::features::text::layout::build_page_text_layout;

    // `TextLayout` does not expose its frames, so the frame count reads the
    // y-up layout built from the mirrored runs, which is what
    // `createTextLayout` lays out and what the generator counts.
    let file = load();
    for (case, page) in pages(&file) {
        let snapshot = snapshot_for(&case.name, page.index);
        let where_ = format!("{} page {}", case.name, page.index);
        let mirrored =
            PageGeometrySnapshot { runs: snapshot.runs.iter().map(mirrored_run).collect() };
        let inner = build_page_text_layout(&mirrored);
        let layout = create_text_layout(snapshot);
        assert_eq!(layout.char_count(), page.layout.glyph_count, "{where_}: glyph count");
        assert_eq!(inner.runs.len() as u32, page.layout.run_count, "{where_}: run count");
        assert_eq!(layout.runs().len() as u32, page.layout.run_count, "{where_}: raw runs");
        assert_eq!(
            inner.frames.len() as u32,
            page.layout.frame_count,
            "{where_}: frame count, which is the orientation clustering",
        );
    }
}

#[test]
fn glyph_quads_and_ranges_agree_with_typescript() {
    let file = load();
    let mut probes = 0usize;
    for (case, page) in pages(&file) {
        let layout = layout_for(&case.name, page.index);
        for probe in &page.layout.glyphs {
            let where_ = format!("{} page {} glyph {}", case.name, page.index, probe.glyph);

            let actual_quad = layout.char_quad(probe.glyph);
            match (&actual_quad, &probe.quad) {
                (None, None) => {}
                (Some(a), Some(e)) => {
                    assert!(same_quad(a, e), "{where_}: quad {a:?} against {e:?}");
                }
                _ => panic!("{where_}: one side produced a quad and the other did not"),
            }

            assert_eq!(
                inclusive(layout.word_at(probe.glyph), probe.glyph),
                probe.word,
                "{where_}: word range",
            );
            assert_eq!(
                inclusive(layout.line_at(probe.glyph), probe.glyph),
                probe.line,
                "{where_}: line range",
            );
            probes += 1;
        }
    }
    assert!(probes >= 80, "only {probes} glyph probes ran");
}

#[test]
fn segments_agree_with_typescript() {
    // The merge, which is the part adapted from Chromium and the part where a
    // reimplementation drifts. Every probe range in the corpus: single
    // glyphs, whole words, whole lines.
    let file = load();
    let mut failures = Vec::new();
    let mut probes = 0usize;

    for (case, page) in pages(&file) {
        let layout = layout_for(&case.name, page.index);
        for probe in &page.layout.segments {
            let actual = layout.segments(TextRange { start: probe.start, count: probe.count });
            let where_ = format!(
                "{} page {} range [{}, +{})",
                case.name, page.index, probe.start, probe.count,
            );
            if actual.len() != probe.segments.len() {
                failures.push(format!(
                    "{where_}: {} segments against {}",
                    actual.len(),
                    probe.segments.len(),
                ));
                continue;
            }
            for (i, (a, e)) in actual.iter().zip(&probe.segments).enumerate() {
                if i32::from(a.advance) != e.advance {
                    failures.push(format!(
                        "{where_} segment {i}: advance {} against {}",
                        a.advance, e.advance,
                    ));
                }
                if !same_quad(&a.quad, &e.quad) {
                    failures.push(format!("{where_} segment {i}: quad differs"));
                }
                if q(a.rect.x) != e.rect.x
                    || q(a.rect.y) != e.rect.y
                    || q(a.rect.width) != e.rect.width
                    || q(a.rect.height) != e.rect.height
                {
                    failures.push(format!(
                        "{where_} segment {i}: rect [{}, {}, {}, {}] against [{}, {}, {}, {}]",
                        q(a.rect.x),
                        q(a.rect.y),
                        q(a.rect.width),
                        q(a.rect.height),
                        e.rect.x,
                        e.rect.y,
                        e.rect.width,
                        e.rect.height,
                    ));
                }
            }
            probes += 1;
        }
    }
    assert!(failures.is_empty(), "segment divergence:\n  {}", failures.join("\n  "));
    assert!(probes >= 150, "only {probes} segment probes ran");
}

#[test]
fn hit_tests_agree_with_typescript() {
    let file = load();
    let mut failures = Vec::new();
    for (case, page) in pages(&file) {
        let layout = layout_for(&case.name, page.index);
        for probe in &page.layout.hits {
            // `charAt` uses the TypeScript's default tolerance factor, 1.5.
            let actual = layout.char_at(PagePoint { x: probe.x, y: probe.y });
            if actual != probe.glyph {
                failures.push(format!(
                    "{} page {} at ({}, {}): {actual:?} against {:?}",
                    case.name, page.index, probe.x, probe.y, probe.glyph,
                ));
            }
        }
    }
    assert!(failures.is_empty(), "hit test divergence:\n  {}", failures.join("\n  "));
}
