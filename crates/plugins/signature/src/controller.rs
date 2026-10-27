//! `controller.ts`: the composition root. It builds the plugin's services
//! once, wires each area with the services it declares, and assembles the
//! capability. No behavior lives here: every verb and read has a home in
//! `read/`, `write/` or `sync/`, each an `impl` block on the one
//! [`SignatureCapability`], whose private state sits in [`Core`] by the
//! TypeScript file that owns it.

use std::cell::{OnceCell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use epdf_kernel::clock::Cancel;
use epdf_kernel::mirror::Mirror;
use epdf_kernel::plugin::ConnectFn;
use epdf_kernel::settings::InstanceSettings;
use epdf_kernel::{Controller, Error, Unsubscribe};
use epdf_plugin_form::{FORM, FormCapability};
use serde_json::Value;

use crate::contract::{SignatureConfig, SignatureSettings, SignatureSettingsChanged};
use crate::model::SignatureState;
use crate::read::signatures::Lists;
use crate::services::events::{SignatureEvents, create_events};
use crate::services::settings::PortTable;

/// `SignatureContext`: the plugin context every area receives, typed to
/// the signature state.
pub type SignatureContext = Controller<SignatureState, Value>;

/// The plugin's services and every area's own data.
pub struct Core {
    pub(crate) cx: SignatureContext,
    /// `ctx.settings()`: the instance's view of the registration's store.
    pub(crate) settings: InstanceSettings,
    /// The registration's key and trust ports (`services/settings.rs`).
    pub(crate) table: Rc<PortTable>,
    // services/events
    pub(crate) events: SignatureEvents,
    // services/siblings: the form plugin, required.
    pub(crate) form: FormCapability,
    // sync/signatures: the mirrored facts, and the parked signings the
    // engine disowned.
    pub(crate) mirror: OnceCell<Mirror>,
    pub(crate) disowned: RefCell<HashSet<String>>,
    // sync/validation: the scheduled re-judgement.
    pub(crate) cancel_revalidate: RefCell<Option<Cancel>>,
    // read/signatures: the lists, memoized per snapshot and verdicts.
    pub(crate) lists: RefCell<Option<Lists>>,
    // write/sign: two-phase signings this session prepared.
    pub(crate) prepared: RefCell<HashMap<String, Value>>,
}

/// `SignatureCapability`, which the host lens equals. Cheap to clone;
/// clones share state.
#[derive(Clone)]
pub struct SignatureCapability(pub(crate) Rc<Core>);

/// `SignatureHostCapability`.
pub type SignatureHostCapability = SignatureCapability;

/// `createSignatureController(ctx)`: the api and its `connect`, which
/// follows the document's edits and wires the armed-mark handler when the
/// interaction hub and the stamp plugin are there. `table` is the
/// registration's port table, which the store's JSON names ports by.
pub fn create_signature_controller(
    cx: SignatureContext,
    table: Rc<PortTable>,
) -> Result<(SignatureCapability, ConnectFn), Error> {
    // `createServices(ctx)`: the siblings, then the events.
    let form = cx.get(&FORM)?;
    let events = create_events(&cx);
    let settings = cx.settings()?;
    let api = SignatureCapability(Rc::new(Core {
        cx: cx.clone(),
        settings,
        table,
        events,
        form,
        mirror: OnceCell::new(),
        disowned: RefCell::default(),
        cancel_revalidate: RefCell::new(None),
        lists: RefCell::new(None),
        prepared: RefCell::default(),
    }));
    // Sync: the engine's facts, mirrored, and the judgement that reacts to
    // them.
    let mirror = api.create_signatures_mirror()?;
    let _ = api.0.mirror.set(mirror);

    let connecting = api.clone();
    let connect: ConnectFn = Rc::new(move || {
        connecting.connect_signatures()?;
        connecting.connect_siblings()?;
        Ok(())
    });
    Ok((api, connect))
}

impl SignatureCapability {
    pub(crate) fn cx(&self) -> &SignatureContext {
        &self.0.cx
    }

    /// The instance lifetime, aborted when the document closes.
    pub fn lifetime(&self) -> &epdf_kernel::signal::AbortSignal {
        self.0.cx.lifetime()
    }

    /// `ctx.state.get()`.
    pub(crate) fn state(&self) -> Rc<SignatureState> {
        self.0.cx.state()
    }

    /// `ctx.state.update(transition)`.
    pub(crate) fn update_state(
        &self,
        transition: impl FnOnce(&Rc<SignatureState>) -> Rc<SignatureState>,
    ) {
        self.0.cx.update_state(|state| transition(&state));
    }

    /// The signatures mirror.
    pub(crate) fn mirror(&self) -> &Mirror {
        self.0.mirror.get().expect("the signatures mirror is built with the controller")
    }

    /// `ctx.cancellable(signal, task)`.
    pub(crate) fn cancellable<T: Clone + 'static>(
        &self,
        signal: Option<epdf_kernel::signal::AbortSignal>,
        task: epdf_kernel::rt::Promise<T>,
    ) -> epdf_kernel::rt::Promise<T> {
        epdf_kernel::cancellable::cancellable("signature", signal, task)
    }

    // ── settings (`SettingsApi<SignatureSettings>`) ──

    /// The settings as the kernel's JSON store holds them, the ports as
    /// handles.
    pub fn settings_json(&self) -> Rc<Value> {
        self.0.settings.get()
    }

    /// `getSettings()`: the settings in effect.
    pub fn get_settings(&self) -> SignatureSettings {
        self.0.table.decode(&self.0.settings.get())
    }

    /// `settings.get()`, as the areas read it.
    pub(crate) fn settings(&self) -> SignatureSettings {
        self.get_settings()
    }

    /// `updateSettings(changes)`: change the settings for every document.
    pub fn update_settings(&self, changes: &SignatureConfig) {
        self.0.settings.store.update_settings(&self.0.table.encode(changes));
        self.0.table.release_unnamed(&self.0.settings.store);
    }

    /// How many ports the registration's table holds (M37's guard).
    #[doc(hidden)]
    pub fn retained_ports(&self) -> usize {
        self.0.table.retained()
    }

    /// `resetSettings()`: back to the registered settings.
    pub fn reset_settings(&self) {
        self.0.settings.store.reset_settings();
    }

    /// `onSettingsChanged(listener)`.
    pub fn on_settings_changed(
        &self,
        listener: impl Fn(&SignatureSettingsChanged) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        let table = self.0.table.clone();
        self.0.settings.events.on(move |event| {
            listener(&SignatureSettingsChanged {
                settings: table.decode(&event.settings),
                changed: event.changed.clone(),
            })
        })
    }

    /// The raw store event, for a caller that prints the settings JSON.
    pub fn on_settings_changed_json(
        &self,
        listener: impl Fn(&Value, &[String]) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.settings.events.on(move |event| listener(&event.settings, &event.changed))
    }
}
