//! `services/`: the plugin-private services every area is built on (the
//! event hooks, the live policy, the registration ports, the serial queue,
//! the catalog read, session authority and the print latch). The queue,
//! the script-node aggregate and the print latch are fields of
//! [`crate::controller::Inner`]; the rest have modules here.

pub mod catalog;
pub mod policy;
pub mod ports;

use std::cell::RefCell;
use std::rc::Rc;

use epdf_acrojs::{ScriptDiagnostic, ScriptExecutionError};
use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::{Controller, DOCUMENTS, Error};
use serde_json::Value;

use crate::ActionsCapability;
use crate::contract::{
    ActionDiagnostic, ActionDispatchEvent, ActionsConfig, OpenSequenceCompletedEvent,
};
use crate::model::{ActionsAction, ActionsState};

/// A `diagnose` callback: collect, emit, or both.
pub(crate) type Diagnose = Rc<dyn Fn(ActionDiagnostic)>;

/// `services/events.ts`: the five observability hooks, disposed with the
/// plugin. A throwing listener goes to the host's `observer_error` sink,
/// where the TypeScript writes it to `console.error`.
pub(crate) struct Events {
    pub(crate) action_hook: EventHookSource<ActionDispatchEvent>,
    pub(crate) open_sequence_hook: EventHookSource<OpenSequenceCompletedEvent>,
    pub(crate) diagnostic_hook: EventHookSource<ActionDiagnostic>,
    pub(crate) script_diagnostic_hook: EventHookSource<ScriptDiagnostic>,
    pub(crate) script_error_hook: EventHookSource<ScriptExecutionError>,
}

fn observer_hook<T: 'static>(config: &ActionsConfig, message: &'static str) -> EventHookSource<T> {
    let sink = config.observer_error.clone();
    EventHookSource::new(Some(Rc::new(move |error: &Error| {
        if let Some(sink) = &sink {
            sink(message, error);
        }
    })))
}

impl Events {
    pub(crate) fn new(
        cx: &Controller<ActionsState, ActionsAction>,
        config: &ActionsConfig,
    ) -> Self {
        let events = Events {
            action_hook: observer_hook(config, "[actions] onExecuted observer failed:"),
            open_sequence_hook: observer_hook(
                config,
                "[actions] onOpenSequenceCompleted listener threw",
            ),
            diagnostic_hook: observer_hook(config, "[actions] onDiagnostic observer failed:"),
            script_diagnostic_hook: observer_hook(
                config,
                "[actions] onScriptDiagnostic observer failed:",
            ),
            script_error_hook: observer_hook(config, "[actions] onScriptError observer failed:"),
        };
        let hooks = (
            events.action_hook.clone(),
            events.diagnostic_hook.clone(),
            events.script_diagnostic_hook.clone(),
            events.script_error_hook.clone(),
            events.open_sequence_hook.clone(),
        );
        cx.defer(Box::new(move || {
            hooks.0.dispose();
            hooks.1.dispose();
            hooks.2.dispose();
            hooks.3.dispose();
            hooks.4.dispose();
            Ok(None)
        }));
        events
    }
}

// ── JSON reads, as JavaScript reads them ──────────────────────────────────────

pub(crate) fn get<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.get(key).filter(|v| !epdf_kernel::is_undefined(v))
}

pub(crate) fn truthy(value: Option<&Value>) -> bool {
    epdf_acrojs::actions::truthy(value)
}

pub(crate) fn str_of<'a>(value: &'a Value, key: &str) -> &'a str {
    get(value, key).and_then(Value::as_str).unwrap_or_default()
}

pub(crate) fn node_type(node: &Value) -> String {
    str_of(node, "type").to_string()
}

/// `a === b` for two JSON numbers (absent is `undefined`).
pub(crate) fn same_number(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => match (a.as_f64(), b.as_f64()) {
            (Some(x), Some(y)) => x == y,
            _ => a == b,
        },
        _ => false,
    }
}

impl ActionsCapability {
    pub(crate) fn emit_diagnostic(&self, diagnostic: ActionDiagnostic) {
        self.0.events.diagnostic_hook.emit(&diagnostic);
    }

    /// A `diagnose` that only emits.
    pub(crate) fn emitter(&self) -> Diagnose {
        let this = self.clone();
        Rc::new(move |d| this.emit_diagnostic(d))
    }

    /// A `diagnose` that collects into `into` and emits.
    pub(crate) fn collector(&self, into: &Rc<RefCell<Vec<ActionDiagnostic>>>) -> Diagnose {
        let (this, into) = (self.clone(), into.clone());
        Rc::new(move |d: ActionDiagnostic| {
            into.borrow_mut().push(d.clone());
            this.emit_diagnostic(d);
        })
    }

    /// `services/authority.ts`: session authority the action plane enforces
    /// itself.
    pub(crate) fn allows_print(&self) -> bool {
        match self.0.cx.try_get(&DOCUMENTS) {
            Ok(Some(documents)) => documents.allows("doc.print", self.0.cx.document_id()),
            _ => true,
        }
    }

    /// The guarded handle. A document plugin always has one once built.
    pub(crate) fn doc(&self) -> Result<Rc<dyn epdf_kernel::engine::DocumentHandle>, Error> {
        self.0.cx.doc()
    }

    /// The document's `PageLayout[]`.
    pub(crate) fn pages(&self) -> Vec<Value> {
        self.0.cx.document().and_then(|meta| meta.pages.as_array().cloned()).unwrap_or_default()
    }

    /// `ctx.document()?.pages[0]?.ref.pageObjectNumber`.
    pub(crate) fn first_pon(&self) -> Option<u32> {
        self.pages()
            .first()
            .and_then(|p| get(p, "ref"))
            .and_then(epdf_kernel::geometry::PageRef::from_json)
            .map(|r| r.page_object_number)
    }

    /// The layout of the page with this object number.
    pub(crate) fn layout_of(&self, pon: u32) -> Option<Value> {
        self.pages().into_iter().find(|p| {
            get(p, "ref")
                .and_then(epdf_kernel::geometry::PageRef::from_json)
                .map(|r| r.page_object_number)
                == Some(pon)
        })
    }
}
