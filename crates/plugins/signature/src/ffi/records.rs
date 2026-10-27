//! The engine's signature DTOs (`packages/engine/core/src/signature/types.ts`)
//! and `epdf-signature-core`'s verdicts, as typed records. The plugin keeps
//! them as JSON; each record here decodes from that JSON through a private
//! serde twin, and a value that doesn't decode is `invalid-state`.
//!
//! Migrated by hand to the Task 13.25 plugin and not compiled until
//! `epdf-ffi` builds (Task 13.33): refs are written in the engine's current
//! spelling (`objectNumber`), since `epdf-ffi`'s `FormFieldRef::to_json`
//! still writes the retired `fieldObjectNumber`.

use epdf_ffi::{AnnotationRef, FormFieldRef, PageRef};
use epdf_kernel::Error;
use epdf_kernel::bytes::Bytes;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

pub(crate) fn invalid_state(what: &str, e: impl std::fmt::Display) -> Error {
    Error::named("InvalidState", format!("[signature] {what} does not decode: {e}"))
        .with("code", "invalid-state")
}

pub(crate) fn decode<T: DeserializeOwned>(what: &str, value: &Value) -> Result<T, Error> {
    serde_json::from_value(value.clone()).map_err(|e| invalid_state(what, e))
}

/// A field ref the engine's way: `{ kind: 'objectNumber', objectNumber }`
/// or `{ kind: 'fqn', name }`.
pub(crate) fn field_json(field: &FormFieldRef) -> Value {
    match field {
        FormFieldRef::ObjectNumber { field_object_number } => {
            json!({ "kind": "objectNumber", "objectNumber": field_object_number })
        }
        FormFieldRef::Fqn { name } => json!({ "kind": "fqn", "name": name }),
    }
}

/// A field ref from the engine's JSON.
pub(crate) fn field_from_json(value: &Value) -> Result<FormFieldRef, Error> {
    match value.get("kind").and_then(Value::as_str) {
        Some("objectNumber") => value
            .get("objectNumber")
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
            .map(|field_object_number| FormFieldRef::ObjectNumber { field_object_number })
            .ok_or_else(|| invalid_state("a field ref", value)),
        Some("fqn") => value
            .get("name")
            .and_then(Value::as_str)
            .map(|name| FormFieldRef::Fqn { name: name.to_string() })
            .ok_or_else(|| invalid_state("a field ref", value)),
        _ => Err(invalid_state("a field ref", value)),
    }
}

fn page_from_json(value: &Value) -> Result<Option<PageRef>, Error> {
    if value.is_null() {
        return Ok(None);
    }
    Ok(Some(PageRef::from_json(value)?))
}

fn annotation_from_json(value: &Value) -> Result<Option<AnnotationRef>, Error> {
    if value.is_null() {
        return Ok(None);
    }
    Ok(Some(AnnotationRef::from_json(value)?))
}

// ── enums ────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, uniffi::Enum)]
#[serde(rename_all = "lowercase")]
pub enum SignatureKind {
    Signature,
    /// A document timestamp (`/DocTimeStamp`).
    Timestamp,
}

/// How a signature's `/ByteRange` relates to the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, uniffi::Enum)]
#[serde(rename_all = "kebab-case")]
pub enum SignatureCoverage {
    /// Exactly one chained revision, the hole being this signature's
    /// `/Contents`.
    WholeRevision,
    Partial,
    Malformed,
}

/// The modification level a signed document allows: `none` < `lta`
/// (timestamps) < `fill` (form fill and signing) < `annotate`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, uniffi::Enum)]
#[serde(rename_all = "lowercase")]
pub enum SignatureModificationLevel {
    None,
    Lta,
    Fill,
    Annotate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, uniffi::Enum)]
#[serde(rename_all = "lowercase")]
pub enum SignatureFieldLockAction {
    All,
    Include,
    Exclude,
}

impl SignatureFieldLockAction {
    fn as_str(self) -> &'static str {
        match self {
            SignatureFieldLockAction::All => "all",
            SignatureFieldLockAction::Include => "include",
            SignatureFieldLockAction::Exclude => "exclude",
        }
    }
}

/// What `placeMark` does with a mark over a field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SignatureMode {
    /// Seal the field with the signer.
    Sign,
    /// Draw the mark into the field without sealing.
    Visual,
    /// Neither: `SignRequested` fires and the app decides.
    Ask,
}

