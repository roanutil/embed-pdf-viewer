//! `SignatureMutator.ts` with `internal/candidateStore.ts` (the memory
//! store), `internal/appearance.ts` and `internal/sealCheck.ts`: the
//! two-phase signing protocol on a session, and the visual fill
//! (`FormMutator.setSignatureAppearance`).
//!
//! `prepare` never touches the live document: it builds a candidate from
//! the bytes a save would write, writes the signature there, saves and
//! seals it, and parks it on the session. `complete` writes the CMS in,
//! proves the sealed bytes carry the signature, and installs them as the
//! session's document. `abort` discards the candidate.
//!
//! The TypeScript's candidate is a fresh layer over a memory base; a plain
//! document over the same bytes is what the fork's candidate calls are
//! specified against, and what this uses. The file-backed store
//! (`FileCandidateStore`) is the cloud's and is not ported.
//! `CandidateFinalizer.ts`, the session-less half of a durable signing, is
//! [`finalize_candidate`].

use pdfium::{Document, Library, PdfSaveMode, SavedCandidate, SignaturePrepareOptions};
use serde::{Deserialize, Serialize};

use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::forms::snapshot::{FormFieldDto, FormFieldFamily, FormFieldRef};
use crate::features::forms::write::read_writable_field;
use crate::identity::PageObjectNumber;
use crate::revision::PageState;
use crate::session::document::DocumentSession;

use super::reader::{
    loaded_bytes, read_contents_at, read_revisions, read_signatures_from_model, read_snapshot,
    resolve_field, version, widget_refs, with_signature_model,
};
use super::types::{
    BaseVersionInfo, DigestAlgorithm, DocumentProtection, DocumentVersionRef, FieldLockAction,
    FieldLockSpec, FormWidget, SignatureCoverage, SignatureDto, SignatureKind, WirePageRef,
};

const DEFAULT_CONTENTS_SIZE: u64 = 8192;
const MIN_CONTENTS_SIZE: u64 = 256;
const MAX_CONTENTS_SIZE: u64 = 4 * 1024 * 1024;

/// `SUBFILTER_CODE`.
fn subfilter_code(sub_filter: &str) -> Option<i32> {
    match sub_filter {
        "adbe.pkcs7.detached" => Some(0),
        "ETSI.CAdES.detached" => Some(1),
        "ETSI.RFC3161" => Some(2),
        _ => None,
    }
}

/// `FIELD_ACTION_CODE`.
fn field_action_code(action: FieldLockAction) -> i32 {
    match action {
        FieldLockAction::All => 1,
        FieldLockAction::Include => 2,
        FieldLockAction::Exclude => 3,
    }
}

fn refused(message: impl Into<String>) -> EngineError {
    EngineError::with_message(EngineErrorCode::SignatureRefused, message)
}

/// `SignaturePrepareInput['attribution']`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attribution {
    pub name: Option<String>,
    pub reason: Option<String>,
    pub location: Option<String>,
    pub contact_info: Option<String>,
}

/// `{ permission }` of a certification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Certify {
    pub permission: u8,
}

/// `SignatureAppearanceInput` on the wire: the PDF travels as payload 0.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppearanceInput {
    #[serde(default)]
    pub page_index: Option<i32>,
}

/// `SignaturePrepareInput`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignaturePrepareInput {
    pub field: FormFieldRef,
    #[serde(default)]
    pub kind: Option<SignatureKind>,
    #[serde(default)]
    pub sub_filter: Option<String>,
    #[serde(default)]
    pub digest: Option<DigestAlgorithm>,
    #[serde(default)]
    pub contents_size: Option<f64>,
    #[serde(default)]
    pub attribution: Option<Attribution>,
    /// The pre-rename spelling of `attribution`, which older clients send.
    #[serde(default)]
    pub signer: Option<Attribution>,
    #[serde(default)]
    pub signing_time: Option<String>,
    #[serde(default)]
    pub certify: Option<Certify>,
    #[serde(default)]
    pub lock: Option<FieldLockSpec>,
    #[serde(default)]
    pub appearance: Option<AppearanceInput>,
}

