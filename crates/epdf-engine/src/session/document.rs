//! One open document, and everything the engine knows about it.
//!
//! G3. Replaces `services/src/document-session/DocumentSession.ts` and the
//! session half of `lifecycle/PdfDocumentOpener.ts`.
//!
//! Owns the bytes, the page registry that maps index to durable object number,
//! the [`RevisionAuthority`], the [`PagePtrPool`] and the mutation sequence.
//! Both the browser worker and the native host build one the same way; only
//! the runtime underneath differs.
//!
//! # States
//!
//! A session is open, locked or closed, and the three are distinguishable
//! rather than collapsed into "not open". A locked session is one whose bytes
//! arrived but whose password did not: it occupies its slot like an open one
//! and holds the transferred bytes so an unlock attempt does not have to ship
//! them again. Everything except the password check refuses.
//!
//! # Every document is a layer
//!
//! `open.layerMemBase` and `open.layerFileBase` open a layer over a base from
//! the base-document registry ([`DocumentSession::open_layer_from`]), and
//! [`DocumentSession::open`] opens bytes as a fresh layer over a private base
//! (`openBytesAsLayer`). A layer document holds its base until after its own
//! handle closes.

use std::cell::Cell;
use std::rc::Rc;

use pdfium::{BaseDocument, Document, Library, PdfiumError, SharedPage};

use super::authority::SessionAuthority;
use super::pages::{PagePtrPool, PageRecord};
use super::revisions::RevisionAuthority;
use crate::access::OwnedDocument;
use crate::auth::scope::{DocCapability, decode_pdf_bits};
use crate::error::{EngineError, EngineErrorCode};
use crate::features::changes::ledger::ChangeLedger;
use crate::features::changes::record::ReverseStep;
use crate::features::signature::types::SignedDocumentPolicy;
use crate::identity::{OBJECT_NUMBER_ISSUE_LIMIT, ObjectNumberRange, PageObjectNumber};
use crate::revision::{PageState, RevisionToken, WeakAnnotationState};

/// How the underlying document was opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenKind {
    /// The whole file in memory, opened plain (`openFatMemoryDocument`).
    /// No open here makes one: `open.fatMem` opens its bytes as a layer
    /// (`openBytesAsLayer`).
    FatMemory,
    /// A base document plus a layer delta: every open, including
    /// `open.fatMem`, whose base is private to the session.
    Layer,
}

/// Who decides which object numbers a create may name: the open kinds'
/// `objectNumbers` (`worker-protocol.ts`). `Session`, the default, hands
/// numbers out itself (`objectNumbers.reserve`) and refuses one it does not
/// hold; `Caller` checked every number before sending the job (a server that
/// hands numbers to editing sessions).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ObjectNumberAuthority {
    #[default]
    Session,
    Caller,
}

/// What a layer opens from (`LayerOpenSource`, `worker-protocol.ts`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayerOpenSource {
    /// An empty layer.
    Fresh,
    /// The delta `EPDFLayer_SaveDelta` wrote, which names no base.
    RawDelta(Vec<u8>),
    /// A layer artifact, which PDFium refuses against any base but its own.
    Artifact(Vec<u8>),
    /// A layer artifact read from a file.
    ArtifactFile(std::path::PathBuf),
}

/// A session id, unique within this process.
///
/// Monotonic rather than random. The id exists for one job: a revision token
/// minted by one session must not validate against another. Uniqueness is the
/// whole requirement, a counter makes a collision structurally impossible, and
/// there is no adversary to hide it from because anyone in-process can call
/// the engine directly.
fn next_session_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    // Process-wide rather than thread-local: two owner threads must not mint
    // the same id, and a token that crossed between them would otherwise
    // validate where it should not.
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
    format!("sess_{n:x}")
}

/// The document half of a session: present when open, absent when locked.
#[derive(Debug)]
struct OpenState {
    document: OwnedDocument,
    kind: OpenKind,
    pages: PagePtrPool,
    revisions: RevisionAuthority,
    /// Index to object number, in display order. Populated lazily.
    records: Vec<PageRecord>,
    fully_enumerated: bool,
    /// The document's stamp drawings, by content (`DrawingIndex`): new with
    /// each document it describes.
    drawings: std::cell::RefCell<crate::features::annotation::write::stamp::DrawingIndex>,
}

