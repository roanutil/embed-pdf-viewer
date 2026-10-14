//! `packages/plugin/actions` in Rust: the PDF action engine.
//!
//! One dispatcher interprets extracted /A and /AA trees (GoTo, URI, Hide,
//! Named, ResetForm, SubmitForm, JavaScript chains) on a per-document serial
//! queue, with origin-aware live policy, can-twins, and a registry where
//! sibling plugins (stage, annotation, link, form, stamp) provide executors
//! and commit sinks. It never detects triggers: feeds report them.
//!
//! JavaScript is one registered interpreter among many. It runs only when
//! the config enables it and the host supplies a sandbox
//! ([`epdf_acrojs::ScriptSandbox`]); a host with none behaves as the
//! TypeScript does with scripting disabled.
//!
//! The module split follows the TypeScript package: [`contract`],
//! [`host_contract`], [`model`], [`controller`] and the areas it composes
//! ([`services`], [`dispatch`], [`lifecycle`], [`scripting`], [`submit`]),
//! plus [`hover_pump`], [`submit_encoding`], and the wiring in
//! [`actions_plugin`].

#![allow(clippy::type_complexity)]

pub mod contract;
pub mod controller;
pub mod dispatch;
pub mod external_uri;
pub mod host_contract;
pub mod hover_pump;
pub mod lifecycle;
pub mod model;
pub mod scripting;
pub mod services;
pub mod submit;
pub mod submit_encoding;

#[cfg(feature = "uniffi")]
pub mod ffi;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

#[cfg(feature = "harness")]
pub mod harness;

use epdf_kernel::{AnyPlugin, define};

pub use contract::*;
pub use controller::{ActionsCapability, create_actions_controller};
pub use dispatch::fold::fold_steps;
pub use dispatch::triggers::same_ref;
pub use external_uri::{DEFAULT_ALLOWED_URI_SCHEMES, sanitize_external_uri};
pub use host_contract::*;
pub use hover_pump::{HoverEvents, HoverPump, HoverTarget, create_hover_pump};
pub use model::{ActionsAction, ActionsState, actions_reducer, initial_actions_state};
pub use services::policy::{default_policy, merge_policy};
pub use submit::intent::{intent_of_payload, intent_of_submit_effect, to_form_submission_request};
pub use submit_encoding::submit_entries_to_url_encoded;

/// `ActionsHostCapability`: the same object as the public capability.
pub type ActionsHostCapability = ActionsCapability;

/// `actionsPlugin(config)`: document-scoped, the dependency root of the
/// action architecture. It never imports another plugin's token: siblings
/// optionally depend on [`ACTIONS`] and register their executors and sinks.
pub fn actions_plugin(config: ActionsConfig) -> AnyPlugin {
    define::<ActionsState, ActionsAction>("actions")
        .document_scoped()
        .initial_state(initial_actions_state)
        .reduce(actions_reducer)
        .create(&ACTIONS, move |cx| create_actions_controller(cx, config.clone()))
        .build()
}
