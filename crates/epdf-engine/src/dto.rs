//! G1. Wire DTOs, with Rust as the definition of record.
//!
//! Replaces `core/src/dto/*`. Generated TypeScript is checked in and a dirty
//! diff fails CI; the zod schemas in `wire/schemas.ts` and the 20 handwritten
//! annotation schema files stay handwritten and are held against these types
//! by a bidirectional parity test.
//!
//! # Why a patch is the first type to port
//!
//! Parsing is the weak half of that parity test, and patches are where it is
//! weakest. `MetadataPatch` (`wire/schemas.ts:105`) is three-state: an absent
//! field leaves the value alone, `null` clears it, and a value sets it. The
//! annotation patches carry the same semantics
//! (`core/src/annotation/patch-base.ts:7-9`). `serde`'s `Option<T>` flattens
//! absent and `null` into `None`, so a mechanical port would silently turn
//! every "leave this alone" into "clear this" the first time a client omitted
//! a field. [`Patch`] is the explicit three-state type that prevents it.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A three-state patch field: leave it, clear it, or set it.
///
/// On the wire, paired with `#[serde(default, skip_serializing_if =
/// "Patch::is_keep")]`, this is exactly TypeScript's `?: T | null`:
///
/// | Rust | JSON | Meaning |
/// |---|---|---|
/// | `Keep` | field absent | leave the stored value alone |
/// | `Clear` | `null` | remove the stored value |
/// | `Set(v)` | `v` | store `v` |
///
/// `Keep` is the `Default`, which is what makes an absent field decode to it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Patch<T> {
    #[default]
    Keep,
    Clear,
    Set(T),
}

impl<T> Patch<T> {
    /// The `skip_serializing_if` predicate. A `Keep` must never reach the
    /// wire: an emitted `null` would mean `Clear`.
    pub fn is_keep(&self) -> bool {
        matches!(self, Self::Keep)
    }

    pub fn is_clear(&self) -> bool {
        matches!(self, Self::Clear)
    }

    pub fn as_set(&self) -> Option<&T> {
        match self {
            Self::Set(value) => Some(value),
            _ => None,
        }
    }

    /// Applies the patch to a stored value.
    pub fn apply(self, stored: &mut Option<T>) {
        match self {
            Self::Keep => {}
            Self::Clear => *stored = None,
            Self::Set(value) => *stored = Some(value),
        }
    }

    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Patch<U> {
        match self {
            Self::Keep => Patch::Keep,
            Self::Clear => Patch::Clear,
            Self::Set(value) => Patch::Set(f(value)),
        }
    }
}

impl<T: Serialize> Serialize for Patch<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            // Reachable only if a field forgot `skip_serializing_if`. Emitting
            // null would silently change the meaning to Clear, so this is the
            // conservative half of that mistake, not a correct encoding.
            Self::Keep | Self::Clear => serializer.serialize_none(),
            Self::Set(value) => value.serialize(serializer),
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Patch<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Reached only when the field was present: an absent one takes
        // `Default`, which is Keep. So `null` here means the client wrote
        // null, which is Clear.
        Ok(match Option::<T>::deserialize(deserializer)? {
            None => Self::Clear,
            Some(value) => Self::Set(value),
        })
    }
}

/// Decodes a field that is optional but NOT nullable: absent is the only way
/// to leave it alone, and an explicit `null` is an error.
///
/// `Option<T>` alone would accept `null` and quietly read it as absent, which
/// is the same flattening [`Patch`] exists to prevent, one field over. The zod
/// schema spells these `.optional()` without `.nullable()`
/// (`wire/schemas.ts:120-121`), so it refuses `null`, and the parity test
/// requires that nothing zod refuses is silently accepted here.
fn optional_not_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    // Only called when the key is present; an absent one takes `default`.
    T::deserialize(deserializer).map(Some)
}

/// `/Trapped`, a tri-valued enum that is always present, so it has no
/// clear-form: omit it to leave it alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DocumentMetadataTrapped {
    True,
    False,
    Unknown,
}

/// Three-state patch over the document Info dictionary.
///
/// Standard fields map to PDF keys (`title` to `/Title`, `created` to
/// `/CreationDate`). `created` and `modified` are ISO 8601 on the wire and the
/// engine formats them into PDF date syntax on write
/// (`crate::clock::format_pdf_date`).
///
/// `custom` is a per-key three-state map over non-standard Info entries: a
/// string sets the key, `null` removes it, and an absent key leaves it. The
/// map itself is two-state, absent or present, because there is no meaning for
/// "clear the concept of custom entries" distinct from clearing each key.
/// `BTreeMap` rather than `HashMap` so the serialized order is the same on
/// every run, which the corpus needs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MetadataPatch {
    #[serde(default, skip_serializing_if = "Patch::is_keep")]
    pub title: Patch<String>,
    #[serde(default, skip_serializing_if = "Patch::is_keep")]
    pub author: Patch<String>,
    #[serde(default, skip_serializing_if = "Patch::is_keep")]
    pub subject: Patch<String>,
    #[serde(default, skip_serializing_if = "Patch::is_keep")]
    pub keywords: Patch<String>,
    #[serde(default, skip_serializing_if = "Patch::is_keep")]
    pub producer: Patch<String>,
    #[serde(default, skip_serializing_if = "Patch::is_keep")]
    pub creator: Patch<String>,
    /// ISO 8601 for `/CreationDate`.
    #[serde(default, skip_serializing_if = "Patch::is_keep")]
    pub created: Patch<String>,
    /// ISO 8601 for `/ModDate`.
    #[serde(default, skip_serializing_if = "Patch::is_keep")]
    pub modified: Patch<String>,
    /// Two-state: `/Trapped` has no clear form.
    #[serde(
        default,
        deserialize_with = "optional_not_nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub trapped: Option<DocumentMetadataTrapped>,
    /// Per-key three-state: `None` removes the key. The map itself is
    /// two-state, so `"custom": null` is rejected rather than read as absent.
    #[serde(
        default,
        deserialize_with = "optional_not_nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub custom: Option<BTreeMap<String, Option<String>>>,
}

