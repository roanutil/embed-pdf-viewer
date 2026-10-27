//! `controller.ts`: the composition root. It builds the plugin's services
//! once, wires each area with what it needs, and the capability is the
//! areas' methods: reads in [`crate::read`], writes in [`crate::write`],
//! the viewport sync in [`crate::sync`]. No behavior lives here.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use epdf_kernel::page_mirror::PageMirror;
use epdf_kernel::plugin::ConnectFn;
use epdf_kernel::settings::InstanceSettings;
use epdf_kernel::{Controller, Error, Unsubscribe};
use epdf_plugin_annotation::AnnotationCapability;
use epdf_plugin_interaction::InteractionCapability;
use serde_json::Value;

use crate::contract::*;
use crate::model::MeasurementState;
use crate::services::{MeasurementEvents, Registry, create_events, resolve_siblings};
use crate::sync::viewports::ScaleMemo;

/// `MeasurementContext`: the kernel's controller context, typed to this
/// plugin's state.
pub type MeasurementContext = Controller<MeasurementState, Value>;

pub(crate) struct Core {
    pub(crate) cx: MeasurementContext,
    pub(crate) events: MeasurementEvents,
    pub(crate) registry: Rc<Registry>,
    pub(crate) annotation: AnnotationCapability,
    pub(crate) interaction: InteractionCapability,
    pub(crate) settings: InstanceSettings,
    /// `sync/viewports.ts`: the page mirror of each page's viewports, and
    /// the public scale memoized per page.
    pub(crate) viewports: PageMirror,
    pub(crate) scales: RefCell<HashMap<u32, ScaleMemo>>,
    /// `write/scale.ts` `pageQueues`: each page's write queue, whose
    /// `idle()` a download waits for.
    pub(crate) queues: crate::write::scale::PageQueues,
}

/// `MeasurementHostCapability`, which is the public `MeasurementCapability`.
/// Cheap to clone; clones share state.
#[derive(Clone)]
pub struct MeasurementCapability(pub(crate) Rc<Core>);

/// `createMeasurementController(ctx)`: the capability and its `connect`
/// half, which wires the calibration capture and then the viewport sync.
pub fn create_measurement_controller(
    cx: MeasurementContext,
) -> Result<(MeasurementCapability, ConnectFn), Error> {
    let events = create_events(&cx);
    let registry = Rc::new(Registry::new(cx.cx.clone()));
    let siblings = resolve_siblings(&cx)?;
    let settings = cx.settings()?;
    let viewports = crate::sync::viewports::create_viewport_mirror(
        &cx,
        &registry,
        &settings,
        &siblings.annotation,
    )?;
    let queues = crate::write::scale::wire_settle(&cx)?;
    let api = MeasurementCapability(Rc::new(Core {
        cx,
        events,
        registry,
        annotation: siblings.annotation,
        interaction: siblings.interaction,
        settings,
        viewports,
        scales: RefCell::default(),
        queues,
    }));
    let connected = api.clone();
    let connect: ConnectFn = Rc::new(move || {
        connected.connect_calibration();
        connected.connect_viewports();
        Ok(())
    });
    Ok((api, connect))
}

impl MeasurementCapability {
    pub(crate) fn core(&self) -> &Core {
        &self.0
    }

    /// The instance lifetime, aborted when the document closes. A foreign
    /// handle outlives the close and checks this before every call.
    #[doc(hidden)]
    pub fn lifetime(&self) -> &epdf_kernel::signal::AbortSignal {
        self.0.cx.lifetime()
    }

    /// May write a page scale and calibrate (`doc.annotate.modify`).
    pub fn can_calibrate(&self) -> bool {
        self.0.registry.can_calibrate()
    }

    // ── settings (`SettingsApi<MeasurementSettings>`) ──

    /// The settings in effect. `Err` for a `presets` value the typed read
    /// cannot hold (one written as JSON past the typed API);
    /// [`settings_json`](Self::settings_json) reads it as it is.
    pub fn get_settings(&self) -> Result<MeasurementSettings, Error> {
        MeasurementSettings::of(&self.0.settings.get())
    }

    /// The settings as the kernel's JSON store holds them, which is what the
    /// TypeScript `getSettings()` returns.
    pub fn settings_json(&self) -> Rc<Value> {
        self.0.settings.get()
    }

    /// Change the settings for every document; `defaultScale` is replaced
    /// whole.
    pub fn update_settings(&self, changes: &MeasurementConfig) {
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

    /// The settings changed. A change the typed read cannot hold goes to
    /// the kernel's report instead of the listener.
    pub fn on_settings_changed(
        &self,
        listener: impl Fn(&MeasurementSettingsChanged) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.settings.events.on(move |event| {
            listener(&MeasurementSettingsChanged {
                settings: MeasurementSettings::of(&event.settings)?,
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

    // ── events ──

    pub fn on_scale_changed(
        &self,
        listener: impl Fn(&MeasurementScaleChangedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.scale_changed.on(listener)
    }

    pub fn on_calibration_requested(
        &self,
        listener: impl Fn(&CalibrationRequestedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.calibration_requested.on(listener)
    }

    pub fn on_calibration_completed(
        &self,
        listener: impl Fn(&CalibrationCompletedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.calibration_completed.on(listener)
    }

    pub fn on_calibration_dismissed(
        &self,
        listener: impl Fn(&CalibrationDismissedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.calibration_dismissed.on(listener)
    }
}
