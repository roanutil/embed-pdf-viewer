//! `controller.ts`: the composition root. It builds the plugin's services
//! once, wires each area, and hands out the one capability object.
//!
//! The TypeScript builds each area as a closure over the services it
//! declares and composes the api from their slices. Rust keeps one shared
//! [`Inner`] and gives each area its own `impl ActionsCapability` block in
//! the module named after the TypeScript file (`dispatch/`, `lifecycle/`,
//! `scripting/`, `submit/`, `services/`), so every verb still has one home.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use epdf_acrojs::{ScriptDiagnostic, ScriptExecutionError, ScriptHost};
use epdf_kernel::plugin::ConnectFn;
use epdf_kernel::rt::Promise;
use epdf_kernel::serial_queue::SerialQueue;
use epdf_kernel::{Controller, Error, Unsubscribe};
use serde_json::Value;

use crate::contract::{
    ActionDiagnostic, ActionDispatchEvent, ActionPolicy, ActionsConfig, OpenSequenceCompletedEvent,
};
use crate::dispatch::triggers::LifecycleAnnot;
use crate::host_contract::PageStateReport;
use crate::lifecycle::page_lifecycle::LastEmitted;
use crate::model::{ActionsAction, ActionsState};
use crate::scripting::environment::ScriptRealmFactory;
use crate::services::policy::merge_policy;
use crate::services::ports::Ports;
use crate::services::{Events, policy::default_policy};

pub(crate) struct Inner {
    pub(crate) cx: Controller<ActionsState, ActionsAction>,
    pub(crate) config: ActionsConfig,
    // ── services ──
    pub(crate) events: Events,
    /// The live policy: read at decision time, never captured.
    pub(crate) policy: RefCell<ActionPolicy>,
    pub(crate) ports: Ports,
    /// The one serial queue every dispatch, verb and open sequence rides.
    pub(crate) queue: SerialQueue,
    /// D11's aggregate: JS nodes run in the current queue operation.
    pub(crate) script_nodes: Cell<f64>,
    /// The one memoized catalog-actions read; a rejected read is evicted.
    pub(crate) catalog: RefCell<Option<Promise<Option<Value>>>>,
    /// The document-print latch: while a print event wrapper is active, a
    /// nested print request is suppressed.
    pub(crate) print_latch: Cell<bool>,
    // ── scripting/realm ──
    pub(crate) realms: Option<ScriptRealmFactory>,
    pub(crate) script_host: Option<ScriptHost>,
    // ── dispatch/triggers ──
    pub(crate) lifecycle_cache: RefCell<Vec<(u32, Vec<LifecycleAnnot>)>>,
    // ── lifecycle/page-lifecycle ──
    pub(crate) barrier_open: Cell<bool>,
    pub(crate) buffered_report: RefCell<Option<PageStateReport>>,
    pub(crate) last_emitted: RefCell<LastEmitted>,
    pub(crate) cascade_rounds: Cell<f64>,
    // ── lifecycle/open-sequence ──
    pub(crate) open_fired: Cell<bool>,
    pub(crate) saw_user_activity: Cell<bool>,
}

/// `ActionsCapability` and `ActionsHostCapability`: one runtime object. The
/// host members (`register_*_commit_sink`, `register_submit_resolver`,
/// `script_realm`, `create_detached_script_realm`, `surface_script_*`,
/// `report_page_state`) are for sibling plugins, not applications. Cheap to
/// clone; clones are the same instance.
#[derive(Clone)]
pub struct ActionsCapability(pub(crate) Rc<Inner>);

impl ActionsCapability {
    pub(crate) fn weak(&self) -> Weak<Inner> {
        Rc::downgrade(&self.0)
    }

    pub(crate) fn upgrade(weak: &Weak<Inner>) -> Option<ActionsCapability> {
        weak.upgrade().map(ActionsCapability)
    }

    /// The instance lifetime, aborted when the document closes. A foreign
    /// handle outlives the close, checks this before every call, and ends
    /// its event streams when it aborts.
    #[cfg(feature = "uniffi")]
    #[doc(hidden)]
    pub fn lifetime(&self) -> &epdf_kernel::signal::AbortSignal {
        self.0.cx.lifetime()
    }

    // ── the observability hooks ───────────────────────────────────────────

    /// Every executed tree: dispatch-driven, verb-driven and lifecycle-driven.
    pub fn on_executed(
        &self,
        listener: impl Fn(&ActionDispatchEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.action_hook.on(listener)
    }

    pub fn on_diagnostic(
        &self,
        listener: impl Fn(&ActionDiagnostic) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.diagnostic_hook.on(listener)
    }

    /// Script-plane observability, from dispatches and from the form
    /// pipeline alike.
    pub fn on_script_diagnostic(
        &self,
        listener: impl Fn(&ScriptDiagnostic) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.script_diagnostic_hook.on(listener)
    }

    pub fn on_script_error(
        &self,
        listener: impl Fn(&ScriptExecutionError) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.script_error_hook.on(listener)
    }

    /// The document-open sequence ran (OpenAction, open destination).
    pub fn on_open_sequence_completed(
        &self,
        listener: impl Fn(&OpenSequenceCompletedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.open_sequence_hook.on(listener)
    }
}

/// `createActionsController(ctx, config)`.
pub fn create_actions_controller(
    cx: Controller<ActionsState, ActionsAction>,
    config: ActionsConfig,
) -> Result<(ActionsCapability, Option<ConnectFn>), Error> {
    let events = Events::new(&cx, &config);
    let policy = merge_policy(&default_policy(), config.policy.as_ref());
    let inner = Rc::new_cyclic(|weak: &Weak<Inner>| {
        let realms = ScriptRealmFactory::for_config(&cx, &config);
        let script_host = realms.as_ref().map(|realms| realms.own_realm(&cx, weak.clone()));
        Inner {
            cx: cx.clone(),
            config: config.clone(),
            events,
            policy: RefCell::new(policy),
            ports: Ports::default(),
            queue: SerialQueue::new(),
            script_nodes: Cell::new(0.0),
            catalog: RefCell::new(None),
            print_latch: Cell::new(false),
            realms,
            script_host,
            lifecycle_cache: RefCell::new(Vec::new()),
            barrier_open: Cell::new(false),
            buffered_report: RefCell::new(None),
            last_emitted: RefCell::new(LastEmitted::default()),
            cascade_rounds: Cell::new(0.0),
            open_fired: Cell::new(false),
            saw_user_activity: Cell::new(false),
        }
    });
    let this = ActionsCapability(inner);
    if let Some(host) = this.0.script_host.clone() {
        this.0.cx.defer(Box::new(move || {
            host.dispose();
            Ok(None)
        }));
    }
    this.subscribe_lifecycle_cache();
    this.register_script_executor();
    this.arm_open_sequence();
    Ok((this, None))
}
