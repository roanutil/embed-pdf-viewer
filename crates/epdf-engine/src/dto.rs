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
use ts_rs::TS;

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
/// `ts-rs` prints "failed to parse serde attribute: deserialize_with =" for
/// every field that names this, and then ignores it. That is the right
/// outcome: `deserialize_with` changes how Rust reads the field, not what the
/// TypeScript type is, and the type is what `ts-rs` emits. The warning is
/// noise, not a signal.
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

/// A moment as an ISO 8601 string: `2017-07-12T21:44:38-07:00`.
/// `core/src/dto/IsoDateTime.ts`.
///
/// Every date in the engine's data has this type. A date read from the PDF
/// keeps the offset it was written with, and one without an offset reads
/// without one ([`crate::clock::pdf_date_to_iso`]). A write takes the same
/// forms and stores the offset it is given ([`crate::clock::format_pdf_date`]).
///
/// A newtype rather than `String` because `IsoDateTimeSchema` is
/// `z.string().datetime({ offset: true, local: true })`, which refuses
/// impossible calendar dates and anything that is not a date and time. A
/// `Patch<String>` would accept both, so the same request would be well
/// formed on one engine and rejected on the other. See
/// [`crate::clock::is_iso_date_time`] for what exactly is accepted.
///
/// Upstream's `DateInput` also takes a `Date`, which only ever arrives
/// in-process and becomes this string on the wire, so the wire type is this
/// alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct IsoDateTime(String);

