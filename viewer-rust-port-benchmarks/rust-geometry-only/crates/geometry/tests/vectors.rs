//! Reads the SAME `vectors/anchor-vectors.json` the TypeScript suite reads.
//!
//! This is the differential-testing mechanism the port plan depends on. The
//! file is frozen and never regenerated here, so a Rust implementation that
//! drifts from the TypeScript reference fails this test instead of being
//! discovered later in a browser.

use poc_geometry::core::{
    anchor_factors, point_in_quad, project_quad, rect_quad, Quad, Rect, Point,
};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

const TOL: f64 = 1e-6;

fn vectors() -> Value {
    // crates/geometry -> crate root, then up to the POC dir, then the shared file.
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../vectors/anchor-vectors.json")
        .canonicalize()
        .expect("shared vectors file must exist; run `pnpm gen:vectors` in typescript-baseline");
    let raw = fs::read_to_string(path).expect("readable vectors file");
    serde_json::from_str(&raw).expect("valid JSON")
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
    assert!(cases.len() >= 20, "expected a real vector set, got {}", cases.len());

    let mut checked = 0;
    for case in cases {
        let name = case["name"].as_str().unwrap_or("<unnamed>");
        let anchored = case["anchored"].as_bool().expect("anchored flag");
        let zoom = f(&case["view"], "zoom");
        let rotation = f(&case["view"], "rotation");

        // The factors, including the null-means-identity contract.
        let expected_factors = &case["factors"];
        match anchor_factors(anchored, zoom, rotation) {
            None => assert!(
                expected_factors.is_null(),
                "{name}: Rust said identity, TypeScript said {expected_factors}"
            ),
            Some((s, r)) => {
                assert!(
                    !expected_factors.is_null(),
                    "{name}: Rust said ({s}, {r}), TypeScript said identity"
                );
                assert!((s - f(expected_factors, "s")).abs() < TOL, "{name}: s");
                assert!((r - f(expected_factors, "r")).abs() < TOL, "{name}: r");
            }
        }

        // The projected quad.
        let base = rect_quad(rect_of(case), f(case, "rot"));
        let out = project_quad(base, anchored, zoom, rotation);
        let expected = case["quad"].as_array().expect("quad array");
        for (i, p) in out.0.iter().enumerate() {
            let e = expected[i].as_array().expect("corner pair");
            let ex = e[0].as_f64().unwrap();
            let ey = e[1].as_f64().unwrap();
            assert!(
                (p.x - ex).abs() < TOL && (p.y - ey).abs() < TOL,
                "{name}: corner {i} was ({}, {}), expected ({ex}, {ey})",
                p.x,
                p.y
            );
        }
        checked += 1;
    }
    assert_eq!(checked, cases.len());
}

#[test]
fn hit_vectors_match_the_typescript_reference() {
    let data = vectors();
    let cases = data["hit"].as_array().expect("hit array");
    assert!(!cases.is_empty());

    for case in cases {
        let name = case["name"].as_str().unwrap_or("<unnamed>");
        let q = rect_quad(rect_of(case), f(case, "rot"));
        let p = Point {
            x: f(&case["point"], "x"),
            y: f(&case["point"], "y"),
        };
        let expected = case["inside"].as_bool().expect("inside flag");
        assert_eq!(point_in_quad(p, q), expected, "{name}");
    }
}

#[test]
fn identity_projection_returns_an_equal_quad() {
    // Rust can return the same VALUE. What it cannot do, once this crosses into
    // JavaScript, is return the caller's same OBJECT. That is the cost the
    // wrapper documents and the demo counts.
    let q = rect_quad(
        Rect {
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 40.0,
        },
        0.0,
    );
    let same: Quad = project_quad(q, true, 1.0, 0.0);
    assert_eq!(same, q);
}