impl From<crate::SignatureMode> for SignatureMode {
    fn from(m: crate::SignatureMode) -> Self {
        match m {
            crate::SignatureMode::Sign => SignatureMode::Sign,
            crate::SignatureMode::Visual => SignatureMode::Visual,
            crate::SignatureMode::Ask => SignatureMode::Ask,
        }
    }
}

impl From<SignatureMode> for crate::SignatureMode {
    fn from(m: SignatureMode) -> Self {
        match m {
            SignatureMode::Sign => crate::SignatureMode::Sign,
            SignatureMode::Visual => crate::SignatureMode::Visual,
            SignatureMode::Ask => crate::SignatureMode::Ask,
        }
    }
}

// ── the snapshot ─────────────────────────────────────────────────────────────

/// A FieldMDP transform or a field `/Lock`: which fields it freezes, and an
/// optional DocMDP `/P` (1, 2 or 3) tightening.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, uniffi::Record)]
pub struct SignatureFieldLock {
    pub action: SignatureFieldLockAction,
    /// Fully qualified names; a name covers its descendants. Empty for
    /// `All`.
    pub fields: Vec<String>,
    #[serde(default)]
    #[uniffi(default = None)]
    pub permission: Option<u8>,
}

impl SignatureFieldLock {
    /// `FieldLockSpec` as the engine reads it.
    pub(crate) fn to_json(&self) -> Value {
        let mut out = json!({ "action": self.action.as_str(), "fields": self.fields });
        if let Some(p) = self.permission {
            out["permission"] = json!(p);
        }
        out
    }
}

/// One saved revision: the byte prefix `[0, end)`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct SignatureRevision {
    /// 0 is the original document.
    pub index: u32,
    pub end: u64,
    pub xref_offset: u64,
    /// The signature that seals exactly this revision.
    pub signature_index: Option<u32>,
}

/// What the signer claimed. None of it is verified.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct SignatureSignerClaims {
    pub name: Option<String>,
    pub reason: Option<String>,
    pub location: Option<String>,
    pub contact_info: Option<String>,
    /// `/M`, when the signer says they signed (`signedAt`, an ISO date
    /// time). A claim, not verified.
    pub signed_at: Option<String>,
}

/// The field's `/SV` seed value.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct SignatureSeedValue {
    pub required_flags: u32,
    pub present_flags: u32,
    pub version: Option<u32>,
    pub mdp: Option<u8>,
    pub filter: Option<String>,
    pub sub_filters: Vec<String>,
    pub digest_methods: Vec<String>,
    pub reasons: Vec<String>,
    /// A required entry the engine does not implement: signing refuses.
    pub unsupported_required: bool,
}

/// The field's widget.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SignatureWidget {
    /// `None` when the widget cannot be addressed as an annotation.
    pub annotation: Option<AnnotationRef>,
    pub annot_object_number: u32,
    /// `None` when no page lists the widget.
    pub page: Option<PageRef>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignatureWidgetJson {
    #[serde(rename = "ref", default)]
    reference: Value,
    /// `FormWidget.objectNumber`: the widget annotation's object number.
    #[serde(rename = "objectNumber")]
    annot_object_number: u32,
    #[serde(default)]
    page: Value,
}

/// One signature field, signed or not.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SignatureInfo {
    /// Position in the snapshot (field order).
    pub index: u32,
    pub field: FormFieldRef,
    pub field_name: String,
    pub widget: Option<SignatureWidget>,
    pub signed: bool,
    pub kind: SignatureKind,
    pub filter: Option<String>,
    pub sub_filter: Option<String>,
    /// Four offsets, `[a, b, c, d]`.
    pub byte_range: Option<Vec<u64>>,
    pub contents_size: u64,
    pub coverage: Option<SignatureCoverage>,
    pub revision_index: Option<u32>,
    pub signer: SignatureSignerClaims,
    pub doc_mdp: Option<u8>,
    /// The catalog names this signature as THE certification.
    pub catalog_certification: bool,
    pub field_mdp: Option<SignatureFieldLock>,
    pub lock: Option<SignatureFieldLock>,
    pub seed_value: Option<SignatureSeedValue>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignatureInfoJson {
    index: u32,
    field: Value,
    field_name: String,
    widget: Option<SignatureWidgetJson>,
    signed: bool,
    kind: SignatureKind,
    filter: Option<String>,
    sub_filter: Option<String>,
    byte_range: Option<Vec<u64>>,
    contents_size: u64,
    coverage: Option<SignatureCoverage>,
    revision_index: Option<u32>,
    signer: SignatureSignerClaims,
    doc_mdp: Option<u8>,
    catalog_certification: bool,
    field_mdp: Option<SignatureFieldLock>,
    lock: Option<SignatureFieldLock>,
    seed_value: Option<SignatureSeedValue>,
}

