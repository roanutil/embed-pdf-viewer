//! `packages/plugin/redaction` in Rust: the two-stage redaction model.
//! Marking is annotation authoring (a mark is a `redact` annotation made
//! through the annotation plugin, from the text selection, an area, a whole
//! page or search matches); applying is the destructive document mutation
//! over the engine's `redaction.apply`. The plugin owns no mark state: the
//! pending view is a live projection of the annotation plane.
//!
//! The files follow the TypeScript package: [`contract`] (with `token.ts`
//! as [`token`]), [`model`], [`controller`] (the composition root), and the
//! areas under [`services`], [`read`], [`write`] and [`sync`], each an
//! `impl` block on the one [`RedactionCapability`].
//!
//! Trust boundary: on a layered document, applying rewrites the layer's
//! bytes; the immutable base keeps the original.

// Callback-shaped types are the port of JavaScript closures, as in the kernel.
#![allow(clippy::type_complexity)]

pub mod contract;
pub mod controller;
pub mod model;
pub mod read;
pub mod services;
pub mod sync;
pub mod token;
pub mod write;

#[cfg(feature = "uniffi")]
pub mod ffi;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

#[cfg(feature = "harness")]
pub mod harness;

use epdf_kernel::settings::SettingsDeclaration;
use epdf_kernel::{AnyPlugin, define};
use epdf_plugin_annotation::ANNOTATION;
use epdf_plugin_search::SEARCH;
use epdf_plugin_selection::SELECTION;
use serde_json::Value;

pub use contract::*;
pub use controller::{RedactionCapability, RedactionContext, create_redaction_controller};
pub use model::{
    RedactionState, finish_apply, initial_redaction_state, set_last_result, start_apply,
};
pub use read::pending::{estimate_collateral, mark_of, page_quad_bounds, regions_of};
pub use services::store::APPLY_CAPABILITIES;
pub use token::REDACTION;

/// `RedactionHostCapability`: the host lens equals the public one.
pub type RedactionHostCapability = RedactionCapability;

/// `redactionPlugin(config)`: document-scoped, requires the annotation
/// plugin, and uses the selection and search plugins when present.
/// `config` is the settings the app registers, over [`redaction_defaults`].
pub fn redaction_plugin(config: RedactionConfig) -> AnyPlugin {
    redaction_plugin_json(Some(config.to_json()))
}

/// [`redaction_plugin`] with the registered settings as JSON, as the
/// TypeScript `config` object arrives (`None` registers nothing).
pub fn redaction_plugin_json(registered: Option<Value>) -> AnyPlugin {
    define::<RedactionState, Value>("redaction")
        .document_scoped()
        .requires(ANNOTATION.any())
        .optional(SELECTION.any())
        .optional(SEARCH.any())
        .initial_state(initial_redaction_state)
        .settings(SettingsDeclaration {
            defaults: redaction_defaults(),
            registered,
            whole: Vec::new(),
        })
        .create(&REDACTION, |cx| {
            let (api, connect) = create_redaction_controller(cx)?;
            Ok((api, Some(connect)))
        })
        .build()
}
