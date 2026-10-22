//! How large a bundle may be (`core/src/transfer/bundleLimits.ts`).
//!
//! One set applies wherever a bundle is made or read, so whatever an export
//! makes, an import accepts. Sizes are measured on the bundle itself: the
//! manifest (its pages and rows as JSON) plus each resource's bytes.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::bundle::BundleKind;
use crate::error::{EngineError, EngineErrorCode};

/// `BundleLimits`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BundleLimits {
    /// The manifest's bytes plus every resource's bytes.
    pub bundle_bytes: u64,
    /// The pages and rows, as UTF-8 JSON.
    pub manifest_bytes: u64,
    /// The rows: an annotation bundle's items, a form bundle's fields and
    /// widgets.
    pub items: u64,
    pub pages: u64,
    pub resources: u64,
    pub resource_bytes: u64,
    /// Width times height of a PNG or JPEG resource, once decoded.
    pub image_pixels: u64,
}

/// `DEFAULT_BUNDLE_LIMITS`.
pub const DEFAULT_BUNDLE_LIMITS: BundleLimits = BundleLimits {
    bundle_bytes: 50 * 1024 * 1024,
    manifest_bytes: 16 * 1024 * 1024,
    items: 10_000,
    pages: 2_000,
    resources: 2_000,
    resource_bytes: 50 * 1024 * 1024,
    image_pixels: 40_000_000,
};

impl Default for BundleLimits {
    fn default() -> Self {
        DEFAULT_BUNDLE_LIMITS
    }
}

/// A limit's name, as `details.limit` spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Limit {
    BundleBytes,
    ManifestBytes,
    Items,
    Pages,
    Resources,
    ResourceBytes,
    ImagePixels,
}

impl Limit {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BundleBytes => "bundleBytes",
            Self::ManifestBytes => "manifestBytes",
            Self::Items => "items",
            Self::Pages => "pages",
            Self::Resources => "resources",
            Self::ResourceBytes => "resourceBytes",
            Self::ImagePixels => "imagePixels",
        }
    }

    fn of(self, limits: &BundleLimits) -> u64 {
        match self {
            Self::BundleBytes => limits.bundle_bytes,
            Self::ManifestBytes => limits.manifest_bytes,
            Self::Items => limits.items,
            Self::Pages => limits.pages,
            Self::Resources => limits.resources,
            Self::ResourceBytes => limits.resource_bytes,
            Self::ImagePixels => limits.image_pixels,
        }
    }
}

/// The bytes of a bundle's manifest: its pages and rows as UTF-8 JSON
/// (`manifestBytesOf`). `rows` is the object `JSON.stringify` is given:
/// `{ pages, items }` or `{ pages, fields, widgets, calculationOrder }`.
pub fn manifest_bytes_of(rows: &Value) -> u64 {
    serde_json::to_string(rows).map_or(0, |text| text.len() as u64)
}

/// Refuse `value` when it is past the limit named `limit`
/// (`assertWithinLimit`).
pub fn assert_within_limit(
    kind: BundleKind,
    limits: &BundleLimits,
    limit: Limit,
    value: u64,
) -> Result<(), EngineError> {
    let max = limit.of(limits);
    if value <= max {
        return Ok(());
    }
    Err(past_limit(
        format!(
            "the {} bundle is past its {} limit: {value} > {max}",
            kind.as_str(),
            limit.as_str()
        ),
        serde_json::json!({ "limit": limit.as_str(), "max": max, "value": value }),
    ))
}

/// A bundle past a limit: `PayloadTooLarge`, with upstream's message and
/// `details`.
pub fn past_limit(message: String, details: Value) -> EngineError {
    EngineError::with_message(EngineErrorCode::PayloadTooLarge, message).with_details(details)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_at_its_limit_passes_and_one_past_it_names_the_limit() {
        let limits = BundleLimits { items: 2, ..DEFAULT_BUNDLE_LIMITS };
        assert!(assert_within_limit(BundleKind::Form, &limits, Limit::Items, 2).is_ok());
        let refused = assert_within_limit(BundleKind::Form, &limits, Limit::Items, 3).unwrap_err();
        assert_eq!(refused.message, "the form bundle is past its items limit: 3 > 2");
        assert_eq!(
            refused.details,
            Some(serde_json::json!({ "limit": "items", "max": 2, "value": 3 }))
        );
    }

    #[test]
    fn limits_read_as_the_wire_spells_them() {
        let limits: BundleLimits = serde_json::from_value(serde_json::json!({
            "bundleBytes": 1, "manifestBytes": 2, "items": 3, "pages": 4,
            "resources": 5, "resourceBytes": 6, "imagePixels": 7,
        }))
        .unwrap();
        assert_eq!(limits.image_pixels, 7);
        assert_eq!(BundleLimits::default(), DEFAULT_BUNDLE_LIMITS);
    }
}