impl SignatureInfo {
    pub(crate) fn from_json(value: &Value) -> Result<Self, Error> {
        let j: SignatureInfoJson = decode("a signature", value)?;
        let widget = match j.widget {
            Some(w) => Some(SignatureWidget {
                annotation: annotation_from_json(&w.reference)?,
                annot_object_number: w.annot_object_number,
                page: page_from_json(&w.page)?,
            }),
            None => None,
        };
        Ok(SignatureInfo {
            index: j.index,
            field: field_from_json(&j.field)?,
            field_name: j.field_name,
            widget,
            signed: j.signed,
            kind: j.kind,
            filter: j.filter,
            sub_filter: j.sub_filter,
            byte_range: j.byte_range,
            contents_size: j.contents_size,
            coverage: j.coverage,
            revision_index: j.revision_index,
            signer: j.signer,
            doc_mdp: j.doc_mdp,
            catalog_certification: j.catalog_certification,
            field_mdp: j.field_mdp,
            lock: j.lock,
            seed_value: j.seed_value,
        })
    }

    pub(crate) fn list_from_json(list: &[Value]) -> Result<Vec<Self>, Error> {
        list.iter().map(Self::from_json).collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, uniffi::Enum)]
#[serde(rename_all = "lowercase")]
pub enum DocumentFieldLockSource {
    Fieldmdp,
    Lock,
}

/// A lock an earlier signature established.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct DocumentFieldLock {
    pub signature_index: u32,
    pub source: DocumentFieldLockSource,
    pub spec: SignatureFieldLock,
}

/// The certification signature and its DocMDP permission.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct DocumentCertification {
    pub signature_index: u32,
    pub permission: u8,
}

/// What the signatures in a document mean for what comes after: what a
/// signer declared (`enforced`, refused) and what a validator holds later
/// changes to (`judged`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct DocumentProtection {
    pub enforced: Option<SignatureModificationLevel>,
    pub judged: Option<SignatureModificationLevel>,
    pub certification: Option<DocumentCertification>,
    pub field_locks: Vec<DocumentFieldLock>,
    pub policy_version: u32,
}

impl DocumentProtection {
    pub(crate) fn from_json(value: &Value) -> Result<Self, Error> {
        decode("a document protection", value)
    }
}

/// The document's signatures, revisions and protection.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SignatureSnapshot {
    /// False when the cross-reference chain is broken: every byte fact is
    /// then indeterminate.
    pub chain_valid: bool,
    pub revisions: Vec<SignatureRevision>,
    pub signatures: Vec<SignatureInfo>,
    pub protection: DocumentProtection,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignatureSnapshotJson {
    chain_valid: bool,
    revisions: Vec<SignatureRevision>,
    signatures: Vec<Value>,
    protection: DocumentProtection,
}

impl SignatureSnapshot {
    pub(crate) fn from_json(value: &Value) -> Result<Self, Error> {
        let j: SignatureSnapshotJson = decode("a signature snapshot", value)?;
        Ok(SignatureSnapshot {
            chain_valid: j.chain_valid,
            revisions: j.revisions,
            signatures: SignatureInfo::list_from_json(&j.signatures)?,
            protection: j.protection,
        })
    }
}

/// A parked two-phase signing: the document is read-only until it
/// completes or is aborted.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SignaturePending {
    pub signing_id: String,
    pub field: FormFieldRef,
}

impl TryFrom<crate::SignaturePending> for SignaturePending {
    type Error = Error;
    fn try_from(p: crate::SignaturePending) -> Result<Self, Error> {
        Ok(SignaturePending { signing_id: p.signing_id, field: field_from_json(&p.field)? })
    }
}

// ── verdicts ─────────────────────────────────────────────────────────────────

