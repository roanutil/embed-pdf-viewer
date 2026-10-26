//! The typed API a Rust caller sees, beyond what the vectors reach.
//! `ScaleChangeReport::page` has no TypeScript counterpart: the TypeScript
//! reads the `page` member both report shapes carry.

use epdf_kernel::geometry::PageRef;
use epdf_plugin_measurement::ScaleChangeReport;
use serde_json::json;

#[test]
fn a_report_names_its_page() {
    let page = PageRef::new(3);
    let failed =
        ScaleChangeReport::ScaleError { page, scale_error: json!({ "message": "refused" }) };
    assert_eq!(failed.page(), Some(page));
    let recalibrated = ScaleChangeReport::Recalibrated(json!({
        "page": page,
        "scale": null,
        "updated": [],
        "skipped": [],
        "failed": [],
    }));
    assert_eq!(recalibrated.page(), Some(page));
}
