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
use super::document::DocumentSession;
use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};

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
}

impl SessionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn bases(&self) -> &BaseDocumentRegistry {
        &self.bases
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
    pub fn open_memory(
        &mut self,
        key: SessionKey,
        bytes: Vec<u8>,
        password: Option<&str>,
        authority: SessionAuthority,
    ) -> Result<(), EngineError> {
        self.refuse_if_destroyed()?;
        if self.sessions.contains_key(&key) {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("a session is already open for {}", describe(&key)),
            ));
        }
        let mut session = DocumentSession::new();
        match session.open_with(bytes.clone(), password, authority) {
            Ok(()) => {
                self.sessions.insert(key, session);
                Ok(())
            }
            // A document that needs a password parks rather than failing
            // outright: the session occupies its key, holds the bytes, and
            // answers DocPasswordRequired until somebody unlocks it. Without
            // that the caller has to transfer the whole document again after
            // the prompt, which for a large file is the expensive half of
            // opening it.
            //
            // The error still propagates. The caller has to know a prompt is
            // needed, and returning Ok here would tell it the document is
            // readable.
            Err(error) if error.code == EngineErrorCode::DocPasswordRequired => {
                session.park_locked(bytes)?;
                self.sessions.insert(key, session);
                Err(error)
            }
            Err(error) => Err(error),
        }
    }

    /// Unlock a parked session with a password.
    ///
    /// `document.checkPasswordPermissions` acts on the result of this, and
    /// only a session parked by [`SessionRegistry::open_memory`] can be
    /// unlocked: an already-open one has nothing to unlock and a missing one
    /// has no bytes.
    pub fn unlock(
        &mut self,
        key: &SessionKey,
        password: &str,
        authority: SessionAuthority,
    ) -> Result<(), EngineError> {
        self.refuse_if_destroyed()?;
        let session = self.sessions.get_mut(key).ok_or_else(|| not_open(key))?;
        session.unlock_with(password, authority)
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
        self.refuse_if_destroyed()?;
        if self.sessions.contains_key(&key) {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("a session is already open for {}", describe(&key)),
            ));
        }
        let base = self.bases.acquire_memory(base_key, bytes, password)?;
        let mut session = DocumentSession::new();
        match session.open_layer(base, authority) {
            Ok(()) => {
                // The base is held by the session now; the registry's own
                // acquisition is handed back so the count reflects sessions
                // rather than calls.
                self.sessions.insert(key, session);
                Ok(())
            }
            Err(error) => {
                // Give the acquisition back, or the base stays registered for
                // a session that does not exist and the next caller for that
                // key inherits a parse nobody asked to keep alive. Untested:
                // a fresh layer over a valid base does not fail, so there is
                // no way to reach this without faking the failure.
                self.bases.release(base_key);
                Err(error)
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
        if self.sessions.contains_key(&key) {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("a session is already open for {}", describe(&key)),
            ));
        }
        let mut session = DocumentSession::new();
        session.open_layer(base, authority)?;
        self.sessions.insert(key, session);
        Ok(())
    }

    /// The session for `key`.
    ///
    /// A locked session reports `DocPasswordRequired` rather than being served
    /// as if it were open, so an operation that reaches one gets a truthful
    /// error naming what it needs.
    pub fn session(&self, key: &SessionKey) -> Result<&DocumentSession, EngineError> {
        let session = self.sessions.get(key).ok_or_else(|| not_open(key))?;
        if session.is_locked() {
            return Err(EngineError::with_message(
                EngineErrorCode::DocPasswordRequired,
                format!("{} is locked and requires a password", describe(key)),
            ));
        }
        Ok(session)
    }

    pub fn session_mut(&mut self, key: &SessionKey) -> Result<&mut DocumentSession, EngineError> {
        // Checked before the mutable borrow so the error can name the key.
        self.session(key)?;
        Ok(self.sessions.get_mut(key).expect("checked above"))
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
        self.bases.release_all();
    }
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
