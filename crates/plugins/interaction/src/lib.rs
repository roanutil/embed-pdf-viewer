//! `packages/plugin/interaction` in Rust: the pointer hub every interactive
//! plugin rides on (one active tool, one cursor, one priority-ordered
//! handler list) and the platform feedback seam.
//!
//! The file split follows the TypeScript package: [`contract`] (with the
//! host lens folded into [`InteractionCapability`]), [`model`],
//! [`controller`], [`feedback`], [`state`], and the wiring in
//! [`interaction_plugin`]. `settings` keeps typed tools in the kernel's
//! JSON settings store; `callbacks` holds `Unregister`, what the
//! registries return.
//!
//! Downstream plugins (selection, annotation, form, stage) implement
//! [`InteractionHandler`] and register it with
//! [`InteractionCapability::register_handler`], as the TypeScript does
//! through the host contract.

pub mod callbacks;
pub mod contract;
pub mod controller;
pub mod feedback;
pub mod model;
mod settings;
pub mod state;

#[cfg(feature = "harness")]
pub mod harness;

#[cfg(feature = "uniffi")]
pub mod ffi;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

use std::rc::Rc;

use epdf_kernel::settings::SettingsDeclaration;
use epdf_kernel::{AnyPlugin, define};
use serde_json::Value;

pub use callbacks::Unregister;
pub use contract::*;
pub use controller::InteractionCapability;
pub use feedback::{
    FEEDBACK, Feedback, FeedbackPluginOptions, ImpactWeight, NotifyKind, PlatformFeedback,
    feedback_plugin,
};
pub use model::{InteractionState, builtin_tools, initial_interaction_state};
pub use settings::plain_tool;
pub use state::{InteractionStateView, interaction_state};

/// `InteractionHostCapability`: the same object as the public capability.
pub type InteractionHostCapability = InteractionCapability;

/// `interactionPlugin(config)`: the interaction hub, document-scoped,
/// depending on nothing. Feature plugins require [`INTERACTION`]; the stage
/// contributes a scroll handler. `config` is the settings the app
/// registers, over [`interaction_defaults`].
pub fn interaction_plugin(config: InteractionConfig) -> AnyPlugin {
    // The registration's tools, shared by its instances as its settings are.
    let table = Rc::new(settings::ToolTable::default());
    let registered = table.register(&config);
    define::<InteractionState, Value>("interaction")
        .document_scoped()
        .initial_state(initial_interaction_state)
        .settings(SettingsDeclaration {
            defaults: settings::defaults_json(),
            registered: Some(registered),
            whole: Vec::new(),
        })
        .create(&INTERACTION, move |cx| {
            controller::create_interaction_controller(cx, builtin_tools(), table.clone())
        })
        .build()
}