/// Do the signed bytes still hash to what the CMS signed?
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, uniffi::Enum)]
#[serde(rename_all = "lowercase")]
pub enum SignatureIntegrity {
    Valid,
    Invalid,
    Indeterminate,
}

/// The CMS signature over its signed attributes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, uniffi::Enum)]
#[serde(rename_all = "lowercase")]
pub enum SignatureCryptography {
    Valid,
    Invalid,
    Unsupported,
}

/// The signer's chain against the trust anchors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, uniffi::Enum)]
#[serde(rename_all = "lowercase")]
pub enum SignatureTrustStatus {
    Trusted,
    Untrusted,
    Unknown,
}

/// When the chain was judged.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Enum)]
pub enum SignatureValidationTime {
    Now,
    /// A verified RFC 3161 token's time, epoch milliseconds.
    Timestamp {
        gen_time_ms: f64,
    },
}

impl SignatureValidationTime {
    fn from_json(value: &Value) -> Result<Self, Error> {
        match value.get("kind").and_then(Value::as_str) {
            Some("now") => Ok(SignatureValidationTime::Now),
            Some("timestamp") => {
                let time = value.get("genTime").and_then(|t| t.get("$date").or(Some(t)));
                let gen_time_ms = time
                    .and_then(Value::as_f64)
                    .ok_or_else(|| invalid_state("a validation time", value))?;
                Ok(SignatureValidationTime::Timestamp { gen_time_ms })
            }
            _ => Err(invalid_state("a validation time", value)),
        }
    }
}

impl From<SignatureValidationTime> for epdf_signature_core::ValidationTime {
    fn from(t: SignatureValidationTime) -> Self {
        match t {
            SignatureValidationTime::Now => epdf_signature_core::ValidationTime::Now,
            SignatureValidationTime::Timestamp { gen_time_ms } => {
                epdf_signature_core::ValidationTime::Timestamp { gen_time: gen_time_ms }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, uniffi::Enum)]
#[serde(rename_all = "lowercase")]
pub enum SignatureModificationsVerdict {
    Unchanged,
    Permitted,
    Forbidden,
    Indeterminate,
}

/// Which bytes were judged: the saved ones or the unsaved working copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, uniffi::Enum)]
#[serde(rename_all = "kebab-case")]
pub enum SignatureModificationsBasis {
    Persisted,
    WorkingCopy,
}

impl SignatureModificationsBasis {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            SignatureModificationsBasis::Persisted => "persisted",
            SignatureModificationsBasis::WorkingCopy => "working-copy",
        }
    }
}

/// What changed after the signature, and how that is judged.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct SignatureModifications {
    pub verdict: SignatureModificationsVerdict,
    #[serde(default)]
    pub detail: Option<String>,
    pub basis: SignatureModificationsBasis,
    #[serde(default)]
    pub later_revisions: Option<u32>,
    #[serde(default)]
    pub undone: Option<bool>,
}

/// The one answer a UI shows. Never `Valid` without trust anchors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, uniffi::Enum)]
#[serde(rename_all = "kebab-case")]
pub enum SignatureSummary {
    Valid,
    ValidUntrusted,
    Invalid,
    Indeterminate,
}

/// One signature, judged. The parsed CMS is not exported: `signature` and
/// `trust_reason` carry what a UI shows.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SignatureVerdict {
    pub signature: SignatureInfo,
    pub integrity: SignatureIntegrity,
    pub cryptography: SignatureCryptography,
    pub trust: SignatureTrustStatus,
    pub trust_reason: Option<String>,
    pub validated_at: SignatureValidationTime,
    pub modifications: SignatureModifications,
    pub summary: SignatureSummary,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignatureVerdictJson {
    signature: Value,
    integrity: SignatureIntegrity,
    cryptography: SignatureCryptography,
    trust: SignatureTrustStatus,
    #[serde(default)]
    trust_reason: Option<String>,
    validated_at: Value,
    modifications: SignatureModifications,
    summary: SignatureSummary,
}

impl SignatureVerdict {
    pub(crate) fn from_json(value: &Value) -> Result<Self, Error> {
        let j: SignatureVerdictJson = decode("a signature verdict", value)?;
        Ok(SignatureVerdict {
            signature: SignatureInfo::from_json(&j.signature)?,
            integrity: j.integrity,
            cryptography: j.cryptography,
            trust: j.trust,
            trust_reason: j.trust_reason,
            validated_at: SignatureValidationTime::from_json(&j.validated_at)?,
            modifications: j.modifications,
            summary: j.summary,
        })
    }