/// `SignaturePrepared`, the digest left out: it is the response's payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignaturePrepared {
    pub signing_id: String,
    pub algorithm: DigestAlgorithm,
    pub byte_range: [u64; 4],
    pub contents_size: u64,
    pub sub_filter: String,
    pub expected_version: DocumentVersionRef,
    pub expires_at: Option<String>,
}

/// `SignatureCompleteInput`, the CMS left out: it is payload 0.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureCompleteInput {
    pub signing_id: String,
    pub expected_version: DocumentVersionRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompletionStatus {
    Completed,
    AlreadyCompleted,
}

/// `MutationMeta`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MutationMeta {
    pub affected_pages: Vec<PageState>,
    pub cache_delta: Option<()>,
}

/// `SignatureCompleteResult`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureCompleteResult {
    pub status: CompletionStatus,
    pub signature: SignatureDto,
    pub version: BaseVersionInfo,
    pub previous: DocumentVersionRef,
    pub protection: DocumentProtection,
    pub meta: MutationMeta,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AbortStatus {
    Aborted,
    AlreadyCompleted,
    Unknown,
}

/// `SignatureAbortResult`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SignatureAbortResult {
    pub status: AbortStatus,
}

/// `PendingSigning`: the parked candidate.
#[derive(Debug, Clone)]
pub struct PendingSigning {
    prepared: SignaturePrepared,
    field_object_number: u32,
    saved: SavedCandidate,
    contents_offset: u64,
    contents_hex_length: u64,
}

/// `SigningCompletion`: the last completed signing, for idempotent replay.
#[derive(Debug, Clone)]
pub struct SigningCompletion {
    signing_id: String,
    cms: Vec<u8>,
    result: SignatureCompleteResult,
}

/// What a session holds for signing: `DocumentSession.pendingSigning` and
/// `lastCompletion`.
#[derive(Debug, Clone, Default)]
pub struct SigningState {
    pub(crate) pending: Option<PendingSigning>,
    pub(crate) last_completion: Option<SigningCompletion>,
}

impl SigningState {
    /// The parked signing's id, if any.
    pub fn pending_id(&self) -> Option<&str> {
        self.pending.as_ref().map(|p| p.prepared.signing_id.as_str())
    }
}

/// `assertNoPendingSigning`, for the mutating kinds.
pub fn assert_no_pending_signing(session: &DocumentSession) -> Result<(), EngineError> {
    match session.signing().borrow().pending_id() {
        Some(id) => Err(EngineError::with_message(
            EngineErrorCode::SigningPending,
            format!(
                "a signing is pending ({id}); complete or abort it before mutating the document"
            ),
        )),
        None => Ok(()),
    }
}

/// `session.versionRef(sha256)`.
fn version_ref(session: &DocumentSession, base_sha256: String) -> DocumentVersionRef {
    DocumentVersionRef { base_sha256, edits_version: session.mutation_sequence() }
}

/// `DocumentSaver.snapshot`: the bytes a save would write, or the loaded
/// bytes when nothing changed since load.
fn snapshot_bytes(session: &DocumentSession) -> Result<Vec<u8>, EngineError> {
    let doc = session.document()?.document();
    if session.has_unsaved_edits()
        && let Some(saved) = doc.save_to_buffer_ex(PdfSaveMode::Incremental).map_err(|_| {
            EngineError::with_message(EngineErrorCode::DocOpenFailed, "failed to save document")
        })?
    {
        return Ok(saved);
    }
    loaded_bytes(doc)
}

/// A document over `bytes` under the session's password, with the owner
/// permissions the session's own open sets (`PdfDocumentOpener`).
fn open_bytes(session: &DocumentSession, bytes: &[u8]) -> Result<Document, EngineError> {
    let document = Document::from_bytes(Library::acquire(), bytes, session.password())
        .map_err(|e| EngineError::with_message(EngineErrorCode::DocOpenFailed, e.to_string()))?;
    if document.is_encrypted() {
        document.set_runtime_owner_permissions(true);
    }
    Ok(document)
}

