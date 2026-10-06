//! `PageGeometryRun` reads in one pass what the earlier decode read through a
//! `serde_json::Value`: the same runs from the same JSON, and an error for the
//! same malformed JSON.

use epdf_plugin_selection::text_layout::{
    PageGeometryRun, PageGeometrySnapshot, RotatedGeometryRun, UprightGeometryRun,
};
use serde_json::{Value, json};

/// The decode this replaced: buffer the run, narrow on `'rotation' in run`.
fn through_value(run: &Value) -> Result<PageGeometryRun, serde_json::Error> {
    if run.get("rotation").is_some() {
        serde_json::from_value::<RotatedGeometryRun>(run.clone()).map(PageGeometryRun::Rotated)
    } else {
        serde_json::from_value::<UprightGeometryRun>(run.clone()).map(PageGeometryRun::Upright)
    }
}

fn rect(l: f64) -> Value {
    json!({ "left": l, "bottom": 1.5, "right": l + 4.0, "top": 9.25 })
}

fn quad(x: f64) -> Value {
    json!({
        "p1": { "x": x, "y": 1.0 }, "p2": { "x": x + 1.0, "y": 1.0 },
        "p3": { "x": x, "y": 0.0 }, "p4": { "x": x + 1.0, "y": 0.0 },
    })
}

fn runs() -> Vec<Value> {
    vec![
        json!({ "rect": rect(0.0), "charStart": 0, "glyphs": [
            { "looseBox": rect(0.0), "flags": 0, "tightBox": rect(0.5) },
            { "looseBox": rect(4.0), "flags": 1 },
            { "looseBox": rect(8.0), "flags": 2, "tightBox": null },
        ] }),
        json!({ "rect": rect(1.0), "charStart": 3, "fontSize": 12.0, "glyphs": [] }),
        json!({ "rect": rect(1.0), "charStart": 3, "fontSize": null, "glyphs": [] }),
        // `rotation` before the glyphs, and after them.
        json!({ "rect": rect(2.0), "charStart": 7, "rotation": 0.5, "ascentFlip": true, "glyphs": [
            { "looseQuad": quad(0.0), "flags": 0, "tightQuad": quad(0.25) },
            { "looseQuad": quad(1.0), "flags": 1 },
        ] }),
        json!({ "rect": rect(2.0), "charStart": 9, "glyphs": [
            { "looseQuad": quad(2.0), "flags": 0 },
        ], "ascentFlip": false, "rotation": -1.25, "fontSize": 7.5 }),
        // Members the run's kind doesn't read are ignored, as before.
        json!({ "rect": rect(3.0), "charStart": 0, "extra": [1, 2], "glyphs": [
            { "looseBox": rect(0.0), "flags": 0, "looseQuad": quad(0.0), "other": true },
        ] }),
    ]
}

#[test]
fn reads_the_same_runs_as_the_value_decode() {
    for run in runs() {
        let text = run.to_string();
        let direct: PageGeometryRun = serde_json::from_str(&text).unwrap();
        assert_eq!(direct, through_value(&run).unwrap(), "{text}");
    }
    let snapshot = json!({ "runs": runs() });
    let direct: PageGeometrySnapshot = serde_json::from_str(&snapshot.to_string()).unwrap();
    let expected: Vec<PageGeometryRun> = runs().iter().map(|r| through_value(r).unwrap()).collect();
    assert_eq!(direct.runs, expected);
    // A snapshot read from a parsed value (a native host's answer) agrees too.
    let from_value: PageGeometrySnapshot = serde_json::from_value(snapshot).unwrap();
    assert_eq!(from_value.runs, expected);
}

#[test]
fn refuses_what_the_value_decode_refused() {
    let bad = [
        // `'rotation' in run` with a null rotation.
        json!({ "rect": rect(0.0), "charStart": 0, "rotation": null, "ascentFlip": true, "glyphs": [] }),
        // A rotated run with no ascentFlip, or upright glyphs.
        json!({ "rect": rect(0.0), "charStart": 0, "rotation": 1.0, "glyphs": [] }),
        json!({ "rect": rect(0.0), "charStart": 0, "rotation": 1.0, "ascentFlip": false, "glyphs": [
            { "looseBox": rect(0.0), "flags": 0 },
        ] }),
        // An upright glyph with no looseBox, a run with no rect.
        json!({ "rect": rect(0.0), "charStart": 0, "glyphs": [{ "flags": 0 }] }),
        json!({ "charStart": 0, "glyphs": [] }),
        json!({ "rect": rect(0.0), "charStart": -1, "glyphs": [] }),
    ];
    for run in bad {
        assert!(through_value(&run).is_err(), "the old decode accepted {run}");
        assert!(serde_json::from_str::<PageGeometryRun>(&run.to_string()).is_err(), "{run}");
    }
}
