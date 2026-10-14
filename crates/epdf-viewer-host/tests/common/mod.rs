//! Helpers more than one host test file shares.
#![allow(dead_code)]

use epdf_ffi::ViewerError;
use epdf_viewer_host::{OpenDocumentOptions, ViewerDelegate};

/// A delegate that ignores every report.
pub struct NullDelegate;

impl ViewerDelegate for NullDelegate {
    fn state_changed(&self, _revision: u64) {}
    fn report(&self, _error: ViewerError) {}
}

/// A fixture's bytes, workspace-relative.
pub fn fixture(relative: &str) -> Vec<u8> {
    test_support::fixture(relative)
}

pub fn options() -> OpenDocumentOptions {
    OpenDocumentOptions { name: None, password: None, activate: true }
}