    pub(crate) fn list_from_json(value: &Value) -> Result<Vec<Self>, Error> {
        value
            .as_array()
            .ok_or_else(|| invalid_state("a verdict list", value))?
            .iter()
            .map(Self::from_json)
            .collect()
    }
}

/// `validate`'s options. Every member optional.
#[derive(Clone, Copy, Debug, Default, PartialEq, uniffi::Record)]
pub struct SignatureValidateRequest {
    /// `None` is now.
    #[uniffi(default = None)]
    pub at: Option<SignatureValidationTime>,
    /// `None` is the working copy, the plugin's default.
    #[uniffi(default = None)]
    pub until: Option<SignatureModificationsBasis>,
}

impl SignatureValidateRequest {
    /// `SignatureValidateOptions`, with the call's signal.
    pub(crate) fn into_core(
        self,
        signal: Option<epdf_kernel::signal::AbortSignal>,
    ) -> crate::SignatureValidateOptions {
        crate::SignatureValidateOptions {
            at: self.at.map(Into::into),
            until: self.until.map(|u| u.as_str().to_string()),
            signal,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, uniffi::Enum)]
#[serde(rename_all = "lowercase")]
pub enum SignatureFindingVerdict {
    Permitted,
    Forbidden,
    Incomplete,
}

/// The finding that decided an analysis.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct SignatureChangeFinding {
    pub rule: String,
    pub verdict: SignatureFindingVerdict,
    pub object_number: u32,
    #[serde(default)]
    pub detail: Option<String>,
}

/// What changed after a signature: the engine's `ChangeAnalysis`, the
/// parts a UI reads.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SignatureChangeAnalysis {
    pub verdict: SignatureModificationsVerdict,
    pub primary: Option<SignatureChangeFinding>,
    pub basis: SignatureModificationsBasis,
    /// Revisions after the signed one.
    pub later_revisions: u32,
    /// Objects rewritten after the signature that hold the signed value
    /// again.
    pub undone_object_numbers: Vec<u32>,
}

impl SignatureChangeAnalysis {
    pub(crate) fn from_json(value: &Value) -> Result<Self, Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Current {
            #[serde(default)]
            primary: Option<SignatureChangeFinding>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Basis {
            source: SignatureModificationsBasis,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Later {
            revision_count: u32,
            undone_object_numbers: Vec<u32>,
        }
        #[derive(Deserialize)]
        struct Json {
            verdict: SignatureModificationsVerdict,
            current: Current,
            basis: Basis,
            later: Later,
        }
        let j: Json = decode("a change analysis", value)?;
        Ok(SignatureChangeAnalysis {
            verdict: j.verdict,
            primary: j.current.primary,
            basis: j.basis.source,
            later_revisions: j.later.revision_count,
            undone_object_numbers: j.later.undone_object_numbers,
        })
    }
}

// ── marks and completions ────────────────────────────────────────────────────

/// What goes into a signature field: a stamp-library asset, or artwork.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SignatureMark {
    /// A stamp-library asset (needs the stamp plugin).
    Asset { asset_id: String },
    /// Artwork bytes: a PDF, PNG or JPEG.
    Source { bytes: Vec<u8> },
}

impl From<SignatureMark> for crate::Mark {
    fn from(m: SignatureMark) -> Self {
        match m {
            SignatureMark::Asset { asset_id } => crate::Mark::Asset { asset_id },
            SignatureMark::Source { bytes } => {
                crate::Mark::Source { source: Bytes(bytes).to_json() }
            }
        }
    }
}

impl TryFrom<&crate::Mark> for SignatureMark {
    type Error = Error;
    fn try_from(m: &crate::Mark) -> Result<Self, Error> {
        match m {
            crate::Mark::Asset { asset_id } => {
                Ok(SignatureMark::Asset { asset_id: asset_id.clone() })
            }
            crate::Mark::Source { source } => Bytes::from_json(source)
                .map(|b| SignatureMark::Source { bytes: b.0 })
                .ok_or_else(|| invalid_state("a mark's bytes", source)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, uniffi::Enum)]
#[serde(rename_all = "kebab-case")]
pub enum SignatureCompletionStatus {
    Completed,
    /// An idempotent replay with the same CMS.
    AlreadyCompleted,
}

/// The saved version the document is on.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct SignatureBaseVersion {
    pub sha256: String,
    pub byte_length: u64,
}

/// What a signing candidate was built on.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct SignatureVersionRef {
    pub base_sha256: String,
    pub edits_version: u64,
}

