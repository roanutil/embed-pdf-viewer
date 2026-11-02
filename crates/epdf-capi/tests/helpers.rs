//! The pure helper exports, driven through the C ABI and checked against the
//! committed vectors: `tooling/engine-conformance/vectors/text/{layout,charmap}.json`
//! with the `pages.geometry` snapshots they were built from, and
//! `tooling/plugin-conformance/vectors/core/geometry-page.json`.
//!
//! The expected values come from the vectors, never from calling the engine
//! algorithm again, so these tests catch argument, range and framing drift at
//! the boundary as well as the algorithm itself.

use epdf_capi::*;
use serde_json::{Value, json};

const OK: u32 = 0;
const INVALID_ARGUMENT: u32 = 2;

const TEXT_SEGMENTS: u32 = 1;
const CHARACTER_QUAD: u32 = 2;
const SLICE_TEXT: u32 = 3;
const TEXT_OFFSETS_TO_CHARS: u32 = 4;
const CHARACTER_BOUNDARY_TO_TEXT: u32 = 5;

// ── calling the export ──

/// Calls a helper with raw argument bytes, returning the status and the
/// response JSON, and releases the result.
fn call_bytes(operation: u32, args: &[u8]) -> (u32, Value) {
    let mut result = u64::MAX;
    let status =
        unsafe { epdf_helper_call(operation, args.as_ptr(), args.len() as u64, &mut result) };
    assert_ne!(result, 0, "operation {operation} returned no result (status {status})");
    let mut ptr = std::ptr::null();
    let mut len = 0;
    assert_eq!(unsafe { epdf_result_view(result, &mut ptr, &mut len) }, OK);
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) }.to_vec();
    assert_eq!(epdf_result_release(result), OK);
    let json_len = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
    let json: Value = serde_json::from_slice(&bytes[4..4 + json_len]).expect("response JSON");
    let count = u32::from_le_bytes(bytes[4 + json_len..8 + json_len].try_into().unwrap());
    assert_eq!(count, 0, "helpers return no binary payloads");
    assert_eq!(bytes.len(), 8 + json_len, "trailing bytes after the frame");
    (status, json)
}

/// Calls a helper that must succeed and returns its `result` member.
fn ok(operation: u32, args: &Value) -> Value {
    let (status, json) = call_bytes(operation, args.to_string().as_bytes());
    assert_eq!(status, OK, "operation {operation} refused: {json}");
    json.get("result").cloned().unwrap_or_else(|| panic!("no result member: {json}"))
}

/// Calls a helper that must refuse with InvalidArgument and an InvalidArg
/// error envelope, returning the error's message.
fn refused(operation: u32, args: &[u8]) -> String {
    let (status, json) = call_bytes(operation, args);
    assert_eq!(status, INVALID_ARGUMENT, "operation {operation} accepted: {json}");
    assert_eq!(json["error"]["code"], "InvalidArg", "{json}");
    assert!(json.get("result").is_none(), "{json}");
    json["error"]["message"].as_str().unwrap_or_default().to_string()
}

fn refuse(operation: u32, args: &Value) -> String {
    refused(operation, args.to_string().as_bytes())
}

// ── vectors ──

