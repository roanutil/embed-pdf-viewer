//! What lives on the kernel thread for one `ViewerHost`: the kernel over a
//! `NativeEngine`, the engine's worker, and the store subscription that
//! tells the delegate the state moved.

use std::rc::Rc;
use std::sync::{Arc, Weak};
use std::sync::atomic::{AtomicU64, Ordering};

use epdf_ffi::{KernelPost, ViewerError};
use epdf_kernel::{Error, Kernel, KernelOptions, Unsubscribe};

use crate::api::{ViewerConfig, ViewerDelegate};
use crate::engine_worker::EngineWorker;
use crate::host_table::HostTable;
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
            // A closed document's capabilities go once no foreign handle
            // holds them. Otherwise the session and its capabilities keep
            // each other alive past the close, and the render plugin's
            // raster store keeps its pixels in the `HostTable` for good.
            release_closed_sessions: true,
            ..KernelOptions::default()
        };
        let mut plugins = Vec::new();
        if config.metadata {
            plugins.push(epdf_plugin_metadata::metadata_plugin());
        }
        if let Some(search) = config.search {
            plugins.push(epdf_plugin_search::search_plugin(search.into()));
        }
        if let Some(render) = config.render {
            // A raster's pixels sit in the `HostTable` behind its `$host` id
            // for as long as the raster store holds the image; this lets go
            // when the store's last handle does. The frame scheduler stays
            // the default 16 ms timer on the kernel thread.
            let host = epdf_plugin_render::RenderHost {
                release_image: Some(Rc::new(|image: &serde_json::Value| {
                    if let Some(id) = image.get("$host").and_then(serde_json::Value::as_u64) {
                        HostTable::release(id);
                    }
                })),
                ..epdf_plugin_render::RenderHost::default()
            };
            plugins.push(epdf_plugin_render::render_plugin_with_host(render.into(), host));
        }
        if config.interaction {
            plugins.push(epdf_plugin_interaction::interaction_plugin(Default::default()));
        }
        if let Some(feedback) = config.feedback {
            plugins.push(epdf_plugin_interaction::feedback_plugin(feedback.to_options()));
        }
        let kernel = match Kernel::new(engine, plugins, options) {
            Ok(kernel) => kernel,
            Err(error) => {
                worker.join();
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

    /// Dropped once the engine's client thread has closed PDFium.
    pub fn engine_alive(&self) -> Weak<()> {
        self.worker.alive()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.on_change.call();
        // On the kernel thread, as it stops: the client thread's last
        // replies post here and fail without blocking, so this join cannot
        // wait on itself. PDFium is closed when it returns.
        self.worker.join();
    }
}
