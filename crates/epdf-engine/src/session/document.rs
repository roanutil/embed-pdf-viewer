//! One open document, and everything the engine knows about it.
//!
//! G3. Replaces `services/src/document-session/DocumentSession.ts` and the
//! fat-memory half of `lifecycle/PdfDocumentOpener.ts`.
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
//! # What is not here yet
//!
//! Layer documents. `open.layerMemBase` and `open.layerFileBase` need the
//! `EPDFLayer_*` calls and a base-document registry, and they are their own
//! piece of work; [`DocumentSession::open`] is the fat-memory path only. The
//! `kind` field exists so adding them does not change this type's shape.

use std::cell::Cell;
use std::rc::Rc;

use pdfium::{Document, Library, PdfiumError, SharedPage};

use super::pages::{PagePtrPool, PageRecord};
use super::revisions::RevisionAuthority;
use crate::access::OwnedDocument;
use crate::error::{EngineError, EngineErrorCode};
use crate::identity::PageObjectNumber;
use crate::revision::{PageState, RevisionToken, WeakAnnotationState};

/// How the underlying document was opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenKind {
    /// The whole file in memory, which is every open today.
    FatMemory,
    /// A base document plus a layer delta. Reserved; see the module docs.
    Layer,
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
}

/// One open, locked or closed document.
pub struct DocumentSession {
    session_id: String,
    open: Option<OpenState>,
    /// Bytes retained for a later unlock attempt. Locked sessions only.
    parked_bytes: Option<Vec<u8>>,
    mutation_sequence: Cell<u64>,
    /// Disposers that run when this session closes, in reverse order.
    ///
    /// `pages.insert` needs this: `FPDF_ImportPagesByIndex` does not fully
    /// detach imported objects from their source, so the source document and
    /// its bytes have to outlive every save of the destination.
    retained: Vec<Box<dyn FnOnce()>>,
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
            mutation_sequence: Cell::new(0),
            retained: Vec::new(),
        }
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn kind(&self) -> Option<OpenKind> {
        self.open.as_ref().map(|state| state.kind)
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
    pub fn open(&mut self, bytes: Vec<u8>, password: Option<&str>) -> Result<(), EngineError> {
        if self.open.is_some() {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                "document already open",
            ));
        }
        let document =
            Document::from_bytes(Library::acquire(), &bytes, password).map_err(|e| match e {
                PdfiumError::Password if password.is_some() => EngineError::with_message(
                    EngineErrorCode::DocPasswordIncorrect,
                    "incorrect document password",
                ),
                PdfiumError::Password => EngineError::with_message(
                    EngineErrorCode::DocPasswordRequired,
                    "document requires a password",
                ),
                other => {
                    EngineError::with_message(EngineErrorCode::DocOpenFailed, other.to_string())
                }
            })?;

        self.open = Some(OpenState {
            document: OwnedDocument::new(document)?,
            kind: OpenKind::FatMemory,
            pages: PagePtrPool::new(),
            revisions: RevisionAuthority::new(self.session_id.clone()),
            records: Vec::new(),
            fully_enumerated: false,
        });
        self.parked_bytes = None;
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

    /// The bytes a locked session is holding.
    pub fn locked_bytes(&self) -> Result<&[u8], EngineError> {
        self.parked_bytes.as_deref().ok_or_else(|| {
            EngineError::with_message(EngineErrorCode::DocNotOpen, "document session is not locked")
        })
    }

    /// Unlock a parked session with a password, consuming the parked bytes.
    pub fn unlock(&mut self, password: &str) -> Result<(), EngineError> {
        let bytes = self.parked_bytes.take().ok_or_else(|| {
            EngineError::with_message(EngineErrorCode::DocNotOpen, "document session is not locked")
        })?;
        match self.open(bytes.clone(), Some(password)) {
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
