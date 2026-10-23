//! This crate's text layout beside the engine's (Ruling13.15).
//!
//! `src/text_layout.rs` is a second transcription of `text/layout.ts` and
//! `pageSpace/text.ts`, because `epdf-engine` links PDFium and the plugins
//! also build for wasm32. Native hosts can build both, so this runs every
//! page-space layout the selection vectors carry, and a few focused
//! layouts (a shared-edge tie, empty text, rotated and mirrored runs, a
//! page with no box at all), through both, and asserts they answer alike:
//! the character count, the character at every point of a grid, the word,
//! line and cell of every character, and the segments of every range. The
//! engine side is built with `fake_engine::hydrate_text_layout`, the
//! harness's one door to the engine's layout.
//!
//! Two vector layouts have runs whose `start`s don't tile the glyphs. The
//! engine's copy indexes past the glyphs there and panics (it reads only
//! PDFium snapshots, which always tile); this crate's skips what a run
//! claims past the end (S2). Those two are compared only through this
//! crate's own vector replay, and `a_layout_that_does_not_tile_is_skipped`
//! pins which they are.

#![cfg(not(target_arch = "wasm32"))]

use epdf_engine::features::text::TextRange as EngineRange;
use epdf_engine::geom::PagePoint;
use epdf_geometry::Point;
use epdf_plugin_selection::text_layout::{
    PageGeometrySnapshot, TextLayout, TextRange, create_text_layout,
};
use plugin_conformance::canonical::equal;
use plugin_conformance::fake_engine::hydrate_text_layout;
use serde::Serialize;
use serde_json::{Value, json};

fn both(snapshot: &Value) -> (TextLayout, epdf_engine::page_space::text::TextLayout) {
    let ours: PageGeometrySnapshot =
        serde_json::from_value(snapshot.clone()).expect("this crate reads the snapshot");
    let theirs =
        hydrate_text_layout(&json!({ "$textLayout": snapshot })).expect("the engine reads it");
    (create_text_layout(ours), theirs)
}

fn value<T: Serialize>(v: &T) -> Value {
    serde_json::to_value(v).expect("plain data")
}

/// The two answers are equal as the vector comparator compares numbers:
/// within one part in 10^12, where a rotated frame's last bit depends on
/// the platform's `hypot` (this crate follows JavaScript's `Math.hypot`).
#[track_caller]
fn same<A: Serialize, B: Serialize>(ours: &A, theirs: &B, what: impl FnOnce() -> String) {
    let (a, b) = (value(ours), value(theirs));
    assert!(equal(&b, &a), "{}\n ours:   {a}\n engine: {b}", what());
}

/// Assert the two layouts of `snapshot` answer alike; returns how many
/// answers it compared.
fn agree(name: &str, snapshot: &Value) -> usize {
    let (ours, theirs) = both(snapshot);
    let n = ours.char_count();
    assert_eq!(n, theirs.char_count() as usize, "{name}: char count");
    let mut compared = 1;

    for i in 0..n {
        let (g, e) = (i as i64, i as u32);
        same(&ours.word_at(g), &theirs.word_at(e), || format!("{name}: word at {i}"));
        same(&ours.line_at(g), &theirs.line_at(e), || format!("{name}: line at {i}"));
        same(&ours.char_quad(g), &theirs.char_quad(e), || format!("{name}: quad {i}"));
        compared += 3;
    }
    for start in 0..=n {
        for count in 0..=(n - start + 1) {
            let a = ours.segments(TextRange { start: start as i64, count: count as i64 });
            let b = theirs.segments(EngineRange { start: start as u32, count: count as u32 });
            same(&a, &b, || format!("{name}: segments {start}+{count}"));
            compared += 1;
        }
    }

    // A grid over the text's bounds and a margin past them: half-unit
    // steps, which land on cell edges and between them, coarsened so a
    // page-sized layout stays near 150 by 150 points.
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for run in snapshot["runs"].as_array().into_iter().flatten() {
        let r = &run["rect"];
        let (x, y) = (r["x"].as_f64().unwrap(), r["y"].as_f64().unwrap());
        let (w, h) = (r["width"].as_f64().unwrap(), r["height"].as_f64().unwrap());
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x + w);
        y1 = y1.max(y + h);
    }
    if x0 <= x1 {
        let step = ((x1 - x0).max(y1 - y0) / 150.0).max(0.5);
        let mut y = y0 - 10.0;
        while y <= y1 + 10.0 {
            let mut x = x0 - 10.0;
            while x <= x1 + 10.0 {
                let a = ours.char_at(Point { x, y }).map(|i| i as u32);
                let b = theirs.char_at(PagePoint { x, y });
                assert_eq!(a, b, "{name}: char at ({x}, {y})");
                compared += 1;
                x += step;
            }
            y += step;
        }
    }
    compared
}

