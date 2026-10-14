//! `SignatureReader.ts` and `internal/readSignatureModel.ts`: the signature
//! snapshot, a signature's `/Contents`, a revision's bytes and the loaded
//! bytes' version. Byte facts come from the bytes the document was loaded
//! from, never from unsaved edits.
//!
//! The TypeScript caches the native model per mutation sequence
//! (`signatureModelCache.ts`); this builds one per call, which costs a walk
//! of the field tree and changes no answer.

use pdfium::{Document, SignatureModel, field_list, string_key};

use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::forms::snapshot::{FormFieldKind, FormFieldRef, read_form_snapshot};
use crate::session::document::{DocumentSession, OpenKind};

use super::analysis::types::{RevisionField, RevisionStructure};
use super::protection::derive_protection;
use super::types::{
    BaseVersionInfo, DigestAlgorithm, DocumentProtection, FieldLockAction, FieldLockSpec,
    FormWidget, PdfRevision, SignatureCoverage, SignatureDto, SignatureKind, SignatureSeedValue,
    SignatureSigner, SignatureSnapshot,
};

// Mirrors public/epdf_signature.h.
const KIND_DOC_TIMESTAMP: i32 = 1;
const SV_V: u32 = 1 << 2;
const SV_LEGAL_ATTESTATION: u32 = 1 << 4;
const SV_ADD_REV_INFO: u32 = 1 << 5;
const SV_LOCK_DOCUMENT: u32 = 1 << 7;
const SV_APPEARANCE_FILTER: u32 = 1 << 8;
const SV_MDP: u32 = 1 << 17;
const SV_LIST_SUBFILTER: i32 = 0;
const SV_LIST_DIGEST_METHOD: i32 = 1;
const SV_LIST_REASONS: i32 = 2;
/// The PDF 1.7 seed-value entry set this engine implements.
const SUPPORTED_SEED_VALUE_VERSION: i32 = 2;
const SV_UNSUPPORTED_REQUIRED: u32 =
    SV_LEGAL_ATTESTATION | SV_ADD_REV_INFO | SV_LOCK_DOCUMENT | SV_APPEARANCE_FILTER;

fn native(e: pdfium::PdfiumError) -> EngineError {
    EngineError::with_message(EngineErrorCode::Unknown, e.to_string())
}

fn coverage(code: i32) -> SignatureCoverage {
    match code {
        0 => SignatureCoverage::WholeRevision,
        1 => SignatureCoverage::Partial,
        _ => SignatureCoverage::Malformed,
    }
}

fn field_action(code: i32) -> Option<FieldLockAction> {
    match code {
        1 => Some(FieldLockAction::All),
        2 => Some(FieldLockAction::Include),
        3 => Some(FieldLockAction::Exclude),
        _ => None,
    }
}

fn permission(value: i32) -> Option<u8> {
    (1..=3).contains(&value).then_some(value as u8)
}

/// `readSignatureModel.ts` `readLock`.
fn read_lock(
    model: &SignatureModel<'_>,
    index: i32,
    which: i32,
) -> Result<Option<FieldLockSpec>, EngineError> {
    let code = if which == field_list::LOCK {
        model.lock_action(index)
    } else {
        model.field_mdp_action(index)
    };
    let Some(action) = field_action(code) else { return Ok(None) };
    let fields = model.field_names(index, which).map_err(native)?;
    let permission =
        if which == field_list::LOCK { permission(model.lock_permission(index)) } else { None };
    Ok(Some(FieldLockSpec { action, fields, permission }))
}

/// `readSignatureModel.ts` `readSeedValue`.
fn read_seed_value(
    model: &SignatureModel<'_>,
    index: i32,
) -> Result<Option<SignatureSeedValue>, EngineError> {
    if !model.has_seed_value(index) {
        return Ok(None);
    }
    let required_flags = model.seed_value_required_flags(index);
    let present_flags = model.seed_value_present_flags(index);
    let version = model.seed_value_version(index);
    let mdp = model.seed_value_mdp(index);
    Ok(Some(SignatureSeedValue {
        required_flags,
        present_flags,
        version: (version > 0).then_some(version),
        mdp: ((present_flags & SV_MDP) != 0 && (0..=3).contains(&mdp)).then_some(mdp as u8),
        filter: model.seed_value_filter(index).map_err(native)?,
        sub_filters: model.seed_value_list(index, SV_LIST_SUBFILTER).map_err(native)?,
        digest_methods: model.seed_value_list(index, SV_LIST_DIGEST_METHOD).map_err(native)?,
        reasons: model.seed_value_list(index, SV_LIST_REASONS).map_err(native)?,
        unsupported_required: (required_flags & SV_UNSUPPORTED_REQUIRED) != 0
            || ((required_flags & SV_V) != 0 && version > SUPPORTED_SEED_VALUE_VERSION),
    }))
}

