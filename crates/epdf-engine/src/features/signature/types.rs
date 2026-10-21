//! `packages/engine/core/src/signature/types.ts`: the signature DTOs, in the
//! wire's own spelling. The TypeScript doc comments explain each field and
//! are not repeated; what is noted is where the Rust shape differs.

use serde::{Deserialize, Serialize};

use crate::features::forms::snapshot::FormFieldRef;
use crate::identity::{PageRef, WirePage};

/// `SignedDocumentPolicy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SignedDocumentPolicy {
    #[default]
    Protect,
    Permit,
}

/// `DigestAlgorithm`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DigestAlgorithm {
    Sha1,
    Sha256,
    Sha384,
    Sha512,
}

impl DigestAlgorithm {
    pub fn native(self) -> pdfium::DigestAlgorithm {
        match self {
            Self::Sha1 => pdfium::DigestAlgorithm::Sha1,
            Self::Sha256 => pdfium::DigestAlgorithm::Sha256,
            Self::Sha384 => pdfium::DigestAlgorithm::Sha384,
            Self::Sha512 => pdfium::DigestAlgorithm::Sha512,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SignatureKind {
    Signature,
    Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SignatureCoverage {
    WholeRevision,
    Partial,
    Malformed,
}

/// `ModificationLevel`, ordered: `None < Lta < Fill < Annotate`
/// (`protection.ts` `LEVEL_RANK`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModificationLevel {
    None,
    Lta,
    Fill,
    Annotate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldLockAction {
    All,
    Include,
    Exclude,
}

/// `FieldLockSpec`. `permission` is a `DocMdpPermission` (1, 2 or 3),
/// absent rather than null when not given.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldLockSpec {
    pub action: FieldLockAction,
    pub fields: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission: Option<u8>,
}

/// `PdfRevision`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfRevision {
    pub index: u32,
    pub end: u64,
    pub xref_offset: u64,
    pub signature_index: Option<u32>,
}

/// `SignatureSigner`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureSigner {
    pub name: Option<String>,
    pub reason: Option<String>,
    pub location: Option<String>,
    pub contact_info: Option<String>,
    pub claimed_time: Option<String>,
}

/// `SignatureSeedValue`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureSeedValue {
    pub required_flags: u32,
    pub present_flags: u32,
    pub version: Option<i32>,
    pub mdp: Option<u8>,
    pub filter: Option<String>,
    pub sub_filters: Vec<String>,
    pub digest_methods: Vec<String>,
    pub reasons: Vec<String>,
    pub unsupported_required: bool,
}

/// An `AnnotationRef` by object number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum WireAnnotationRef {
    #[serde(rename = "objectNumber", rename_all = "camelCase")]
    ObjectNumber {
        page: PageRef,
        #[serde(rename = "objectNumber")]
        annot_object_number: u32,
    },
}

/// `FormWidget`, the shape `formWidget(objectNumber, page)` builds:
/// `{ref, objectNumber, page}` (595bdc6c7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormWidget {
    #[serde(rename = "ref")]
    pub reference: Option<WireAnnotationRef>,
    /// The widget annotation's object number, whether or not it is on a
    /// page; `0` for a widget stored as a direct object.
    #[serde(rename = "objectNumber")]
    pub annot_object_number: u32,
    pub page: Option<PageRef>,
}

impl FormWidget {
    /// `formWidget(objectNumber, widgetPageRef(pageObjectNumber))`.
    pub fn new(annot_object_number: u32, page_object_number: u32) -> FormWidget {
        let page = (page_object_number > 0).then(|| page_object_number.to_page_ref());
        FormWidget {
            reference: match page {
                Some(page) if annot_object_number > 0 => {
                    Some(WireAnnotationRef::ObjectNumber { page, annot_object_number })
                }
                _ => None,
            },
            annot_object_number,
            page,
        }
    }
}

/// `SignatureDTO`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureDto {
    pub index: u32,
    pub field: FormFieldRef,
    pub field_name: String,
    pub widget: Option<FormWidget>,
    pub signed: bool,
    pub kind: SignatureKind,
    pub filter: Option<String>,
    pub sub_filter: Option<String>,
    pub byte_range: Option<[u64; 4]>,
    pub contents_size: u64,
    pub coverage: Option<SignatureCoverage>,
    pub revision_index: Option<u32>,
    pub signer: SignatureSigner,
    pub doc_mdp: Option<u8>,
    pub catalog_certification: bool,
    pub field_mdp: Option<FieldLockSpec>,
    pub lock: Option<FieldLockSpec>,
    pub seed_value: Option<SignatureSeedValue>,
}

impl SignatureDto {
    /// The field's object number. Every DTO this engine reads carries an
    /// `objectNumber` ref; 0 for the `fqn` shape no reader produces.
    pub fn field_object_number(&self) -> u32 {
        match self.field {
            FormFieldRef::ObjectNumber { field } => field,
            FormFieldRef::Fqn { .. } => 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldLockSource {
    Fieldmdp,
    Lock,
}

/// `DocumentFieldLock`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentFieldLock {
    pub signature_index: u32,
    pub source: FieldLockSource,
    pub spec: FieldLockSpec,
}

/// `DocumentProtection['certification']`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Certification {
    pub signature_index: u32,
    pub permission: u8,
}

/// `DocumentProtection`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentProtection {
    pub enforced: Option<ModificationLevel>,
    pub judged: Option<ModificationLevel>,
    pub certification: Option<Certification>,
    pub field_locks: Vec<DocumentFieldLock>,
    pub policy_version: u32,
}

/// `SignatureSnapshot`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureSnapshot {
    pub chain_valid: bool,
    pub revisions: Vec<PdfRevision>,
    pub signatures: Vec<SignatureDto>,
    pub protection: DocumentProtection,
}

/// `BaseVersionInfo`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseVersionInfo {
    pub sha256: String,
    pub byte_length: u64,
}

/// `DocumentVersionRef`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentVersionRef {
    pub base_sha256: String,
    pub edits_version: u64,
}