/// Every distinct page-space layout `selection-geometry.json` hands a layout
/// function, and every one its scenarios' fake engine answers.
/// Whether the runs' `start`s tile the glyphs, as a PDFium snapshot's do.
fn tiles(snapshot: &Value) -> bool {
    let mut next = 0;
    for run in snapshot["runs"].as_array().into_iter().flatten() {
        if run["start"].as_u64() != Some(next) {
            return false;
        }
        next += run["glyphs"].as_array().map_or(0, Vec::len) as u64;
    }
    true
}

fn vector_layouts() -> Vec<(String, Value)> {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../tooling/plugin-conformance/vectors");
    let read = |file: &str| -> Value {
        serde_json::from_str(&std::fs::read_to_string(format!("{root}/{file}")).unwrap()).unwrap()
    };
    let mut out: Vec<(String, Value)> = Vec::new();
    let mut push = |name: String, snapshot: &Value| {
        if !out.iter().any(|(_, s)| s == snapshot) {
            out.push((name, snapshot.clone()));
        }
    };
    let layout_fns = [
        "textGlyphAt",
        "expandTextRangeToWord",
        "expandTextRangeToLine",
        "textGlyphQuad",
        "textSegmentsForRange",
        "layoutSweep",
        "glyphAtGrid",
        "pairProbe",
    ];
    for case in read("plugins/selection-geometry.json")["cases"].as_array().unwrap() {
        if layout_fns.contains(&case["fn"].as_str().unwrap_or_default()) {
            push(case["name"].as_str().unwrap().to_string(), &case["args"][0]);
        }
    }
    for case in read("plugins/selection.json")["cases"].as_array().unwrap() {
        let responses = case["engine"]["documents"]
            .as_object()
            .into_iter()
            .flat_map(|docs| docs.values())
            .flat_map(|doc| doc["responses"].as_array().cloned().unwrap_or_default());
        for response in responses {
            if response["path"] == "page.text.layout"
                && let Some(snapshot) = response["value"].get("$textLayout")
            {
                push(format!("{}/pon {}", case["name"], response["pon"]), snapshot);
            }
        }
    }
    out
}

#[test]
fn every_vector_layout_answers_as_the_engines_does() {
    let layouts = vector_layouts();
    assert!(layouts.len() >= 20, "the vectors still carry their layouts ({})", layouts.len());
    let compared: usize = layouts
        .iter()
        .filter(|(_, snapshot)| tiles(snapshot))
        .map(|(name, snapshot)| agree(name, snapshot))
        .sum();
    eprintln!("compared {compared} answers over {} layouts", layouts.len());
    assert!(compared > 10_000, "compared {compared} answers");
}

#[test]
fn a_layout_that_does_not_tile_is_skipped() {
    let skipped: Vec<String> = vector_layouts()
        .into_iter()
        .filter(|(_, snapshot)| !tiles(snapshot))
        .map(|(name, _)| name)
        .collect();
    assert_eq!(
        skipped,
        [
            "buildPageTextLayout/rotated-all-empty",
            "textSegmentsForRange/char-start-past-the-glyphs"
        ]
    );
}