/// `bakeWidgetAppearance`.
fn bake_widget_appearance(
    doc: &Document,
    widget: &FormWidget,
    pdf: &[u8],
    page_index: i32,
) -> Result<(), EngineError> {
    let Some(WirePageRef::ObjectNumber { page_object_number }) = widget.page else {
        return Err(EngineError::with_message(
            EngineErrorCode::NotFound,
            "the widget page could not be loaded",
        ));
    };
    let artwork = doc.open_sibling(pdf).map_err(|_| {
        EngineError::with_message(
            EngineErrorCode::MalformedPdf,
            "the appearance PDF could not be opened",
        )
    })?;
    doc.bake_widget_appearance(page_object_number, widget.annot_object_number, &artwork, page_index)
        .map_err(|step| match step {
            "page" => EngineError::with_message(
                EngineErrorCode::NotFound,
                "the widget page could not be loaded",
            ),
            "widget" => EngineError::with_message(
                EngineErrorCode::NotFound,
                "the signature widget could not be loaded",
            ),
            "draw" => EngineError::with_message(
                EngineErrorCode::InvalidArg,
                "the appearance page could not be drawn into the widget",
            ),
            _ => EngineError::with_message(
                EngineErrorCode::Unknown,
                "the appearance page could not be fitted into the widget",
            ),
        })
}

