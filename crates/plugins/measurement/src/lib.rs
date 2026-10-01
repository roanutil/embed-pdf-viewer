//! `packages/plugin/measurement` in Rust: page scale, calibration and
//! measurement readouts, document-scoped. The annotation plugin owns the
//! measurement annotations; this plugin owns each page's viewports (the
//! scale), keeps the annotation plane's measure in step, and turns the
//! calibrate tool's drafts into scale requests.
//!
//! The files follow the TypeScript package: [`contract`] (with
//! `host-contract.ts`), [`model`], [`scale`], [`services`], [`read`],
//! [`write`], [`sync`], [`controller`], [`token`], and the wiring of
//! `measurement.plugin.ts` in [`measurement_plugin`]. [`units`] carries the
//! `engine-core` measurement helpers the plugin calls that
//! `epdf_annotation_core::measure` does not.

// Callback-shaped types are the port of JavaScript closures, as in the kernel.
#![allow(clippy::type_complexity)]

mod text;
pub mod contract;
pub mod controller;
pub mod model;
pub mod read;
pub mod scale;
pub mod services;
pub mod sync;
pub mod token;
pub mod units;
pub mod write;

#[cfg(feature = "harness")]
pub mod harness;

use std::rc::Rc;

use epdf_kernel::plugin::ConnectFn;
use epdf_kernel::{AnyPlugin, define};
use epdf_plugin_annotation::ANNOTATION;
use epdf_plugin_interaction::INTERACTION;

pub use contract::*;
pub use controller::{MeasurementCapability, MeasurementContext, create_measurement_controller};
pub use model::{
    MeasurementAction, MeasurementState, PageEntry, initial_measurement_state, measurement_reducer,
};
pub use scale::{default_measure, default_presets, select_page_scale};
pub use token::MEASUREMENT;

/// `MeasurementHostCapability`: the host lens equals the public one.
pub type MeasurementHostCapability = MeasurementCapability;

/// `measurementPlugin(config)`: document-scoped, requires the annotation
/// plugin and the interaction hub.
pub fn measurement_plugin(config: MeasurementConfig) -> AnyPlugin {
    build_measurement_plugin(config)
}

pub(crate) fn build_measurement_plugin(config: MeasurementConfig) -> AnyPlugin {
    define::<MeasurementState, MeasurementAction>("measurement")
        .document_scoped()
        .requires(ANNOTATION.any())
        .requires(INTERACTION.any())
        .initial_state(initial_measurement_state)
        .reduce(measurement_reducer)
        .slice_text(text::measurement_text())
        .create(&MEASUREMENT, move |cx| {
            let api = create_measurement_controller(cx, config.clone())?;
            let connected = api.clone();
            let connect: ConnectFn = Rc::new(move || connected.connect());
            Ok((api, Some(connect)))
        })
        .build()
}
