//! `packages/plugin/measurement` in Rust: page scale, calibration and
//! measurement readouts, document-scoped. The annotation plugin owns the
//! measurement annotations; this plugin owns each page's viewports (the
//! scale, a kernel page mirror of the engine's `page.measure` reads), keeps
//! the annotation plugin's measure in step, and turns the calibrate tool's
//! drafts into scale requests.
//!
//! The files follow the TypeScript package: [`contract`], [`model`],
//! [`scale`], [`services`], [`read`], [`write`], [`sync`], [`controller`],
//! [`token`], and the wiring of `measurement.plugin.ts` in
//! [`measurement_plugin`]. [`units`] carries the `engine-core` measurement
//! helpers the plugin calls that `epdf_annotation_core::engine::measure`
//! does not.

// Callback-shaped types are the port of JavaScript closures, as in the kernel.
#![allow(clippy::type_complexity)]

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

#[cfg(feature = "uniffi")]
pub mod ffi;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

use epdf_kernel::settings::SettingsDeclaration;
use epdf_kernel::{AnyPlugin, define};
use epdf_plugin_annotation::ANNOTATION;
use epdf_plugin_interaction::INTERACTION;
use serde_json::Value;

pub use contract::*;
pub use controller::{MeasurementCapability, MeasurementContext, create_measurement_controller};
pub use model::{MeasurementState, initial_measurement_state, page_scale_of, select_page_scale};
pub use scale::{default_measure, default_presets, with_area_unit, with_precision, with_unit};
pub use token::MEASUREMENT;

/// `MeasurementHostCapability`: the host lens equals the public one.
pub type MeasurementHostCapability = MeasurementCapability;

/// `measurementPlugin(config)`: document-scoped, requires the annotation
/// plugin and the interaction hub. `config` is the settings the app
/// registers, over [`measurement_defaults`].
pub fn measurement_plugin(config: MeasurementConfig) -> AnyPlugin {
    measurement_plugin_json(Some(config.to_json()))
}

/// [`measurement_plugin`] with the registered settings as JSON, as the
/// TypeScript `config` object arrives (`None` registers nothing).
pub fn measurement_plugin_json(registered: Option<Value>) -> AnyPlugin {
    define::<MeasurementState, Value>("measurement")
        .document_scoped()
        .requires(ANNOTATION.any())
        .requires(INTERACTION.any())
        .initial_state(initial_measurement_state)
        // A scale is one value: a change replaces it, never merges into it.
        .settings(SettingsDeclaration {
            defaults: measurement_defaults(),
            registered,
            whole: vec!["defaultScale".into()],
        })
        .create(&MEASUREMENT, |cx| {
            let (api, connect) = create_measurement_controller(cx)?;
            Ok((api, Some(connect)))
        })
        .build()
}
