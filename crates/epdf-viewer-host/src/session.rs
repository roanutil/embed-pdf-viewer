//! What lives on the kernel thread for one `ViewerHost`: the kernel over a
//! `NativeEngine`, the engine's worker, and the store subscription that
//! tells the delegate the state moved.

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use epdf_ffi::{KernelPost, ViewerError};
use epdf_kernel::{Error, Kernel, KernelOptions, Unsubscribe};

use crate::api::{ViewerConfig, ViewerDelegate};
use crate::engine_worker::EngineWorker;
use crate::native_engine::NativeEngine;

pub struct Session {
    pub kernel: Kernel,
    worker: EngineWorker,
    on_change: Unsubscribe,
}

impl Session {
    /// On the kernel thread. Every store change stores the new revision in
    /// `revision`, then tells the delegate.
    pub fn build(
        config: ViewerConfig,
        delegate: Arc<dyn ViewerDelegate>,
        revision: Arc<AtomicU64>,
        post: KernelPost,
    ) -> Result<Session, Error> {
        let worker = EngineWorker::spawn();
        let engine = Rc::new(NativeEngine::new(worker.clone(), post));
        let reporter = delegate.clone();
        let options = KernelOptions {
            report: Some(Rc::new(move |error: &Error| {
                reporter.report(ViewerError::from(error.clone()));
            })),
            ..KernelOptions::default()
        };
        let mut plugins = Vec::new();
        if config.metadata {
            plugins.push(epdf_plugin_metadata::metadata_plugin());
        }
        if let Some(search) = config.search {
            plugins.push(epdf_plugin_search::search_plugin(search.into()));
        }
        let kernel = match Kernel::new(engine, plugins, options) {
            Ok(kernel) => kernel,
            Err(error) => {
                worker.shutdown();
                return Err(error);
            }
        };
        // Weak: the store holds this listener, so a strong store here would
        // keep the store alive past the session.
        let store = Rc::downgrade(&kernel.store());
        let on_change = kernel.subscribe(Rc::new(move || {
            if let Some(store) = store.upgrade() {
                let now = store.revision();
                revision.store(now, Ordering::SeqCst);
                delegate.state_changed(now);
            }
            Ok(())
        }));
        Ok(Session { kernel, worker, on_change })
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.on_change.call();
        self.worker.shutdown();
    }
}
