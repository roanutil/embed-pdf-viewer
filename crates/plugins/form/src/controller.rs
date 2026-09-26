//! `controller.ts`: the composition root. It builds the plugin's services
//! once and assembles the host capability from the areas' slices.
//!
//! The TypeScript builds each area as a closure over the services it
//! declares and composes their API objects. Here the areas are `impl`
//! blocks on one [`FormCapability`], one per TypeScript file (`read/`,
//! `write/`, `sync/`, `services/`), and their private state sits in
//! [`Core`].
//!
//! Turn counts are behaviour: the trace records how an engine answer and the
//! model updates interleave. Every TypeScript `async` function is a spawned
//! promise ([`epdf_kernel::rt::Promise::spawn`]), an awaited engine call is
//! adopted ([`crate::promise::await_engine`]), and an `async` function that
//! returns a promise costs the adoption turns ([`crate::promise::adopt`]).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use epdf_kernel::engine::DocumentHandle;
use epdf_kernel::rt::Promise;
use epdf_kernel::scope::Teardown;
use epdf_kernel::serial_queue::SerialQueue;
use epdf_kernel::{Controller, Error};
use epdf_plugin_actions::{ACTIONS, ActionsCapability, HoverPump};
use epdf_plugin_annotation::{ANNOTATION, AnnotationCapability};

use crate::contract::*;
use crate::core::fill_items::FillItem;
use crate::core::model::{FieldKey, Model, Msg, update};
use crate::model::{FormAction, FormState};
use crate::scripting::controller::FormScriptingController;
use crate::services::events::FormEvents;

type FillCache = HashMap<String, (f64, bool, Rc<Vec<FillItem>>)>;
type FillItemCache = HashMap<String, (f64, bool, Option<Rc<FillItem>>)>;

/// The plugin's private state, by the area that owns it.
pub(crate) struct Core {
    pub(crate) cx: Controller<FormState, FormAction>,
    pub(crate) host: FormHost,
    // services/
    pub(crate) events: FormEvents,
    /// The widget plane, when the annotation plugin is present.
    pub(crate) annotation: Option<AnnotationCapability>,
    /// The script realm owner and the one surface port for script results.
    pub(crate) actions: Option<ActionsCapability>,
    /// The K/V/C/F transaction over the actions plugin's realm, when
    /// scripting is on.
    pub(crate) scripting: Option<FormScriptingController>,
    /// Every durable write rides this one queue.
    pub(crate) queue: SerialQueue,
    // sync/hydration
    pub(crate) refresh_run: RefCell<Option<Promise<()>>>,
    pub(crate) refresh_again: Cell<bool>,
    // read/widgets
    pub(crate) geom_loading: RefCell<Vec<f64>>,
    pub(crate) fill_cache: RefCell<FillCache>,
    pub(crate) fill_item_cache: RefCell<FillItemCache>,
    // write/activation
    pub(crate) widget_pump: RefCell<Option<HoverPump>>,
}

/// `FormHostCapability`, which includes the public `FormCapability`. Cheap
/// to clone; clones share state.
#[derive(Clone)]
pub struct FormCapability(pub(crate) Rc<Core>);

/// `FormHostCapability`: the same object as the public capability.
pub type FormHostCapability = FormCapability;

/// `createFormController(ctx, config)`: the capability and its `connect`,
/// which subscribes to the document events and starts hydration.
pub fn create_form_controller(
    cx: Controller<FormState, FormAction>,
    config: &FormConfig,
) -> Result<FormCapability, Error> {
    let events = FormEvents::new(&cx, config.host.console_error.clone());
    let annotation = cx.try_get(&ANNOTATION)?;
    let actions = cx.try_get(&ACTIONS)?;
    let scripting = crate::services::scripting::create_scripting_controller(&cx, config, &actions);
    let core = Core {
        cx,
        host: config.host.clone(),
        events,
        annotation,
        actions,
        scripting,
        queue: SerialQueue::new(),
        refresh_run: RefCell::new(None),
        refresh_again: Cell::new(false),
        geom_loading: RefCell::new(Vec::new()),
        fill_cache: RefCell::new(HashMap::new()),
        fill_item_cache: RefCell::new(HashMap::new()),
        widget_pump: RefCell::new(None),
    };
    Ok(FormCapability(Rc::new(core)))
}

impl FormCapability {
    /// `connect()`: subscribe to the document events, then hydrate.
    pub fn connect(&self) {
        crate::sync::document_events::subscribe_document_events(self);
        self.refresh().ignore();
    }

    // ── plumbing over `ctx` ───────────────────────────────────────────────────

    pub(crate) fn cx(&self) -> &Controller<FormState, FormAction> {
        &self.0.cx
    }

    /// `ctx.doc`: the guarded handle, `None` once there is none.
    pub(crate) fn doc(&self) -> Option<Rc<dyn DocumentHandle>> {
        self.0.cx.doc().ok()
    }

    /// `store.model()`.
    pub fn model(&self) -> Rc<Model> {
        self.0.cx.state().model.clone()
    }

    /// `store.apply(msg)`: the one write door to the model.
    pub(crate) fn apply(&self, msg: Msg) {
        let model = update(&self.model(), &msg);
        self.0.cx.dispatch(FormAction::SetModel { model });
    }

    /// `store.setStatus(status)`.
    pub(crate) fn set_status(&self, status: ResourceStatus) {
        if self.0.cx.state().status != status {
            self.0.cx.dispatch(FormAction::SetStatus { status });
        }
    }

    /// `store.clearGeom()`: drop every page's cached widget geometry.
    pub(crate) fn clear_geom(&self) {
        self.apply(Msg::ClearGeom { page_object_number: None });
    }

    pub(crate) fn warn(&self, message: &str, error: &Error) {
        if let Some(sink) = &self.0.host.console_warn {
            sink(message, error);
        }
    }

    pub(crate) fn console_error(&self, message: &str, error: &Error) {
        if let Some(sink) = &self.0.host.console_error {
            sink(message, error);
        }
    }

    /// `ctx.cleanup(fn)`.
    pub(crate) fn defer_fn(&self, f: impl FnOnce() + 'static) {
        let teardown: Teardown = Box::new(move || {
            f();
            Ok(None)
        });
        self.0.cx.defer(teardown);
    }
}

/// `store.keyOf(ref)`: `obj:N` for an object-number ref, `fqn:name`
/// otherwise.
pub fn key_of(field_ref: &serde_json::Value) -> FieldKey {
    crate::scripting::overlay::ref_key(field_ref)
}
