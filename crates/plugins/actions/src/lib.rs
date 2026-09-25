//! `packages/plugin/actions` in Rust: the PDF action engine.
//!
//! One dispatcher interprets extracted /A and /AA trees (GoTo, URI, Hide,
//! Named, ResetForm, SubmitForm, JavaScript chains) on a per-document serial
//! queue, with origin-aware policy, can-twins, and a registry where sibling
//! plugins (stage, annotation, link, form) provide executors and commit
//! sinks. It never detects triggers: feeds report them.
//!
//! JavaScript is one registered interpreter among many. It runs only when
//! the config enables it and the host supplies a sandbox
//! ([`epdf_acrojs::ScriptSandbox`]); a host with none behaves as the
//! TypeScript does with scripting disabled.
//!
//! The module split follows the TypeScript package: [`types`], [`reducer`],
//! [`capability`], [`hover_pump`], [`submit_encoding`], and the wiring in
//! [`actions_plugin`].

#![allow(clippy::type_complexity)]

pub mod capability;
pub mod hover_pump;
pub mod reducer;
pub mod submit_encoding;
pub mod types;

#[cfg(feature = "harness")]
pub mod harness;

use epdf_kernel::{AnyPlugin, define};

pub use capability::{ActionsCapability, DocumentVerb, create_actions_capability, default_policy};
pub use hover_pump::{HoverEvents, HoverPump, HoverTarget, create_hover_pump};
pub use reducer::{actions_reducer, initial_actions_state};
pub use submit_encoding::submit_entries_to_url_encoded;
pub use types::*;

/// `actionsPlugin(config)`: document-scoped, the dependency root of the
/// action architecture.
pub fn actions_plugin(config: ActionsPluginConfig) -> AnyPlugin {
    define::<ActionsState, ActionsAction>("actions")
        .document_scoped()
        .initial_state(initial_actions_state)
        .reduce(actions_reducer)
        .capability(&ACTIONS, move |ctx| Ok(create_actions_capability(ctx, config.clone())))
        .build()
}