/// Per-page cache-busting integers embedded in immutable cloud and CDN leaf
/// URLs.
///
/// Not page state: read coordinates for cacheable endpoints. `/text` uses
/// `contentVersion`, `/annotations` uses `annotationVersion`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CachePins {
    pub content_version: u64,
    pub annotation_version: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_three_states_decode_from_absent_null_and_value() {
        let keep: MetadataPatch = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(keep.title, Patch::Keep);

        let clear: MetadataPatch =
            serde_json::from_value(serde_json::json!({ "title": null })).unwrap();
        assert_eq!(clear.title, Patch::Clear);

        let set: MetadataPatch =
            serde_json::from_value(serde_json::json!({ "title": "Report" })).unwrap();
        assert_eq!(set.title, Patch::Set("Report".into()));
    }

    #[test]
    fn keep_is_absent_on_the_wire_and_clear_is_null() {
        let mut patch = MetadataPatch::default();
        assert_eq!(serde_json::to_value(&patch).unwrap(), serde_json::json!({}));

        patch.title = Patch::Clear;
        patch.author = Patch::Set("Ada".into());
        assert_eq!(
            serde_json::to_value(&patch).unwrap(),
            serde_json::json!({ "title": null, "author": "Ada" })
        );
    }

    #[test]
    fn every_state_round_trips_through_json() {
        for state in [Patch::Keep, Patch::Clear, Patch::Set("x".to_string())] {
            let patch = MetadataPatch { title: state.clone(), ..Default::default() };
            let json = serde_json::to_string(&patch).unwrap();
            let back: MetadataPatch = serde_json::from_str(&json).unwrap();
            assert_eq!(back.title, state, "{json}");
        }
    }

    #[test]
    fn applying_a_patch_does_what_each_state_says() {
        let mut stored = Some("before".to_string());
        Patch::Keep.apply(&mut stored);
        assert_eq!(stored.as_deref(), Some("before"));

        Patch::Set("after".to_string()).apply(&mut stored);
        assert_eq!(stored.as_deref(), Some("after"));

        Patch::<String>::Clear.apply(&mut stored);
        assert_eq!(stored, None);

        // Keep over an already-absent value leaves it absent rather than
        // materializing one.
        Patch::Keep.apply(&mut stored);
        assert_eq!(stored, None);
    }

    #[test]
    fn trapped_has_no_clear_form() {
        let absent: MetadataPatch = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(absent.trapped, None);
        let set: MetadataPatch =
            serde_json::from_value(serde_json::json!({ "trapped": "unknown" })).unwrap();
        assert_eq!(set.trapped, Some(DocumentMetadataTrapped::Unknown));
        // `.optional()` without `.nullable()`: zod refuses `null` here, so a
        // plain Option<T> reading it as absent would accept what the schema
        // rejects.
        assert!(
            serde_json::from_value::<MetadataPatch>(serde_json::json!({ "trapped": null })).is_err()
        );
        assert!(
            serde_json::from_value::<MetadataPatch>(serde_json::json!({ "custom": null })).is_err()
        );
    }

    #[test]
    fn custom_entries_are_three_state_per_key() {
        let patch: MetadataPatch = serde_json::from_value(serde_json::json!({
            "custom": { "Department": "Legal", "Obsolete": null }
        }))
        .unwrap();
        let custom = patch.custom.as_ref().unwrap();
        assert_eq!(custom.get("Department"), Some(&Some("Legal".to_string())));
        assert_eq!(custom.get("Obsolete"), Some(&None));
        // An absent key is absent, not a removal.
        assert_eq!(custom.get("Untouched"), None);
        assert_eq!(
            serde_json::to_value(&patch).unwrap(),
            serde_json::json!({ "custom": { "Department": "Legal", "Obsolete": null } })
        );
    }

    #[test]
    fn an_unknown_field_is_rejected_rather_than_dropped() {
        // The zod schema is `.strict()`, so anything it would refuse must not
        // be silently ignored here either.
        let err = serde_json::from_value::<MetadataPatch>(
            serde_json::json!({ "titel": "typo" }),
        )
        .unwrap_err();
        assert!(err.to_string().contains("titel"), "{err}");
    }
}