/// `readSignaturesFromModel`: every signature field, in model order.
pub fn read_signatures_from_model(
    model: &SignatureModel<'_>,
) -> Result<Vec<SignatureDto>, EngineError> {
    let count = model.count();
    let mut out = Vec::with_capacity(count.max(0) as usize);
    let s = |i: i32, key: i32| model.string(i, key).map_err(native);
    for i in 0..count {
        let signed = model.is_signed(i);
        let widget_obj_num = model.widget_object_number(i);
        let revision_index = model.revision_index(i);
        out.push(SignatureDto {
            index: i as u32,
            field: FormFieldRef::ObjectNumber { field: model.field_object_number(i) },
            field_name: model.field_name(i).map_err(native)?.unwrap_or_default(),
            widget: (widget_obj_num > 0)
                .then(|| FormWidget::new(widget_obj_num, model.widget_page_object_number(i))),
            signed,
            kind: if model.kind(i) == KIND_DOC_TIMESTAMP {
                SignatureKind::Timestamp
            } else {
                SignatureKind::Signature
            },
            filter: s(i, string_key::FILTER)?,
            sub_filter: s(i, string_key::SUBFILTER)?,
            byte_range: if signed { model.byte_range(i) } else { None },
            contents_size: if signed { model.contents_len(i) } else { 0 },
            coverage: signed.then(|| coverage(model.coverage(i))),
            revision_index: (signed && revision_index >= 0).then_some(revision_index as u32),
            signer: SignatureSigner {
                name: s(i, string_key::NAME)?,
                reason: s(i, string_key::REASON)?,
                location: s(i, string_key::LOCATION)?,
                contact_info: s(i, string_key::CONTACT_INFO)?,
                claimed_time: s(i, string_key::M)?,
            },
            doc_mdp: permission(model.doc_mdp_permission(i)),
            catalog_certification: model.is_catalog_certification(i),
            field_mdp: read_lock(model, i, field_list::FIELDMDP)?,
            lock: read_lock(model, i, field_list::LOCK)?,
            seed_value: read_seed_value(model, i)?,
        });
    }
    Ok(out)
}

/// `readContentsAt`: `NotFound` when unsigned or malformed.
pub fn read_contents_at(model: &SignatureModel<'_>, index: i32) -> Result<Vec<u8>, EngineError> {
    if model.contents_len(index) == 0 {
        return Err(EngineError::with_message(
            EngineErrorCode::NotFound,
            "signature has no usable /Contents (malformed encoding)",
        ));
    }
    model.contents(index).map_err(|_| {
        EngineError::with_message(EngineErrorCode::Unknown, "failed to read signature contents")
    })
}

/// `readRevisions`: the chained revisions, oldest first.
pub fn read_revisions(
    doc: &Document,
    signatures: &[SignatureDto],
) -> Result<Vec<PdfRevision>, EngineError> {
    let count = doc.revision_count();
    let mut revisions = Vec::new();
    for i in 0..count.max(0) {
        let (end, xref_offset) = doc.revision(i).ok_or_else(|| {
            EngineError::with_message(
                EngineErrorCode::Unknown,
                format!("failed to read revision {i}"),
            )
        })?;
        let sealed_by = signatures.iter().find(|s| s.revision_index == Some(i as u32));
        revisions.push(PdfRevision {
            index: i as u32,
            end,
            xref_offset,
            signature_index: sealed_by.map(|s| s.index),
        });
    }
    Ok(revisions)
}