/// One open, locked or closed document.
pub struct DocumentSession {
    session_id: String,
    open: Option<OpenState>,
    /// Bytes retained for a later unlock attempt. Locked sessions only.
    parked_bytes: Option<Vec<u8>>,
    /// The authority a parked open asked for, applied when it unlocks.
    parked_authority: Option<SessionAuthority>,
    /// A parked signing and the last completed one (`SignatureMutator.ts`).
    signing: std::cell::RefCell<crate::features::signature::mutator::SigningState>,
    /// The password the document opened with, for documents derived from its
    /// bytes (a signature's working copy, the sealed bytes it reloads onto).
    password: Option<String>,
    /// The mutation sequence the current bytes were loaded at
    /// (`DocumentSession.ts` `loadedSeq`).
    loaded_sequence: Cell<u64>,
    mutation_sequence: Cell<u64>,
    /// What this session's caller may do. Grants nothing until open sets it,
    /// so a session that failed to open cannot be read through.
    authority: SessionAuthority,
    /// Disposers that run when this session closes, in reverse order.
    ///
    /// `pages.insert` needs this: `FPDF_ImportPagesByIndex` does not fully
    /// detach imported objects from their source, so the source document and
    /// its bytes have to outlive every save of the destination.
    retained: Vec<Box<dyn FnOnce()>>,
    /// `DocumentSession.objectNumberAuthority`, set by the open.
    object_number_authority: ObjectNumberAuthority,
    /// `DocumentSession.signedDocumentPolicy`, set by the open. Kept here for
    /// the signature engine to enforce (Task 8.26).
    signed_document_policy: SignedDocumentPolicy,
    /// Numbers this session handed out that no committed create has used
    /// (`heldObjectNumbers`), as the ranges they were handed out in: a
    /// reserve can hand out millions.
    held_object_numbers: Vec<ObjectNumberRange>,
    /// The write transaction running, if any (see
    /// [`DocumentSession::begin_transaction`]).
    transaction: Option<OpenTransaction>,
    /// What this session keeps of its changes: every answer under its
    /// `opId`, and the records that undo them (`changeLedgerOf`).
    ledger: ChangeLedger,
    /// Whether a write on a layer session also answers the layer artifact
    /// (`persistLayerArtifact`). False for a session opened from bytes,
    /// whose caller holds the document and never asked for artifacts.
    persist_layer_artifact: bool,
}

/// What every meta of a write carries: its id, and whether it can be undone
/// (`WriteStamp`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteStamp {
    pub op_id: String,
    pub undoable: bool,
}

/// The write transaction a session is running (`OpenTransaction`).
///
/// Upstream runs every write inside an `EPDFLayer` transaction that its
/// abort drops whole. This engine opens no layer transaction around its
/// writes yet, so a transaction here keeps a journal of the reverse steps of
/// what it wrote instead, and an abort runs them backwards
/// ([`crate::features::changes::transaction`]). Task 8.30 extends it to
/// every write kind, with the layer ceiling check at commit.
#[derive(Debug)]
struct OpenTransaction {
    stamp: WriteStamp,
    journal: Vec<ReverseStep>,
    /// Held numbers the transaction made objects at (`objectNumbersInUse`):
    /// spent when it commits, free again when it aborts.
    numbers_in_use: Vec<u32>,
}

/// `objectNumberUnavailable`: a create named a number it can't have,
/// because the session doesn't hold it (`not-held`) or something has it
/// (`taken`). Upstream answers `ObjectNumberUnavailable`, a code this engine
/// gains with the rest of the drift in Task 8.27; until then the refusal is
/// `InvalidArg` with the same details.
pub fn object_number_unavailable(number: u32, reason: &str) -> EngineError {
    EngineError::with_message(
        EngineErrorCode::InvalidArg,
        format!("object number {number} is not available ({reason})"),
    )
    .with_details(serde_json::json!({ "field": "objectNumber", "objectNumber": number,
        "reason": reason }))
}

/// `bytes` as a fresh layer over a private memory base
/// (`WorkerHost.openBytesAsLayer`). The layer document holds its base, and
/// releases it after its own handle closes, so anything holding the document
/// (a pooled page) keeps the base alive too.
///
/// A missing password reports `DocPasswordRequired` and a wrong one
/// `DocPasswordIncorrect`, as a plain open does, so a locked document parks
/// and unlocks the same way.
fn open_bytes_as_layer(bytes: &[u8], password: Option<&str>) -> Result<Document, EngineError> {
    let base =
        BaseDocument::from_bytes(Library::acquire(), bytes, password).map_err(|e| match e {
            PdfiumError::Password if password.is_some() => EngineError::with_message(
                EngineErrorCode::DocPasswordIncorrect,
                "incorrect document password",
            ),
            PdfiumError::Password => EngineError::with_message(
                EngineErrorCode::DocPasswordRequired,
                "document requires a password",
            ),
            other => EngineError::with_message(EngineErrorCode::DocOpenFailed, other.to_string()),
        })?;
    let layer = Rc::new(base)
        .open_layer()
        .map_err(|e| EngineError::with_message(EngineErrorCode::DocOpenFailed, e.to_string()))?;
    Ok(layer.into_parts().0)
}

impl DocumentSession {
    pub fn new() -> Self {
        Self::with_id(next_session_id())
    }

    pub fn with_id(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            open: None,
            parked_bytes: None,
            parked_authority: None,
            signing: Default::default(),
            password: None,
            loaded_sequence: Cell::new(0),
            mutation_sequence: Cell::new(0),
            authority: SessionAuthority::default(),
            retained: Vec::new(),
            object_number_authority: ObjectNumberAuthority::default(),
            signed_document_policy: SignedDocumentPolicy::default(),
            held_object_numbers: Vec::new(),
            transaction: None,
            ledger: ChangeLedger::new(),
            persist_layer_artifact: true,
        }
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn kind(&self) -> Option<OpenKind> {
        self.open.as_ref().map(|state| state.kind)
    }

