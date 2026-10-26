//! `packages/plugin/form` in Rust: the field plane. Document-scoped and
//! requiring the interaction hub, it holds the form's fields and widget
//! rows in a mirror of the engine's, shows this session's fills and designs
//! on top until the engine answers, projects fill controls for each page,
//! writes values, resets, interchange and field design through
//! `ctx.changes`, and registers the palette tools and the draw-to-place
//! handler. With the annotation plugin it lends its widgets for design;
//! beside the actions plugin it registers the `reset-form` executor, the
//! submit dataset resolver, the form reader and the script commit sink, and
//! runs value writes through the K/V/C/F pipeline in the actions plugin's
//! script realm.
//!
//! The files follow the TypeScript package: [`contract`] (with `token.ts`
//! and `host-contract.ts`), [`model`], [`controller`] (the composition
//! root), `connect`, and the areas under [`services`], [`read`], [`write`],
//! [`sync`], [`scripting`] and [`tools`], each an `impl Core` block.
//! [`engine`] carries the pure engine-core rules the plugin calls.
//!
//! Engine DTOs (the snapshot, fields, widget rows, refs, drafts, patches,
//! bundles) stay `serde_json::Value`, carried and printed as the engine
//! wrote them.

#![allow(clippy::type_complexity)]

mod connect;
pub mod console;
pub mod contract;
pub mod controller;
pub mod engine;
pub mod field_selection;
pub mod model;
pub mod promise;
pub mod read;
pub mod scripting;
pub mod services;
pub mod sync;
pub mod tools;
pub mod value;
pub mod write;

#[cfg(feature = "harness")]
pub mod harness;

#[cfg(feature = "uniffi")]
pub mod ffi;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

use std::sync::LazyLock;

use epdf_kernel::settings::SettingsDeclaration;
use epdf_kernel::{AnyPlugin, Token, define};
use epdf_plugin_actions::ACTIONS;
use epdf_plugin_annotation::ANNOTATION;
use epdf_plugin_interaction::INTERACTION;
use epdf_plugin_render::RENDER;
use serde_json::Value;

pub use contract::*;
pub use controller::{Core, FormCapability, create_form_controller};
pub use scripting::controller::{
    FormScriptResult, FormScriptingController, FormScriptingControllerOptions, FormScripts,
    ScriptStatus, TransactionBody, TransactionPort, create_form_scripting_controller,
};
pub use tools::definitions::{FORM_TOOLS, FormToolDef, PLACE_TAGS, form_tool_by_id};

/// `FormToken`: the public lens and the host lens are one capability.
pub static FORM: LazyLock<Token<FormCapability>> = LazyLock::new(|| {
    Token::with_hint("form", "add formPlugin() from '@embedpdf/plugin-form' to your plugins list")
});

/// `formPlugin(config)`: document-scoped; requires the interaction hub and
/// optionally uses the annotation, actions and render plugins. `config` is
/// the settings the app registers, over [`form_defaults`].
pub fn form_plugin(config: Option<Value>) -> AnyPlugin {
    define::<Value, Value>("form")
        .document_scoped()
        .requires(INTERACTION.any())
        .optional(ANNOTATION.any())
        .optional(ACTIONS.any())
        .optional(RENDER.any())
        .initial_state(model::initial_form_state)
        .settings(SettingsDeclaration {
            defaults: form_defaults(),
            registered: config.filter(|c| !epdf_kernel::is_undefined(c)),
            whole: Vec::new(),
        })
        .create(&FORM, create_form_controller)
        .build()
}