fn read_json(relative: &str) -> Value {
    let path = test_support::workspace_root().join(relative);
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

fn layout_vectors() -> Value {
    let file = read_json("tooling/engine-conformance/vectors/text/layout.json");
    assert_eq!(file["schemaVersion"], 2, "layout vector envelope moved");
    assert_eq!(file["kind"], "text.layout");
    file
}

fn charmap_vectors() -> Value {
    let file = read_json("tooling/engine-conformance/vectors/text/charmap.json");
    assert_eq!(file["schemaVersion"], 1, "charmap vector envelope moved");
    assert_eq!(file["kind"], "text.charmap");
    file
}

/// The page-space geometry snapshot a layout case was built from.
fn geometry_snapshot(case: &str, page: u64) -> Value {
    let file = read_json(&format!("tooling/engine-conformance/vectors/pages.geometry/{case}.json"));
    file["pages"]
        .as_array()
        .expect("pages")
        .iter()
        .find(|p| p["index"] == page)
        .unwrap_or_else(|| panic!("{case} has no page {page}"))["snapshot"]
        .clone()
}

/// Every (case name, page index, layout probes) triple in layout.json.
fn layout_pages() -> Vec<(String, u64, Value)> {
    let file = layout_vectors();
    let mut pages = Vec::new();
    for case in file["cases"].as_array().expect("cases") {
        for page in case["pages"].as_array().expect("pages") {
            pages.push((
                case["case"].as_str().expect("case").to_string(),
                page["index"].as_u64().expect("index"),
                page["layout"].clone(),
            ));
        }
    }
    pages
}

/// Rounds to the layout corpus quantum the way its generator does
/// (`Math.round`, half up).
fn q(value: f64) -> f64 {
    (value * 10_000.0 + 0.5).floor() / 10_000.0
}

/// Whether an actual geometry value matches a quantized expected one, member
/// by member, quantizing only the actual numbers.
fn same_quantized(actual: &Value, expected: &Value) -> bool {
    match (actual, expected) {
        (Value::Number(a), Value::Number(e)) => q(a.as_f64().unwrap()) == e.as_f64().unwrap(),
        (Value::Object(a), Value::Object(e)) => {
            a.len() == e.len()
                && e.iter().all(|(k, ev)| a.get(k).is_some_and(|av| same_quantized(av, ev)))
        }
        (Value::Array(a), Value::Array(e)) => {
            a.len() == e.len() && a.iter().zip(e).all(|(av, ev)| same_quantized(av, ev))
        }
        _ => actual == expected,
    }
}

fn charmap_snapshot(case: &Value) -> Value {
    let mut snapshot = json!({ "text": case["text"], "charCount": case["charCount"] });
    if let Some(map) = case.get("charMap") {
        snapshot["charMap"] =
            map.as_array().unwrap().iter().map(|p| json!({ "char": p[0], "text": p[1] })).collect();
    }
    snapshot
}

// ── text segments and character quads ──

#[test]
fn segments_agree_with_the_layout_vectors() {
    let mut probes = 0;
    let mut reversed = 0;
    let mut merged = 0;
    for (case, page, layout) in layout_pages() {
        let geometry = geometry_snapshot(&case, page);
        for probe in layout["segments"].as_array().unwrap() {
            let range = json!({ "start": probe["start"], "count": probe["count"] });
            let actual = ok(TEXT_SEGMENTS, &json!({ "geometry": geometry, "range": range }));
            let expected = &probe["segments"];
            assert!(
                same_quantized(&actual, expected),
                "{case} page {page} range {range}: {actual} against {expected}",
            );
            let segments = expected.as_array().unwrap();
            reversed += segments.iter().filter(|s| s["advance"] == -1).count();
            merged += usize::from(segments.len() > 1);
            probes += 1;
        }
    }
    assert!(probes >= 150, "only {probes} segment probes ran");
    assert!(reversed > 0, "no right-to-left segment in the corpus");
    assert!(merged > 0, "no range spans more than one visual line");
}

#[test]
fn character_quads_agree_with_the_layout_vectors() {
    let (mut with_quad, mut without) = (0, 0);
    for (case, page, layout) in layout_pages() {
        let geometry = geometry_snapshot(&case, page);
        for probe in layout["glyphs"].as_array().unwrap() {
            let actual =
                ok(CHARACTER_QUAD, &json!({ "geometry": geometry, "index": probe["glyph"] }));
            let expected = &probe["quad"];
            assert!(
                same_quantized(&actual, expected),
                "{case} page {page} glyph {}: {actual} against {expected}",
                probe["glyph"],
            );
            if expected.is_null() { without += 1 } else { with_quad += 1 }
        }
    }
    assert!(with_quad + without >= 80, "only {} glyph probes ran", with_quad + without);
    assert!(with_quad > 0, "no character in the corpus has a quad");
    assert!(without > 0, "no zero-geometry character in the corpus");
}

#[test]
fn rotated_sheared_and_mirrored_runs_reach_the_layout() {
    // embedded-orientations has upright, rotated, sheared, mirrored and
    // vertical runs; every one of its glyph probes goes through the helper.
    let geometry = geometry_snapshot("embedded-orientations", 0);
    let runs = geometry["runs"].as_array().unwrap();
    assert!(runs.iter().any(|r| r.get("rotation").is_some()), "no rotated run");
    assert!(runs.iter().any(|r| r["ascentFlip"] == true), "no mirrored run");
}

#[test]
fn an_empty_range_has_no_segments_and_the_page_end_is_in_range() {
    let geometry = geometry_snapshot("non-identity-text", 0);
    let count = geometry["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["glyphs"].as_array().unwrap().len())
        .sum::<usize>();
    for start in [0, count] {
        let args = json!({ "geometry": geometry, "range": { "start": start, "count": 0 } });
        assert_eq!(ok(TEXT_SEGMENTS, &args), json!([]));
    }
    let empty = json!({ "runs": [] });
    assert_eq!(
        ok(TEXT_SEGMENTS, &json!({ "geometry": empty, "range": { "start": 0, "count": 0 } })),
        json!([])
    );
}

#[test]
fn out_of_range_characters_are_refused() {
    let geometry = geometry_snapshot("non-identity-text", 0);
    let count: u64 = geometry["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["glyphs"].as_array().unwrap().len() as u64)
        .sum();
    assert!(count > 0);
    for (start, length) in [(count, 1), (count + 1, 0), (0, count + 1), (u32::MAX as u64, 1)] {
        refuse(
            TEXT_SEGMENTS,
            &json!({ "geometry": geometry, "range": { "start": start, "count": length } }),
        );
    }
    refuse(TEXT_SEGMENTS, &json!({ "geometry": geometry, "range": { "start": -1, "count": 1 } }));
    for index in [count, u32::MAX as u64] {
        refuse(CHARACTER_QUAD, &json!({ "geometry": geometry, "index": index }));
    }
    refuse(CHARACTER_QUAD, &json!({ "geometry": { "runs": [] }, "index": 0 }));
}

fn upright_run(start: u64, glyphs: usize) -> Value {
    let glyph = json!({ "loose": { "x": 0, "y": 0, "width": 1, "height": 1 } });
    json!({
        "rect": { "x": 0, "y": 0, "width": glyphs, "height": 1 },
        "start": start,
        "glyphs": vec![glyph; glyphs],
    })
}

#[test]
fn snapshots_whose_runs_do_not_tile_the_characters_are_refused() {
    let tiled = json!({ "runs": [upright_run(0, 2), upright_run(2, 1)] });
    let quad = ok(CHARACTER_QUAD, &json!({ "geometry": tiled, "index": 2 }));
    assert_eq!(quad["upperLeft"]["x"].as_f64(), Some(0.0));
    for runs in [
        json!([upright_run(1, 2)]), // a gap before the first run
        json!([upright_run(0, 2), upright_run(3, 1)]), // a gap between runs
        json!([upright_run(0, 2), upright_run(1, 1)]), // overlapping runs
        json!([upright_run(0, 2), upright_run(0, 1)]), // a run restarting at zero
        json!([upright_run(0, 0)]), // a run with no characters
    ] {
        let message = refuse(CHARACTER_QUAD, &json!({ "geometry": { "runs": runs }, "index": 0 }));
        assert!(message.contains("tile"), "{message}");
    }
}

#[test]
fn snapshots_with_out_of_range_numbers_are_refused() {
    // JSON has no NaN or Infinity; an overflowing literal is the nearest
    // spelling and must not decode to a non-finite coordinate.
    let raw = r#"{"geometry":{"runs":[{"rect":{"x":1e999,"y":0,"width":1,"height":1},
        "start":0,"glyphs":[{"loose":{"x":0,"y":0,"width":1,"height":1}}]}]},"index":0}"#;
    refused(CHARACTER_QUAD, raw.as_bytes());
    let huge = json!({ "runs": [{
        "rect": { "x": 0, "y": 0, "width": 1, "height": 1 },
        "start": 0,
        "glyphs": [{ "loose": { "x": f64::MAX, "y": 0, "width": f64::MAX, "height": 1 } }],
    }]});
    // Finite in, but the cell's far edge overflows.
    refuse(CHARACTER_QUAD, &json!({ "geometry": huge, "index": 0 }));
}

