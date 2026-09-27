//! The typed API a Rust caller sees, beyond what the vectors reach.
//! `ScaleChangeReport::page` has no TypeScript counterpart: the TypeScript
//! reads the `page` member both report shapes carry.

use epdf_annotation_core::PageRef;
use epdf_plugin_annotation::RecalibrationReport;
use epdf_plugin_measurement::ScaleChangeReport;
use serde_json::{Value, json};

#[test]
fn a_report_names_its_page() {
    let page = PageRef::new(3);
    let failed =
        ScaleChangeReport::ScaleError { page, scale_error: json!({ "message": "refused" }) };
    assert_eq!(failed.page(), page);
    let recalibrated = ScaleChangeReport::Recalibrated(RecalibrationReport {
        page,
        scale: Value::Null,
        error: None,
        updated: Vec::new(),
        skipped: Vec::new(),
        failed: Vec::new(),
    });
    assert_eq!(recalibrated.page(), page);
}