    /// Whether a write answers the layer artifact (`persistLayerArtifact`):
    /// a layer session that was not opened from bytes.
    pub fn persists_layer_artifact(&self) -> bool {
        self.kind() == Some(OpenKind::Layer) && self.persist_layer_artifact
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// True while the session holds bytes it could not open without a password.
    pub fn is_locked(&self) -> bool {
        self.open.is_none() && self.parked_bytes.is_some()
    }

    /// Open a document from bytes.
    ///
    /// A missing password reports `DocPasswordRequired` and a wrong one
    /// `DocPasswordIncorrect`, rather than both collapsing into a generic open
    /// failure. A prompt can act on the first; only the second means the
    /// user got it wrong.
    /// What this session's caller may do.
    pub fn authority(&self) -> &SessionAuthority {
        &self.authority
    }

    /// Refuse unless the capability is granted. The native enforcement point.
    pub fn require(&self, capability: DocCapability) -> Result<(), EngineError> {
        self.authority.require(capability)
    }

    /// Refuse unless at least one of the capabilities is granted.
    pub fn require_any(&self, capabilities: &[DocCapability]) -> Result<(), EngineError> {
        self.authority.require_any(capabilities)
    }

    /// Open with full authority.
    ///
    /// For a host with no notion of scopes: the smoke apps and the conformance
    /// harness. Named rather than spelled as a wildcard at each call site, so
    /// grepping finds every place that opted out of enforcement.
    pub fn open(&mut self, bytes: Vec<u8>, password: Option<&str>) -> Result<(), EngineError> {
        self.open_with(bytes, password, SessionAuthority::unrestricted())
    }

    /// Open under a caller's authority.
    ///
    /// The bytes open as a fresh layer over a private memory base
    /// (`WorkerHost.openBytesAsLayer`): reads fall through to the base, a
    /// write promotes only what it touches, and the session hands out object
    /// numbers past the file's. The session's kind is [`OpenKind::Layer`],
    /// but a write answers no artifact: the caller holds the document, not
    /// a layer (`persistLayerArtifact = false`).
    pub fn open_with(
        &mut self,
        bytes: Vec<u8>,
        password: Option<&str>,
        authority: SessionAuthority,
    ) -> Result<(), EngineError> {
        if self.open.is_some() {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                "document already open",
            ));
        }
        let document = open_bytes_as_layer(&bytes, password)?;

        // PDFium's own subsystems, forms especially, consult the permission
        // bits and would otherwise refuse work this engine has already
        // authorized. The fork's header is explicit that this proves nothing
        // and authorizes nobody: "Callers must enforce JWT/PDF permission
        // policy outside PDFium." `self.authority` below is that enforcement,
        // and elevating without it would hand every caller an owner-unlocked
        // document.
        if document.is_encrypted() {
            document.set_runtime_owner_permissions(true);
        }

        // The declared permission word, for `pdf.permissions` expansion. Read
        // before the elevation above would make it meaningless: after it,
        // `permissions()` reports the owner word for every encrypted document.
        let mut authority = authority;
        authority.set_pdf_bits(decode_pdf_bits(Some(i64::from(document.user_permissions()))));
        authority.set_protection(document_protection(&document));

