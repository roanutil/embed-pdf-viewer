//! `controller.ts`: the composition root. It builds the services once and
//! the per-area state, and the capability is the areas' methods: reads in
//! [`crate::read`], writes in [`crate::write`], viewport sync in
//! [`crate::sync`]. No behavior lives here.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use epdf_annotation_core::PageObjectNumber;
use epdf_kernel::rt::Promise;
use epdf_kernel::{Controller, Error, Unsubscribe};

use crate::contract::*;
use crate::model::{MeasurementAction, MeasurementState};
use crate::scale::default_presets;
use crate::services::Services;

/// The plugin context every area receives.
pub type MeasurementContext = Controller<MeasurementState, MeasurementAction>;

pub(crate) struct Core {
    pub(crate) services: Services,
    /// `config.presets ?? DEFAULT_PRESETS`.
    pub(crate) presets: Vec<ScalePreset>,
    /// `sync/viewports.ts`: the refresh epoch per page and the read in
    /// flight.
    pub(crate) epochs: Rc<RefCell<HashMap<PageObjectNumber, u64>>>,
    pub(crate) reading: RefCell<HashMap<PageObjectNumber, Promise<()>>>,
    /// `write/scale.ts`: the change queue per page.
    pub(crate) queues: Rc<RefCell<HashMap<PageObjectNumber, Promise<ScaleChangeReport>>>>,
}

/// `MeasurementHostCapability`, which is the public `MeasurementCapability`.
/// Cheap to clone; clones share state.
#[derive(Clone)]
pub struct MeasurementCapability(pub(crate) Rc<Core>);

/// `createMeasurementController(ctx, config)`: the capability; the plugin's
/// `connect` then calls [`MeasurementCapability::connect`].
pub fn create_measurement_controller(
    cx: MeasurementContext,
    config: MeasurementConfig,
) -> Result<MeasurementCapability, Error> {
    let presets = config.presets.clone().unwrap_or_else(default_presets);
    let services = Services::new(cx.clone(), config)?;
    // The areas' own cleanups, in the TypeScript's order: the viewport
    // sync's epochs, then the scale writes' queues.
    let epochs: Rc<RefCell<HashMap<PageObjectNumber, u64>>> = Rc::default();
    let e = epochs.clone();
    cx.defer(Box::new(move || {
        e.borrow_mut().clear();
        Ok(None)
    }));
    let queues: Rc<RefCell<HashMap<PageObjectNumber, Promise<ScaleChangeReport>>>> = Rc::default();
    let q = queues.clone();
    cx.defer(Box::new(move || {
        q.borrow_mut().clear();
        Ok(None)
    }));
    Ok(MeasurementCapability(Rc::new(Core {
        services,
        presets,
        epochs,
        reading: RefCell::default(),
        queues,
    })))
}

impl MeasurementCapability {
    pub(crate) fn s(&self) -> &Services {
        &self.0.services
    }

    /// `connect()`: the calibration capture, then the viewport sync.
    pub fn connect(&self) -> Result<(), Error> {
        self.connect_calibration();
        self.connect_viewports()
    }

    /// The instance lifetime, aborted when the document closes. Not in the
    /// TypeScript: the foreign handle's `instance-closed` check reads it.
    pub(crate) fn lifetime(&self) -> &epdf_kernel::signal::AbortSignal {
        self.s().cx.lifetime()
    }

    /// May write a page scale (`doc.annotate.modify`).
    pub fn can_calibrate(&self) -> bool {
        self.s().can_calibrate()
    }

    pub fn on_scale_changed(
        &self,
        listener: impl Fn(&MeasurementScaleChangedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.s().events.scale_changed.on(listener)
    }

    pub fn on_calibration_requested(
        &self,
        listener: impl Fn(&CalibrationRequestedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.s().events.calibration_requested.on(listener)
    }

    pub fn on_calibration_completed(
        &self,
        listener: impl Fn(&CalibrationCompletedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.s().events.calibration_completed.on(listener)
    }

    pub fn on_calibration_dismissed(
        &self,
        listener: impl Fn(&CalibrationDismissedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.s().events.calibration_dismissed.on(listener)
    }
}
