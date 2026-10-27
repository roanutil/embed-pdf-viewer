//! G5's entry gate: the character-to-text translation agrees with TypeScript.
//!
//! `core/src/text/charmap.ts` stays in TypeScript because web selection
//! plugins consume it, so the Rust copy is a deliberate duplicate rather than
//! a port, and these vectors are what keep the two the same rule. A map that
//! slices wrong produces plausible text from the wrong characters, which
//! nothing downstream can detect.
//!
//! The probes are exhaustive: every boundary, every offset, every half-open
//! range in both spaces. The cases are small enough that a sweep is cheap, and
//! it catches an off-by-one that chosen probes step straight over.
//!
//! Regenerate with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run charmap

use epdf_engine::features::text::charmap::{
    CharBoundaryBias, CharMapAnchor, PageTextSnapshot, boundary_text_offset,
    char_boundary_at_text_offset, char_map_violation, char_range_for_text_offsets,
    slice_text_by_chars,
};
use serde::Deserialize;

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VectorFile {
    schema_version: u32,
    kind: String,
    cases: Vec<Case>,
    violations: Vec<ViolationCase>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    name: String,
    covers: String,
    text: String,
    char_count: u32,
    #[serde(default)]
    char_map: Vec<(u32, u32)>,
    text_length: u32,
    boundary_text_offsets: Vec<u32>,
    start_bias_boundaries: Vec<u32>,
    end_bias_boundaries: Vec<u32>,
    char_ranges: Vec<RangeProbe>,
    slices: Vec<SliceProbe>,
    violation: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RangeProbe {
    start: u32,
    end: u32,
    range: Range,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct Range {
    start: u32,
    end: u32,
}

#[derive(Debug, Deserialize)]
struct SliceProbe {
    start: u32,
    end: u32,
    text: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ViolationCase {
    name: String,
    covers: String,
    char_count: u32,
    text_length: u32,
    #[serde(default)]
    char_map: Vec<(u32, u32)>,
    violation: Option<String>,
}

impl Case {
    fn snapshot(&self) -> PageTextSnapshot {
        PageTextSnapshot {
            text: self.text.clone(),
            char_count: self.char_count,
            char_map: self.char_map.iter().map(|(c, t)| CharMapAnchor::new(*c, *t)).collect(),
        }
    }
}

fn load() -> VectorFile {
    let path = test_support::corpus::vectors_dir("text").join("charmap.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: VectorFile = serde_json::from_str(&text).expect("charmap.json");
    assert_eq!(file.schema_version, SCHEMA_VERSION, "vector envelope moved");
    assert_eq!(file.kind, "text.charmap");
    file
}

#[test]
fn utf16_lengths_agree_with_javascript() {
    // The conversion the Rust side has to get right and JavaScript gets for
    // free. A supplementary character is one scalar, four bytes and two code
    // units, and only the last is what TEXT space counts.
    let file = load();
    for case in &file.cases {
        assert_eq!(
            case.snapshot().text_len_utf16(),
            case.text_length,
            "{}: {:?} is not {} UTF-16 units ({})",
            case.name,
            case.text,
            case.text_length,
            case.covers,
        );
    }
}

#[test]
fn every_case_in_the_corpus_is_a_valid_map() {
    // Otherwise the probes below describe behaviour on input the engine would
    // refuse, which is worse than no coverage.
    let file = load();
    for case in &file.cases {
        assert_eq!(case.violation, None, "{}", case.name);
        assert_eq!(
            char_map_violation(case.char_count, case.text_length, &case.snapshot().char_map,),
            None,
            "{}: Rust rejected a map TypeScript accepted",
            case.name,
        );
    }
}

#[test]
fn boundary_text_offsets_agree_with_typescript() {
    let file = load();
    assert!(file.cases.len() >= 16, "the corpus shrank");
    let mut probes = 0usize;
    for case in &file.cases {
        let snapshot = case.snapshot();
        for (boundary, expected) in case.boundary_text_offsets.iter().enumerate() {
            let actual = boundary_text_offset(&snapshot, boundary as u32);
            assert_eq!(
                actual, *expected,
                "{}: boundary {boundary} is {actual}, TypeScript said {expected} ({})",
                case.name, case.covers,
            );
            probes += 1;
        }
    }
    assert!(probes >= 60, "only {probes} boundaries checked");
}

#[test]
fn both_bias_rules_agree_with_typescript() {
    // The pair is the whole point: start scans forward past a zero-width
    // plateau and end stops before it, so a range never picks up a dropped
    // character on either side.
    let file = load();
    for case in &file.cases {
        let snapshot = case.snapshot();
        for (offset, expected) in case.start_bias_boundaries.iter().enumerate() {
            assert_eq!(
                char_boundary_at_text_offset(&snapshot, offset as u32, CharBoundaryBias::Start),
                *expected,
                "{}: start bias at offset {offset} ({})",
                case.name,
                case.covers,
            );
        }
        for (offset, expected) in case.end_bias_boundaries.iter().enumerate() {
            assert_eq!(
                char_boundary_at_text_offset(&snapshot, offset as u32, CharBoundaryBias::End),
                *expected,
                "{}: end bias at offset {offset} ({})",
                case.name,
                case.covers,
            );
        }
    }
}

#[test]
fn every_text_range_maps_to_the_same_character_range() {
    let file = load();
    let mut probes = 0usize;
    for case in &file.cases {
        let snapshot = case.snapshot();
        for probe in &case.char_ranges {
            let actual = char_range_for_text_offsets(&snapshot, probe.start, probe.end);
            assert_eq!(
                Range { start: actual.start, end: actual.end },
                probe.range,
                "{}: text range [{}, {}) ({})",
                case.name,
                probe.start,
                probe.end,
                case.covers,
            );
            probes += 1;
        }
    }
    assert!(probes >= 200, "only {probes} ranges checked");
}

#[test]
fn every_character_range_slices_to_the_same_text() {
    // The copy primitive. This is what a selection produces, so a divergence
    // here is text a user copied that is not what they highlighted.
    let file = load();
    let mut probes = 0usize;
    for case in &file.cases {
        let snapshot = case.snapshot();
        for probe in &case.slices {
            assert_eq!(
                slice_text_by_chars(&snapshot, probe.start, probe.end),
                probe.text,
                "{}: chars [{}, {}) ({})",
                case.name,
                probe.start,
                probe.end,
                case.covers,
            );
            probes += 1;
        }
    }
    assert!(probes >= 200, "only {probes} slices checked");
}

#[test]
fn slicing_the_whole_page_returns_the_whole_text() {
    // The identity every consumer assumes, stated once rather than inferred
    // from the sweep above.
    let file = load();
    for case in &file.cases {
        let snapshot = case.snapshot();
        assert_eq!(
            slice_text_by_chars(&snapshot, 0, case.char_count),
            case.text,
            "{}: {}",
            case.name,
            case.covers,
        );
    }
}

#[test]
fn malformed_maps_are_rejected_the_same_way() {
    // The message is diagnostic and not compared; what must agree is whether
    // the map is refused at all. A map accepted on one platform and refused on
    // the other means a page that reads on web and fails on native.
    let file = load();
    assert!(file.violations.len() >= 13, "the corpus shrank");
    for case in &file.violations {
        let map: Vec<CharMapAnchor> =
            case.char_map.iter().map(|(c, t)| CharMapAnchor::new(*c, *t)).collect();
        let actual = char_map_violation(case.char_count, case.text_length, &map);
        assert_eq!(
            actual.is_some(),
            case.violation.is_some(),
            "{}: Rust said {actual:?}, TypeScript said {:?} ({})",
            case.name,
            case.violation,
            case.covers,
        );
    }
}

// --- `test/text/charmap.test.ts`, upstream's own cases.
//
// Upstream's ranges are `TextRange`s (`start`, `count`); this crate's charmap
// still takes half-open `[start, end)` pairs, so each case converts. The
// cases with a negative boundary or count have no `u32` spelling and are
// left out, and the `PageTextSnapshotSchema` cases belong to the wire schema,
// not to this module.

/// Fixtures named by their character list. `x` marks a non-printing
/// character (a character slot, zero text units); the emoji contributes a
/// surrogate pair (two units).
mod upstream {
    use super::*;

    fn snapshot(text: &str, char_count: u32, anchors: &[(u32, u32)]) -> PageTextSnapshot {
        PageTextSnapshot {
            text: text.to_string(),
            char_count,
            char_map: anchors.iter().map(|&(c, t)| CharMapAnchor::new(c, t)).collect(),
        }
    }

    fn identity() -> PageTextSnapshot {
        snapshot("ABC", 3, &[])
    }

    /// bug_1139's shape: a leading non-printing character shifts every text
    /// index by one.
    fn leading_drop() -> PageTextSnapshot {
        snapshot("AB", 3, &[(1, 0)])
    }

    /// `[A, x]`: the trailing zero-width character must stay outside a range
    /// ending at "A".
    fn trailing_drop() -> PageTextSnapshot {
        snapshot("A", 2, &[(2, 1)])
    }

    /// `[A, x, B]`.
    fn interior_drop() -> PageTextSnapshot {
        snapshot("AB", 3, &[(2, 1)])
    }

    /// `[x, x, A]`: a plateau of two zero-width characters before the first
    /// real unit.
    fn double_drop() -> PageTextSnapshot {
        snapshot("A", 3, &[(1, 0), (2, 0)])
    }

    /// `[emoji, A]`.
    fn astral() -> PageTextSnapshot {
        snapshot("\u{1F600}A", 2, &[(1, 2)])
    }

    fn all() -> [PageTextSnapshot; 6] {
        [identity(), leading_drop(), trailing_drop(), interior_drop(), double_drop(), astral()]
    }

    /// `charRangeForTextOffsets` as upstream spells it: a text range in, a
    /// character range out, both `(start, count)`.
    fn char_range(s: &PageTextSnapshot, start: u32, count: u32) -> (u32, u32) {
        let r = char_range_for_text_offsets(s, start, start + count);
        (r.start, r.end - r.start)
    }

    fn slice(s: &PageTextSnapshot, start: u32, count: u32) -> String {
        slice_text_by_chars(s, start, start + count)
    }

    fn offsets(s: &PageTextSnapshot, boundaries: std::ops::RangeInclusive<u32>) -> Vec<u32> {
        boundaries.map(|b| boundary_text_offset(s, b)).collect()
    }

    #[test]
    fn the_identity_map_is_the_identity_clamped() {
        let s = identity();
        assert_eq!(offsets(&s, 0..=3), [0, 1, 2, 3]);
        assert_eq!(boundary_text_offset(&s, 99), 3);
    }

    #[test]
    fn dropped_characters_advance_the_boundary_but_not_the_offset() {
        assert_eq!(offsets(&leading_drop(), 0..=3), [0, 0, 1, 2]);
        assert_eq!(offsets(&trailing_drop(), 0..=2), [0, 1, 1]);
        assert_eq!(offsets(&double_drop(), 0..=3), [0, 0, 0, 1]);
    }

    #[test]
    fn a_supplementary_character_advances_by_two() {
        assert_eq!(offsets(&astral(), 0..=2), [0, 2, 3]);
    }

    #[test]
    fn the_final_boundary_always_lands_on_the_text_length() {
        for s in all() {
            assert_eq!(boundary_text_offset(&s, s.char_count), s.text_len_utf16(), "{s:?}");
        }
    }

    #[test]
    fn a_start_is_right_biased_past_leading_zero_width_characters() {
        // "A" in [x, A, B] must not include the dropped character's slot.
        assert_eq!(char_range(&leading_drop(), 0, 1), (1, 1));
    }

    #[test]
    fn an_end_is_left_biased_before_trailing_zero_width_characters() {
        // "A" in [A, x] must not swallow the trailing dropped character.
        assert_eq!(char_range(&trailing_drop(), 0, 1), (0, 1));
    }

    #[test]
    fn an_interior_zero_width_character_belongs_to_neither_neighbour() {
        assert_eq!(char_range(&interior_drop(), 0, 1), (0, 1)); // "A"
        assert_eq!(char_range(&interior_drop(), 1, 1), (2, 1)); // "B"
    }

    #[test]
    fn a_plateau_of_several_zero_width_characters_resolves_to_its_far_edge() {
        assert_eq!(char_range(&double_drop(), 0, 1), (2, 1));
    }

    #[test]
    fn an_offset_inside_a_surrogate_pair_covers_the_whole_character() {
        assert_eq!(char_range(&astral(), 1, 2), (0, 2));
        assert_eq!(char_range(&astral(), 0, 1), (0, 1));
    }

    #[test]
    fn an_empty_input_normalizes_to_a_single_boundary_and_never_inverts() {
        assert_eq!(char_range(&double_drop(), 0, 0).1, 0);
        assert_eq!(char_range(&identity(), 2, 0), (2, 0));
    }

    #[test]
    fn the_full_range_is_always_the_whole_text() {
        for s in all() {
            assert_eq!(slice(&s, 0, s.char_count), s.text, "{s:?}");
        }
    }

    #[test]
    fn dropped_characters_contribute_nothing_to_a_slice() {
        assert_eq!(slice(&leading_drop(), 0, 1), "");
        assert_eq!(slice(&leading_drop(), 0, 2), "A");
        assert_eq!(slice(&interior_drop(), 0, 2), "A");
        assert_eq!(slice(&interior_drop(), 1, 2), "B");
    }

    #[test]
    fn a_supplementary_character_arrives_whole() {
        assert_eq!(slice(&astral(), 0, 1), "\u{1F600}");
        assert_eq!(slice(&astral(), 1, 1), "A");
    }

    #[test]
    fn an_empty_range_slices_to_nothing() {
        assert_eq!(slice(&identity(), 2, 0), "");
    }

    #[test]
    fn start_and_end_bias_disagree_exactly_on_plateaus() {
        let s = trailing_drop();
        // [A, x] has a plateau at offset 1: start crosses it, end stops before.
        assert_eq!(char_boundary_at_text_offset(&s, 1, CharBoundaryBias::Start), 2);
        assert_eq!(char_boundary_at_text_offset(&s, 1, CharBoundaryBias::End), 1);
        // No plateau at offset 0.
        assert_eq!(char_boundary_at_text_offset(&s, 0, CharBoundaryBias::Start), 0);
        assert_eq!(char_boundary_at_text_offset(&s, 0, CharBoundaryBias::End), 0);
    }

    #[test]
    fn every_fixture_is_a_valid_map() {
        for s in all() {
            assert_eq!(char_map_violation(s.char_count, s.text_len_utf16(), &s.char_map), None);
        }
    }

    fn violation(char_count: u32, text_len: u32, anchors: &[(u32, u32)]) -> String {
        let map: Vec<CharMapAnchor> =
            anchors.iter().map(|&(c, t)| CharMapAnchor::new(c, t)).collect();
        char_map_violation(char_count, text_len, &map).expect("a violation")
    }

    #[test]
    fn identity_requires_matching_lengths() {
        assert!(violation(3, 2, &[]).contains("identity"));
    }

    #[test]
    fn anchors_must_strictly_increase_and_stay_on_the_page() {
        assert!(violation(3, 2, &[(1, 0), (1, 0)]).contains("strictly"));
        assert!(violation(3, 2, &[(0, 0)]).contains("strictly"));
        assert!(violation(3, 2, &[(4, 2)]).contains("outside"));
    }

    #[test]
    fn a_step_other_than_zero_or_two_is_malformed() {
        // An anchor implying the previous character contributed exactly one
        // unit is redundant: the canonical form has no such anchor.
        assert!(violation(3, 3, &[(1, 1)]).contains("expected 0 or 2"));
        assert!(violation(3, 5, &[(1, 3)]).contains("expected 0 or 2"));
    }

    #[test]
    fn a_tail_that_misses_the_text_length_is_malformed() {
        assert!(violation(3, 3, &[(1, 0)]).contains("text length"));
    }
}