        self.open = Some(OpenState {
            document: OwnedDocument::new(document)?,
            kind: OpenKind::Layer,
            pages: PagePtrPool::new(),
            revisions: RevisionAuthority::new(self.session_id.clone()),
            records: Vec::new(),
            fully_enumerated: false,
            drawings: Default::default(),
        });
        self.authority = authority;
        self.parked_bytes = None;
        self.password = password.map(str::to_owned);
        self.persist_layer_artifact = false;
        self.loaded_sequence.set(self.mutation_sequence.get());
        Ok(())
    }

    /// Open a fresh layer over a shared base document.
    ///
    /// `open.layerMemBase`, for a base already registered with
    /// [`super::base_registry::BaseDocumentRegistry`]. The layer is an
    /// ordinary document; what makes it a layer is that its storage belongs to
    /// the base, which several sessions share.
    ///
    /// The layer document holds the base and releases it after its own handle
    /// closes, so the base outlives the document and every page still holding
    /// it. Getting that order wrong leaves the layer reading freed memory.
    pub fn open_layer(
        &mut self,
        base: Rc<BaseDocument>,
        authority: SessionAuthority,
    ) -> Result<(), EngineError> {
        self.open_layer_from(base, &LayerOpenSource::Fresh, authority)
    }

    /// [`DocumentSession::open_layer`] from what the layer starts as
    /// (`LayerOpenSource`): empty, a raw delta, or an artifact.
    pub fn open_layer_from(
        &mut self,
        base: Rc<BaseDocument>,
        source: &LayerOpenSource,
        authority: SessionAuthority,
    ) -> Result<(), EngineError> {
        if self.open.is_some() {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                "document already open",
            ));
        }
        let access = |made: Result<pdfium::FileAccess, PdfiumError>| {
            made.map_err(|e| {
                EngineError::with_message(
                    EngineErrorCode::DocOpenFailed,
                    format!("the layer could not be read: {e}"),
                )
            })
        };
        let opened = match source {
            LayerOpenSource::Fresh => base.open_layer(),
            LayerOpenSource::RawDelta(bytes) => {
                base.open_layer_delta(&access(pdfium::FileAccess::from_bytes(bytes.clone()))?)
            }
            LayerOpenSource::Artifact(bytes) => {
                base.open_layer_artifact(&access(pdfium::FileAccess::from_bytes(bytes.clone()))?)
            }
            LayerOpenSource::ArtifactFile(path) => {
                base.open_layer_artifact(&access(pdfium::FileAccess::from_file(path))?)
            }
        };
        let layer = opened.map_err(|e| {
            let code = match e {
                pdfium::LayerOpenError::PasswordRequired => EngineErrorCode::DocPasswordRequired,
                pdfium::LayerOpenError::MalformedDelta | pdfium::LayerOpenError::BaseMismatch => {
                    EngineErrorCode::MalformedPdf
                }
                pdfium::LayerOpenError::OpenFailed => EngineErrorCode::DocOpenFailed,
            };
            EngineError::with_message(code, e.to_string())
        })?;
        drop(base);
        // The layer document holds its base until after its handle closes.
        let (document, _) = layer.into_parts();

        let mut authority = authority;
        authority.set_pdf_bits(decode_pdf_bits(Some(i64::from(document.user_permissions()))));
        authority.set_protection(document_protection(&document));

        self.open = Some(OpenState {
            document: OwnedDocument::new(document)?,
            kind: OpenKind::Layer,
            pages: PagePtrPool::new(),
            revisions: RevisionAuthority::new(self.session_id.clone()),
            records: Vec::new(),
            fully_enumerated: false,
            drawings: Default::default(),
        });
        self.authority = authority;
        self.parked_bytes = None;
        self.loaded_sequence.set(self.mutation_sequence.get());
        Ok(())
    }

    /// Park the session in the password-locked state.
    ///
    /// The bytes are kept so an unlock does not have to transfer them again,
    /// which for a large document is the expensive half of opening it.
    pub fn park_locked(&mut self, bytes: Vec<u8>) -> Result<(), EngineError> {
        if self.open.is_some() {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                "document already open",
            ));
        }
        self.parked_bytes = Some(bytes);
        Ok(())
    }

    /// [`DocumentSession::park_locked`], remembering the authority the open
    /// asked for so the unlock grants the same and no more.
    pub fn park_locked_under(
        &mut self,
        bytes: Vec<u8>,
        authority: SessionAuthority,
    ) -> Result<(), EngineError> {
        self.park_locked(bytes)?;
        self.parked_authority = Some(authority);
        Ok(())
    }

    /// Unlock a parked session under the authority it was parked with, or
    /// full authority when it was parked without one.
    pub fn unlock_parked(&mut self, password: &str) -> Result<(), EngineError> {
        let authority =
            self.parked_authority.clone().unwrap_or_else(SessionAuthority::unrestricted);
        self.unlock_with(password, authority)?;
        self.parked_authority = None;
        Ok(())
    }

    /// The bytes a locked session is holding.
    pub fn locked_bytes(&self) -> Result<&[u8], EngineError> {
        self.parked_bytes.as_deref().ok_or_else(|| {
            EngineError::with_message(EngineErrorCode::DocNotOpen, "document session is not locked")
        })
    }

    /// Unlock a parked session with a password, consuming the parked bytes.
    pub fn unlock(&mut self, password: &str) -> Result<(), EngineError> {
        self.unlock_with(password, SessionAuthority::unrestricted())
    }

    /// Unlock under a caller's authority.
    pub fn unlock_with(
        &mut self,
        password: &str,
        authority: SessionAuthority,
    ) -> Result<(), EngineError> {
        let bytes = self.parked_bytes.take().ok_or_else(|| {
            EngineError::with_message(EngineErrorCode::DocNotOpen, "document session is not locked")
        })?;
        match self.open_with(bytes.clone(), Some(password), authority) {
            Ok(()) => Ok(()),
            Err(error) => {
                // A wrong password leaves the session locked rather than
                // closing it, so the caller can prompt again without
                // retransferring the document.
                self.parked_bytes = Some(bytes);
                Err(error)
            }
        }
    }

    fn state(&self) -> Result<&OpenState, EngineError> {
        self.open.as_ref().ok_or_else(|| {
            if self.parked_bytes.is_some() {
                EngineError::with_message(
                    EngineErrorCode::DocPasswordRequired,
                    "document is locked and requires a password",
                )
            } else {
                EngineError::with_message(EngineErrorCode::DocNotOpen, "document is not open")
            }
        })
    }

    fn state_mut(&mut self) -> Result<&mut OpenState, EngineError> {
        if self.open.is_none() {
            // Re-run the read path for its error, which distinguishes locked
            // from closed.
            self.state()?;
        }
        Ok(self.open.as_mut().expect("checked above"))
    }

    pub fn document(&self) -> Result<&OwnedDocument, EngineError> {
        Ok(&self.state()?.document)
    }

    /// The open document's stamp drawings, by content (`drawingIndex`).
    pub fn drawing_index(
        &self,
    ) -> Result<
        &std::cell::RefCell<crate::features::annotation::write::stamp::DrawingIndex>,
        EngineError,
    > {
        Ok(&self.state()?.drawings)
    }

    pub fn page_count(&self) -> Result<u32, EngineError> {
        Ok(self.state()?.document.page_count())
    }

    /// Enumerate every page once, caching index to object number.
    ///
    /// A page with no indirect object number makes the document unusable for
    /// identity, so it is refused here with a message that says what is wrong
    /// rather than routed through a weak identity path. ISO 32000-1 7.7.3.3
    /// requires every `/Page` to be referenced indirectly from the page tree;
    /// PDFium's loader is permissive enough to surface direct page dicts from
    /// broken generators.
    pub fn ensure_full_page_registry(&mut self) -> Result<(), EngineError> {
        let state = self.state_mut()?;
        if state.fully_enumerated {
            return Ok(());
        }
        let count = state.document.page_count();
        let mut records = Vec::with_capacity(count as usize);
        for index in 0..count {
            let page = state.document.document().page(index).map_err(|e| {
                EngineError::with_message(EngineErrorCode::RuntimeUnavailable, e.to_string())
            })?;
            let number = PageObjectNumber::new(page.object_number()).map_err(|_| {
                EngineError::with_message(
                    EngineErrorCode::MalformedPdf,
                    format!(
                        "page at index {index} is a direct (non-indirect) PDF object; \
                         the engine requires every page to have a stable indirect object number"
                    ),
                )
            })?;
            records.push(PageRecord { page_object_number: number, page_index: index });
        }
        state.records = records;
        state.fully_enumerated = true;
        Ok(())
    }

    /// The record for one page, enumerating if the cache has not been filled.
    pub fn record_by_object_number(
        &mut self,
        page_object_number: PageObjectNumber,
    ) -> Result<PageRecord, EngineError> {
        self.ensure_full_page_registry()?;
        self.state()?
            .records
            .iter()
            .find(|record| record.page_object_number == page_object_number)
            .copied()
            .ok_or_else(|| {
                EngineError::with_message(
                    EngineErrorCode::NotFound,
                    format!("no page with object number {page_object_number}"),
                )
            })
    }

    /// Every page record, in display order.
    pub fn all_records(&mut self) -> Result<&[PageRecord], EngineError> {
        self.ensure_full_page_registry()?;
        Ok(&self.state()?.records)
    }

    /// Drop the index-to-object-number cache and rebuild it.
    ///
    /// Called after an operation shuffles page positions. Revision counters
    /// and weak-annotation knowledge are keyed by durable object number and
    /// survive a reorder, so they are deliberately left alone.
    pub fn refresh_page_registry(&mut self) -> Result<(), EngineError> {
        let state = self.state_mut()?;
        state.document.reindex()?;
        state.records.clear();
        state.fully_enumerated = false;
        self.ensure_full_page_registry()
    }

    /// A page from the pool, shared with any other holder.
    pub fn acquire_page(
        &self,
        page_object_number: PageObjectNumber,
    ) -> Result<Rc<SharedPage>, EngineError> {
        let state = self.state()?;
        state.pages.acquire(&state.document, page_object_number)
    }

    /// How many pages currently have a holder.
    pub fn held_page_count(&self) -> usize {
        self.open.as_ref().map_or(0, |state| state.pages.held_count())
    }

    pub fn is_page_held(&self, page_object_number: PageObjectNumber) -> bool {
        self.open.as_ref().is_some_and(|state| state.pages.is_held(page_object_number))
    }

    /// The page pool of an open session: its working sets and their order.
    pub fn page_pool(&self) -> Option<&PagePtrPool> {
        self.open.as_ref().map(|state| &state.pages)
    }

    /// Per-page liveness, for annotation reads and mutation results.
    pub fn page_state(
        &mut self,
        page_object_number: PageObjectNumber,
    ) -> Result<PageState, EngineError> {
        // Validates that the page exists; liveness carries no display order,
        // which lives in the page layout instead.
        self.record_by_object_number(page_object_number)?;
        let state = self.state()?;
        Ok(PageState {
            page_object_number,
            revision: state.revisions.token(page_object_number),
            weak_annotation_state: state.revisions.weak_annotation_state(page_object_number),
        })
    }

    pub fn weak_annotation_state(
        &self,
        page_object_number: PageObjectNumber,
    ) -> Result<WeakAnnotationState, EngineError> {
        Ok(self.state()?.revisions.weak_annotation_state(page_object_number))
    }

    pub fn record_weak_annotation_state(
        &mut self,
        page_object_number: PageObjectNumber,
        weak: WeakAnnotationState,
    ) -> Result<(), EngineError> {
        self.state_mut()?.revisions.record_weak_annotation_state(page_object_number, weak);
        Ok(())
    }

    pub fn record_weak_flag(
        &mut self,
        page_object_number: PageObjectNumber,
        has_weak: bool,
    ) -> Result<(), EngineError> {
        self.record_weak_annotation_state(
            page_object_number,
            WeakAnnotationState::Known { has_any_weak_annotations: has_weak },
        )
    }

    pub fn bump_revision(
        &mut self,
        page_object_number: PageObjectNumber,
    ) -> Result<RevisionToken, EngineError> {
        Ok(self.state_mut()?.revisions.bump(page_object_number))
    }

    pub fn validate_revision(&self, token: &RevisionToken) -> Result<(), EngineError> {
        self.state()?.revisions.validate(token)
    }

    /// Forget a deleted page's per-session state.
    pub fn drop_page_state(
        &mut self,
        page_object_number: PageObjectNumber,
    ) -> Result<(), EngineError> {
        self.state_mut()?.revisions.drop_page(page_object_number);
        Ok(())
    }

    /// Monotonic count of successful mutations in this session.
    ///
    /// The version key for a detached snapshot cache such as the form model:
    /// an entry built at sequence N is valid exactly while the sequence is
    /// still N. Coarse on purpose, because widgets are annotations and page
    /// operations move widgets, so any mutation may invalidate derived form
    /// state. Per-domain counters are a later optimization.
    pub fn mutation_sequence(&self) -> u64 {
        self.mutation_sequence.get()
    }

    /// `hasUnsavedEdits`: a mutation since the current bytes were loaded.
    pub fn has_unsaved_edits(&self) -> bool {
        self.mutation_sequence.get() != self.loaded_sequence.get()
    }

    /// The password the document opened with.
    pub fn password(&self) -> Option<&str> {
        self.password.as_deref()
    }

    /// The session's signing state.
    pub fn signing(
        &self,
    ) -> &std::cell::RefCell<crate::features::signature::mutator::SigningState> {
        &self.signing
    }

    /// `DocumentSession.install`: move the session onto new bytes (a sealed
    /// signature), opened under its password. Every page it had is re-pinned
    /// (its revision bumped) and the bytes count as loaded.
    ///
    /// The sealed bytes open as a fresh layer over their own memory base, as
    /// upstream's candidate does, so the session stays a layer. A session
    /// that was not one persists no artifact from here on
    /// (`SignatureMutator.verifySealed`); a layer keeps what it had.
    pub fn install(&mut self, bytes: Vec<u8>) -> Result<(), EngineError> {
        let document = open_bytes_as_layer(&bytes, self.password.as_deref())
            .map_err(|e| EngineError::with_message(EngineErrorCode::DocOpenFailed, e.message))?;
        if self.kind() != Some(OpenKind::Layer) {
            self.persist_layer_artifact = false;
        }
        if document.is_encrypted() {
            document.set_runtime_owner_permissions(true);
        }
        let protection = document_protection(&document);
        let document = OwnedDocument::new(document)?;
        self.authority.set_protection(protection);
        let state = self.state_mut()?;
        let previous: Vec<PageObjectNumber> =
            state.records.iter().map(|r| r.page_object_number).collect();
        state.pages.close_all();
        state.pages = PagePtrPool::new();
        state.document = document;
        state.kind = OpenKind::Layer;
        state.records.clear();
        state.fully_enumerated = false;
        state.drawings.get_mut().forget();
        for pon in previous {
            state.revisions.bump(pon);
        }
        self.mutation_sequence.set(self.mutation_sequence.get().saturating_add(1));
        self.loaded_sequence.set(self.mutation_sequence.get());
        // The new version's objects may sit at numbers this session held.
        self.held_object_numbers.clear();
        if let Some(open) = self.transaction.as_mut() {
            open.numbers_in_use.clear();
        }
        Ok(())
    }

    /// Record one successful mutation. Saturating, for the reason in
    /// [`RevisionAuthority::bump`].
    pub fn note_mutation(&self) {
        self.mutation_sequence.set(self.mutation_sequence.get().saturating_add(1));
    }

    /// Park a disposer to run when this session closes.
    pub fn retain_until_close(&mut self, dispose: impl FnOnce() + 'static) {
        self.retained.push(Box::new(dispose));
    }

    /// Close the session. Idempotent.
    ///
    /// Retained disposers run last and in reverse order: whatever they release
    /// was referenced by the document, which is gone by then.
    ///
    /// Does not close a page somebody is still holding. The pool hands out
    /// counted references, so a page outlives this call if it has to, and
    /// [`DocumentSession::held_page_count`] reports how many did.
    pub fn close(&mut self) {
        if let Some(mut state) = self.open.take() {
            state.pages.close_all();
            state.revisions.clear();
            state.records.clear();
            // `state` drops here, which drops the `OwnedDocument` and closes
            // the PDFium handle, on this thread.
        }
        self.parked_bytes = None;
        self.transaction = None;
        self.ledger.clear();
        // Back to granting nothing, so a closed session cannot be read through
        // even if a caller holds a reference to it.
        self.authority = SessionAuthority::default();
        for dispose in std::mem::take(&mut self.retained).into_iter().rev() {
            dispose();
        }
    }
}