/// `SignatureMutator.prepare`. Answers the prepared record and its digest.
pub fn prepare(
    session: &DocumentSession,
    input: &SignaturePrepareInput,
    appearance_pdf: Option<&[u8]>,
    signing_id: String,
) -> Result<(SignaturePrepared, Vec<u8>), EngineError> {
    if let Some(id) = session.signing().borrow().pending_id() {
        return Err(EngineError::with_message(
            EngineErrorCode::SigningPending,
            format!("a signing is pending ({id}); complete or abort it first"),
        ));
    }
    let loaded = session.document()?.document();
    let field = resolve_field(loaded, &input.field)?;
    if field.signed {
        return Err(refused("the signature field is already signed"));
    }
    if input.appearance.is_some() && field.widget.is_none() {
        return Err(refused("the signature field has no widget to carry an appearance"));
    }
    let algorithm = input.digest.unwrap_or(DigestAlgorithm::Sha256);
    if algorithm == DigestAlgorithm::Sha1 {
        return Err(refused("unsupported digest 'sha1'"));
    }
    let kind = input.kind.unwrap_or(SignatureKind::Signature);
    let sub_filter = if kind == SignatureKind::Timestamp {
        "ETSI.RFC3161".to_string()
    } else {
        input.sub_filter.clone().unwrap_or_else(|| "ETSI.CAdES.detached".into())
    };
    let Some(subfilter) = subfilter_code(&sub_filter) else {
        return Err(refused(format!("unsupported subFilter '{sub_filter}'")));
    };
    let contents_size = input.contents_size.unwrap_or(DEFAULT_CONTENTS_SIZE as f64);
    if contents_size.fract() != 0.0
        || contents_size < MIN_CONTENTS_SIZE as f64
        || contents_size > MAX_CONTENTS_SIZE as f64
    {
        return Err(refused(format!(
            "contentsSize must be an integer in [{MIN_CONTENTS_SIZE}, {MAX_CONTENTS_SIZE}]"
        )));
    }
    let contents_size = contents_size as u64;
    let expected_version = version_ref(session, version(session)?.sha256);

    let candidate = open_bytes(session, &snapshot_bytes(session)?)?;
    let attribution =
        input.attribution.clone().or_else(|| input.signer.clone()).unwrap_or_default();
    let lock = input.lock.as_ref();
    let value_obj_num = candidate
        .prepare_signature(
            field.field_object_number,
            &SignaturePrepareOptions {
                subfilter,
                digest: algorithm.native() as i32,
                contents_size,
                name: attribution.name,
                reason: attribution.reason,
                location: attribution.location,
                contact_info: attribution.contact_info,
                signing_time: input.signing_time.clone(),
                docmdp_permission: input.certify.map_or(0, |c| i32::from(c.permission)),
                fieldmdp_action: lock.map_or(0, |l| field_action_code(l.action)),
                fieldmdp_fields: match lock {
                    Some(l) if l.action != FieldLockAction::All => l.fields.clone(),
                    _ => Vec::new(),
                },
                lock_permission: lock.and_then(|l| l.permission).map_or(0, i32::from),
            },
        )
        .map_err(|e| EngineError::with_message(EngineErrorCode::InvalidArg, e.to_string()))?;
    if value_obj_num == 0 {
        return Err(refused(
            "the engine refused to author the signature (the field is signed, read-only or locked, a certification is not allowed here, the seed value requires what is not implemented, or the request is inconsistent)",
        ));
    }
    if let (Some(appearance), Some(widget)) = (input.appearance, field.widget.as_ref()) {
        let pdf = appearance_pdf.ok_or_else(|| {
            EngineError::with_message(
                EngineErrorCode::WireFormat,
                "an appearance carries its PDF as payload 0",
            )
        })?;
        bake_widget_appearance(&candidate, widget, pdf, appearance.page_index.unwrap_or(0))?;
    }

    let mut saved = candidate.save_signature_candidate(value_obj_num).map_err(|_| {
        EngineError::with_message(EngineErrorCode::Unknown, "failed to save the signing candidate")
    })?;
    drop(candidate);
    let sealed =
        pdfium::seal_signature_candidate(&mut saved, algorithm.native()).ok_or_else(|| {
            EngineError::with_message(
                EngineErrorCode::Unknown,
                "failed to seal the signing candidate",
            )
        })?;
    let prepared = SignaturePrepared {
        signing_id,
        algorithm,
        byte_range: sealed.byte_range,
        contents_size,
        sub_filter,
        expected_version,
        expires_at: None,
    };
    session.signing().borrow_mut().pending = Some(PendingSigning {
        prepared: prepared.clone(),
        field_object_number: field.field_object_number,
        saved,
        contents_offset: sealed.contents_offset,
        contents_hex_length: sealed.contents_hex_length,
    });
    Ok((prepared, sealed.digest))
}

