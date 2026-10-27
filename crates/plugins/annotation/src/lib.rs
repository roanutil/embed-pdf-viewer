//! `packages/plugin/annotation` in Rust: the pure `epdf-annotation-core`
//! wired to the engine (a records mirror, and changes shown at once until
//! their writes settle) and to the interaction hub (ambient editing and the
//! draw tools). Text markup lights up when a selection plugin is present,
//! the hover feed when an actions plugin is, and the engine's pictures come
//! through the render plugin.
//!
//! The files follow the TypeScript package: [`contract`], [`host_contract`]
//! (with `priorities.ts`), [`model`], [`controller`] (the composition root),
//! `connect`, and the areas under [`services`], [`read`], [`write`],
//! [`sync`], [`comments`] and [`tools`], each an `impl Core` block. The
//! engine's annotations, drafts, patches and refs stay JSON, as
//! `epdf-annotation-core` keeps them.

// Callback-shaped types are the port of JavaScript closures, as in the kernel.
#![allow(clippy::type_complexity)]

pub mod binary;
pub mod comments;
mod connect;
pub mod console;
pub mod contract;
pub mod controller;
pub mod engine;
pub mod host_contract;
pub mod js;
pub mod model;
pub mod promise;
pub mod read;
pub mod rich_text;
pub mod services;
pub mod sync;
pub mod text_item;
pub mod tools;
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
use epdf_plugin_interaction::INTERACTION;
use epdf_plugin_render::RENDER;
use epdf_plugin_selection::SELECTION;
use serde_json::Value;

pub use contract::{ARMED_STAMP_TOOL_ID, annotation_defaults};
pub use controller::{AnnotationCapability, Core, create_annotation_controller};
pub use host_contract::*;
pub use model::{AnnotationState, initial_annotation_state};
pub use tools::definitions::{ResolvedTool, build_tool_registry, default_tools, tool_touch_of};

/// `AnnotationToken`: the public lens and the host lens are one capability.
pub static ANNOTATION: LazyLock<Token<AnnotationCapability>> = LazyLock::new(|| {
    Token::with_hint(
        "annotation",
        "add annotationPlugin() from '@embedpdf/plugin-annotation' to your plugins list",
    )
});

/// `annotationPlugin(config)`: document-scoped; requires the interaction
/// hub and optionally uses the selection, actions and render plugins.
/// `config` is the settings the app registers, over [`annotation_defaults`].
pub fn annotation_plugin(config: Option<Value>) -> AnyPlugin {
    define::<AnnotationState, Value>("annotation")
        .document_scoped()
        .requires(INTERACTION.any())
        .optional(SELECTION.any())
        .optional(ACTIONS.any())
        .optional(RENDER.any())
        .initial_state(initial_annotation_state)
        .settings(SettingsDeclaration {
            defaults: annotation_defaults(),
            registered: config.filter(|c| !epdf_kernel::is_undefined(c)),
            whole: Vec::new(),
        })
        .create(&ANNOTATION, create_annotation_controller)
        .build()
}
