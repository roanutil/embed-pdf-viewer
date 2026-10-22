//! Every open session, and the control frames that end them.
//!
//! G3. The Rust counterpart of the session bookkeeping in
//! `services/src/worker-host/WorkerHost.ts`: the map of open sessions, the
//! shared base documents behind them, the in-flight jobs an abort can reach,
//! and the `close`, `layer.close`, `abort` and `shutdown` frames.
//!
//! This is not the worker host. It holds no transport, decodes nothing and
//! knows no request kinds; a host adapter owns those and calls in here. What
//! lives here is the part that has to be the same on every platform, which is
//! what happens to a session when somebody says stop.
//!
//! # Closing is a hierarchy
//!
//! A document may have several sessions: the document itself and a layer per
//! name. `layer.close` ends one, `close` ends every session for a document,
//! and `shutdown` ends everything. Each is idempotent and none of them fail:
//! closing something that is already closed is the state the caller asked for.

use std::collections::BTreeMap;
use std::rc::Rc;

use pdfium::BaseDocument;

use super::authority::SessionAuthority;
use super::base_registry::BaseDocumentRegistry;
use super::document::{DocumentSession, LayerOpenSource, ParkedBase, ParkedLayer};
use pdfium::Library;

use crate::cancel::CancelToken;
use crate::clock::Clock;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::security::reader::{PasswordMode, check_password, probe_bytes, probe_file};
use crate::features::security::state::SecurityProbeInfo;

/// Which session a request is addressed to.
///
/// A document and each of its layers are separate sessions under one document
/// id. The `None` layer is the document's own session, which is why this is an
/// `Option` rather than two key types.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SessionKey {
    pub doc_id: String,
    pub layer_name: Option<String>,
}

impl SessionKey {
    pub fn document(doc_id: impl Into<String>) -> Self {
        Self { doc_id: doc_id.into(), layer_name: None }
    }

    pub fn layer(doc_id: impl Into<String>, layer_name: impl Into<String>) -> Self {
        Self { doc_id: doc_id.into(), layer_name: Some(layer_name.into()) }
    }

    pub fn belongs_to(&self, doc_id: &str) -> bool {
        self.doc_id == doc_id
    }
}

/// A job that can be aborted while it runs.
pub type JobId = String;

/// Open sessions, shared bases, and in-flight jobs.
#[derive(Debug, Default)]
pub struct SessionRegistry {
    sessions: BTreeMap<SessionKey, DocumentSession>,
    bases: BaseDocumentRegistry,
    /// Cancel tokens for jobs currently running, by job id.
    ///
    /// A finished job removes its own entry, so an abort naming a job that
    /// already returned is a no-op rather than an error: the two race by
    /// nature and the caller cannot tell which won.
    jobs: BTreeMap<JobId, CancelToken>,
    destroyed: bool,
    /// The parsed pages every session here keeps its pages in: one per host
    /// (`WorkerHost.residency`).
    residency: super::pages::SharedResidency,
}