/// `SignatureMutator.complete`.
pub fn complete(
    session: &mut DocumentSession,
    input: &SignatureCompleteInput,
    cms: &[u8],
) -> Result<SignatureCompleteResult, EngineError> {
    let pending = session.signing().borrow().pending.clone();
    let Some(pending) = pending.filter(|p| p.prepared.signing_id == input.signing_id) else {
        let last = session.signing().borrow().last_completion.clone();
        if let Some(last) = last.filter(|l| l.signing_id == input.signing_id) {
            if last.cms != cms {
                return Err(refused("this signing already completed with a different CMS"));
            }
            return Ok(SignatureCompleteResult {
                status: CompletionStatus::AlreadyCompleted,
                ..last.result
            });
        }
        return Err(EngineError::with_message(
            EngineErrorCode::NotFound,
            format!("no pending signing '{}'", input.signing_id),
        ));
    };
    if pending.prepared.expected_version != input.expected_version {
        return Err(EngineError::with_message(
            EngineErrorCode::SigningVersionMismatch,
            "expectedVersion is not the version the candidate was prepared on",
        ));
    }
    if cms.first() != Some(&0x30) {
        return Err(refused("the CMS is not a DER SEQUENCE"));
    }
    if cms.len() as u64 > pending.prepared.contents_size {
        return Err(refused(format!(
            "the CMS ({} bytes) does not fit the reserved {} bytes",
            cms.len(),
            pending.prepared.contents_size
        )));
    }

    let mut bytes = pending.saved.bytes.clone();
    if !pdfium::write_signature_contents(
        &mut bytes,
        pending.contents_offset,
        pending.contents_hex_length,
        cms,
    ) {
        return Err(refused("the CMS is not one DER object that fits the reserved /Contents"));
    }
    {
        let sealed = open_bytes(session, &bytes)?;
        assert_sealed_signature(
            &sealed,
            pending.field_object_number,
            pending.prepared.byte_range,
            cms,
        )?;
    }

    // Install: the one place a live session changes its bytes.
    session.install(bytes)?;
    session.signing().borrow_mut().pending = None;

    let doc = session.document()?.document();
    let snapshot = read_snapshot(doc)?;
    let signature = snapshot
        .signatures
        .into_iter()
        .find(|s| s.field_object_number() == pending.field_object_number)
        .ok_or_else(|| {
            EngineError::with_message(
                EngineErrorCode::Unknown,
                "the installed document lost the signature field",
            )
        })?;
    let version = version(session)?;
    let pages: Vec<PageObjectNumber> =
        session.all_records()?.iter().map(|r| r.page_object_number).collect();
    let affected_pages =
        pages.into_iter().map(|pon| session.page_state(pon)).collect::<Result<Vec<_>, _>>()?;
    let result = SignatureCompleteResult {
        status: CompletionStatus::Completed,
        signature,
        version,
        previous: pending.prepared.expected_version.clone(),
        protection: snapshot.protection,
        meta: MutationMeta { affected_pages, cache_delta: None },
    };
    session.signing().borrow_mut().last_completion = Some(SigningCompletion {
        signing_id: input.signing_id.clone(),
        cms: cms.to_vec(),
        result: result.clone(),
    });
    Ok(result)
}

/// `SignatureMutator.abort`.
pub fn abort(session: &DocumentSession, signing_id: &str) -> SignatureAbortResult {
    let mut signing = session.signing().borrow_mut();
    if signing.pending_id() == Some(signing_id) {
        signing.pending = None;
        return SignatureAbortResult { status: AbortStatus::Aborted };
    }
    if signing.last_completion.as_ref().is_some_and(|l| l.signing_id == signing_id) {
        return SignatureAbortResult { status: AbortStatus::AlreadyCompleted };
    }
    SignatureAbortResult { status: AbortStatus::Unknown }
}

/// `FinalizeCandidateInput`, less the CMS, which travels as a payload.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FinalizeCandidateInput {
    /// The rebuilt candidate on this filesystem; patched in place.
    pub path: String,
    /// The `/ByteRange` the prepare reported.
    pub byte_range: [u64; 4],
    /// The reserved `/Contents` size the prepare reported.
    pub contents_size: u64,
    pub field_object_number: u32,
    #[serde(default)]
    pub password: Option<String>,
}

/// `FinalizedCandidate`: what the sealed FILE says, not what the writer
/// meant.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FinalizedCandidate {
    pub signature: SignatureDto,
    pub protection: DocumentProtection,
    /// The version the sealed file IS: hash and length of the whole file.
    pub version: BaseVersionInfo,
}

