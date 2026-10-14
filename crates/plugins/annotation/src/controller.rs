//! `controller.ts`: the composition root. It builds the plugin's services
//! once and assembles the host capability from the areas' slices.
//!
//! The TypeScript builds each area as a closure over the services it
//! declares and composes their API objects. Here the areas are `impl`
//! blocks on one [`AnnotationCapability`], one per TypeScript file
//! (`read/`, `write/`, `sync/`, `comments/`, `tools/`, `services/`), and
//! their private state sits in [`Core`], grouped by the area that owns it.
//!
//! Turn counts are behaviour: the trace records how an engine answer and
//! the model updates interleave. Where the TypeScript chains `.then` on an
//! engine call the port reacts one turn after it settles
//! ([`promise::then`]); where it `await`s one, it adopts it
//! ([`promise::await_engine`]); every TypeScript `async` function is a
//! spawned promise, so awaiting it costs the turn awaiting a native promise
//! does.
//!
//! The TypeScript memoizes its projections by model identity so a
//! subscribed selector gets a stable reference. Canonical JSON has no
//! identity, so the port recomputes them; the values are the same.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use epdf_annotation_core::Model;
use epdf_kernel::engine::DocumentHandle;
use epdf_kernel::{Controller, Error};
use epdf_plugin_actions::ACTIONS;

use crate::comments::threads::ThreadsMemo;
use crate::contract::*;
use crate::model::{AnnotationAction, AnnotationState};
use crate::rich_text::FontHandle;
use crate::services::events::AnnotationEvents;
use crate::services::write_sink::WriteSinkState;
use crate::sync::hydration::HydrationState;
use crate::tools::definitions::{AnnotationToolDef, ToolRegistry, build_tool_registry};
use crate::tools::hover_feed::{AnnotationHoverFeed, create_annotation_hover_feed};
use crate::write::links::LinkChains;
use crate::write::measurement::PageViewports;
use crate::write::stamps::ArmedStamp;
use crate::write::text_editing::TextTimers;

/// The plugin's private state, by the area that owns it.
pub(crate) struct Core {
    pub(crate) cx: Controller<AnnotationState, AnnotationAction>,
    pub(crate) host: AnnotationHost,
    // services/
    pub(crate) events: AnnotationEvents,
    pub(crate) writes: WriteSinkState,
    pub(crate) file_picker: RefCell<Option<FilePickerProvider>>,
    pub(crate) file_picker_serial: Cell<u64>,
    // tools/
    pub(crate) config_tools: Vec<AnnotationToolDef>,
    pub(crate) registry: RefCell<ToolRegistry>,
    pub(crate) behaviors: RefCell<Vec<Rc<dyn Behavior>>>,
    // sync/
    pub(crate) hydration: HydrationState,
    // write/
    pub(crate) text_timers: TextTimers,
    pub(crate) link_chains: LinkChains,
    pub(crate) armed: RefCell<Option<ArmedStamp>>,
    pub(crate) viewports: PageViewports,
    pub(crate) hover_feed: Option<AnnotationHoverFeed>,
    pub(crate) uuid_seq: Cell<u64>,
    // comments/
    pub(crate) threads_memo: ThreadsMemo,
}

/// `AnnotationHostCapability`, which includes the public
/// `AnnotationCapability`. Cheap to clone; clones share state.
#[derive(Clone)]
pub struct AnnotationCapability(pub(crate) Rc<Core>);

/// `createAnnotationController(ctx, config)`. Throws when a config tool
/// fails validation.
pub fn create_annotation_controller(
    cx: Controller<AnnotationState, AnnotationAction>,
    config: &AnnotationConfig,
) -> Result<AnnotationCapability, Error> {
    let config_tools = config.tools.clone().unwrap_or_default();
    let registry = build_tool_registry(&config_tools)?;
    let host = config.host.clone();
    let events = AnnotationEvents::new(&cx, host.console_error.clone());
    // The E/X trigger feed, when an actions plugin is present.
    let hover_feed = match cx.try_get(&ACTIONS)? {
        Some(actions) => {
            let model_cx = cx.clone();
            Some(create_annotation_hover_feed(actions, move |id| {
                model_cx.state().model.by_id.get(id).cloned()
            }))
        }
        None => None,
    };
    let core = Core {
        cx: cx.clone(),
        host,
        events,
        writes: WriteSinkState::default(),
        file_picker: RefCell::new(None),
        file_picker_serial: Cell::new(0),
        config_tools,
        registry: RefCell::new(registry),
        behaviors: RefCell::new(Vec::new()),
        hydration: HydrationState::default(),
        text_timers: TextTimers::default(),
        link_chains: LinkChains::default(),
        armed: RefCell::new(None),
        viewports: PageViewports::default(),
        hover_feed,
        uuid_seq: Cell::new(0),
        threads_memo: ThreadsMemo::default(),
    };
    let api = AnnotationCapability(Rc::new(core));
    let weak = Rc::downgrade(&api.0);
    cx.defer_fn(move || {
        if let Some(core) = weak.upgrade() {
            core.viewports.clear();
        }
    });
    Ok(api)
}

impl AnnotationCapability {
    // ── plumbing over `ctx` ───────────────────────────────────────────────────

    pub(crate) fn cx(&self) -> &Controller<AnnotationState, AnnotationAction> {
        &self.0.cx
    }

    /// The instance lifetime, aborted when the document closes. A foreign
    /// handle outlives the close, checks this before every call, and ends
    /// its event streams when it aborts.
    #[cfg(feature = "uniffi")]
    pub(crate) fn lifetime(&self) -> &epdf_kernel::signal::AbortSignal {
        self.0.cx.lifetime()
    }

    pub(crate) fn state(&self) -> Rc<AnnotationState> {
        self.0.cx.state()
    }

    /// `store.model()`: the current model, what every read projects from.
    pub fn model(&self) -> Rc<Model> {
        self.state().model.clone()
    }

    /// `ctx.doc`: the guarded handle, `None` once there is none.
    pub(crate) fn doc(&self) -> Option<Rc<dyn DocumentHandle>> {
        self.0.cx.doc().ok()
    }

    pub(crate) fn console_error(&self, message: &str, error: Option<&Error>) {
        if let Some(sink) = &self.0.host.console_error {
            sink(message, error);
        }
    }

    /// `crypto.randomUUID()`.
    pub(crate) fn random_uuid(&self) -> String {
        match &self.0.host.random_uuid {
            Some(f) => f(),
            None => {
                let n = self.0.uuid_seq.get() + 1;
                self.0.uuid_seq.set(n);
                format!("00000000-0000-4000-8000-{n:012x}")
            }
        }
    }

    /// `services.fonts()`: the registered fonts.
    pub(crate) fn fonts(&self) -> Vec<FontHandle> {
        self.0.host.fonts.as_ref().map(|f| f()).unwrap_or_default()
    }
}

/// `ctx.cleanup(fn)` on the controller context.
pub(crate) trait DeferFn {
    fn defer_fn(&self, f: impl FnOnce() + 'static);
}

impl DeferFn for Controller<AnnotationState, AnnotationAction> {
    fn defer_fn(&self, f: impl FnOnce() + 'static) {
        self.defer(Box::new(move || {
            f();
            Ok(None)
        }));
    }
}