impl std::fmt::Debug for DocumentSession {
    /// Says what state the session is in and nothing about the document.
    ///
    /// Reaching the contents from a formatter would defeat the confinement
    /// this type exists to hold, and a closure list has nothing to print.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DocumentSession")
            .field("session_id", &self.session_id)
            .field("open", &self.is_open())
            .field("locked", &self.is_locked())
            .field("held_pages", &self.held_page_count())
            .field("mutation_sequence", &self.mutation_sequence())
            .field("retained", &self.retained.len())
            .finish()
    }
}

/// Transactions (`DocumentSession.ts`, "transactions") and the change
/// ledger.
impl DocumentSession {
    /// Whether a write transaction is open.
    pub fn in_transaction(&self) -> bool {
        self.transaction.is_some()
    }

    /// Open a write transaction for the write `op_id` names. Until
    /// [`DocumentSession::commit_transaction`] or
    /// [`DocumentSession::abort_transaction`], what it writes can be taken
    /// back. One at a time.
    pub fn begin_transaction(&mut self, op_id: impl Into<String>) -> Result<(), EngineError> {
        if self.transaction.is_some() {
            return Err(EngineError::with_message(
                EngineErrorCode::Unknown,
                "a transaction is already open",
            ));
        }
        self.document()?;
        self.transaction = Some(OpenTransaction {
            stamp: WriteStamp { op_id: op_id.into(), undoable: false },
            journal: Vec::new(),
            numbers_in_use: Vec::new(),
        });
        Ok(())
    }