/// `CandidateFinalizer.finalize`: the session-less half of a durable
/// signing. The CMS is hex-encoded into the candidate file's `/Contents`
/// hole in place (only the hole is read and written), then the file is
/// opened like any other and read back through the signature model.
/// Nothing here touches a live session.
pub fn finalize_candidate(
    input: &FinalizeCandidateInput,
    cms: &[u8],
) -> Result<FinalizedCandidate, EngineError> {
    use std::io::{Read, Seek, SeekFrom, Write};

    let invalid = |message: String| EngineError::with_message(EngineErrorCode::InvalidArg, message);
    let path = std::path::Path::new(&input.path);
    let size = std::fs::metadata(path)
        .map_err(|e| {
            EngineError::with_message(
                EngineErrorCode::NotFound,
                format!("the candidate could not be opened: {e}"),
            )
        })?
        .len();
    let [r0, r1, r2, r3] = input.byte_range;
    if r0 != 0 || r1 >= r2 || r2.checked_add(r3) != Some(size) {
        return Err(invalid(format!(
            "byteRange [{r0} {r1} {r2} {r3}] does not span a {size}-byte candidate"
        )));
    }
    // The hole is `<` + hex digits + `>`: the range's gap, less the
    // delimiters.
    let hex_length = (r2 - r1).checked_sub(2);
    if hex_length != input.contents_size.checked_mul(2) {
        return Err(invalid(format!(
            "the /Contents hole holds {} hex digits, not the {} of contentsSize",
            (r2 - r1) as i128 - 2,
            i128::from(input.contents_size) * 2
        )));
    }
    let hex_length = hex_length.expect("equal to a checked product");
    if cms.first() != Some(&0x30) {
        return Err(refused("the CMS is not a DER SEQUENCE"));
    }
    if cms.len() as u64 > input.contents_size {
        return Err(refused(format!(
            "the CMS ({} bytes) does not fit the reserved {} bytes",
            cms.len(),
            input.contents_size
        )));
    }

    // `writeContentsIntoFile`: the hole alone, its delimiters checked so a
    // wrong offset patches nothing.
    let io = |e: std::io::Error| {
        EngineError::with_message(
            EngineErrorCode::Unknown,
            format!("patching the candidate failed: {e}"),
        )
    };
    let mut file = std::fs::OpenOptions::new().read(true).write(true).open(path).map_err(io)?;
    let mut span = vec![0u8; (hex_length + 2) as usize];
    file.seek(SeekFrom::Start(r1)).map_err(io)?;
    file.read_exact(&mut span).map_err(io)?;
    if span.first() != Some(&b'<') || span.last() != Some(&b'>') {
        return Err(invalid(
            "the byteRange gap is not a <\u{2026}> /Contents hole of the candidate".to_owned(),
        ));
    }
    if !pdfium::write_signature_contents(&mut span, 1, hex_length, cms) {
        return Err(refused("the CMS is not one DER object that fits the reserved /Contents"));
    }
    file.seek(SeekFrom::Start(r1)).map_err(io)?;
    file.write_all(&span).map_err(io)?;
    file.flush().map_err(io)?;
    drop(file);

    // Opened afresh, never through a session: the file is this attempt's
    // own.
    let access = pdfium::FileAccess::from_file(path).map_err(|e| {
        EngineError::with_message(
            EngineErrorCode::NotFound,
            format!("the candidate could not be opened: {e}"),
        )
    })?;
    let document =
        Document::from_file_access(Library::acquire(), &access, input.password.as_deref())
            .map_err(|e| {
                EngineError::with_message(EngineErrorCode::DocOpenFailed, e.to_string())
            })?;
    if document.is_encrypted() {
        document.set_runtime_owner_permissions(true);
    }
    assert_sealed_signature(&document, input.field_object_number, input.byte_range, cms)?;
    let snapshot = read_snapshot(&document)?;
    let signature = snapshot
        .signatures
        .into_iter()
        .find(|s| s.field_object_number() == input.field_object_number)
        .ok_or_else(|| refused("the sealed bytes lost the signature field"))?;
    let byte_length = document.base_bytes_size();
    let digest = super::reader::digest_range(
        &document,
        [0, byte_length, byte_length, 0],
        DigestAlgorithm::Sha256,
    )?;
    Ok(FinalizedCandidate {
        signature,
        protection: snapshot.protection,
        version: BaseVersionInfo { sha256: super::reader::to_hex(&digest), byte_length },
    })
}

