//! `src/engine/` beside `epdf-engine`'s copies of the same rules (Task 14.3).
//!
//! `src/engine/measure.rs` reads a `/Measure` off the annotation's JSON and
//! `epdf-engine`'s `features/measure/derive.rs` reads it into a struct;
//! `src/engine/resolve.rs` `measurement_follows` and the engine's
//! `pdfDeriveAnnotationPatch` merge a caption into a dimension patch. Each
//! pair is run here on the same inputs, malformed ones included, and
//! checked against what the TypeScript answered for them
//! (`measurementReadout` and `measurementFollows`, run under `tsx`).

#![cfg(not(target_arch = "wasm32"))]

use epdf_annotation_core::engine::measure::{DimensionInput, measurement_readout};
use epdf_annotation_core::engine::resolve::measurement_follows;
use epdf_engine::features::annotation::subtype::AnnotationSubtype;
use epdf_engine::features::annotation::write::patch::pdf_derive_annotation_patch;
use epdf_engine::features::measure::derive as engine;
use epdf_engine::geom::PdfPoint;
use serde_json::{Map, Value, json};

/// A line dimension from (0, 0) to `end`, with `measure`.
fn line(measure: Value, end: (f64, f64)) -> Map<String, Value> {
    json!({
        "subtype": "line", "intent": "line-dimension", "measure": measure,
        "linePoints": { "start": { "x": 0, "y": 0 }, "end": { "x": end.0, "y": end.1 } },
    })
    .as_object()
    .unwrap()
    .clone()
}

/// The readout this crate answers, as the TypeScript object prints it.
fn ours(dimension: &Map<String, Value>) -> Value {
    measurement_readout(&DimensionInput::of(dimension)).to_value()
}

/// The readout the engine answers, printed the same way.
fn theirs(dimension: &Map<String, Value>) -> Value {
    let point = |p: &Value| PdfPoint { x: p["x"].as_f64().unwrap(), y: p["y"].as_f64().unwrap() };
    let readout = engine::measurement_readout(&engine::DimensionInput {
        subtype: "line",
        intent: Some("line-dimension"),
        measure: dimension.get("measure"),
        line_points: Some((
            point(&dimension["linePoints"]["start"]),
            point(&dimension["linePoints"]["end"]),
        )),
        vertices: None,
    });
    match readout {
        Ok(r) => {
            let mut out = json!({ "kind": r.kind, "value": r.value, "label": r.label });
            if let Some(p) = r.perimeter {
                out["perimeter"] = json!(p);
            }
            out
        }
        Err(reason) => json!({ "unavailable": reason }),
    }
}

/// Numbers compared as numbers: `5` and `5.0` are one value.
fn canonical(value: Value) -> Value {
    match value {
        Value::Number(n) => json!(n.as_f64().unwrap()),
        Value::Object(map) => {
            Value::Object(map.into_iter().map(|(k, v)| (k, canonical(v))).collect())
        }
        other => other,
    }
}