    /// What every meta of the running write carries: its `opId`, and
    /// whether it can be undone.
    pub fn write_stamp(&self) -> Result<WriteStamp, EngineError> {
        Ok(self.require_transaction()?.stamp.clone())
    }

    /// The running write records its reverse, so `{ undoOf: opId }` can
    /// undo it.
    pub fn mark_undoable(&mut self) -> Result<(), EngineError> {
        self.require_transaction_mut()?.stamp.undoable = true;
        Ok(())
    }

    /// Note the steps that reverse what the running write just wrote, in the
    /// order they run. An abort runs every step noted, last first.
    pub fn journal(&mut self, steps: &[ReverseStep]) -> Result<(), EngineError> {
        self.require_transaction_mut()?.journal.extend_from_slice(steps);
        Ok(())
    }

    /// Keep everything the open transaction wrote.
    pub fn commit_transaction(&mut self) -> Result<(), EngineError> {
        self.require_transaction()?;
        let spent = self.transaction.take().map(|open| open.numbers_in_use).unwrap_or_default();
        for number in spent {
            self.release_held(number);
        }
        Ok(())
    }

    /// `claimObjectNumber`: a create in the open transaction takes
    /// `number`, one the session holds when it hands numbers out itself, and
    /// that no earlier create of the transaction took. Whether another
    /// object has it the create finds out.
    pub fn claim_object_number(&mut self, number: u32) -> Result<(), EngineError> {
        if number < 1 {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("{number} is not an object number"),
            )
            .with_details(serde_json::json!({ "field": "objectNumber" })));
        }
        if self.object_number_authority == ObjectNumberAuthority::Session
            && !self.holds_object_number(number)
        {
            return Err(object_number_unavailable(number, "not-held"));
        }
        let open = self.require_transaction_mut()?;
        if open.numbers_in_use.contains(&number) {
            return Err(object_number_unavailable(number, "taken"));
        }
        open.numbers_in_use.push(number);
        Ok(())
    }

    /// Stop holding `number`: a committed create spent it.
    fn release_held(&mut self, number: u32) {
        let mut kept = Vec::with_capacity(self.held_object_numbers.len() + 1);
        for range in self.held_object_numbers.drain(..) {
            if number < range.first || number - range.first >= range.count {
                kept.push(range);
                continue;
            }
            let before = number - range.first;
            if before > 0 {
                kept.push(ObjectNumberRange { first: range.first, count: before });
            }
            let after = range.count - before - 1;
            if after > 0 {
                kept.push(ObjectNumberRange { first: number + 1, count: after });
            }
        }
        self.held_object_numbers = kept;
    }

    /// The steps that take back what the open transaction wrote, in the
    /// order they were noted, leaving the transaction open with an empty
    /// journal so the steps can run inside it. The caller runs them last
    /// first, then aborts ([`crate::features::changes::transaction`]).
    pub fn take_journal(&mut self) -> Vec<ReverseStep> {
        self.transaction.as_mut().map(|open| std::mem::take(&mut open.journal)).unwrap_or_default()
    }

    /// Close the open transaction without keeping it. What it wrote must
    /// already have been taken back ([`DocumentSession::take_journal`]).
    pub fn abort_transaction(&mut self) {
        self.transaction = None;
    }

    /// This session's change ledger.
    pub fn ledger(&self) -> &ChangeLedger {
        &self.ledger
    }

    pub fn ledger_mut(&mut self) -> &mut ChangeLedger {
        &mut self.ledger
    }

    fn require_transaction(&self) -> Result<&OpenTransaction, EngineError> {
        self.transaction.as_ref().ok_or_else(no_transaction)
    }

    fn require_transaction_mut(&mut self) -> Result<&mut OpenTransaction, EngineError> {
        self.transaction.as_mut().ok_or_else(no_transaction)
    }
}