/// A completed signing.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SignatureCompletion {
    pub status: SignatureCompletionStatus,
    pub signature: SignatureInfo,
    /// The version the sealed bytes became.
    pub version: SignatureBaseVersion,
    /// What they were built on.
    pub previous: SignatureVersionRef,
    pub protection: DocumentProtection,
}

impl SignatureCompletion {
    pub(crate) fn from_json(value: &Value) -> Result<Self, Error> {
        #[derive(Deserialize)]
        struct Json {
            status: SignatureCompletionStatus,
            signature: Value,
            version: SignatureBaseVersion,
            previous: SignatureVersionRef,
            protection: DocumentProtection,
        }
        let j: Json = decode("a signature completion", value)?;
        Ok(SignatureCompletion {
            status: j.status,
            signature: SignatureInfo::from_json(&j.signature)?,
            version: j.version,
            previous: j.previous,
            protection: j.protection,
        })
    }
}

// ── events ───────────────────────────────────────────────────────────────────

/// The plugin's nine change hooks, as one stream. Origins are dropped.
// A uniffi enum crosses by value; boxing the completion would change the
// Swift type for no gain on a value built once per event.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum SignatureEvent {
    Signed {
        field: FormFieldRef,
        result: SignatureCompletion,
    },
    /// A visual mark was drawn into a field.
    Filled {
        field: FormFieldRef,
    },
    /// A field's visual mark was removed.
    Cleared {
        field: FormFieldRef,
    },
    Validated {
        verdicts: Vec<SignatureVerdict>,
    },
    ProtectionChanged {
        protection: DocumentProtection,
    },
    /// An unsaved edit just turned a held signature into one a save would
    /// invalidate. Once per edge.
    InvalidationPredicted {
        field: FormFieldRef,
        detail: String,
    },
    TargetChanged {
        field: Option<FormFieldRef>,
    },
    /// Mode `ask` (or a certification to offer): the app decides.
    SignRequested {
        field: FormFieldRef,
        mark: SignatureMark,
    },
    /// A signed field was activated.
    InspectionRequested {
        field: FormFieldRef,
    },
}

#[uniffi::export(with_foreign)]
pub trait SignatureEventListener: Send + Sync {
    fn on_event(&self, event: SignatureEvent);
    /// The stream ended: the document closed.
    fn on_end(&self);
}

/// Register the nine hooks, each decoding into one `SignatureEvent`.
pub(crate) fn register_hooks(
    c: &crate::SignatureCapability,
    listener: std::sync::Arc<dyn SignatureEventListener>,
) -> epdf_kernel::Unsubscribe {
    let l = listener.clone();
    let signed = c.on_signed(move |e| {
        l.on_event(SignatureEvent::Signed {
            field: field_from_json(&e.field)?,
            result: SignatureCompletion::from_json(&e.result)?,
        });
        Ok(())
    });
    let l = listener.clone();
    let filled = c.on_filled(move |e| {
        l.on_event(SignatureEvent::Filled { field: field_from_json(&e.field)? });
        Ok(())
    });
    let l = listener.clone();
    let cleared = c.on_cleared(move |e| {
        l.on_event(SignatureEvent::Cleared { field: field_from_json(&e.field)? });
        Ok(())
    });
    let l = listener.clone();
    let validated = c.on_validated(move |e| {
        l.on_event(SignatureEvent::Validated {
            verdicts: SignatureVerdict::list_from_json(&e.verdicts)?,
        });
        Ok(())
    });
    let l = listener.clone();
    let protection = c.on_protection_changed(move |e| {
        l.on_event(SignatureEvent::ProtectionChanged {
            protection: DocumentProtection::from_json(&e.protection)?,
        });
        Ok(())
    });
    let l = listener.clone();
    let invalidating = c.on_invalidation_predicted(move |e| {
        l.on_event(SignatureEvent::InvalidationPredicted {
            field: field_from_json(&e.field)?,
            detail: e.detail.clone(),
        });
        Ok(())
    });
    let l = listener.clone();
    let target = c.on_target_changed(move |e| {
        let field = if e.field.is_null() { None } else { Some(field_from_json(&e.field)?) };
        l.on_event(SignatureEvent::TargetChanged { field });
        Ok(())
    });
    let l = listener.clone();
    let requested = c.on_sign_requested(move |e| {
        l.on_event(SignatureEvent::SignRequested {
            field: field_from_json(&e.field)?,
            mark: SignatureMark::try_from(&e.mark)?,
        });
        Ok(())
    });
    let l = listener;
    let inspection = c.on_inspection_requested(move |e| {
        l.on_event(SignatureEvent::InspectionRequested { field: field_from_json(&e.field)? });
        Ok(())
    });
    let offs = [
        signed,
        filled,
        cleared,
        validated,
        protection,
        invalidating,
        target,
        requested,
        inspection,
    ];
    epdf_kernel::Unsubscribe::new(move || offs.iter().for_each(epdf_kernel::Unsubscribe::call))
}

