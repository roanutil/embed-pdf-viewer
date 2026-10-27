//! `packages/plugin/actions` in Rust: the PDF action engine.
//!
//! One dispatcher interprets extracted /A and /AA trees (GoTo, URI, Hide,
//! Named, ResetForm, SubmitForm, JavaScript chains) on a per-document serial
//! queue, with origin-aware live policy, can-twins, and a registry where
//! sibling plugins (stage, annotation, link, form, stamp) provide executors
//! and commit sinks. It never detects triggers: feeds report them.
//!
//! JavaScript is one registered interpreter among many. It runs only when
//! the `javascript.enabled` setting is on, in a sandbox the host supplies
//! ([`epdf_acrojs::ScriptSandbox`]). Rust ships no JavaScript engine: with
//! scripting on and no sandbox factory, the realm exists as the
//! TypeScript's does and every sandbox build fails with
//! [`NO_DEFAULT_SANDBOX`].
//!
//! The module split follows the TypeScript package: [`contract`],
//! [`host_contract`], [`controller`] and the areas it composes
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
pub mod scripting;
pub mod services;
mod settings;
pub mod submit;
pub mod submit_encoding;

#[cfg(feature = "uniffi")]
pub mod ffi;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

#[cfg(feature = "harness")]
pub mod harness;

use std::rc::Rc;

use epdf_kernel::settings::SettingsDeclaration;
use epdf_kernel::{AnyPlugin, define, undefined};
use serde_json::Value;

pub use contract::*;
pub use controller::ActionsCapability;
pub use dispatch::fold::fold_steps;
pub use external_uri::{DEFAULT_ALLOWED_URI_SCHEMES, sanitize_external_uri};
pub use host_contract::*;
pub use hover_pump::{HoverEvents, HoverPump, HoverTarget, create_hover_pump};
pub use services::annotation_key;
pub use services::policy::{default_policy, merge_policy};
pub use submit::intent::{intent_of_payload, intent_of_submit_effect, to_form_submission_request};
pub use submit_encoding::submit_entries_to_url_encoded;

/// `ActionsHostCapability`: the same object as the public capability.
pub type ActionsHostCapability = ActionsCapability;

/// `actionsPlugin(config)`: document-scoped and stateless, the dependency
/// root of the action architecture. It never imports another plugin's
/// token: siblings optionally depend on [`ACTIONS`] and register their
/// executors and sinks. Its settings are what the app registered over
/// [`actions_defaults`].
pub fn actions_plugin(config: ActionsConfig) -> AnyPlugin {
    // The registration's function identities, shared by its instances as
    // its settings are.
    let table = Rc::new(settings::IdentityTable::default());
    let registered = table.register(&config);
    define::<Value, Value>("actions")
        .document_scoped()
        .initial_state(undefined)
        .settings(SettingsDeclaration {
            defaults: settings::defaults_json(),
            registered: Some(registered),
            whole: Vec::new(),
        })
        .create(&ACTIONS, move |cx| {
            controller::create_actions_controller(cx, config.clone(), table.clone())
        })
        .build()
}