#[test]
fn a_malformed_measure_reads_alike_in_both_copies_and_as_the_typescript_does() {
    let good = json!({ "subtype": "rectilinear",
        "x": [{ "unit": "m", "conversion": 1 }],
        "distance": [{ "unit": "m", "conversion": 1, "precision": 100 }], "area": [] });
    let with = |member: &str, value: Value| {
        let mut measure = good.clone();
        measure[member] = value;
        measure
    };
    let fraction = |fixed: Value| {
        with(
            "distance",
            json!([{ "unit": "m", "conversion": 1, "fraction": "fraction", "precision": 16,
                "fixed": fixed }]),
        )
    };
    let five = json!({ "kind": "distance", "value": 5, "label": "5.00 m" });
    let eighth = |label: &str| json!({ "kind": "distance", "value": 5.125, "label": label });
    let unavailable = |reason: &str| json!({ "unavailable": reason });
    // Each answer is the TypeScript's for the same input.
    let cases = [
        ("good", good.clone(), (3.0, 4.0), five.clone()),
        ("false", json!(false), (3.0, 4.0), unavailable("no-measure")),
        ("zero", json!(0), (3.0, 4.0), unavailable("no-measure")),
        ("empty string", json!(""), (3.0, 4.0), unavailable("no-measure")),
        ("string", json!("abc"), (3.0, 4.0), unavailable("foreign-measure")),
        ("number", json!(5), (3.0, 4.0), unavailable("foreign-measure")),
        (
            "x: a number first",
            with("x", json!([5, { "unit": "m", "conversion": 1 }])),
            (3.0, 4.0),
            unavailable("no-scale"),
        ),
        ("x: a string", with("x", json!("abc")), (3.0, 4.0), unavailable("no-scale")),
        (
            "distance: a number",
            with("distance", json!([5])),
            (3.0, 4.0),
            unavailable("invalid-format"),
        ),
        (
            "distance: a string",
            with("distance", json!(["m"])),
            (3.0, 4.0),
            unavailable("invalid-format"),
        ),
        ("y: false", with("y", json!(false)), (3.0, 4.0), five.clone()),
        ("y: 0", with("y", json!(0)), (3.0, 4.0), five),
        (
            "y: a number entry",
            {
                let mut m = with("y", json!([5]));
                m["cyx"] = json!(1);
                m
            },
            (3.0, 4.0),
            unavailable("no-scale"),
        ),
        ("fixed: 1", fraction(json!(1)), (5.125, 0.0), eighth("5 2/16 m")),
        ("fixed: 'no'", fraction(json!("no")), (5.125, 0.0), eighth("5 2/16 m")),
        ("fixed: 0", fraction(json!(0)), (5.125, 0.0), eighth("5 1/8 m")),
    ];
    for (name, measure, end, typescript) in cases {
        let dimension = line(measure, end);
        let (a, b) = (canonical(ours(&dimension)), canonical(theirs(&dimension)));
        assert_eq!(a, b, "{name}: this crate and the engine");
        assert_eq!(a, canonical(typescript), "{name}: the TypeScript");
    }
}

#[test]
fn a_caption_member_the_annotation_lacks_is_left_out_in_both_copies() {
    // `measurementFollows` spreads `current.captionPosition` and the rest,
    // `undefined` where the annotation has none, and the wire leaves an
    // `undefined` member out. The TypeScript answered `{"captionEnabled":
    // true,"subtype":"line"}`, `{"subtype":"line"}` and
    // `{"captionEnabled":true,"subtype":"polygon"}`.
    let object = |v: Value| v.as_object().unwrap().clone();
    let line = object(json!({ "subtype": "line", "intent": "line-dimension",
        "linePoints": { "start": { "x": 0, "y": 0 }, "end": { "x": 3, "y": 4 } } }));
    let polygon = object(json!({ "subtype": "polygon", "intent": "polygon-dimension",
        "vertices": [{ "x": 0, "y": 0 }, { "x": 4, "y": 0 }, { "x": 4, "y": 3 }] }));
    let cases = [
        (
            AnnotationSubtype::Line,
            &line,
            json!({ "captionEnabled": true }),
            json!({ "captionEnabled": true }),
        ),
        (AnnotationSubtype::Line, &line, json!({ "captionPosition": null }), json!({})),
        (
            AnnotationSubtype::Polygon,
            &polygon,
            json!({ "captionEnabled": true }),
            json!({ "captionEnabled": true }),
        ),
    ];
    for (subtype, current, patch, typescript) in cases {
        let mut ours = measurement_follows(current, &object(patch.clone())).unwrap();
        // The TypeScript names the subtype in the patch it hands on; the
        // engine's patch never carries it.
        ours.shift_remove("subtype");
        let theirs =
            pdf_derive_annotation_patch(subtype, current, object(patch.clone()), None).unwrap();
        assert_eq!(Value::Object(ours.clone()), typescript, "{patch}: this crate");
        assert_eq!(Value::Object(theirs), typescript, "{patch}: the engine");
    }
}