// ── signing inputs ───────────────────────────────────────────────────────────

/// What the signature dictionary says about the signer
/// (`SignatureSignerInput`, sent as the prepare input's `signer`). Every
/// member optional; the certificate's common name is the default `name`.
#[derive(Clone, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct SignatureAttribution {
    #[uniffi(default = None)]
    pub name: Option<String>,
    #[uniffi(default = None)]
    pub reason: Option<String>,
    #[uniffi(default = None)]
    pub location: Option<String>,
    #[uniffi(default = None)]
    pub contact_info: Option<String>,
}

impl SignatureAttribution {
    /// The members that are set, in the engine's spelling.
    pub(crate) fn to_json(&self) -> Value {
        let mut out = serde_json::Map::new();
        for (key, value) in [
            ("name", &self.name),
            ("reason", &self.reason),
            ("location", &self.location),
            ("contactInfo", &self.contact_info),
        ] {
            if let Some(value) = value {
                out.insert(key.into(), json!(value));
            }
        }
        Value::Object(out)
    }
}

/// `sign`'s input.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SignatureSignInput {
    pub field: FormFieldRef,
    /// The artwork drawn into the field; `None` signs with no appearance.
    #[uniffi(default = None)]
    pub mark: Option<SignatureMark>,
    #[uniffi(default = None)]
    pub attribution: Option<SignatureAttribution>,
    /// The DocMDP permission (1, 2 or 3) to certify with. Needs
    /// `allow_certify` and `doc.sign.certify`, and no earlier signature.
    #[uniffi(default = None)]
    pub certify: Option<u8>,
    /// A FieldMDP for this signature, mirrored as the field's `/Lock`.
    #[uniffi(default = None)]
    pub lock: Option<SignatureFieldLock>,
}

impl SignatureSignInput {
    /// `SignFieldInput`, with a per-call key.
    pub(crate) fn into_core(self, key: Option<crate::SignatureKey>) -> crate::SignFieldInput {
        crate::SignFieldInput {
            field: field_json(&self.field),
            mark: self.mark.map(Into::into),
            key,
            signer: self.attribution.map(|a| a.to_json()),
            appearance: None,
            certify: self.certify.map(|p| json!({ "permission": p })),
            lock: self.lock.map(|l| l.to_json()),
        }
    }
}

/// `prepare_signature`'s input: `sign`'s, plus the CMS profile and digest
/// the external signer will use.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SignaturePrepareInput {
    pub field: FormFieldRef,
    #[uniffi(default = None)]
    pub mark: Option<SignatureMark>,
    #[uniffi(default = None)]
    pub attribution: Option<SignatureAttribution>,
    #[uniffi(default = None)]
    pub certify: Option<u8>,
    #[uniffi(default = None)]
    pub lock: Option<SignatureFieldLock>,
    /// `adbe.pkcs7.detached` or `ETSI.CAdES.detached` (the default).
    #[uniffi(default = None)]
    pub sub_filter: Option<String>,
    #[uniffi(default = None)]
    pub digest: Option<super::ports::SignatureDigest>,
}

impl SignaturePrepareInput {
    pub(crate) fn into_core(self) -> crate::PrepareSignatureInput {
        crate::PrepareSignatureInput {
            field: field_json(&self.field),
            mark: self.mark.map(Into::into),
            appearance: None,
            signer: self.attribution.map(|a| a.to_json()),
            certify: self.certify.map(|p| json!({ "permission": p })),
            lock: self.lock.map(|l| l.to_json()),
            sub_filter: self.sub_filter.map(Value::String),
            digest: self.digest.map(|d| json!(d.name())),
        }
    }
}

