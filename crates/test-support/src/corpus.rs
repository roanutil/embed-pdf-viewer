//! Reading the frozen cross-language conformance vectors.
//!
//! The vectors live in `tooling/engine-conformance/vectors/<operation>/` and
//! are generated from the shipping TypeScript engine by
//! `@embedpdf/tooling-engine-conformance`. That package is the only thing that
//! writes them; this module only reads.
//!
//! The direction matters. TypeScript is the oracle for every operation it
//! already implements, so a Rust reader that disagrees is wrong by definition
//! until someone changes the corpus on purpose and says why in the commit.

use std::path::PathBuf;

use serde::Deserialize;
use serde_json::Value;

/// Vector envelope version. Bumped by the generator when the shape changes; a
/// stale checkout then fails loudly instead of comparing the wrong fields.
pub const SCHEMA_VERSION: u32 = 1;

/// Units per PDF point in the committed vectors. The Rust twin of
/// `QUANTIZATION_SCALE` in `tooling/engine-conformance/src/quantize.ts`.
pub const QUANTIZATION_SCALE: f64 = 10_000.0;

#[derive(Debug, Deserialize)]
pub struct VectorFile {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    pub operation: String,
    #[serde(rename = "case")]
    pub case_name: String,
    /// Repository-relative path to the PDF this was read from.
    pub fixture: String,
    /// One line on what the document is in the corpus for.
    pub covers: String,
    /// The user password, for encrypted fixtures.
    #[serde(default)]
    pub password: Option<String>,
    pub pages: Vec<PageVector>,
}

#[derive(Debug, Deserialize)]
pub struct PageVector {
    /// Zero-based page index, as the TypeScript engine reported it.
    pub index: u32,
    /// The page's PDF indirect object number, as the TypeScript engine
    /// reported it. Comparing this against the Rust index walk is itself a
    /// page-identity parity check.
    #[serde(rename = "pageObjectNumber")]
    pub page_object_number: u32,
    /// The operation's result, already quantized by the generator.
    pub snapshot: Value,
}