// ── charmap ──

#[test]
fn slices_agree_with_the_charmap_vectors() {
    let file = charmap_vectors();
    let mut probes = 0;
    for case in file["cases"].as_array().unwrap() {
        let text = charmap_snapshot(case);
        for probe in case["slices"].as_array().unwrap() {
            let range = json!({ "start": probe["start"], "end": probe["end"] });
            let actual = ok(SLICE_TEXT, &json!({ "text": text, "range": range }));
            assert_eq!(actual, probe["text"], "{} chars {range}", case["name"]);
            probes += 1;
        }
        let whole = json!({ "start": 0, "end": case["charCount"] });
        assert_eq!(ok(SLICE_TEXT, &json!({ "text": text, "range": whole })), case["text"]);
    }
    assert!(probes >= 200, "only {probes} slices ran");
}

#[test]
fn text_offsets_map_to_the_vector_character_ranges() {
    let file = charmap_vectors();
    let mut probes = 0;
    for case in file["cases"].as_array().unwrap() {
        let text = charmap_snapshot(case);
        for probe in case["charRanges"].as_array().unwrap() {
            let range = json!({ "start": probe["start"], "end": probe["end"] });
            let actual = ok(TEXT_OFFSETS_TO_CHARS, &json!({ "text": text, "range": range }));
            assert_eq!(actual, probe["range"], "{} text {range}", case["name"]);
            probes += 1;
        }
    }
    assert!(probes >= 200, "only {probes} ranges ran");
}

