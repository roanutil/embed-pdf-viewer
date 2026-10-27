//! `controller.ts`: the composition root. It builds the plugin's services
//! once and holds them in one [`StampCapability`]; every verb and read has
//! a home in `read/` or `write/` as an `impl` block on it. The one
//! state-change event (`onArmChanged`) is derived here.

use std::collections::HashSet;
use std::rc::Rc;

use epdf_kernel::settings::InstanceSettings;
use epdf_kernel::{Controller, Error, Unsubscribe};
use serde_json::Value;

use crate::contract::*;
use crate::model::StampState;
use crate::services::asset_engine::AssetEngineState;
use crate::services::binaries::Binaries;
use crate::services::events::{StampEvents, create_events};
use crate::services::settings::EngineTable;

/// `StampContext`: the plugin context every area receives, typed to the
/// stamp state.
pub type StampContext = Controller<StampState, Value>;

/// The plugin's services, by the TypeScript file that owns each.
pub struct Core {
    pub(crate) cx: StampContext,
    /// `ctx.settings()`: the instance's view of the registration's store.
    pub(crate) settings: InstanceSettings,
    /// The registration's asset engine handles (`services/settings.rs`).
    pub(crate) table: Rc<EngineTable>,
    pub(crate) events: StampEvents,
    pub(crate) binaries: Rc<Binaries>,
    pub(crate) asset_engine: Rc<AssetEngineState>,
}

/// `StampCapability`. Cheap to clone; clones share state.
#[derive(Clone)]
pub struct StampCapability(pub(crate) Rc<Core>);

/// `createStampController(ctx)`. `table` is the registration's asset
/// engine table, which the store's JSON names engines by.
pub fn create_stamp_controller(
    cx: StampContext,
    table: Rc<EngineTable>,
) -> Result<StampCapability, Error> {
    // `createServices(ctx)`, in its order: the binaries, the asset engine
    // (which reads the settings), the events.
    let binaries = Rc::new(Binaries::default());
    let cleared = binaries.clone();
    cx.defer(Box::new(move || {
        cleared.clear();
        Ok(None)
    }));
    let settings = cx.settings()?;
    table.attach(&settings.store);
    let asset_engine = Rc::new(AssetEngineState::default());
    let released = asset_engine.clone();
    cx.defer(Box::new(move || {
        released.release();
        Ok(None)
    }));
    let events = create_events(&cx);
    let api = StampCapability(Rc::new(Core {
        cx: cx.clone(),
        settings,
        table,
        events,
        binaries,
        asset_engine,
    }));

    // What is armed on a document is state: the event follows it, whichever
    // verb, tool switch or closed document changed it.
    let emitter = api.clone();
    cx.on_state_change(move |next, previous| {
        if next.armed == previous.armed {
            return Ok(());
        }
        let mut seen = HashSet::new();
        let ids: Vec<String> = previous
            .armed
            .keys()
            .chain(next.armed.keys())
            .filter(|id| seen.insert((*id).clone()))
            .cloned()
            .collect();
        for document_id in ids {
            let asset_id = next.armed.get(&document_id);
            if previous.armed.get(&document_id) == asset_id {
                continue;
            }
            let asset = asset_id.and_then(|id| next.assets.get(id)).cloned();
            emitter.0.events.arm_changed.emit(&StampArmChangedEvent { document_id, asset });
        }
        Ok(())
    });

    // Placement forgets the arms the annotation plugin dropped on its own.
    api.watch_dropped_arms()?;
    Ok(api)
}

impl StampCapability {
    pub(crate) fn cx(&self) -> &StampContext {
        &self.0.cx
    }

    /// `ctx.state.get()`.
    pub(crate) fn state(&self) -> Rc<StampState> {
        self.0.cx.state()
    }

    /// `ctx.state.update(transition)`.
    pub(crate) fn update_state(&self, transition: impl FnOnce(&Rc<StampState>) -> Rc<StampState>) {
        self.0.cx.update_state(|state| transition(&state));
    }

    // ── settings (`SettingsApi<StampSettings>`) ──

    /// The settings as the kernel's JSON store holds them.
    pub fn settings_json(&self) -> Rc<Value> {
        self.0.settings.get()
    }

    /// `getSettings()`: the settings in effect.
    pub fn get_settings(&self) -> StampSettings {
        self.0.table.decode(&self.0.settings.get())
    }

    /// `updateSettings(changes)`: change the settings for the workspace.
    pub fn update_settings(&self, changes: &StampConfig) {
        self.0.settings.store.update_settings(&self.0.table.encode(changes));
        self.0.table.release_unnamed(&self.0.settings.store);
    }

    /// How many engines the registration's table holds (M37's guard).
    #[doc(hidden)]
    pub fn retained_engines(&self) -> usize {
        self.0.table.retained()
    }

    /// `resetSettings()`: back to the registered settings.
    pub fn reset_settings(&self) {
        self.0.settings.store.reset_settings();
        self.0.table.release_unnamed(&self.0.settings.store);
    }

    /// `onSettingsChanged(listener)`.
    pub fn on_settings_changed(
        &self,
        listener: impl Fn(&StampSettingsChanged) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        let table = self.0.table.clone();
        self.0.settings.events.on(move |event| {
            listener(&StampSettingsChanged {
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

    // ── events (services, not areas) ──

    /// `onLibraryChanged`: a library's bytes changed (subscribe to persist).
    pub fn on_library_changed(
        &self,
        listener: impl Fn(&StampLibraryChangedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.library_changed.on(listener)
    }

    /// `onLibraryCreated`.
    pub fn on_library_created(
        &self,
        listener: impl Fn(&StampLibraryEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.library_created.on(listener)
    }

    /// `onLibraryUpdated`.
    pub fn on_library_updated(
        &self,
        listener: impl Fn(&StampLibraryEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.library_updated.on(listener)
    }

    /// `onLibraryDeleted` (`library: null`).
    pub fn on_library_deleted(
        &self,
        listener: impl Fn(&StampLibraryEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.library_deleted.on(listener)
    }

    /// `onAssetCreated`.
    pub fn on_asset_created(
        &self,
        listener: impl Fn(&StampAssetEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.asset_created.on(listener)
    }

    /// `onAssetUpdated`.
    pub fn on_asset_updated(
        &self,
        listener: impl Fn(&StampAssetEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.asset_updated.on(listener)
    }

    /// `onAssetDeleted` (`asset: null`).
    pub fn on_asset_deleted(
        &self,
        listener: impl Fn(&StampAssetEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.asset_deleted.on(listener)
    }

    /// `onArmChanged`: a stamp was armed or disarmed on a document.
    pub fn on_arm_changed(
        &self,
        listener: impl Fn(&StampArmChangedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.arm_changed.on(listener)
    }
}
