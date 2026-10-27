//! An annotation bundle's checks (`core/src/transfer/AnnotationBundle.ts`).
//!
//! Annotations and the bytes beside them. Each item is a row: its data,
//! exactly what a read returns, and the resources it names by id. Each
//! resource is held once, however many items name it, under the SHA-256 of
//! its bytes.

use std::collections::HashSet;

use serde_json::{Map, Value, json};

use super::bundle::{
    Bundle, BundleKind, assert_bundle_pages, assert_known_fields, assert_resource_ids,
    assert_resources_named, assert_row_resources, bundle_page_ref, invalid_bundle, json_text,
    page_key, record, whole_number,
};
use super::limits::{BundleLimits, Limit, assert_within_limit, manifest_bytes_of};
use crate::error::EngineError;

pub const ANNOTATION_BUNDLE_FORMAT: &str = "embedpdf/annotations";
pub const ANNOTATION_BUNDLE_VERSION: u64 = 1;

/// The manifest's keys, in the order a file writes them
/// (`ANNOTATION_BUNDLE_KEYS`).
pub const ANNOTATION_BUNDLE_KEYS: &[&str] = &["format", "version", "pages", "items"];

const BUNDLE_FIELDS: &[&str] = &["format", "version", "pages", "items", "resources"];
const ITEM_FIELDS: &[&str] = &["data", "resources"];

/// The resource roles an item names (`ANNOTATION_RESOURCE_ROLE_NAMES`).
pub const ANNOTATION_RESOURCE_ROLES: &[&str] = &["appearance", "file"];

const KIND: BundleKind = BundleKind::Annotation;

/// Check a bundle before anything uses it, the checks cheapest first
/// (`assertAnnotationBundle`): its manifest, then every resource against
/// its id.
pub fn assert_annotation_bundle(bundle: &Bundle, limits: &BundleLimits) -> Result<(), EngineError> {
    assert_annotation_bundle_manifest(&bundle.manifest, &bundle.sizes(), limits)?;
    assert_resource_ids(KIND, &bundle.resources)
}

/// The checks that need no resource bytes, only their sizes by id: format
/// and version, counts, sizes, the shape of pages and items, and that items
/// and resources name each other exactly (`assertAnnotationBundleManifest`).
/// `manifest` may hold `resources` too, as a file's object does.
pub fn assert_annotation_bundle_manifest(
    manifest: &Map<String, Value>,
    sizes: &[(String, usize)],
    limits: &BundleLimits,
) -> Result<(), EngineError> {
    assert_known_fields(KIND, "the bundle", manifest, BUNDLE_FIELDS)?;
    if manifest.get("format").and_then(Value::as_str) != Some(ANNOTATION_BUNDLE_FORMAT) {
        return Err(invalid_bundle(
            KIND,
            &format!("unknown format {}", json_text(manifest.get("format"))),
        ));
    }
    if manifest.get("version").and_then(whole_number) != Some(ANNOTATION_BUNDLE_VERSION) {
        return Err(invalid_bundle(
            KIND,
            &format!("unknown version {}", json_text(manifest.get("version"))),
        ));
    }
    let Some(pages) = manifest.get("pages").and_then(Value::as_array) else {
        return Err(invalid_bundle(KIND, "pages is not a list"));
    };
    let Some(items) = manifest.get("items").and_then(Value::as_array) else {
        return Err(invalid_bundle(KIND, "items is not a list"));
    };

    assert_within_limit(KIND, limits, Limit::Items, items.len() as u64)?;
    assert_within_limit(KIND, limits, Limit::Pages, pages.len() as u64)?;
    assert_within_limit(KIND, limits, Limit::Resources, sizes.len() as u64)?;
    let mut bundle_bytes = manifest_bytes_of(&json!({ "pages": pages, "items": items }));
    assert_within_limit(KIND, limits, Limit::ManifestBytes, bundle_bytes)?;
    for (_, size) in sizes {
        assert_within_limit(KIND, limits, Limit::ResourceBytes, *size as u64)?;
        bundle_bytes += *size as u64;
    }
    assert_within_limit(KIND, limits, Limit::BundleBytes, bundle_bytes)?;

    let page_keys = assert_bundle_pages(KIND, pages)?;
    let mut named = HashSet::new();
    for (index, item) in items.iter().enumerate() {
        let data = record(item).and_then(|item| item.get("data")).and_then(record);
        let Some(data) = data.filter(|data| data.get("subtype").is_some_and(Value::is_string))
        else {
            return Err(invalid_bundle(KIND, &format!("item {index} has no annotation data")));
        };
        let fields = record(item).expect("checked above");
        assert_known_fields(KIND, &format!("item {index}"), fields, ITEM_FIELDS)?;
        let page = data.get("ref").and_then(record).and_then(|r| bundle_page_ref(r.get("page")));
        if !page.is_some_and(|page| page_keys.contains(&page_key(page))) {
            return Err(invalid_bundle(
                KIND,
                &format!("item {index} is on a page the bundle doesn't list"),
            ));
        }
        assert_row_resources(
            KIND,
            &format!("item {index}"),
            fields.get("resources"),
            ANNOTATION_RESOURCE_ROLES,
            sizes,
            &mut named,
        )?;
    }
    assert_resources_named(KIND, "item", sizes, &named)
}

