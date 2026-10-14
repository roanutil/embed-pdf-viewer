//! What lives on the kernel thread for one `ViewerHost`: the kernel over a
//! `NativeEngine`, the engine's worker, and the store subscription that
//! tells the delegate the state moved.

use std::cell::RefCell;
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
    /// The stamp plugin's asset engine thread, once its factory has run.
    asset_worker: Rc<RefCell<Option<EngineWorker>>>,
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
        HostTable::install_bytes_resolver();
        let engine = Rc::new(NativeEngine::new(worker.clone(), post.clone()));
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
        if let Some(stage) = config.stage {
            // A config that does not convert ends the build, and the worker
            // goes with it, as a failed `Kernel::new` does below.
            let config = match stage.into_config() {
                Ok(config) => config,
                Err(error) => {
                    worker.join();
                    return Err(error);
                }
            };
            plugins.push(epdf_plugin_stage::stage_plugin(epdf_plugin_stage::StagePluginOptions {
                config,
                ..Default::default()
            }));
        }
        if config.selection {
            plugins.push(epdf_plugin_selection::selection_plugin(Default::default()));
        }
        if config.link {
            plugins.push(epdf_plugin_link::link_plugin(Default::default()));
        }
        if let Some(annotation) = config.annotation {
            plugins.push(epdf_plugin_annotation::annotation_plugin(annotation.into()));
        }
        if let Some(form) = config.form {
            plugins.push(epdf_plugin_form::form_plugin(form.into()));
        }
        if let Some(redaction) = config.redaction {
            plugins.push(epdf_plugin_redaction::redaction_plugin(redaction.into()));
        }
        if let Some(measurement) = config.measurement {
            plugins.push(epdf_plugin_measurement::measurement_plugin(measurement.into()));
        }
        let asset_worker: Rc<RefCell<Option<EngineWorker>>> = Rc::default();
        if let Some(stamp) = config.stamp {
            // The asset engine starts on the plugin's first open, on its own
            // engine thread, and is joined with the session.
            let (slot, assets_post) = (asset_worker.clone(), post.clone());
            let factory: Rc<dyn Fn() -> Result<epdf_plugin_stamp::AssetEngineRef, Error>> =
                Rc::new(move || {
                    let (engine, worker) = NativeEngine::assets(assets_post.clone());
                    if let Some(old) = slot.borrow_mut().replace(worker) {
                        old.join();
                    }
                    Ok(epdf_plugin_stamp::AssetEngineRef::Ready(Rc::new(engine)))
                });
            let asset_engine = epdf_plugin_stamp::AssetEngine::Factory(factory);
            plugins.push(epdf_plugin_stamp::stamp_plugin(stamp.into_core(Some(asset_engine))));
        }
        if config.page_edit {
            plugins.push(epdf_plugin_page_edit::page_edit_plugin());
        }
        if config.view_manager {
            plugins.push(epdf_plugin_view_manager::view_manager_plugin());
        }
        if config.shell {
            plugins.push(epdf_plugin_shell::shell_plugin());
        }
        if let Some(i18n) = config.i18n {
            match i18n.into_plugin_config(post.clone()) {
                Ok(i18n) => plugins.push(epdf_plugin_i18n::i18n_plugin(i18n)),
                Err(error) => {
                    worker.join();
                    return Err(error);
                }
            }
        }
        if config.commands {
            plugins.push(epdf_plugin_commands::commands_plugin(
                epdf_plugin_commands::CommandsConfig::default(),
            ));
        }
        if let Some(actions) = config.actions {
            // A throwing observer on one of the five hooks goes where the
            // kernel's own reports go.
            let observer = delegate.clone();
            let config =
                actions.into_plugin_config(Some(Rc::new(move |_hook: &str, error: &Error| {
                    observer.report(ViewerError::from(error.clone()));
                })));
            plugins.push(epdf_plugin_actions::actions_plugin(config));
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
        Ok(Session { kernel, worker, asset_worker, on_change })
    }

    /// Dropped once the engine's client thread has closed PDFium.
    pub fn engine_alive(&self) -> Weak<()> {
        self.worker.alive()
    }

    /// The asset engine's, once the stamp plugin has started it.
    pub fn asset_engine_alive(&self) -> Option<Weak<()>> {
        self.asset_worker.borrow().as_ref().map(EngineWorker::alive)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.on_change.call();
        // On the kernel thread, as it stops: the client thread's last
        // replies post here and fail without blocking, so this join cannot
        // wait on itself. PDFium is closed when it returns.
        self.worker.join();
        let assets = self.asset_worker.borrow_mut().take();
        if let Some(worker) = assets {
            worker.join();
        }
    }
}
