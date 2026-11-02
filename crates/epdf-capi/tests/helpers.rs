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

// ── page transform ──

const PAGE_TRANSFORM: u32 = 6;

/// Exact equality with numbers compared as doubles, so `1056` and `1056.0`
/// are the same value and nothing is rounded.
fn same_exact(actual: &Value, expected: &Value) -> bool {
    match (actual, expected) {
        (Value::Number(a), Value::Number(e)) => a.as_f64() == e.as_f64(),
        (Value::Object(a), Value::Object(e)) => {
            a.len() == e.len()
                && e.iter().all(|(k, ev)| a.get(k).is_some_and(|av| same_exact(av, ev)))
        }
        (Value::Array(a), Value::Array(e)) => {
            a.len() == e.len() && a.iter().zip(e).all(|(av, ev)| same_exact(av, ev))
        }
        _ => actual == expected,
    }
}

fn mentions_non_finite(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.contains_key("$number") || map.values().any(mentions_non_finite),
        Value::Array(items) => items.iter().any(mentions_non_finite),
        _ => false,
    }
}

/// Whether the binding's contract admits this input: a quarter turn, and
/// finite positive size, scale, dpr and (when given) base scale.
fn admitted(input: &Value) -> bool {
    let positive = |v: &Value| v.as_f64().is_some_and(|n| n.is_finite() && n > 0.0);
    !mentions_non_finite(input)
        && [0.0, 90.0, 180.0, 270.0].contains(&input["rotation"].as_f64().unwrap_or(-1.0))
        && positive(&input["pageSize"]["width"])
        && positive(&input["pageSize"]["height"])
        && positive(&input["scale"])
        && positive(&input["dpr"])
        && (input.get("baseScale").is_none_or(Value::is_null) || positive(&input["baseScale"]))
}

const PROBE_KEYS: [&str; 6] = [
    "toPixels",
    "fromPixels",
    "contentToView",
    "viewToContent",
    "contentToViewRect",
    "viewToContentRect",
];

#[test]
fn page_transforms_agree_with_the_geometry_vectors() {
    let file = read_json("tooling/plugin-conformance/vectors/core/geometry-page.json");
    let (mut compared, mut refused_cases) = (0, 0);
    let mut turns = std::collections::BTreeSet::new();
    let mut probed = 0;
    let mut user_unit = 0;
    for case in file["cases"].as_array().unwrap().iter().filter(|c| c["fn"] == "pageTransform") {
        let name = case["name"].as_str().unwrap();
        let input = &case["args"][0];
        let probe = case["args"].get(1).cloned().unwrap_or(json!({}));
        let points = probe.get("points").cloned().unwrap_or(json!([]));
        let rects = probe.get("rects").cloned().unwrap_or(json!([]));
        let args = json!({
            "input": input,
            "pagePoints": points,
            "pageRects": rects,
            "viewPoints": points,
            "viewRects": rects,
        });
        // Outside the contract, singular, or a metric the vector records as
        // non-finite: the binding refuses rather than serializing it.
        let outcome = &case["outcome"];
        if !admitted(input) || outcome.get("returned").is_none() || mentions_non_finite(outcome) {
            refuse(PAGE_TRANSFORM, &args);
            refused_cases += 1;
            continue;
        }
        let returned = &outcome["returned"];
        let actual = ok(PAGE_TRANSFORM, &args);
        let mut metrics = returned.clone();
        for key in PROBE_KEYS {
            metrics.as_object_mut().unwrap().remove(key);
        }
        assert!(
            same_exact(&actual["transform"], &metrics),
            "{name}: {} against {metrics}",
            actual["transform"]
        );
        for (key, vector) in [
            ("pageToView", "contentToView"),
            ("viewToPage", "viewToContent"),
            ("pageRectsToView", "contentToViewRect"),
            ("viewRectsToPage", "viewToContentRect"),
        ] {
            assert!(
                same_exact(&actual[key], &returned[vector]),
                "{name} {key}: {} against {}",
                actual[key],
                returned[vector],
            );
        }
        turns.insert(input["rotation"].as_u64().unwrap());
        probed += usize::from(!points.as_array().unwrap().is_empty());
        user_unit += usize::from(name.contains("userunit"));
        compared += 1;
    }
    assert!(compared >= 50, "only {compared} transforms compared");
    assert!(refused_cases >= 10, "only {refused_cases} out-of-contract inputs refused");
    assert_eq!(turns.len(), 4, "not every quarter turn was compared: {turns:?}");
    assert!(probed >= 40, "only {probed} cases mapped points and rects");
    assert!(user_unit >= 4, "only {user_unit} UserUnit-folded scales compared");
}

fn letter(extra: Value) -> Value {
    let mut input = json!({ "pageSize": { "width": 612, "height": 792 }, "rotation": 90, "scale": 1.5, "dpr": 2 });
    for (k, v) in extra.as_object().unwrap() {
        input[k] = v.clone();
    }
    input
}