#[cfg(test)]
mod tests {
    //! `core/test/transfer/AnnotationBundle.test.ts`.

    use super::*;
    use crate::error::EngineErrorCode;
    use crate::features::transfer::bundle::resource_id_of;
    use crate::features::transfer::limits::DEFAULT_BUNDLE_LIMITS;

    fn page() -> Value {
        json!({ "kind": "objectNumber", "objectNumber": 3 })
    }

    fn size() -> Value {
        json!({ "width": 612, "height": 792 })
    }

    const DRAWING: &[u8] = b"%PDF-1.7 a drawing";

    fn bundle_with(change: impl FnOnce(&mut Bundle)) -> Bundle {
        let id = resource_id_of(DRAWING).unwrap();
        let manifest = json!({
            "format": "embedpdf/annotations",
            "version": 1,
            "pages": [{ "page": page(), "position": 0, "size": size() }],
            "items": [{
                "data": { "subtype": "stamp",
                    "ref": { "kind": "objectNumber", "page": page(), "objectNumber": 12 } },
                "resources": { "appearance": id },
            }],
        });
        let mut bundle = Bundle {
            manifest: manifest.as_object().unwrap().clone(),
            resources: vec![(id, DRAWING.to_vec())],
        };
        change(&mut bundle);
        bundle
    }

    fn refusal(bundle: &Bundle) -> EngineError {
        assert_annotation_bundle(bundle, &DEFAULT_BUNDLE_LIMITS).unwrap_err()
    }

    #[test]
    fn accepts_a_bundle_whose_resources_match_their_ids() {
        assert_annotation_bundle(&bundle_with(|_| {}), &DEFAULT_BUNDLE_LIMITS).unwrap();
    }

    #[test]
    fn refuses_what_is_not_a_bundle_naming_why() {
        type Change = Box<dyn FnOnce(&mut Bundle)>;
        let cases: Vec<(&str, Change)> = vec![
            (
                "doesn't match its id",
                Box::new(|b: &mut Bundle| b.resources[0].1 = b"other".to_vec()),
            ),
            (
                "listed twice",
                Box::new(|b: &mut Bundle| {
                    b.manifest["pages"] = json!([
                        { "page": page(), "position": 0, "size": size() },
                        { "page": page(), "position": 1, "size": size() },
                    ]);
                }),
            ),
            (
                "position 0 is listed twice",
                Box::new(|b: &mut Bundle| {
                    b.manifest["pages"] = json!([
                        { "page": page(), "position": 0, "size": size() },
                        { "page": { "kind": "objectNumber", "objectNumber": 4 }, "position": 0,
                          "size": size() },
                    ]);
                }),
            ),
            (
                "unknown resource role 'thumbnail'",
                Box::new(|b: &mut Bundle| {
                    let id = b.resources[0].0.clone();
                    b.manifest["items"][0]["resources"] = json!({ "thumbnail": id });
                }),
            ),
            (
                "unknown field 'note'",
                Box::new(|b: &mut Bundle| b.manifest["items"][0]["note"] = json!("x")),
            ),
        ];
        for (message, change) in cases {
            let refused = refusal(&bundle_with(change));
            assert_eq!(refused.code, EngineErrorCode::InvalidArg, "{message}");
            assert!(refused.message.contains(message), "{message}: {}", refused.message);
        }
    }
}
