//! The typed API a Rust caller sees, beyond what the vectors reach.
//! `ScaleChangeReport::page` has no TypeScript counterpart: the TypeScript
//! reads the `page` member both report shapes carry.

use epdf_kernel::geometry::PageRef;
use epdf_plugin_measurement::{DefaultScale, MeasurementSettings, ScaleChangeReport};
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

#[test]
fn a_presets_setting_the_typed_read_cannot_hold_is_refused() {
    let settings = MeasurementSettings::of(&json!({ "defaultScale": "imperial", "presets": [] }))
        .expect("an empty list is a list");
    assert_eq!(settings.default_scale, DefaultScale::Imperial);
    assert!(settings.presets.is_empty());
    for presets in [json!("metric-1"), json!([{ "id": "x" }]), json!(null)] {
        let error =
            MeasurementSettings::of(&json!({ "defaultScale": "metric", "presets": presets }))
                .expect_err("not a list of presets");
        assert_eq!(error.name, "TypeError");
        assert!(
            error.message.starts_with("[measurement] the presets setting"),
            "{}",
            error.message
        );
    }
}