/// Loads every vector for one operation, sorted by case name so a failure
/// reports the same first case on every machine.
///
/// Panics when the directory is missing or empty: an operation with no
/// vectors is a silently green suite, which is worse than a red one.
pub fn load(operation: &str) -> Vec<VectorFile> {
    let dir = vectors_dir(operation);
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .map(|entry| entry.expect("dir entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "no vectors in {}", dir.display());

    paths
        .into_iter()
        .map(|path| {
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
            let file: VectorFile = serde_json::from_str(&text)
                .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
            assert_eq!(
                file.schema_version,
                SCHEMA_VERSION,
                "{} was written by a different generator; regenerate the corpus",
                path.display()
            );
            assert_eq!(
                file.operation,
                operation,
                "{} sits under {operation}/ but declares {}",
                path.display(),
                file.operation
            );
            file
        })
        .collect()
}

pub fn vectors_dir(operation: &str) -> PathBuf {
    crate::workspace_root().join("tooling/engine-conformance/vectors").join(operation)
}

/// Rounds to the nearest 1/[`QUANTIZATION_SCALE`], half away from zero.
///
/// Must agree exactly with `quantize()` in
/// `tooling/engine-conformance/src/quantize.ts`. `f64::round` already breaks
/// ties away from zero; the TypeScript side is the one that needs a sign
/// split to match.
pub fn quantize_units(value: f64) -> i64 {
    assert!(value.is_finite(), "cannot quantize a non-finite value: {value}");
    (value * QUANTIZATION_SCALE).round() as i64
}

/// Rewrites every number in a JSON tree as its quantized unit count, leaving
/// structure, strings and booleans alone.
///
/// Comparing unit counts rather than reconstructed floats keeps the whole
/// comparison in integers, so nothing in the gate depends on float equality.
/// Applying it to an already-quantized value is a no-op, which is what lets
/// the committed vector and a fresh Rust read meet in the same space.
pub fn quantized(value: &Value) -> Value {
    match value {
        Value::Number(number) => {
            let raw =
                number.as_f64().unwrap_or_else(|| panic!("number out of f64 range: {number}"));
            Value::Number(quantize_units(raw).into())
        }
        Value::Array(items) => Value::Array(items.iter().map(quantized).collect()),
        Value::Object(fields) => {
            Value::Object(fields.iter().map(|(k, v)| (k.clone(), quantized(v))).collect())
        }
        other => other.clone(),
    }
}

/// The first place two JSON trees diverge, as a JSON-pointer-ish path plus
/// both values, or `None` when they agree.
///
/// `assert_eq!` on two 5,000-glyph snapshots prints two megabytes and tells
/// you nothing. This prints one line.
pub fn first_difference(expected: &Value, actual: &Value) -> Option<String> {
    fn walk(path: &str, expected: &Value, actual: &Value) -> Option<String> {
        match (expected, actual) {
            (Value::Object(want), Value::Object(got)) => {
                for (key, want_child) in want {
                    let child_path = format!("{path}.{key}");
                    match got.get(key) {
                        Some(got_child) => {
                            if let Some(found) = walk(&child_path, want_child, got_child) {
                                return Some(found);
                            }
                        }
                        None => {
                            return Some(format!("{child_path}: expected {want_child}, absent"));
                        }
                    }
                }
                for key in got.keys() {
                    if !want.contains_key(key) {
                        return Some(format!("{path}.{key}: unexpected {}", got[key]));
                    }
                }
                None
            }
            (Value::Array(want), Value::Array(got)) => {
                if want.len() != got.len() {
                    return Some(format!(
                        "{path}: expected {} items, got {}",
                        want.len(),
                        got.len()
                    ));
                }
                want.iter()
                    .zip(got)
                    .enumerate()
                    .find_map(|(i, (w, g))| walk(&format!("{path}[{i}]"), w, g))
            }
            (want, got) if want == got => None,
            (want, got) => Some(format!("{path}: expected {want}, got {got}")),
        }
    }
    walk("", expected, actual)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn quantization_rounds_halves_away_from_zero_on_both_signs() {
        // The tie that would separate Rust's f64::round from JavaScript's
        // Math.round if the generator did not split on sign: 1/32 of a point
        // is exactly 312.5 units.
        assert_eq!(quantize_units(0.03125), 313);
        assert_eq!(quantize_units(-0.03125), -313);
    }

    #[test]
    fn quantization_is_idempotent_through_the_committed_decimal() {
        let raw = 107.450_073_242_187_5;
        let units = quantize_units(raw);
        let committed = units as f64 / QUANTIZATION_SCALE;
        assert_eq!(quantize_units(committed), units);
    }

    #[test]
    fn a_negative_zero_quantizes_to_the_same_units_as_zero() {
        assert_eq!(quantize_units(-0.0), quantize_units(0.0));
    }

    #[test]
    fn values_within_a_quantum_compare_equal() {
        let a = json!({ "x": 1.00001 });
        let b = json!({ "x": 1.00002 });
        assert_eq!(quantized(&a), quantized(&b));
    }

    #[test]
    fn values_a_quantum_apart_do_not() {
        let a = json!({ "x": 1.0 });
        let b = json!({ "x": 1.0002 });
        assert_eq!(
            first_difference(&quantized(&a), &quantized(&b)).as_deref(),
            Some(".x: expected 10000, got 10002"),
        );
    }

    #[test]
    fn a_missing_field_reports_its_path() {
        let want = json!({ "runs": [{ "charStart": 0, "fontSize": 10.0 }] });
        let got = json!({ "runs": [{ "charStart": 0 }] });
        assert_eq!(
            first_difference(&quantized(&want), &quantized(&got)).as_deref(),
            Some(".runs[0].fontSize: expected 100000, absent"),
        );
    }

    #[test]
    fn an_extra_field_reports_its_path() {
        let want = json!({ "runs": [{ "charStart": 0 }] });
        let got = json!({ "runs": [{ "charStart": 0, "rotation": 0.0 }] });
        assert_eq!(
            first_difference(&quantized(&want), &quantized(&got)).as_deref(),
            Some(".runs[0].rotation: unexpected 0"),
        );
    }

    #[test]
    fn a_length_mismatch_reports_both_lengths() {
        let want = json!({ "runs": [1, 2, 3] });
        let got = json!({ "runs": [1, 2] });
        assert_eq!(
            first_difference(&quantized(&want), &quantized(&got)).as_deref(),
            Some(".runs: expected 3 items, got 2"),
        );
    }
}