#[test]
fn page_transform_lists_default_to_empty_and_keep_their_order() {
    let result = ok(PAGE_TRANSFORM, &json!({ "input": letter(json!({})) }));
    for key in ["pageToView", "viewToPage", "pageRectsToView", "viewRectsToPage"] {
        assert_eq!(result[key], json!([]), "{key}");
    }
    let points = json!([{ "x": 0, "y": 0 }, { "x": 612, "y": 792 }]);
    let result = ok(PAGE_TRANSFORM, &json!({ "input": letter(json!({})), "pagePoints": points }));
    // A quarter turn puts the page's top-left at the view's top-right.
    assert!(same_exact(
        &result["pageToView"],
        &json!([{ "x": 1188, "y": 0 }, { "x": 0, "y": 918 }])
    ));
    let back = ok(
        PAGE_TRANSFORM,
        &json!({ "input": letter(json!({})), "viewPoints": result["pageToView"] }),
    );
    assert!(same_exact(&back["viewToPage"], &points), "{}", back["viewToPage"]);
    assert_eq!(
        ok(PAGE_TRANSFORM, &json!({ "input": letter(json!({ "baseScale": null })) }))["transform"]
            ["baseScale"],
        1.5
    );
}

#[test]
fn page_transforms_outside_the_contract_are_refused() {
    for extra in [
        json!({ "rotation": 45 }),
        json!({ "rotation": 360 }),
        json!({ "rotation": -90 }),
        json!({ "scale": 0 }),
        json!({ "dpr": -1 }),
        json!({ "baseScale": 0 }),
        json!({ "pageSize": { "width": 0, "height": 10 } }),
        json!({ "userUnit": 2 }),
    ] {
        refuse(PAGE_TRANSFORM, &json!({ "input": letter(extra) }));
    }
    let message = refuse(
        PAGE_TRANSFORM,
        &json!({ "input": { "pageSize": { "width": 1e300, "height": 1e300 }, "rotation": 0, "scale": 1e-300, "dpr": 1 } }),
    );
    assert!(message.contains("invert"), "{message}");
    // A finite input whose device size overflows.
    refuse(PAGE_TRANSFORM, &json!({ "input": letter(json!({ "scale": 1e300, "dpr": 1e300 })) }));
    let raw =
        r#"{"input":{"pageSize":{"width":612,"height":792},"rotation":0,"scale":1e999,"dpr":1}}"#;
    refused(PAGE_TRANSFORM, raw.as_bytes());
    refuse(
        PAGE_TRANSFORM,
        &json!({ "input": letter(json!({})), "pagePoints": [{ "x": f64::MAX, "y": 0 }] , "pageRects": [{ "x": 0, "y": 0, "width": f64::MAX, "height": 1 }] }),
    );
    refuse(PAGE_TRANSFORM, &json!({ "input": letter(json!({})), "points": [] }));
}

/// The raw response JSON of a helper call that must succeed.
fn ok_raw(operation: u32, args: &str) -> String {
    let mut result = u64::MAX;
    let status =
        unsafe { epdf_helper_call(operation, args.as_ptr(), args.len() as u64, &mut result) };
    let mut ptr = std::ptr::null();
    let mut len = 0;
    assert_eq!(unsafe { epdf_result_view(result, &mut ptr, &mut len) }, OK);
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) }.to_vec();
    assert_eq!(epdf_result_release(result), OK);
    let json_len = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
    let json = String::from_utf8(bytes[4..4 + json_len].to_vec()).unwrap();
    assert_eq!(status, OK, "{json}");
    json
}

#[test]
fn argument_doubles_decode_exactly() {
    // baseScale passes through page_transform untouched, so its output token
    // shows whether the argument decoder read the shortest round-trip
    // spelling of each double back to the same bits.
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    let mut wrong = Vec::new();
    for _ in 0..20_000 {
        state =
            state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        let mantissa = state >> 12;
        let exponent = 1023 - 20 + (state >> 3) % 40;
        let value = f64::from_bits((exponent << 52) | mantissa);
        let args = format!(
            r#"{{"input":{{"pageSize":{{"width":612,"height":792}},"rotation":0,"scale":1,"baseScale":{value:?},"dpr":1}}}}"#
        );
        let json = ok_raw(PAGE_TRANSFORM, &args);
        let token = json.split("\"baseScale\":").nth(1).unwrap().split([',', '}']).next().unwrap();
        if token.parse::<f64>().unwrap().to_bits() != value.to_bits() {
            wrong.push(format!("{value:?} came back as {token}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} of 20000 doubles changed, e.g. {:?}",
        wrong.len(),
        &wrong[..wrong.len().min(3)]
    );
}
