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