#[test]
fn character_boundaries_map_to_the_vector_text_offsets() {
    let file = charmap_vectors();
    let mut probes = 0;
    for case in file["cases"].as_array().unwrap() {
        let text = charmap_snapshot(case);
        for (boundary, expected) in
            case["boundaryTextOffsets"].as_array().unwrap().iter().enumerate()
        {
            let actual =
                ok(CHARACTER_BOUNDARY_TO_TEXT, &json!({ "text": text, "boundary": boundary }));
            assert_eq!(&actual, expected, "{} boundary {boundary}", case["name"]);
            probes += 1;
        }
    }
    assert!(probes >= 60, "only {probes} boundaries ran");
}

#[test]
fn the_charmap_corpus_has_dropped_and_supplementary_characters() {
    let file = charmap_vectors();
    let cases = file["cases"].as_array().unwrap();
    let dropped = cases.iter().any(|c| c["charCount"].as_u64() > c["textLength"].as_u64());
    let astral = cases.iter().any(|c| c["charCount"].as_u64() < c["textLength"].as_u64());
    assert!(dropped && astral, "the corpus lost its zero-text or supplementary cases");
}

#[test]
fn malformed_character_maps_are_refused_as_the_vectors_say() {
    let file = charmap_vectors();
    let (mut rejected, mut accepted) = (0, 0);
    for case in file["violations"].as_array().unwrap() {
        let length = case["textLength"].as_u64().unwrap() as usize;
        let mut source = case.clone();
        source["text"] = json!("a".repeat(length));
        let text = charmap_snapshot(&source);
        let args = json!({ "text": text, "boundary": 0 });
        if case["violation"].is_null() {
            ok(CHARACTER_BOUNDARY_TO_TEXT, &args);
            accepted += 1;
        } else {
            let message = refuse(CHARACTER_BOUNDARY_TO_TEXT, &args);
            assert!(message.contains("char map"), "{}: {message}", case["name"]);
            for (op, args) in [
                (SLICE_TEXT, json!({ "text": text, "range": { "start": 0, "end": 0 } })),
                (TEXT_OFFSETS_TO_CHARS, json!({ "text": text, "range": { "start": 0, "end": 0 } })),
            ] {
                refuse(op, &args);
            }
            rejected += 1;
        }
    }
    assert!(rejected >= 8 && accepted >= 3, "{rejected} rejected, {accepted} accepted");
}