impl IsoDateTime {
    /// Validates and wraps, returning the offending string on rejection.
    pub fn new(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if crate::clock::is_iso_date_time(&value) { Ok(Self(value)) } else { Err(value) }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for IsoDateTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for IsoDateTime {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::new(raw).map_err(|value| {
            serde::de::Error::custom(format!(
                "expected an ISO 8601 date and time (YYYY-MM-DDTHH:MM[:SS[.fff]][Z|±HH:MM]), got `{value}`"
            ))
        })
    }
}

/// `/Trapped`, a tri-valued enum that is always present, so it has no
/// clear-form: omit it to leave it alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, export_to = "../../../packages/engine/core/src/dto/generated/")]
pub enum DocumentMetadataTrapped {
    True,
    False,
    Unknown,
}

/// Three-state patch over the document Info dictionary, consistent with the
/// annotation patches. `core/src/dto/MetadataPatch.ts`, held against
/// `MetadataPatchSchema` (`wire/schemas.ts`) by `tests/schema_parity.rs`:
///
/// - field absent: leave the stored value alone
/// - `null`: clear the field
/// - a value: set the field to it
///
/// Standard fields map to PDF keys (`title` to `/Title`, `createdAt` to
/// `/CreationDate`, `modifiedAt` to `/ModDate`). The dates are ISO 8601 on
/// the wire, with or without an offset, and the engine writes them as PDF
/// dates keeping that offset ([`crate::clock::format_pdf_date`]).
///
/// `trapped` has no clear form, because it is a tri-valued enum that is always
/// present. Omit it to leave it untouched. `''` is a value everywhere: it
/// reads back `''`, not `null`. The dict's other keys change through
/// [`CustomMetadataPatch`], and a `custom` field here is refused.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export, export_to = "../../../packages/engine/core/src/dto/generated/")]
pub struct MetadataPatch {
    #[serde(default, skip_serializing_if = "Patch::is_keep")]
    #[ts(type = "string | null", optional)]
    pub title: Patch<String>,
    #[serde(default, skip_serializing_if = "Patch::is_keep")]
    #[ts(type = "string | null", optional)]
    pub author: Patch<String>,
    #[serde(default, skip_serializing_if = "Patch::is_keep")]
    #[ts(type = "string | null", optional)]
    pub subject: Patch<String>,
    #[serde(default, skip_serializing_if = "Patch::is_keep")]
    #[ts(type = "string | null", optional)]
    pub keywords: Patch<String>,
    #[serde(default, skip_serializing_if = "Patch::is_keep")]
    #[ts(type = "string | null", optional)]
    pub producer: Patch<String>,
    #[serde(default, skip_serializing_if = "Patch::is_keep")]
    #[ts(type = "string | null", optional)]
    pub creator: Patch<String>,
    /// `/CreationDate`.
    #[serde(default, skip_serializing_if = "Patch::is_keep")]
    #[ts(type = "string | null", optional)]
    pub created_at: Patch<IsoDateTime>,
    /// `/ModDate`.
    #[serde(default, skip_serializing_if = "Patch::is_keep")]
    #[ts(type = "string | null", optional)]
    pub modified_at: Patch<IsoDateTime>,
    /// Two-state: `/Trapped` has no clear form.
    #[serde(
        default,
        deserialize_with = "optional_not_nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub trapped: Option<DocumentMetadataTrapped>,
}

/// A change to the Info dict's other keys, where every key is a field of its
/// own. `core/src/dto/CustomMetadataPatch.ts`, held against
/// `CustomMetadataPatchSchema` (`wire/schemas.ts`) by `tests/schema_parity.rs`:
///
/// - key left out: the key stays as it is
/// - `null`: remove the key
/// - a string: set the key to it (`''` is a value)
///
/// The map itself is not nullable: `null` in its place is refused. Which keys
/// a PDF can hold is the engine's check at write time, not the type's, so a
/// refusal names the key the same way on both engines. `BTreeMap` rather
/// than `HashMap` so the serialized order is the same on every run, which the
/// corpus needs. A newtype, which serde reads and writes as the map itself.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/engine/core/src/dto/generated/")]
pub struct CustomMetadataPatch(pub BTreeMap<String, Option<String>>);

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
            serde_json::from_value::<MetadataPatch>(serde_json::json!({ "trapped": null }))
                .is_err()
        );
    }

    #[test]
    fn custom_entries_are_three_state_per_key() {
        let patch: CustomMetadataPatch = serde_json::from_value(serde_json::json!({
            "Department": "Legal", "Obsolete": null
        }))
        .unwrap();
        assert_eq!(patch.0.get("Department"), Some(&Some("Legal".to_string())));
        assert_eq!(patch.0.get("Obsolete"), Some(&None));
        // An absent key is absent, not a removal.
        assert_eq!(patch.0.get("Untouched"), None);
        assert_eq!(
            serde_json::to_value(&patch).unwrap(),
            serde_json::json!({ "Department": "Legal", "Obsolete": null })
        );
        // The map itself is not nullable.
        assert!(serde_json::from_value::<CustomMetadataPatch>(serde_json::Value::Null).is_err());
    }

    #[test]
    fn custom_entries_are_their_own_patch() {
        // `MetadataPatchSchema` is strict and has no `custom`.
        assert!(
            serde_json::from_value::<MetadataPatch>(
                serde_json::json!({ "custom": { "Department": "Legal" } })
            )
            .is_err()
        );
    }

    #[test]
    fn dates_take_the_create_and_modify_names_and_keep_their_offset() {
        let patch: MetadataPatch = serde_json::from_value(serde_json::json!({
            "createdAt": "2020-01-02T03:04:05+05:30",
            "modifiedAt": "2017-07-12T21:44:38",
        }))
        .unwrap();
        assert_eq!(
            patch.created_at.as_set().map(IsoDateTime::as_str),
            Some("2020-01-02T03:04:05+05:30")
        );
        assert_eq!(
            patch.modified_at.as_set().map(IsoDateTime::as_str),
            Some("2017-07-12T21:44:38")
        );
        // The pre-v3 spelling is an unknown field.
        assert!(
            serde_json::from_value::<MetadataPatch>(
                serde_json::json!({ "created": "2020-01-02T03:04:05Z" })
            )
            .is_err()
        );
    }

    #[test]
    fn an_unknown_field_is_rejected_rather_than_dropped() {
        // The zod schema is `.strict()`, so anything it would refuse must not
        // be silently ignored here either.
        let err = serde_json::from_value::<MetadataPatch>(serde_json::json!({ "titel": "typo" }))
            .unwrap_err();
        assert!(err.to_string().contains("titel"), "{err}");
    }
}
