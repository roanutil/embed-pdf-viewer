//! Fixture helpers for the workspace's tests. Dev-dependency only; nothing
//! here ships.
//!
//! Tests read the PDFs the repo already keeps for its JavaScript suites and
//! examples rather than carrying a second copy, so a fixture fix reaches both
//! implementations at once.

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