impl SessionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn bases(&self) -> &BaseDocumentRegistry {
        &self.bases
    }

    /// The parsed pages every session here keeps its pages in.
    pub fn residency(&self) -> &super::pages::SharedResidency {
        &self.residency
    }

    /// A session whose pages the host's residency keeps.
    fn new_session(&self) -> DocumentSession {
        let mut session = DocumentSession::new();
        session.set_residency(Rc::clone(&self.residency));
        session
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    fn refuse_if_destroyed(&self) -> Result<(), EngineError> {
        if self.destroyed {
            return Err(EngineError::with_message(
                EngineErrorCode::RuntimeUnavailable,
                "the engine has been shut down",
            ));
        }
        Ok(())
    }

    /// Open a document from bytes under `key`.
    ///
    /// `open.fatMem`. A key already in use is refused rather than replaced:
    /// silently dropping the first session would close a document another
    /// caller is still reading through.
    ///
    /// A document whose password is missing or wrong parks
    /// (`WorkerHost.handleOpen`, `isPasswordOpenError`): the session occupies
    /// its key, holds the bytes, and answers `DocPasswordRequired` until an
    /// unlock loads it. The open's error still comes back, so the caller
    /// knows a prompt is needed.
    pub fn open_memory(
        &mut self,
        key: SessionKey,
        bytes: Vec<u8>,
        password: Option<&str>,
        authority: SessionAuthority,
    ) -> Result<(), EngineError> {
        self.refuse_if_destroyed()?;
        self.refuse_if_taken(&key)?;
        let mut session = self.new_session();
        let parked_authority = authority.clone();
        match session.open_with(bytes.clone(), password, authority) {
            Ok(()) => {
                self.sessions.insert(key, session);
                Ok(())
            }
            // Without parking, the caller would transfer the whole document
            // again after the prompt, which for a large file is the
            // expensive half of opening it.
            Err(error) if is_password_open_error(&error) => {
                session.park_locked_under(bytes, parked_authority)?;
                self.sessions.insert(key, session);
                Err(error)
            }
            Err(error) => Err(error),
        }
    }

    /// Unlock a parked session with a password, under `authority`.
    ///
    /// `document.checkPasswordPermissions` acts on the result of this: an
    /// already-open session has nothing to unlock and a missing one has
    /// nothing parked. A wrong password is `DocPasswordIncorrect` and leaves
    /// the session parked for another try.
    pub fn unlock(
        &mut self,
        key: &SessionKey,
        password: &str,
        authority: SessionAuthority,
    ) -> Result<(), EngineError> {
        self.refuse_if_destroyed()?;
        let session = self.sessions.get_mut(key).ok_or_else(|| not_open(key))?;
        let Some(layer) = session.parked_layer().cloned() else {
            return session.unlock_with(password, authority);
        };
        let base = match &layer.base {
            ParkedBase::Memory(bytes) => {
                self.bases.acquire_memory(&layer.base_key, bytes, Some(password))?
            }
            ParkedBase::File(path) => {
                self.bases.acquire_file(&layer.base_key, path, Some(password))?
            }
        };
        match session.open_layer_from(base, &layer.source, authority) {
            Ok(()) => {
                session.set_password(Some(password));
                Ok(())
            }
            Err(error) => {
                self.bases.release(&layer.base_key);
                Err(error)
            }
        }
    }

    /// Whether `key` names a session parked for a password.
    pub fn is_locked(&self, key: &SessionKey) -> bool {
        self.sessions.get(key).is_some_and(DocumentSession::is_locked)
    }

    /// Unlock a parked session under the authority its open asked for.
    pub fn unlock_parked(&mut self, key: &SessionKey, password: &str) -> Result<(), EngineError> {
        let authority = self.sessions.get(key).ok_or_else(|| not_open(key))?.parked_authority();
        self.unlock(key, password, authority)
    }

    /// Open a fresh layer over a base, registering the base under `base_key`.
    ///
    /// `open.layerMemBase`.
    pub fn open_layer_memory(
        &mut self,
        key: SessionKey,
        base_key: &str,
        bytes: &[u8],
        password: Option<&str>,
        authority: SessionAuthority,
    ) -> Result<(), EngineError> {
        self.open_layer_memory_from(
            key,
            base_key,
            bytes,
            &LayerOpenSource::Fresh,
            password,
            authority,
        )
    }

    /// [`SessionRegistry::open_layer_memory`], the layer opened from `source`.
    pub fn open_layer_memory_from(
        &mut self,
        key: SessionKey,
        base_key: &str,
        bytes: &[u8],
        source: &LayerOpenSource,
        password: Option<&str>,
        authority: SessionAuthority,
    ) -> Result<(), EngineError> {
        self.refuse_if_destroyed()?;
        self.refuse_if_taken(&key)?;
        match self.bases.acquire_memory(base_key, bytes, password) {
            Ok(base) => self.attach_layer(key, base_key, base, source, password, authority),
            Err(error) => {
                let parked = ParkedLayer {
                    base_key: base_key.to_owned(),
                    base: ParkedBase::Memory(bytes.to_vec()),
                    source: source.clone(),
                };
                self.park_layer(key, parked, authority, error)
            }
        }
    }

    /// A layer open that failed: parked when it failed on its password
    /// (`WorkerHost.handleOpen`), and the error answered either way.
    fn park_layer(
        &mut self,
        key: SessionKey,
        parked: ParkedLayer,
        authority: SessionAuthority,
        error: EngineError,
    ) -> Result<(), EngineError> {
        if is_password_open_error(&error) {
            let mut session = self.new_session();
            session.park_layer(parked, authority)?;
            self.sessions.insert(key, session);
        }
        Err(error)
    }

    fn refuse_if_taken(&self, key: &SessionKey) -> Result<(), EngineError> {
        if self.sessions.contains_key(key) {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("a session is already open for {}", describe(key)),
            ));
        }
        Ok(())
    }

    fn attach_layer(
        &mut self,
        key: SessionKey,
        base_key: &str,
        base: Rc<BaseDocument>,
        source: &LayerOpenSource,
        password: Option<&str>,
        authority: SessionAuthority,
    ) -> Result<(), EngineError> {
        let mut session = self.new_session();
        match session.open_layer_from(base, source, authority) {
            Ok(()) => {
                session.set_password(password);
                self.sessions.insert(key, session);
                Ok(())
            }
            Err(error) => {
                // Give the acquisition back, or the base stays registered for
                // a session that does not exist and the next caller for that
                // key inherits a parse nobody asked to keep alive. A fresh
                // layer over a valid base does not fail; an artifact saved
                // against another base does.
                self.bases.release(base_key);
                Err(error)
            }
        }
    }

    /// Open a fresh layer over a base read from a file.
    ///
    /// `open.layerFileBase`. Identical to
    /// [`SessionRegistry::open_layer_memory`] except that the base reads
    /// through the file rather than a copy, which is the whole reason the
    /// frame is separate: a base shared by several sessions is exactly the
    /// case where not holding the bytes twice matters.
    pub fn open_layer_file(
        &mut self,
        key: SessionKey,
        base_key: &str,
        path: &std::path::Path,
        password: Option<&str>,
        authority: SessionAuthority,
    ) -> Result<(), EngineError> {
        self.open_layer_file_from(key, base_key, path, &LayerOpenSource::Fresh, password, authority)
    }

    /// [`SessionRegistry::open_layer_file`], the layer opened from `source`.
    pub fn open_layer_file_from(
        &mut self,
        key: SessionKey,
        base_key: &str,
        path: &std::path::Path,
        source: &LayerOpenSource,
        password: Option<&str>,
        authority: SessionAuthority,
    ) -> Result<(), EngineError> {
        self.refuse_if_destroyed()?;
        self.refuse_if_taken(&key)?;
        match self.bases.acquire_file(base_key, path, password) {
            Ok(base) => self.attach_layer(key, base_key, base, source, password, authority),
            Err(error) => {
                let parked = ParkedLayer {
                    base_key: base_key.to_owned(),
                    base: ParkedBase::File(path.to_owned()),
                    source: source.clone(),
                };
                self.park_layer(key, parked, authority, error)
            }
        }
    }

    /// Open a fresh layer over a base already registered.
    pub fn open_layer_on(
        &mut self,
        key: SessionKey,
        base: Rc<BaseDocument>,
        authority: SessionAuthority,
    ) -> Result<(), EngineError> {
        self.refuse_if_destroyed()?;
        self.refuse_if_taken(&key)?;
        let mut session = self.new_session();
        session.open_layer(base, authority)?;
        self.sessions.insert(key, session);
        Ok(())
    }

    /// The session for `key` (`requireSession`).
    ///
    /// A locked session reports `DocPasswordRequired` rather than being served
    /// as if it were open, so an operation that reaches one gets a truthful
    /// error naming what it needs. One a transaction left in an unknown
    /// state is `DocNotOpen` (`assertUsable`).
    pub fn session(&self, key: &SessionKey) -> Result<&DocumentSession, EngineError> {
        let session = self.sessions.get(key).ok_or_else(|| not_open(key))?;
        if session.is_locked() {
            return Err(EngineError::with_message(
                EngineErrorCode::DocPasswordRequired,
                format!("{} is locked and requires a password", describe(key)),
            ));
        }
        session.assert_usable()?;
        Ok(session)
    }

    pub fn session_mut(&mut self, key: &SessionKey) -> Result<&mut DocumentSession, EngineError> {
        // Checked before the mutable borrow so the error can name the key.
        self.session(key)?;
        Ok(self.sessions.get_mut(key).expect("checked above"))
    }

    /// The session for `key`, locked or open, to set what its open asked
    /// for (`objectNumbers`, `signedDocumentPolicy`), which a locked session
    /// keeps for its unlock as upstream's does.
    pub fn configure(&mut self, key: &SessionKey) -> Option<&mut DocumentSession> {
        self.sessions.get_mut(key)
    }

    /// The session under `key` whatever state it is in, for the host to
    /// finish the transaction it opened on it.
    pub fn entry_mut(&mut self, key: &SessionKey) -> Option<&mut DocumentSession> {
        self.sessions.get_mut(key)
    }

    pub fn contains(&self, key: &SessionKey) -> bool {
        self.sessions.contains_key(key)
    }

    /// `layer.close`: end one session.
    ///
    /// Returns whether there was one. Closing a session that is not open is
    /// the state the caller asked for, so it is not an error.
    pub fn close_layer(&mut self, key: &SessionKey) -> bool {
        match self.sessions.remove(key) {
            Some(mut session) => {
                session.close();
                true
            }
            None => false,
        }
    }

    /// `close`: end every session for a document, layers included.
    ///
    /// Returns how many were closed.
    pub fn close_document(&mut self, doc_id: &str) -> usize {
        let keys: Vec<SessionKey> =
            self.sessions.keys().filter(|key| key.belongs_to(doc_id)).cloned().collect();
        let mut closed = 0;
        for key in keys {
            if let Some(mut session) = self.sessions.remove(&key) {
                session.close();
                closed += 1;
            }
        }
        closed
    }

    /// `document.probeSecurityFile`: what a file's security is, before any
    /// session exists.
    ///
    /// The one frame that needs no session, which is why it takes a path
    /// rather than a key. It opens and closes its own document.
    pub fn probe_security_file(
        &self,
        path: &std::path::Path,
        password: Option<&str>,
        clock: &dyn Clock,
    ) -> Result<SecurityProbeInfo, EngineError> {
        self.refuse_if_destroyed()?;
        Ok(probe_file(Library::acquire(), path, password, clock))
    }

    /// `document.probeSecurityFile`, from bytes already in hand.
    pub fn probe_security_bytes(
        &self,
        bytes: &[u8],
        password: Option<&str>,
        clock: &dyn Clock,
    ) -> Result<SecurityProbeInfo, EngineError> {
        self.refuse_if_destroyed()?;
        Ok(probe_bytes(Library::acquire(), bytes, password, clock))
    }

    /// `document.checkPasswordPermissions`: would this password help?
    ///
    /// Answered against an open session without changing it, so a prompt can
    /// ask before committing to a reopen. A locked session is the one case
    /// this cannot serve: there is no open document to check against, and the
    /// caller wants [`SessionRegistry::unlock`] instead.
    pub fn check_password_permissions(
        &self,
        key: &SessionKey,
        password: &str,
        mode: PasswordMode,
        clock: &dyn Clock,
    ) -> Result<SecurityProbeInfo, EngineError> {
        self.refuse_if_destroyed()?;
        let session = self.session(key)?;
        check_password(session.document()?.document(), password, mode, clock)
    }

    /// Register a job's cancel token so [`SessionRegistry::abort`] can reach it.
    pub fn begin_job(&mut self, job_id: impl Into<JobId>, cancel: CancelToken) {
        self.jobs.insert(job_id.into(), cancel);
    }

    /// Forget a job that has finished.
    pub fn end_job(&mut self, job_id: &str) {
        self.jobs.remove(job_id);
    }

    pub fn running_jobs(&self) -> usize {
        self.jobs.len()
    }

    /// `abort`: cancel one in-flight job.
    ///
    /// Returns whether a job was there to cancel. Signalling only: the job
    /// observes its token at its own checkpoints and unwinds itself, because
    /// stopping PDFium mid-call is not a thing that can be done safely.
    pub fn abort(&mut self, job_id: &str) -> bool {
        match self.jobs.remove(job_id) {
            Some(cancel) => {
                cancel.cancel();
                true
            }
            None => false,
        }
    }

    /// Cancel every in-flight job without closing anything.
    pub fn abort_all(&mut self) {
        for (_, cancel) in std::mem::take(&mut self.jobs) {
            cancel.cancel();
        }
    }

    /// `shutdown`: cancel everything, close every session, release every base.
    ///
    /// Idempotent, and the order matters. Jobs are cancelled first so nothing
    /// is mid-read when its session closes; sessions close next, which is what
    /// releases the layer documents; the bases go last, because a layer points
    /// into its base.
    pub fn shutdown(&mut self) {
        if self.destroyed {
            return;
        }
        self.destroyed = true;
        self.abort_all();
        for (_, mut session) in std::mem::take(&mut self.sessions) {
            session.close();
        }
        self.residency.borrow_mut().close_idle();
        self.bases.release_all();
    }
}

/// `isPasswordOpenError`: an open that failed on its password, missing or
/// wrong, which parks rather than failing.
pub fn is_password_open_error(error: &EngineError) -> bool {
    matches!(
        error.code,
        EngineErrorCode::DocPasswordRequired | EngineErrorCode::DocPasswordIncorrect
    )
}

fn describe(key: &SessionKey) -> String {
    match &key.layer_name {
        Some(layer) => format!("document {} layer {layer}", key.doc_id),
        None => format!("document {}", key.doc_id),
    }
}

fn not_open(key: &SessionKey) -> EngineError {
    EngineError::with_message(EngineErrorCode::DocNotOpen, format!("{} is not open", describe(key)))
}
