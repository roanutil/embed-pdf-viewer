//! `controller.ts`: the composition root. It builds the plugin's services
//! once, wires each area, and hands out the one capability object.
//!
//! The TypeScript builds each area as a closure over the services it
//! declares and composes the api from their slices. Rust keeps one shared
//! [`Inner`] and gives each area its own `impl ActionsCapability` block in
//! the module named after the TypeScript file (`dispatch/`, `lifecycle/`,
//! `scripting/`, `submit/`, `services/`), so every verb still has one home.
//! The plugin keeps no session state: its rules are its settings, and its
//! live data (ports, queue, latches) belong to the areas.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use epdf_acrojs::{ScriptDiagnostic, ScriptHost};
use epdf_kernel::plugin::ConnectFn;
use epdf_kernel::rt::Promise;
use epdf_kernel::serial_queue::SerialQueue;
use epdf_kernel::settings::InstanceSettings;
use epdf_kernel::{Controller, Error, Unsubscribe};
use serde_json::Value;

use crate::contract::{
    ActionDiagnosticReportedEvent, ActionExecutedEvent, ActionsConfig, ActionsSettings,
    ActionsSettingsChanged, ActionsSettingsPatch, DocumentVerb, JavaScriptConfig,
    OpenSequenceCompletedEvent, ScriptFailedEvent,
};
use crate::dispatch::triggers::LifecycleReads;
use crate::host_contract::PageStateReport;
use crate::lifecycle::page_lifecycle::LastEmitted;
use crate::scripting::environment::ScriptRealmFactory;
use crate::services::ports::Ports;
use crate::services::{DocumentPrint, Events};
use crate::settings::IdentityTable;

/// The controller context: stateless, so its slice is `undefined`.
pub(crate) type ActionsController = Controller<Value, Value>;

pub(crate) struct Inner {
    pub(crate) cx: ActionsController,
    /// The plugin's settings store as this instance sees it.
    pub(crate) settings: InstanceSettings,
    /// The registration's function identities (see `settings.rs`).
    pub(crate) table: Rc<IdentityTable>,
    /// The script environment, read from the config here and nowhere else.
    pub(crate) environment: JavaScriptConfig,
    // ── services ──
    pub(crate) events: Events,
    pub(crate) ports: Ports,
    /// The one serial queue every dispatch, verb and open sequence rides.
    pub(crate) queue: SerialQueue,
    /// The JS nodes run in the current queue operation.
    pub(crate) script_nodes: Cell<f64>,
    /// The one memoized catalog-actions read; a rejected read is evicted.
    pub(crate) catalog: RefCell<Option<Promise<Option<Value>>>>,
    /// The kernel's print, when the host gave it a printer.
    pub(crate) document_print: DocumentPrint,
    /// The document-print latch: while a print wrapper is active, a nested
    /// print request is suppressed.
    pub(crate) print_latch: Cell<bool>,
    // ── scripting/realm ──
    pub(crate) realms: Option<ScriptRealmFactory>,
    pub(crate) script_host: Option<ScriptHost>,
    // ── dispatch/triggers ──
    pub(crate) lifecycle_reads: RefCell<LifecycleReads>,
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
/// host members (`register_*_commit_sink`, `register_form_reader`,
/// `register_submit_resolver`, `script_realm`,
/// `create_detached_script_realm`, `surface_script_*`,
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

    // ── the settings ──────────────────────────────────────────────────────

    /// The settings in effect, for every document of this registration.
    pub fn get_settings(&self) -> ActionsSettings {
        self.settings_now()
    }

    /// Merge `changes` into the settings, for every document: policy rows
    /// merge origin by origin. Fires `on_settings_changed` once when a
    /// setting changed.
    pub fn update_settings(&self, changes: &ActionsSettingsPatch) {
        self.0.settings.store.update_settings(&self.0.table.encode(changes));
    }

    /// Go back to what the app registered, not to the defaults.
    pub fn reset_settings(&self) {
        self.0.settings.store.reset_settings();
    }

    /// Fires once for each call that changed a setting. This instance's
    /// listeners go when its document closes.
    pub fn on_settings_changed(
        &self,
        listener: impl Fn(&ActionsSettingsChanged) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        let table = Rc::downgrade(&self.0.table);
        self.0.settings.events.on(move |event| {
            let Some(table) = table.upgrade() else { return Ok(()) };
            listener(&ActionsSettingsChanged {
                settings: table.decode(&event.settings),
                changed: event.changed.clone(),
            })
        })
    }

