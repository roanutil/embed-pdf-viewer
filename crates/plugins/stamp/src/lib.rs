//! `packages/plugin/stamp` in Rust: workspace-scoped stamp libraries and
//! assets. A library is a PDF: its `/Title` is the name, its `/Names
//! /Pages` registry the assets, its pages the artwork, `/PieceInfo` only
//! what has no standard home. Marks (ink, text, images, PDF pages) are
//! authored into pages. Placement targets a document through its annotation
//! plugin; a form-backed (dynamic) asset is first evaluated on a detached
//! copy in a script realm the target document's actions plugin mints, then
//! flattened.
//!
//! The files follow the TypeScript package: [`contract`] (with `token.ts`
//! as [`token`]), [`model`], [`convention`], [`blank_library`],
//! [`controller`] (the composition root), [`persistence`], and the areas
//! under [`services`], [`read`] and [`write`], each an `impl` block on the
//! one [`StampCapability`].

// Callback-shaped types are the port of JavaScript closures, as in the kernel.
#![allow(clippy::type_complexity)]

pub mod blank_library;
pub mod console;
pub mod contract;
pub mod controller;
pub mod convention;
pub mod js;
pub mod model;
pub mod persistence;
pub mod read;
pub mod services;
pub mod token;
pub mod write;

#[cfg(feature = "harness")]
pub mod harness;

#[cfg(feature = "uniffi")]
pub mod ffi;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

use std::rc::Rc;

use epdf_kernel::settings::SettingsDeclaration;
use epdf_kernel::types::PluginScope;
use epdf_kernel::{AnyPlugin, Error, define};
use epdf_plugin_actions::ACTIONS;
use epdf_plugin_annotation::ANNOTATION;
use serde_json::Value;

pub use contract::*;
pub use controller::{StampCapability, StampContext, create_stamp_controller};
pub use model::{StampState, initial_stamp_state};
pub use persistence::{
    MemoryStampStore, PersistOptions, StampLibraryStore, StoredLibrary, memory_stamp_store,
    persist_stamp_libraries, restore_stamp_libraries,
};
pub use services::binaries::AssetBinary;
use services::settings::{EngineTable, defaults_json, register};

/// `StampHostCapability`: the host lens equals the public one.
pub type StampHostCapability = StampCapability;

/// `stampPlugin(config)`: workspace-scoped (libraries outlive any one
/// document); placement targets a document through its annotation plugin.
/// `config` is the settings the app registers, over [`stamp_defaults`].
pub fn stamp_plugin(config: StampConfig) -> AnyPlugin {
    let (table, registered) = register(&config);
    stamp_plugin_with(table, Some(registered))
}

/// [`stamp_plugin`] over a registration's engine table and its registered
/// settings JSON (`None` registers nothing).
pub fn stamp_plugin_with(table: Rc<EngineTable>, registered: Option<Value>) -> AnyPlugin {
    stamp_plugin_creating(table, registered, Rc::new(create_stamp_controller))
}

/// What builds the controller: [`create_stamp_controller`], or a host's
/// wrapper around it.
pub type CreateStamp = Rc<dyn Fn(StampContext, Rc<EngineTable>) -> Result<StampCapability, Error>>;

/// [`stamp_plugin_with`], building each controller with `create`.
pub fn stamp_plugin_creating(
    table: Rc<EngineTable>,
    registered: Option<Value>,
    create: CreateStamp,
) -> AnyPlugin {
    define::<StampState, Value>("stamp")
        .scope(PluginScope::Workspace)
        .optional(ANNOTATION.any())
        .optional(ACTIONS.any())
        .initial_state(initial_stamp_state)
        // An engine is one value: a change replaces it, never merges into it.
        .settings(SettingsDeclaration {
            defaults: defaults_json(),
            registered,
            whole: vec!["assetEngine".to_string()],
        })
        .create(&STAMP, move |cx| {
            let api = create(cx, table.clone())?;
            Ok((api, None))
        })
        .build()
}
