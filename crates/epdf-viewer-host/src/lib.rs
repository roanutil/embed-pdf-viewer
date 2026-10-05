//! The kernel and plugins over `epdf-engine`, exported through uniffi: the
//! P6 crate `multiplatform-plugins-plan.md` §4 names.
//!
//! `ViewerHost` is the object Swift holds. It owns the kernel thread, where
//! a `Session` holds the kernel; every export posts a job there.

pub mod api;
pub mod engine_worker;
#[cfg(feature = "manual-bridge")]
mod experimental;
pub mod host_table;
pub mod native_engine;
pub mod session;
pub mod translate;

// Nothing here calls `epdf_uniffi`, and rustc leaves an unreferenced
// dependency out of the staticlib. The Swift smoke tests drive its engine-level
// exports through this archive, so link it explicitly.
extern crate epdf_uniffi;
// The same for the experimental manual bridge's C exports, when built in.
#[cfg(feature = "manual-bridge")]
extern crate epdf_manual_bridge;

use std::future::Future;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use epdf_ffi::{KernelPost, KernelThread, PromiseSlot, Subscription, ViewerError};
use epdf_kernel::{Error, OpenOptions, OpenSource, Promise, Unsubscribe};
use epdf_plugin_metadata::ffi::MetadataHandle;
use epdf_plugin_render::ffi::RenderHandle;
use epdf_plugin_search::ffi::SearchHandle;
use serde_json::{Map, Value, json};

pub use api::{
    DocumentEvent, DocumentEventListener, DocumentInfo, DocumentStatus, OpenDocumentOptions,
    PageInfo, ViewerConfig, ViewerDelegate,
};
use api::{Lifecycle, decode_pages};
use host_table::HostTable;
use session::Session;

#[derive(uniffi::Object)]
pub struct ViewerHost {
    thread: Mutex<Option<KernelThread<Session>>>,
    /// The store revision, mirrored so `state_revision` never posts a job.
    revision: Arc<AtomicU64>,
    post: KernelPost,
    /// This host's entry in the manual bridge's registry.
    #[cfg(feature = "manual-bridge")]
    token: u64,
}

impl ViewerHost {
    fn call<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Session) -> Result<T, Error> + Send + 'static,
    ) -> impl Future<Output = Result<T, ViewerError>> + Send {
        let reply = self
            .thread
            .lock()
            .unwrap()
            .as_ref()
            .map(|thread| thread.call(move |s| f(s).map_err(ViewerError::from)));
        async move {
            match reply {
                Some(reply) => reply.await.unwrap_or(Err(ViewerError::Closed)),
                None => Err(ViewerError::Closed),
            }
        }
    }

    /// Dropping the returned future aborts the promise.
    fn call_promise<U: Clone + 'static, T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Session) -> Result<Promise<U>, Error> + Send + 'static,
        map: impl FnOnce(U) -> T + Send + 'static,
    ) -> impl Future<Output = Result<T, ViewerError>> + Send {
        let (slot, reply) = PromiseSlot::pending(self.post.clone(), map);
        // No thread, or a thread already stopping: the job, and with it the
        // slot, is dropped, and `reply` answers `Closed`.
        self.fire(move |s| slot.start(f(s).map_err(ViewerError::from)));
        reply
    }

    /// No reply. A job posted after `shutdown` is dropped unrun.
    fn fire(&self, f: impl FnOnce(&mut Session) + Send + 'static) {
        if let Some(thread) = self.thread.lock().unwrap().as_ref() {
            drop(thread.call(f));
        }
    }
}

#[uniffi::export]
impl ViewerHost {
    #[uniffi::constructor]
    pub fn new(
        config: ViewerConfig,
        delegate: Arc<dyn ViewerDelegate>,
    ) -> Result<Arc<Self>, ViewerError> {
        let revision = Arc::new(AtomicU64::new(0));
        let mirror = revision.clone();
        let thread =
            KernelThread::spawn(move |post| Session::build(config, delegate, mirror, post))?;
        let post = thread.post();
        #[cfg(not(feature = "manual-bridge"))]
        let host = Arc::new(ViewerHost { thread: Mutex::new(Some(thread)), revision, post });
        #[cfg(feature = "manual-bridge")]
        let host = Arc::new_cyclic(|weak| ViewerHost {
            token: experimental::register(weak.clone(), post.clone()),
            thread: Mutex::new(Some(thread)),
            revision,
            post,
        });
        Ok(host)
    }

    pub async fn start(&self) -> Result<(), ViewerError> {
        self.call_promise(|s| Ok(s.kernel.start()), |()| ()).await
    }

    /// Close every document and dispose the kernel. The thread keeps
    /// running until `shutdown`.
    pub async fn destroy(&self) -> Result<(), ViewerError> {
        self.call_promise(|s| Ok(s.kernel.destroy()), |()| ()).await
    }

    /// A locked document rejects with code `DocPasswordRequired`: the
    /// native engine fails the open itself, so the kernel records a failed
    /// tab rather than a locked one. Open again with `password` set.
    pub async fn open_document(
        &self,
        bytes: Vec<u8>,
        options: OpenDocumentOptions,
    ) -> Result<DocumentInfo, ViewerError> {
        let OpenDocumentOptions { name, password, activate } = options;
        self.call_promise(
            move |s| {
                let bytes = HostTable::put(bytes);
                // The kernel hands `engine` to `Engine::open` as its options,
                // which is where `NativeEngine::open` reads the password.
                let mut engine = Map::new();
                if let Some(password) = password {
                    engine.insert("password".into(), password.into());
                }
                let documents = s.kernel.documents();
                let opened = documents.open(
                    OpenSource::Input(json!({ "$host": bytes })),
                    OpenOptions { activate: Some(activate), name, engine },
                );
                // An open refused before the engine took the bytes (a
                // destroyed kernel) leaves them in the table.
                opened.on_settle(move |_| HostTable::release(bytes));
                Ok(opened.map(move |id| {
                    let id = id.as_str().unwrap_or_default().to_owned();
                    documents.get(&id).map(DocumentInfo::from).ok_or_else(|| {
                        Error::new(format!("[epdf-viewer-host] {id} opened but is not listed"))
                    })
                }))
            },
            |info: DocumentInfo| info,
        )
        .await
    }