    // ── the events ────────────────────────────────────────────────────────

    /// Actions ran, whatever started them: one event per tree.
    pub fn on_executed(
        &self,
        listener: impl Fn(&ActionExecutedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.executed.on(listener)
    }

    /// An action was blocked, reported or couldn't run; also dispatch, sink
    /// and budget problems.
    pub fn on_diagnostic_reported(
        &self,
        listener: impl Fn(&ActionDiagnosticReportedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.diagnostic_reported.on(listener)
    }

    /// A script was stopped from doing something, or used something the
    /// viewer doesn't support.
    pub fn on_script_diagnostic_reported(
        &self,
        listener: impl Fn(&ScriptDiagnostic) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.script_diagnostic_reported.on(listener)
    }

    /// A script run ended with an error.
    pub fn on_script_failed(
        &self,
        listener: impl Fn(&ScriptFailedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.script_failed.on(listener)
    }

    /// The document-open sequence ran (OpenAction, open destination).
    pub fn on_open_sequence_completed(
        &self,
        listener: impl Fn(&OpenSequenceCompletedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.open_sequence_completed.on(listener)
    }
}

/// `createActionsController(ctx, config)`. `config` is what the app
/// registered; its settings reach the controller through the settings store,
/// and only the script environment is read from it here.
pub(crate) fn create_actions_controller(
    cx: ActionsController,
    config: ActionsConfig,
    table: Rc<IdentityTable>,
) -> Result<(ActionsCapability, Option<ConnectFn>), Error> {
    let settings = cx.settings()?;
    table.watch(&settings.store);
    let events = Events::new(&cx);
    let environment = config.javascript.clone().unwrap_or_default();
    let queue = cx.serial_queue("actions");
    // `javascript.enabled`, as it is when the document opens, decides
    // whether a realm factory exists.
    let enabled = table.decode(&settings.get()).javascript.enabled;
    let inner = Rc::new_cyclic(|weak: &Weak<Inner>| {
        let realms = enabled
            .then(|| ScriptRealmFactory::new(&cx, &environment, table.clone(), settings.clone()))
            .flatten();
        let script_host = realms.as_ref().map(|realms| realms.own_realm(&cx, weak.clone()));
        Inner {
            cx: cx.clone(),
            settings,
            table,
            environment,
            events,
            ports: Ports::default(),
            queue,
            script_nodes: Cell::new(0.0),
            catalog: RefCell::new(None),
            document_print: DocumentPrint::new(&cx),
            print_latch: Cell::new(false),
            realms,
            script_host,
            lifecycle_reads: RefCell::new(LifecycleReads::default()),
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
    this.register_script_executor();
    let weak = this.weak();
    let connect: ConnectFn = Rc::new(move || {
        let Some(this) = ActionsCapability::upgrade(&weak) else { return Ok(()) };
        this.connect()
    });
    Ok((this, Some(connect)))
}

impl ActionsCapability {
    /// The `connect` half.
    fn connect(&self) -> Result<(), Error> {
        // Confirmed document events of every origin invalidate the per-page
        // lifecycle-annotation cache.
        let doc = self.doc()?;
        let weak = self.weak();
        let unsubscribe = doc.subscribe(Rc::new(move |event: &Value| {
            if let Some(this) = ActionsCapability::upgrade(&weak) {
                this.invalidate_lifecycle_reads(event);
            }
        }));
        self.0.cx.listen(unsubscribe);
        // Fires the open sequence at once for 'headless', releases the page
        // barrier for 'off', and waits for an adapter or user activity for
        // 'auto'.
        self.arm_open_sequence();
        // The document's save actions (/WS, /DS) run around every download
        // of its file, so what WillSave changes is in the bytes.
        let weak = self.weak();
        self.0.cx.around_download(Rc::new(move |read| {
            let Some(this) = ActionsCapability::upgrade(&weak) else { return Ok(read()) };
            Ok(this.run_document_verb(DocumentVerb::Save, move || Ok(read()), None))
        }))?;
        // Its print actions (/WP, /DP) run around every print, so what
        // WillPrint changes is on the paper: the kernel prepares a print
        // again when the document changed.
        let weak = self.weak();
        self.0.cx.around_print(Rc::new(move |print| {
            let Some(this) = ActionsCapability::upgrade(&weak) else { return Ok(print()) };
            Ok(this.run_document_verb(DocumentVerb::Print, move || Ok(print()), None))
        }))?;
        Ok(())
    }
}
