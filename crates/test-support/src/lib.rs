//! Fixture helpers for the workspace's tests. Dev-dependency only; nothing
//! here ships.
//!
//! Tests read the PDFs the repo already keeps for its JavaScript suites and
//! examples rather than carrying a second copy, so a fixture fix reaches both
//! implementations at once.

pub mod corpus;

use std::path::{Path, PathBuf};

/// The workspace root, derived from the calling crate's manifest directory.
///
/// Cargo sets `CARGO_MANIFEST_DIR` per package, and every member of this
/// workspace sits at `crates/<name>`, so two levels up is the root.
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("workspace root")
}

/// Resolves a repo-relative path, panicking with the full path when it is
/// missing so a moved fixture reads as a moved fixture.
pub fn fixture_path(relative: &str) -> PathBuf {
    let path = workspace_root().join(relative);
    assert!(path.is_file(), "missing fixture: {}", path.display());
    path
}

/// Reads a repo-relative fixture.
pub fn fixture(relative: &str) -> Vec<u8> {
    let path = fixture_path(relative);
    std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// A small, unencrypted, text-bearing document.
pub const REPORT: &str = "pdfium-multiplatform-prototype/fixtures/report.pdf";
/// A colored document, for channel-order checks that a grayscale page cannot
/// make.
pub const COLORS: &str = "pdfium-multiplatform-prototype/fixtures/colors.pdf";
/// Encrypted with the user password `hello`.
pub const ENCRYPTED: &str = "pdfium-multiplatform-prototype/fixtures/encrypted-hello.pdf";
/// Carries text runs that are not upright, which is the only way to reach the
/// rotated arm of the geometry reader.
pub const ROTATED_TEXT: &str = "examples/viewer-react/public/rotated_text.pdf";
/// Three 600 by 400 pages. Page 1 has an internal `GoTo` link to page 3
/// (`/Rect [40 300 320 330]`) and a `URI` link to
/// `https://example.invalid/target` (`/Rect [40 240 360 270]`).
pub const FEATURES: &str = "tooling/viewer-e2e/fixtures/features.pdf";

/// Put a JSON tree into the one spelling both engines agree on.
///
/// Two things differ between a `serde_json` value and the JavaScript that
/// produced the vector, and neither is a difference in what the engines did.
///
/// A whole number: JSON has one number type, and `serde_json` writes `700.0`
/// where JavaScript writes `700`. Every integral float becomes an integer.
///
/// The last bit of a fraction: `serde_json`'s float parser is not always
/// correctly rounded. `0.9019607843137255`, which is what JavaScript prints
/// for `230 / 255`, parses back one unit in the last place high, so a value
/// the Rust engine COMPUTED as `230.0 / 255.0` differs from the same value
/// read out of the vector file. Rounding both to fifteen significant digits
/// erases that and nothing else: an `f64` carries not quite sixteen, and any
/// difference in what a writer actually wrote shows up in the third decimal
/// rather than the sixteenth.
pub fn canonical_numbers(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Number(number) => match number.as_f64() {
            Some(float) if float.fract() == 0.0 && float.abs() < 9e15 => {
                serde_json::Value::from(float as i64)
            }
            Some(float) => {
                serde_json::Value::from(format!("{float:.14e}").parse::<f64>().unwrap_or(float))
            }
            None => value.clone(),
        },
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(canonical_numbers).collect())
        }
        serde_json::Value::Object(fields) => serde_json::Value::Object(
            fields.iter().map(|(key, entry)| (key.clone(), canonical_numbers(entry))).collect(),
        ),
        _ => value.clone(),
    }
}