    pub async fn unlock_document(
        &self,
        document_id: String,
        password: String,
    ) -> Result<(), ViewerError> {
        self.call_promise(
            move |s| Ok(s.kernel.documents().unlock(&document_id, &password)),
            |()| (),
        )
        .await
    }

    pub async fn close_document(&self, document_id: String) -> Result<(), ViewerError> {
        self.call_promise(move |s| Ok(s.kernel.documents().close(&document_id)), |()| ()).await
    }

    /// Every tab, in order: ready, loading, locked and failed alike. Only a
    /// ready document has a page count.
    pub async fn documents(&self) -> Result<Vec<DocumentInfo>, ViewerError> {
        self.call(|s| Ok(s.kernel.documents().list().into_iter().map(DocumentInfo::from).collect()))
            .await
    }

    pub async fn pages(&self, document_id: String) -> Result<Vec<PageInfo>, ViewerError> {
        self.call(move |s| {
            let documents = s.kernel.documents();
            if !documents.has(&document_id) {
                return Err(Error::new(format!("[epdf-viewer-host] no document {document_id}"))
                    .with("code", "NotFound"));
            }
            decode_pages(&documents.list_pages(Some(&document_id)))
        })
        .await
    }

    /// The listener is registered on the kernel thread after this returns;
    /// a cancel before then still takes effect.
    pub fn subscribe_documents(
        &self,
        listener: Arc<dyn DocumentEventListener>,
    ) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.fire(move |s| {
            let Some(events) = s.kernel.documents().events() else { return };
            let hooks = [
                (Lifecycle::Opened, &events.opened),
                (Lifecycle::OpenFailed, &events.open_failed),
                (Lifecycle::Locked, &events.locked),
                (Lifecycle::Closed, &events.closed),
                (Lifecycle::ActiveChanged, &events.active_changed),
                (Lifecycle::PagesChanged, &events.pages_changed),
            ];
            // A payload that does not decode is returned as an error, which
            // the hook reports through `KernelOptions.report`.
            let offs: Vec<Unsubscribe> = hooks
                .into_iter()
                .map(|(kind, hook)| {
                    let listener = listener.clone();
                    hook.on(move |payload: &Value| {
                        listener.on_event(kind.decode(payload)?);
                        Ok(())
                    })
                })
                .collect();
            slot.fill(Unsubscribe::new(move || offs.iter().for_each(Unsubscribe::call)));
        });
        subscription
    }

    /// The metadata capability of an open document. Needs
    /// `ViewerConfig.metadata`; without it the kernel refuses the token.
    pub async fn metadata(&self, document_id: String) -> Result<Arc<MetadataHandle>, ViewerError> {
        let post = self.post.clone();
        self.call(move |s| {
            let cap = s.kernel.capability(&epdf_plugin_metadata::METADATA, Some(&document_id))?;
            Ok(MetadataHandle::new(cap, post))
        })
        .await
    }

    /// The search capability of an open document. Needs
    /// `ViewerConfig.search`; without it the kernel refuses the token.
    pub async fn search(&self, document_id: String) -> Result<Arc<SearchHandle>, ViewerError> {
        let post = self.post.clone();
        self.call(move |s| {
            let cap = s.kernel.capability(&epdf_plugin_search::SEARCH, Some(&document_id))?;
            Ok(SearchHandle::new(cap, post))
        })
        .await
    }

    /// The render capability of an open document: page rasters copied out
    /// of the `HostTable` into owned pixels. Needs `ViewerConfig.render`;
    /// without it the kernel refuses the token.
    pub async fn render(&self, document_id: String) -> Result<Arc<RenderHandle>, ViewerError> {
        let post = self.post.clone();
        self.call(move |s| {
            let cap = s.kernel.capability(&epdf_plugin_render::RENDER, Some(&document_id))?;
            Ok(RenderHandle::new(cap, post, Rc::new(HostTable::get)))
        })
        .await
    }

    pub fn state_revision(&self) -> u64 {
        self.revision.load(Ordering::SeqCst)
    }

    /// Stop and join the kernel thread. Idempotent; every later call
    /// answers `Closed`.
    pub fn shutdown(&self) {
        #[cfg(feature = "manual-bridge")]
        experimental::unregister(self.token);
        let thread = self.thread.lock().unwrap().take();
        drop(thread);
    }
}

impl ViewerHost {
    /// How many byte buffers the `HostTable` holds: a test's view of what
    /// the host has not let go of. Not exported.
    #[doc(hidden)]
    pub async fn host_table_entries(&self) -> Result<usize, ViewerError> {
        self.call(|_| Ok(HostTable::len())).await
    }

    /// Upgrades while the engine's client thread is alive: a test's view of
    /// whether dropping the host ended it. Not exported.
    #[doc(hidden)]
    pub async fn engine_thread_witness(&self) -> Result<std::sync::Weak<()>, ViewerError> {
        self.call(|s| Ok(s.engine_alive())).await
    }
}

impl Drop for ViewerHost {
    fn drop(&mut self) {
        self.shutdown();
    }
}

uniffi::setup_scaffolding!();