fn cell(x: f64, y: f64, w: f64) -> Value {
    json!({ "loose": { "x": x, "y": y, "width": w, "height": 10 } })
}

#[test]
fn a_shared_edge_keeps_the_first_cell_in_both() {
    // The first cell's right edge is 0.1 + 0.2, a rounding bit past the
    // second's left edge at 0.3, so a point there is nearer the second by
    // that bit alone; both layouts keep the first.
    let snapshot = json!({ "runs": [{
        "rect": { "x": 0.1, "y": 0, "width": 0.4, "height": 10 }, "start": 0,
        "glyphs": [
            { "loose": { "x": 0.1, "y": 0, "width": 0.2, "height": 10 } },
            { "loose": { "x": 0.3, "y": 0, "width": 0.2, "height": 10 } }
        ]
    }] });
    let (ours, theirs) = both(&snapshot);
    let point = Point { x: 0.3, y: -1e-12 };
    assert_eq!(ours.char_at(point), Some(0));
    assert_eq!(theirs.char_at(PagePoint { x: point.x, y: point.y }), Some(0));
    agree("shared-edge", &snapshot);
}

#[test]
fn empty_text_and_boxless_runs_answer_alike() {
    agree("no runs", &json!({ "runs": [] }));
    agree(
        "only empty characters",
        &json!({ "runs": [{ "rect": { "x": 0, "y": 0, "width": 0, "height": 0 }, "start": 0,
            "glyphs": [
                { "empty": true, "loose": { "x": 0, "y": 0, "width": 0, "height": 0 } },
                { "empty": true, "loose": { "x": 0, "y": 0, "width": 0, "height": 0 } }
            ] }] }),
    );
    let (ours, _) = both(&json!({ "runs": [] }));
    assert_eq!(ours.char_count(), 0);
    assert_eq!(ours.char_at(Point { x: 0.0, y: 0.0 }), None);
    assert!(ours.segments(TextRange { start: 0, count: 5 }).is_empty());
}

#[test]
fn rotated_mirrored_and_mixed_lines_answer_alike() {
    let column = |x: f64, ys: [f64; 3], flip: bool| {
        let glyphs: Vec<Value> = ys
            .iter()
            .map(|&y| {
                let (l, r) = if flip { (x + 12.0, x) } else { (x, x + 12.0) };
                json!({ "loose": {
                    "upperLeft": { "x": l, "y": y + 8.0 }, "upperRight": { "x": l, "y": y },
                    "lowerLeft": { "x": r, "y": y + 8.0 }, "lowerRight": { "x": r, "y": y }
                } })
            })
            .collect();
        json!({ "rect": { "x": x, "y": ys[2], "width": 12, "height": 24 }, "start": 0,
            "rotation": 270, "ascentFlip": flip, "glyphs": glyphs })
    };
    let mut up = column(88.0, [756.0, 748.0, 740.0], false);
    up["start"] = json!(4);
    let mut mirrored = column(120.0, [756.0, 748.0, 740.0], true);
    mirrored["start"] = json!(7);
    let snapshot = json!({ "runs": [
        { "rect": { "x": 10, "y": 700, "width": 24, "height": 10 }, "start": 0, "fontSize": 10,
          "glyphs": [cell(10.0, 700.0, 8.0), cell(18.0, 700.0, 8.0),
                     { "space": true, "loose": { "x": 26, "y": 700, "width": 8, "height": 10 } },
                     cell(60.0, 700.0, 8.0)] },
        up,
        mirrored,
        { "rect": { "x": 10, "y": 712, "width": 16, "height": 10 }, "start": 10, "fontSize": 30,
          "glyphs": [cell(10.0, 712.0, 8.0), cell(18.0, 712.0, 8.0)] },
    ] });
    agree("rotated, mirrored, mixed", &snapshot);
}
