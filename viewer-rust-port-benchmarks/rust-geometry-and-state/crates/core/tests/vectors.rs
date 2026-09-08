//! The same frozen `vectors/anchor-vectors.json` typescript-baseline's TypeScript suite and
//! rust-geometry-only's Rust crate read. Three implementations, one contract.

use poc_core::geom::{point_in_quad, rect_quad, Rect, Point};
use poc_core::model::{anchor_factors, project_quad, ViewEnv};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

const TOL: f64 = 1e-6;

fn vectors() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../vectors/anchor-vectors.json")
        .canonicalize()
        .expect("shared vectors file must exist; run `pnpm gen:vectors` in typescript-baseline");
    serde_json::from_str(&fs::read_to_string(path).expect("readable")).expect("valid JSON")
}

fn f(v: &Value, key: &str) -> f64 {
    v[key].as_f64().unwrap_or_else(|| panic!("missing number {key}"))
}

fn rect_of(v: &Value) -> Rect {
    let r = &v["rect"];
    Rect {
        x: f(r, "x"),
        y: f(r, "y"),
        width: f(r, "width"),
        height: f(r, "height"),
    }
}

#[test]
fn projection_vectors_match_the_typescript_reference() {
    let data = vectors();
    let cases = data["projection"].as_array().expect("projection array");
    assert!(cases.len() >= 20, "expected a real vector set");

    for case in cases {
        let name = case["name"].as_str().unwrap_or("<unnamed>");
        let anchored = case["anchored"].as_bool().expect("anchored");
        let view = ViewEnv {
            zoom: f(&case["view"], "zoom"),
            rotation: f(&case["view"], "rotation"),
        };

        let expected_factors = &case["factors"];
        match anchor_factors(anchored, view) {
            None => assert!(expected_factors.is_null(), "{name}: expected identity"),
            Some((s, r)) => {
                assert!(!expected_factors.is_null(), "{name}: expected factors");
                assert!((s - f(expected_factors, "s")).abs() < TOL, "{name}: s");
                assert!((r - f(expected_factors, "r")).abs() < TOL, "{name}: r");
            }
        }

        let out = project_quad(rect_quad(rect_of(case), f(case, "rot")), anchored, view);
        let expected = case["quad"].as_array().expect("quad");
        for (i, p) in out.0.iter().enumerate() {
            let e = expected[i].as_array().unwrap();
            assert!(
                (p.x - e[0].as_f64().unwrap()).abs() < TOL
                    && (p.y - e[1].as_f64().unwrap()).abs() < TOL,
                "{name}: corner {i}"
            );
        }
    }
}

#[test]
fn hit_vectors_match_the_typescript_reference() {
    let data = vectors();
    for case in data["hit"].as_array().expect("hit array") {
        let name = case["name"].as_str().unwrap_or("<unnamed>");
        let q = rect_quad(rect_of(case), f(case, "rot"));
        let p = Point {
            x: f(&case["point"], "x"),
            y: f(&case["point"], "y"),
        };
        assert_eq!(
            point_in_quad(p, q),
            case["inside"].as_bool().unwrap(),
            "{name}"
        );
    }
}