fn no_transaction() -> EngineError {
    EngineError::with_message(EngineErrorCode::Unknown, "no transaction is open")
}

/// Object numbers (`DocumentSession.ts`, "object numbers"): the layer's last
/// object number, read, raised and handed out.
impl DocumentSession {
    pub fn object_number_authority(&self) -> ObjectNumberAuthority {
        self.object_number_authority
    }

    pub fn set_object_number_authority(&mut self, authority: ObjectNumberAuthority) {
        self.object_number_authority = authority;
    }

    pub fn signed_document_policy(&self) -> SignedDocumentPolicy {
        self.signed_document_policy
    }

    pub fn set_signed_document_policy(&mut self, policy: SignedDocumentPolicy) {
        self.signed_document_policy = policy;
    }

    /// `lastObjectNumber()`: every object the layer has is at or below it.
    /// `None` for a session whose document is not a layer, which no open
    /// makes.
    pub fn last_object_number(&self) -> Result<Option<u32>, EngineError> {
        let state = self.state()?;
        if state.kind != OpenKind::Layer {
            return Ok(None);
        }
        Ok(Some(state.document.document().layer_last_object_number()))
    }

    /// `raiseLastObjectNumber`: raise the layer's last object number to at
    /// least `last`, so the objects the next write makes for itself land
    /// above it. A raise outlives an abort. Refused with `LayerFull` past the
    /// parser's limit.
    pub fn raise_last_object_number(&self, last: u32) -> Result<(), EngineError> {
        let state = self.state()?;
        let document = state.document.document();
        if !document.raise_layer_last_object_number(last) {
            return Err(EngineError::with_message(
                EngineErrorCode::LayerFull,
                format!("the document would pass object number {last}"),
            )
            .with_details(
                serde_json::json!({ "lastObjectNumber": document.layer_last_object_number() }),
            ));
        }
        Ok(())
    }