#[test]
fn out_of_range_text_indices_are_refused_and_inverted_ranges_are_empty() {
    // "a😀b": three characters, four UTF-16 units.
    let text =
        json!({ "text": "a\u{1F600}b", "charCount": 3, "charMap": [{ "char": 2, "text": 3 }] });
    assert_eq!(
        ok(SLICE_TEXT, &json!({ "text": text, "range": { "start": 1, "end": 2 } })),
        "\u{1F600}"
    );
    // Inverted ranges keep the vectors' empty answer rather than refusing.
    assert_eq!(ok(SLICE_TEXT, &json!({ "text": text, "range": { "start": 2, "end": 1 } })), "");
    assert_eq!(
        ok(TEXT_OFFSETS_TO_CHARS, &json!({ "text": text, "range": { "start": 3, "end": 1 } })),
        json!({ "start": 2, "end": 2 })
    );
    // An offset inside the surrogate pair covers the whole character.
    assert_eq!(
        ok(TEXT_OFFSETS_TO_CHARS, &json!({ "text": text, "range": { "start": 2, "end": 3 } })),
        json!({ "start": 1, "end": 2 })
    );
    assert_eq!(ok(CHARACTER_BOUNDARY_TO_TEXT, &json!({ "text": text, "boundary": 3 })), 4);
    for (op, args) in [
        (SLICE_TEXT, json!({ "text": text, "range": { "start": 0, "end": 4 } })),
        (SLICE_TEXT, json!({ "text": text, "range": { "start": 4, "end": 4 } })),
        (TEXT_OFFSETS_TO_CHARS, json!({ "text": text, "range": { "start": 0, "end": 5 } })),
        (TEXT_OFFSETS_TO_CHARS, json!({ "text": text, "range": { "start": 5, "end": 5 } })),
        (CHARACTER_BOUNDARY_TO_TEXT, json!({ "text": text, "boundary": 4 })),
        (CHARACTER_BOUNDARY_TO_TEXT, json!({ "text": text, "boundary": u32::MAX })),
        (CHARACTER_BOUNDARY_TO_TEXT, json!({ "text": text, "boundary": -1 })),
    ] {
        refuse(op, &args);
    }
}

// ── argument envelopes ──

#[test]
fn malformed_argument_envelopes_are_refused() {
    let text = json!({ "text": "ab", "charCount": 2 });
    let geometry = json!({ "runs": [] });
    for (op, args) in [
        (SLICE_TEXT, &b""[..]),
        (SLICE_TEXT, &b"not json"[..]),
        (SLICE_TEXT, &b"[]"[..]),
        (TEXT_SEGMENTS, &b"{}"[..]),
    ] {
        refused(op, args);
    }
    for (op, args) in [
        (SLICE_TEXT, json!({ "text": text, "range": { "start": 0, "end": 1 }, "extra": 1 })),
        (SLICE_TEXT, json!({ "text": text, "range": { "start": 0, "count": 1 } })),
        (SLICE_TEXT, json!({ "text": text })),
        (CHARACTER_BOUNDARY_TO_TEXT, json!({ "text": text, "boundary": 1.5 })),
        (TEXT_SEGMENTS, json!({ "geometry": geometry, "range": { "start": 0, "end": 0 } })),
        (CHARACTER_QUAD, json!({ "geometry": geometry, "index": 0, "docId": "x" })),
        (CHARACTER_QUAD, json!({ "text": text, "index": 0 })),
    ] {
        refuse(op, &args);
    }
}

#[test]
fn unknown_operations_and_bad_spans_keep_their_statuses() {
    let mut result = u64::MAX;
    let status = unsafe { epdf_helper_call(7, std::ptr::null(), 0, &mut result) };
    assert_eq!(status, INVALID_ARGUMENT);
    assert_ne!(result, 0);
    assert_eq!(epdf_result_release(result), OK);
    let status = unsafe { epdf_helper_call(SLICE_TEXT, std::ptr::null(), 4, &mut result) };
    assert_eq!(status, INVALID_ARGUMENT);
    assert_eq!(epdf_result_release(result), OK);
    let status = unsafe { epdf_helper_call(SLICE_TEXT, std::ptr::null(), u64::MAX, &mut result) };
    assert_eq!(status, 6);
    assert_eq!(epdf_result_release(result), OK);
    assert_eq!(
        unsafe { epdf_helper_call(SLICE_TEXT, std::ptr::null(), 0, std::ptr::null_mut()) },
        2
    );
}