/// `withSignatureModel`.
pub fn with_signature_model<T>(
    doc: &Document,
    body: impl FnOnce(&SignatureModel<'_>) -> Result<T, EngineError>,
) -> Result<T, EngineError> {
    let model = doc.signature_model().map_err(|_| {
        EngineError::with_message(EngineErrorCode::Unknown, "failed to build signature model")
    })?;
    body(&model)
}

/// `hasSignedSignature`.
pub fn has_signed_signature(doc: &Document) -> Result<bool, EngineError> {
    with_signature_model(doc, |model| Ok((0..model.count()).any(|i| model.is_signed(i))))
}

/// `SignatureReader.readSnapshot`.
pub fn read_snapshot(doc: &Document) -> Result<SignatureSnapshot, EngineError> {
    with_signature_model(doc, |model| {
        let chain_valid = model.is_revision_chain_valid();
        let signatures = read_signatures_from_model(model)?;
        let revisions = if chain_valid { read_revisions(doc, &signatures)? } else { Vec::new() };
        let protection = derive_protection(&signatures);
        Ok(SignatureSnapshot { chain_valid, revisions, signatures, protection })
    })
}

/// `SignatureReader.readProtection`.
pub fn read_protection(doc: &Document) -> Result<DocumentProtection, EngineError> {
    with_signature_model(doc, |model| Ok(derive_protection(&read_signatures_from_model(model)?)))
}

/// `SignatureReader.readContents`: the DER `/Contents` of a signed field.
pub fn read_contents(doc: &Document, field: &FormFieldRef) -> Result<Vec<u8>, EngineError> {
    with_signature_model(doc, |model| {
        let index = require_signed_index(model, field)?;
        read_contents_at(model, index)
    })
}

/// `SignatureReader.digest`: hash a signed field's `/ByteRange`.
pub fn digest(
    doc: &Document,
    field: &FormFieldRef,
    algorithm: DigestAlgorithm,
) -> Result<Vec<u8>, EngineError> {
    let range = with_signature_model(doc, |model| {
        let index = require_signed_index(model, field)?;
        model.byte_range(index).ok_or_else(|| {
            EngineError::with_message(
                EngineErrorCode::NotFound,
                "signature has no usable /ByteRange",
            )
        })
    })?;
    digest_range(doc, range, algorithm)
}

/// `SignatureReader.revisionBytes`: the exact bytes of one revision.
pub fn revision_bytes(doc: &Document, revision_index: u32) -> Result<Vec<u8>, EngineError> {
    let snapshot = read_snapshot(doc)?;
    if !snapshot.chain_valid {
        return Err(EngineError::with_message(
            EngineErrorCode::MalformedPdf,
            "revision chain is not valid",
        ));
    }
    let revision = snapshot.revisions.get(revision_index as usize).ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::NotFound,
            format!("no revision {revision_index}"),
        )
    })?;
    read_loaded_bytes(doc, 0, revision.end)
}

/// What `SignatureReader.resolveField` answers.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedSignatureField {
    pub index: i32,
    pub field_object_number: u32,
    pub signed: bool,
    pub widget: Option<FormWidget>,
}

fn not_found(field: &FormFieldRef) -> EngineError {
    EngineError::with_message(
        EngineErrorCode::NotFound,
        match field {
            FormFieldRef::ObjectNumber { field } => {
                format!("signature field not found: object {field}")
            }
            FormFieldRef::Fqn { name } => format!("signature field not found: \"{name}\""),
        },
    )
}

/// `SignatureReader.resolveField`.
pub fn resolve_field(
    doc: &Document,
    field: &FormFieldRef,
) -> Result<ResolvedSignatureField, EngineError> {
    with_signature_model(doc, |model| {
        let index = index_of(model, field)?;
        if index < 0 {
            return Err(not_found(field));
        }
        let widget_obj_num = model.widget_object_number(index);
        Ok(ResolvedSignatureField {
            index,
            field_object_number: model.field_object_number(index),
            signed: model.is_signed(index),
            widget: (widget_obj_num > 0)
                .then(|| FormWidget::new(widget_obj_num, model.widget_page_object_number(index))),
        })
    })
}

/// `SignatureReader.readSignatureByObjectNumber`.
pub fn read_signature_by_object_number(
    doc: &Document,
    field_object_number: u32,
) -> Result<SignatureDto, EngineError> {
    read_snapshot(doc)?
        .signatures
        .into_iter()
        .find(|s| s.field_object_number() == field_object_number)
        .ok_or_else(|| not_found(&FormFieldRef::ObjectNumber { field: field_object_number }))
}

/// `SignatureReader.loadedBytes`.
pub fn loaded_bytes(doc: &Document) -> Result<Vec<u8>, EngineError> {
    read_loaded_bytes(doc, 0, doc.loaded_bytes_size())
}