/// `sealCheck.ts` `assertSealedSignature`: the sealed bytes carry a
/// whole-revision signature on the field, over the last revision, with the
/// sealed `/ByteRange` and exactly the CMS installed.
fn assert_sealed_signature(
    doc: &Document,
    field_object_number: u32,
    byte_range: [u64; 4],
    cms: &[u8],
) -> Result<(), EngineError> {
    let refuse = |why: &str| refused(format!("the sealed bytes {why}"));
    with_signature_model(doc, |model| {
        let signatures = read_signatures_from_model(model)?;
        let signature = signatures
            .iter()
            .find(|s| s.field_object_number() == field_object_number)
            .ok_or_else(|| refuse("lost the signature field"))?;
        let Some(range) = signature.byte_range.filter(|_| {
            signature.signed && signature.coverage == Some(SignatureCoverage::WholeRevision)
        }) else {
            return Err(refuse("do not carry a whole-revision signature on the field"));
        };
        if range != byte_range {
            let text = |r: [u64; 4]| r.iter().map(u64::to_string).collect::<Vec<_>>().join(" ");
            return Err(refuse(&format!(
                "carry /ByteRange [{}] where [{}] was sealed",
                text(range),
                text(byte_range)
            )));
        }
        if !model.is_revision_chain_valid() {
            return Err(refuse("have a broken cross-reference chain"));
        }
        let revisions = read_revisions(doc, &signatures)?;
        if signature.revision_index != Some(revisions.len().saturating_sub(1) as u32) {
            return Err(refuse("seal an earlier revision than the candidate"));
        }
        if read_contents_at(model, signature.index as i32)? != cms {
            return Err(refuse("carry /Contents that are not the CMS installed"));
        }
        Ok(())
    })
}

/// What `forms.setSignatureAppearance` answers: the field read back, and
/// the pages its widgets sit on.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureAppearanceResult {
    pub field: FormFieldDto,
    pub meta: MutationMeta,
}

/// `FormMutator.setSignatureAppearance`: draw page `page_index` of `pdf`
/// into every widget of an unsigned signature field.
pub fn set_signature_appearance(
    session: &mut DocumentSession,
    reference: &FormFieldRef,
    pdf: &[u8],
    page_index: i32,
    cancel: &CancelToken,
) -> Result<SignatureAppearanceResult, EngineError> {
    cancel.check()?;
    let (field_object_number, pages) = {
        let doc = session.document()?.document();
        let before = read_writable_field(doc, reference, cancel)?;
        let name = &before.base.name;
        if before.base.family != FormFieldFamily::Signature {
            let family = serde_json::to_value(before.base.family).unwrap_or_default();
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!(
                    "'{name}' is a {} field, not a signature field",
                    family.as_str().unwrap_or("unknown")
                ),
            ));
        }
        let field_object_number = before.base.field_object_number;
        let signed = with_signature_model(doc, |model| {
            Ok(read_signatures_from_model(model)?
                .iter()
                .any(|s| s.signed && s.field_object_number() == field_object_number))
        })?;
        if signed {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("'{name}' is signed; its appearance is sealed with the signature"),
            ));
        }
        let widgets = widget_refs(&before.kind);
        if widgets.is_empty() {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("'{name}' has no widget to draw into"),
            ));
        }
        for (annot, page) in &widgets {
            bake_widget_appearance(doc, &FormWidget::new(*annot, *page), pdf, page_index)?;
        }
        let mut pages: Vec<PageObjectNumber> = Vec::new();
        for (_, page) in widgets {
            if let Ok(page) = PageObjectNumber::new(page)
                && !pages.contains(&page)
            {
                pages.push(page);
            }
        }
        (field_object_number, pages)
    };
    session.note_mutation();
    for pon in &pages {
        session.bump_revision(*pon)?;
    }
    let field = read_writable_field(
        session.document()?.document(),
        &FormFieldRef::ObjectNumber { field: field_object_number },
        cancel,
    )?;
    let affected_pages =
        pages.into_iter().map(|pon| session.page_state(pon)).collect::<Result<Vec<_>, _>>()?;
    Ok(SignatureAppearanceResult {
        field,
        meta: MutationMeta { affected_pages, cache_delta: None },
    })
}