/// Phase one of an external signing: the digest to sign elsewhere.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SignaturePrepared {
    pub signing_id: String,
    pub digest: Vec<u8>,
    /// `sha256`, `sha384` or `sha512`.
    pub algorithm: String,
    pub byte_range: Vec<u64>,
    pub contents_size: u64,
    pub sub_filter: String,
    /// What `complete_signature` must present.
    pub expected_version: SignatureVersionRef,
    pub expires_at: Option<String>,
}

impl SignaturePrepared {
    pub(crate) fn from_json(value: &Value) -> Result<Self, Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Json {
            signing_id: String,
            digest: Value,
            algorithm: String,
            byte_range: Vec<u64>,
            contents_size: u64,
            sub_filter: String,
            expected_version: SignatureVersionRef,
            #[serde(default)]
            expires_at: Option<String>,
        }
        let j: Json = decode("a prepared signing", value)?;
        Ok(SignaturePrepared {
            signing_id: j.signing_id,
            digest: Bytes::from_json(&j.digest)
                .map(|b| b.0)
                .ok_or_else(|| invalid_state("a digest", &j.digest))?,
            algorithm: j.algorithm,
            byte_range: j.byte_range,
            contents_size: j.contents_size,
            sub_filter: j.sub_filter,
            expected_version: j.expected_version,
            expires_at: j.expires_at,
        })
    }
}

/// Where `place_mark` puts a mark.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum SignaturePlaceTarget {
    /// Into a signature field, by `mode`.
    Field { field: FormFieldRef },
    /// Freely on a page, as a stamp. `at` is the centre, in page space.
    Free { page: PageRef, at: epdf_ffi::Point, target_width: Option<f64>, rotation: Option<f64> },
}

impl TryFrom<SignaturePlaceTarget> for crate::PlaceTarget {
    type Error = Error;
    fn try_from(t: SignaturePlaceTarget) -> Result<Self, Error> {
        match t {
            SignaturePlaceTarget::Field { field } => {
                Ok(crate::PlaceTarget::Field(field_json(&field)))
            }
            SignaturePlaceTarget::Free { page, at, target_width, rotation } => {
                // A `StampPlacement`: the page, the centre, and the width
                // and rotation when given.
                let mut placement = json!({
                    "page": { "kind": "objectNumber", "objectNumber": page.page_object_number },
                    "center": { "x": at.x, "y": at.y },
                });
                if let Some(width) = target_width {
                    placement["targetWidth"] = json!(width);
                }
                if let Some(rotation) = rotation {
                    placement["rotation"] = json!(rotation);
                }
                Ok(crate::PlaceTarget::Free(placement))
            }
        }
    }
}

/// What `place_mark` did.
// A uniffi enum crosses by value; boxing the completion would change the
// Swift type for no gain on a value built once per event.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum SignaturePlaceResult {
    Signed {
        field: FormFieldRef,
        result: SignatureCompletion,
    },
    Filled {
        field: FormFieldRef,
    },
    /// `SignRequested` fired: the app decides.
    Requested {
        field: FormFieldRef,
    },
    /// A free placement became this stamp annotation.
    Placed {
        annotation: AnnotationRef,
    },
}

impl TryFrom<crate::PlaceMarkResult> for SignaturePlaceResult {
    type Error = Error;
    fn try_from(r: crate::PlaceMarkResult) -> Result<Self, Error> {
        Ok(match r {
            crate::PlaceMarkResult::Signed { field, result } => SignaturePlaceResult::Signed {
                field: field_from_json(&field)?,
                result: SignatureCompletion::from_json(&result)?,
            },
            crate::PlaceMarkResult::Filled { field } => {
                SignaturePlaceResult::Filled { field: field_from_json(&field)? }
            }
            crate::PlaceMarkResult::Requested { field } => {
                SignaturePlaceResult::Requested { field: field_from_json(&field)? }
            }
            crate::PlaceMarkResult::Placed { annotation } => SignaturePlaceResult::Placed {
                annotation: annotation_from_json(&annotation)?
                    .ok_or_else(|| invalid_state("a placed annotation", &annotation))?,
            },
        })
    }
}