/// `SignatureReader.version`: SHA-256 and length of the base the session
/// is on. A layer reports its base's hash; a plain session hashes its
/// loaded bytes.
pub fn version(session: &DocumentSession) -> Result<BaseVersionInfo, EngineError> {
    let doc = session.document()?.document();
    let byte_length = doc.base_bytes_size();
    if byte_length == 0 {
        return Err(EngineError::with_message(
            EngineErrorCode::DocNotOpen,
            "document has no loaded bytes",
        ));
    }
    if session.kind() == Some(OpenKind::Layer) {
        let sha = doc.layer_base_sha256().ok_or_else(|| {
            EngineError::with_message(EngineErrorCode::Unknown, "failed to read the base hash")
        })?;
        return Ok(BaseVersionInfo { sha256: to_hex(&sha), byte_length });
    }
    let digest = digest_range(doc, [0, byte_length, byte_length, 0], DigestAlgorithm::Sha256)?;
    Ok(BaseVersionInfo { sha256: to_hex(&digest), byte_length })
}

/// `SignatureReader.digestRange`.
pub fn digest_range(
    doc: &Document,
    range: [u64; 4],
    algorithm: DigestAlgorithm,
) -> Result<Vec<u8>, EngineError> {
    doc.digest_byte_range(range, algorithm.native()).ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "byte range is not within the loaded bytes",
        )
    })
}

/// `SignatureReader.readLoadedBytes`.
pub fn read_loaded_bytes(doc: &Document, offset: u64, length: u64) -> Result<Vec<u8>, EngineError> {
    doc.read_loaded_bytes(offset, length).map_err(|_| {
        EngineError::with_message(EngineErrorCode::Unknown, "failed to read the loaded bytes")
    })
}

/// `SignatureReader.indexOf`: -1 when no field matches.
fn index_of(model: &SignatureModel<'_>, field: &FormFieldRef) -> Result<i32, EngineError> {
    match field {
        FormFieldRef::ObjectNumber { field } => Ok(model.index_by_field_object_number(*field)),
        FormFieldRef::Fqn { name } => {
            for i in 0..model.count() {
                if model.field_name(i).map_err(native)?.as_deref() == Some(name.as_str()) {
                    return Ok(i);
                }
            }
            Ok(-1)
        }
    }
}

/// `SignatureReader.requireSignedIndex`.
fn require_signed_index(
    model: &SignatureModel<'_>,
    field: &FormFieldRef,
) -> Result<i32, EngineError> {
    let index = index_of(model, field)?;
    if index < 0 {
        return Err(not_found(field));
    }
    if !model.is_signed(index) {
        return Err(EngineError::with_message(
            EngineErrorCode::NotFound,
            "signature field is not signed",
        ));
    }
    Ok(index)
}

pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The widget annotation numbers of a field, in snapshot order.
fn widget_numbers(kind: &FormFieldKind) -> Vec<u32> {
    match kind {
        FormFieldKind::Text { widgets, .. }
        | FormFieldKind::Combobox { widgets, .. }
        | FormFieldKind::Listbox { widgets, .. }
        | FormFieldKind::Pushbutton { widgets }
        | FormFieldKind::Signature { widgets }
        | FormFieldKind::Unknown { widgets, .. } => {
            widgets.iter().map(|w| w.annot_object_number).collect()
        }
        FormFieldKind::Checkbox { widgets, .. } | FormFieldKind::Radio { widgets, .. } => {
            widgets.iter().map(|w| w.widget.annot_object_number).collect()
        }
    }
}

/// `readSignatureModel.ts` `readStructure`: the catalog, AcroForm and page
/// tree object numbers, the pages, the terminal fields with their widgets,
/// and the signatures of one document (a revision prefix, usually).
pub fn read_structure(doc: &Document) -> Result<RevisionStructure, EngineError> {
    let (root, acro_form, pages_root) = doc.structure_object_numbers();
    let pages = (0..doc.page_count()).map(|i| doc.page_object_number_by_index(i)).collect();
    let mut fields = Vec::new();
    if let Some(model) = doc.form_model() {
        let snapshot = read_form_snapshot(&model, &CancelToken::never())?;
        for f in snapshot.fields {
            fields.push(RevisionField {
                object_number: f.base.field_object_number,
                name: f.base.name,
                family: f.base.family,
                widgets: widget_numbers(&f.kind),
                flags: Some(f.base.flags.raw),
            });
        }
    }
    let signatures = with_signature_model(doc, read_signatures_from_model)?;
    Ok(RevisionStructure { root, acro_form, pages_root, pages, fields, signatures })
}