    /// `reserveObjectNumbers`: hand out `count` more numbers. The layer's
    /// last object number moves up by `count`, so nothing the engine makes
    /// lands on them, and they are this session's to name objects with.
    pub fn reserve_object_numbers(&mut self, count: u64) -> Result<ObjectNumberRange, EngineError> {
        if self.object_number_authority != ObjectNumberAuthority::Session {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                "object numbers are handed out by the caller, not this session",
            ));
        }
        if count < 1 {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("cannot reserve {count} object numbers"),
            )
            .with_details(serde_json::json!({ "field": "count" })));
        }
        let state = self.state()?;
        let document = state.document.document();
        let last = document.layer_last_object_number();
        if u64::from(last).saturating_add(count) > u64::from(OBJECT_NUMBER_ISSUE_LIMIT) {
            return Err(EngineError::with_message(
                EngineErrorCode::LayerFull,
                "no more object numbers to hand out",
            )
            .with_details(serde_json::json!({ "lastObjectNumber": last })));
        }
        // Within the issue limit, so it fits a u32.
        let count = count as u32;
        if !document.raise_layer_last_object_number(last + count) {
            return Err(EngineError::with_message(
                EngineErrorCode::Unknown,
                "EPDFLayer_RaiseLastObjectNumber refused",
            ));
        }
        let range = ObjectNumberRange { first: last + 1, count };
        self.held_object_numbers.push(range);
        Ok(range)
    }

    /// Whether this session holds `object_number` (handed out, not yet
    /// spent). For the creates that name one (8.14, 8.18, 8.21).
    pub fn holds_object_number(&self, object_number: u32) -> bool {
        self.held_object_numbers
            .iter()
            .any(|range| object_number >= range.first && object_number - range.first < range.count)
    }
}

impl Default for DocumentSession {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for DocumentSession {
    fn drop(&mut self) {
        self.close();
    }
}

/// What a document's signatures forbid, for its authority. A document
/// whose signature model cannot be built is not known to be protected.
fn document_protection(
    document: &Document,
) -> Option<crate::features::signature::types::DocumentProtection> {
    crate::features::signature::reader::read_protection(document)
        .ok()
        .filter(|p| p.judged.is_some())
}
