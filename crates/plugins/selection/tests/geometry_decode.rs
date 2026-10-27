//! `PageGeometryRun` reads in one pass: `'rotation' in run` decides the
//! kind, wherever `rotation` sits, and a glyph whose cells are the other
//! kind's shape, a rotated run with a null rotation or no `ascentFlip`, and
//! a negative `start` are refused.

use epdf_plugin_selection::text_layout::{
    PageCoordinates, PageGeometryRun, PageGeometrySnapshot, PdfCoordinates,
};
use serde_json::{Value, json};

fn rect(x: f64) -> Value {
    json!({ "x": x, "y": 1.5, "width": 4, "height": 7.75 })
}

fn quad(x: f64) -> Value {
    json!({
        "upperLeft": { "x": x, "y": 0.0 }, "upperRight": { "x": x + 1.0, "y": 0.0 },
        "lowerLeft": { "x": x, "y": 1.0 }, "lowerRight": { "x": x + 1.0, "y": 1.0 },
    })
}

fn decode(run: &Value) -> Result<PageGeometryRun<PageCoordinates>, serde_json::Error> {
    serde_json::from_str(&run.to_string())
}

#[test]
fn reads_each_kind_wherever_rotation_sits() {
    let upright = json!({ "rect": rect(0.0), "start": 0, "glyphs": [
        { "loose": rect(0.0), "tight": rect(0.5) },
        { "space": true, "loose": rect(4.0) },
        { "empty": true, "loose": rect(8.0) },
    ] });
    let PageGeometryRun::Upright { glyphs, font_size, .. } = decode(&upright).unwrap() else {
        panic!("upright")
    };
    assert_eq!(font_size, None);
    assert!(glyphs[1].space && !glyphs[1].empty && glyphs[2].empty && glyphs[0].tight.is_some());

    let before = json!({ "rect": rect(2.0), "start": 7, "rotation": 90, "ascentFlip": true,
        "glyphs": [{ "loose": quad(0.0), "tight": quad(0.25) }] });
    let after = json!({ "rect": rect(2.0), "start": 7, "glyphs": [{ "loose": quad(0.0),
        "tight": quad(0.25) }], "ascentFlip": true, "rotation": 90 });
    assert_eq!(decode(&before).unwrap(), decode(&after).unwrap());
    assert!(
        matches!(decode(&before).unwrap(), PageGeometryRun::Rotated { rotation, .. } if rotation == 90.0)
    );

    // A snapshot read from a parsed value (a native host's answer) agrees.
    let snapshot = json!({ "runs": [upright, before] });
    let direct: PageGeometrySnapshot = serde_json::from_str(&snapshot.to_string()).unwrap();
    let parsed: PageGeometrySnapshot = serde_json::from_value(snapshot).unwrap();
    assert_eq!(direct, parsed);
}

#[test]
fn the_file_coordinates_read_edges() {
    let run = json!({ "rect": { "left": 0, "bottom": 1, "right": 4, "top": 9 }, "start": 0,
        "glyphs": [{ "loose": { "left": 0, "bottom": 1, "right": 4, "top": 9 } }] });
    assert!(serde_json::from_value::<PageGeometryRun<PdfCoordinates>>(run).is_ok());
}

#[test]
fn refuses_what_does_not_fit() {
    let bad = [
        // `'rotation' in run` with a null rotation.
        json!({ "rect": rect(0.0), "start": 0, "rotation": null, "ascentFlip": true, "glyphs": [] }),
        // A rotated run with no ascentFlip, or upright cells.
        json!({ "rect": rect(0.0), "start": 0, "rotation": 1.0, "glyphs": [] }),
        json!({ "rect": rect(0.0), "start": 0, "rotation": 1.0, "ascentFlip": false,
            "glyphs": [{ "loose": rect(0.0) }] }),
        // An upright run with rotated cells, a glyph with no loose cell, no rect.
        json!({ "rect": rect(0.0), "start": 0, "glyphs": [{ "loose": quad(0.0) }] }),
        json!({ "rect": rect(0.0), "start": 0, "glyphs": [{ "space": true }] }),
        json!({ "start": 0, "glyphs": [] }),
        json!({ "rect": rect(0.0), "start": -1, "glyphs": [] }),
    ];
    for run in bad {
        assert!(decode(&run).is_err(), "{run}");
    }
}
