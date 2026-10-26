//! `controller.ts`: the composition root. It builds the plugin's services
//! once and holds them in one [`RedactionCapability`]; every verb and read
//! lives in `read/`, `write/` or `sync/` as an `impl` block on it.

use std::rc::Rc;

use epdf_kernel::plugin::ConnectFn;
use epdf_kernel::settings::InstanceSettings;
use epdf_kernel::{Controller, Error, Unsubscribe};
use serde_json::Value;

use crate::contract::*;
use crate::model::RedactionState;
use crate::read::pending::PendingMemo;
use crate::services::{
    RedactionEvents, RedactionSiblings, RedactionStore, create_events, resolve_siblings,
};
use crate::write::apply::ApplyQueue;

/// `RedactionContext`: the kernel's controller context, typed to this
/// plugin's state.
pub type RedactionContext = Controller<RedactionState, Value>;

pub(crate) struct Core {
    pub(crate) cx: RedactionContext,
    pub(crate) events: RedactionEvents,
    pub(crate) store: RedactionStore,
    pub(crate) siblings: RedactionSiblings,
    pub(crate) settings: InstanceSettings,
    pub(crate) pending: PendingMemo,
    pub(crate) queue: ApplyQueue,
}

/// `RedactionCapability`, which is also the host lens. Cheap to clone;
/// clones share state.
#[derive(Clone)]
pub struct RedactionCapability(pub(crate) Rc<Core>);

/// `createRedactionController(ctx)`: the capability and its `connect` half,
/// which subscribes the document and annotation feeds and writes the
/// overlay settings into the redact tool's defaults.
pub fn create_redaction_controller(
    cx: RedactionContext,
) -> Result<(RedactionCapability, ConnectFn), Error> {
    let events = create_events(&cx);
    let store = RedactionStore::new(cx.cx.clone());
    let siblings = resolve_siblings(&cx)?;
    let settings = cx.settings()?;
    let queue = ApplyQueue::new(&cx)?;
    let api = RedactionCapability(Rc::new(Core {
        cx,
        events,
        store,
        siblings,
        settings,
        pending: PendingMemo::default(),
        queue,
    }));
    let connected = api.clone();
    let connect: ConnectFn = Rc::new(move || {
        connected.subscribe_changes()?;
        connected.sync_overlay()
    });
    Ok((api, connect))
}

impl RedactionCapability {
    pub(crate) fn core(&self) -> &Core {
        &self.0
    }

    /// The instance lifetime, aborted when the document closes. A foreign
    /// handle outlives the close and checks this before every call.
    #[doc(hidden)]
    pub fn lifetime(&self) -> &epdf_kernel::signal::AbortSignal {
        self.0.cx.lifetime()
    }

    // ── settings (`SettingsApi<RedactionSettings>`) ──

    /// The settings in effect.
    pub fn get_settings(&self) -> RedactionSettings {
        RedactionSettings::of(&self.0.settings.get())
    }

    /// The settings as the kernel's JSON store holds them, which is what the
    /// TypeScript `getSettings()` returns.
    pub fn settings_json(&self) -> Rc<Value> {
        self.0.settings.get()
    }

    /// Change the settings for every document.
    pub fn update_settings(&self, changes: &RedactionConfig) {
        self.update_settings_json(&changes.to_json());
    }

    /// [`update_settings`](Self::update_settings) with the change as JSON.
    pub fn update_settings_json(&self, changes: &Value) {
        self.0.settings.store.update_settings(changes);
    }

    /// Back to the registered settings.
    pub fn reset_settings(&self) {
        self.0.settings.store.reset_settings();
    }

    /// The settings changed.
    pub fn on_settings_changed(
        &self,
        listener: impl Fn(&RedactionSettingsChanged) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.settings.events.on(move |event| {
            listener(&RedactionSettingsChanged {
                settings: RedactionSettings::of(&event.settings),
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

    // ── the controller's own members ──

    /// Whether marking is allowed: a mark is an annotation, so this is the
    /// session's annotation create permission, what the annotation plugin's
    /// `canCreate` reads too.
    pub fn can_mark(&self) -> bool {
        self.0.cx.allows("annotations:create")
    }

    /// Whether applying is allowed: the engine has a redaction service and
    /// every capability its apply asserts is granted.
    pub fn can_apply(&self) -> bool {
        self.0.store.can_apply()
    }

    /// An apply of this session is running at the engine.
    pub fn is_applying(&self) -> bool {
        self.0.cx.state().applying
    }

    /// The last confirmed apply result, from this session or another.
    pub fn get_last_result(&self) -> Option<RedactionApplyResult> {
        self.0.cx.state().last_result.clone()
    }

    /// A confirmed apply, from this session or another; fires after
    /// `get_last_result()` changed.
    pub fn on_applied(
        &self,
        listener: impl Fn(&RedactionAppliedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.applied.on(listener)
    }

    /// Marks were created, changed or removed on some pages.
    pub fn on_pending_changed(
        &self,
        listener: impl Fn(&RedactionPendingChangedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.pending_changed.on(listener)
    }
}
