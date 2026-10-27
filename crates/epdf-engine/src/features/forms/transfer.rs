//! A form's fields out of a document as a bundle
//! (`services/src/features/forms/FormExporter.ts`, 7f0b73a3c).
//! Operation: `forms.export`. The imports are change ops
//! ([`crate::features::changes::fields::import_form`],
//! [`crate::features::changes::forms::import_form_values`]), as upstream's
//! `FormImporter` runs inside `formChanges.ts`.

use serde_json::{Map, Value, json};

use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::transfer::bundle::{Bundle, BundleKind};
use crate::features::transfer::form_bundle::{FORM_BUNDLE_FORMAT, FORM_BUNDLE_VERSION};
use crate::features::transfer::form_export::{FormExportSelection, form_export_rows_of};
use crate::features::transfer::limits::{
    BundleLimits, Limit, assert_within_limit, manifest_bytes_of,
};
use crate::features::transfer::resources::{bundle_pages_of, import_targets};
use crate::identity::PageRef;
use crate::session::document::DocumentSession;

fn unserializable(e: impl std::fmt::Display) -> EngineError {
    EngineError::with_message(
        EngineErrorCode::Unknown,
        format!("a form read did not serialize: {e}"),
    )
}

/// `forms.export`: the fields a selection takes, each whole with all of its
/// widgets, as one bundle in page space (`FormExporter.export`). It reads
/// the form as `forms.list` does and writes nothing. A widget is drawn from
/// its data, so the bundle holds no resources. It is checked against
/// `limits` as it is built, so an export never makes one an import refuses.
pub fn export_form(
    session: &DocumentSession,
    selection: &FormExportSelection,
    limits: &BundleLimits,
    cancel: &CancelToken,
) -> Result<Bundle, EngineError> {
    cancel.check()?;
    let owned = session.document()?;
    let document = owned.document();
    let model = super::write::load_model(document)?;
    let form = super::widgets::read_form(document, &model, cancel)?;
    drop(model);
    let mut box_of = crate::features::pages::visible_box_reader(document);
    let snapshot = crate::page_space::forms::page_form_snapshot_of(form, &mut box_of)?;
    let snapshot = serde_json::to_value(snapshot).map_err(unserializable)?;
    let pages: Vec<PageRef> = import_targets(owned).into_iter().map(|target| target.page).collect();
    let rows = form_export_rows_of(&snapshot, selection, &pages)?;
    let kind = BundleKind::Form;
    assert_within_limit(
        kind,
        limits,
        Limit::Items,
        (rows.fields.len() + rows.widgets.len()) as u64,
    )?;
    let bundle_pages =
        serde_json::to_value(bundle_pages_of(owned, &rows.widgets)?).map_err(unserializable)?;
    let page_count = bundle_pages.as_array().map_or(0, Vec::len) as u64;
    let fields: Vec<Value> = rows.fields.into_iter().map(|data| json!({ "data": data })).collect();
    let widgets: Vec<Value> =
        rows.widgets.into_iter().map(|data| json!({ "data": data, "resources": {} })).collect();
    let calculation_order = Value::Array(rows.calculation_order);
    assert_within_limit(kind, limits, Limit::Pages, page_count)?;
    let manifest_bytes = manifest_bytes_of(&json!({
        "pages": bundle_pages, "fields": fields, "widgets": widgets,
        "calculationOrder": calculation_order,
    }));
    assert_within_limit(kind, limits, Limit::ManifestBytes, manifest_bytes)?;
    assert_within_limit(kind, limits, Limit::BundleBytes, manifest_bytes)?;
    let mut manifest = Map::new();
    manifest.insert("format".into(), json!(FORM_BUNDLE_FORMAT));
    manifest.insert("version".into(), json!(FORM_BUNDLE_VERSION));
    manifest.insert("pages".into(), bundle_pages);
    manifest.insert("fields".into(), Value::Array(fields));
    manifest.insert("widgets".into(), Value::Array(widgets));
    manifest.insert("calculationOrder".into(), calculation_order);
    let Value::Object(manifest) =
        crate::features::transfer::bundle::js_numbers(Value::Object(manifest))
    else {
        unreachable!("an object stays an object")
    };
    Ok(Bundle { manifest, resources: Vec::new() })
}
