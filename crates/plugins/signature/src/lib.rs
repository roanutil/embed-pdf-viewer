//! `packages/plugin/signature` in Rust: the act of signing, document-scoped.
//! It owns no mark: marks are stamp-library assets (or bytes the embedder
//! brings), and the mark is the appearance the engine draws into the field.
//! Signing goes through `epdf-signature-core` with the `key` setting's
//! signer port; the engine seals.
//!
//! With the interaction hub and the stamp plugin present, an armed mark
//! from a `signatures` library dropped over an unsigned signature field goes
//! into the field (sign, visual fill or ask, by mode) instead of onto the
//! page.
//!
//! The files follow the TypeScript package: [`contract`], [`model`],
//! [`blank_page`], [`connect`], [`controller`] (the composition root), and
//! the areas under [`services`], [`read`], [`sync`], [`write`] and
//! [`tools`], each an `impl` block on the one [`SignatureCapability`].

// Callback-shaped types are the port of JavaScript closures, as in the kernel.
#![allow(clippy::type_complexity)]

pub mod blank_page;
pub mod connect;
pub mod console;
pub mod contract;
pub mod controller;
pub mod js;
pub mod model;
pub mod promise;
pub mod read;
pub mod services;
pub mod sync;
pub mod tools;
pub mod write;

#[cfg(feature = "harness")]
pub mod harness;

#[cfg(feature = "uniffi")]
pub mod ffi;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

use std::rc::Rc;

use epdf_kernel::settings::SettingsDeclaration;
use epdf_kernel::{AnyPlugin, Error, define};
use epdf_plugin_annotation::ANNOTATION;
use epdf_plugin_form::FORM;
use epdf_plugin_interaction::INTERACTION;
use epdf_plugin_stamp::STAMP;
use serde_json::Value;

pub use blank_page::blank_page_pdf;
pub use contract::*;
pub use controller::{
    SignatureCapability, SignatureContext, SignatureHostCapability, create_signature_controller,
};
pub use model::{
    SignatureState, initial_signature_state, same_field_ref, set_busy, set_target, set_verdicts,
};
pub use services::settings::PortTable;
pub use tools::armed_mark::ArmedMarkHandler;

use services::settings::{defaults_json, register};

/// `signaturePlugin(config)`: document-scoped; requires the form plugin,
/// and takes the interaction hub, the stamp plugin and the annotation plugin
/// when they are there. `config` is the settings the app registers, over
/// [`signature_defaults`].
pub fn signature_plugin(config: SignatureConfig) -> AnyPlugin {
    let (table, registered) = register(&config);
    signature_plugin_with(table, Some(registered))
}

/// [`signature_plugin`] over a registration's port table and its registered
/// settings JSON (`None` registers nothing).
pub fn signature_plugin_with(table: Rc<PortTable>, registered: Option<Value>) -> AnyPlugin {
    signature_plugin_creating(table, registered, Rc::new(create_signature_controller))
}

/// What builds the controller: [`create_signature_controller`], or a host's
/// wrapper around it.
pub type CreateSignature = Rc<
    dyn Fn(
        SignatureContext,
        Rc<PortTable>,
    ) -> Result<(SignatureCapability, epdf_kernel::plugin::ConnectFn), Error>,
>;

/// [`signature_plugin_with`], building each controller with `create`.
pub fn signature_plugin_creating(
    table: Rc<PortTable>,
    registered: Option<Value>,
    create: CreateSignature,
) -> AnyPlugin {
    define::<SignatureState, Value>("signature")
        .document_scoped()
        .requires(FORM.any())
        .optional(INTERACTION.any())
        .optional(STAMP.any())
        .optional(ANNOTATION.any())
        .initial_state(initial_signature_state)
        // A key and a trust port are one value each: a change replaces
        // them, never merges into them.
        .settings(SettingsDeclaration {
            defaults: defaults_json(),
            registered,
            whole: vec!["key".to_string(), "trust".to_string()],
        })
        .create(&SIGNATURE, move |cx| {
            let (api, connect) = create(cx, table.clone())?;
            Ok((api, Some(connect)))
        })
        .build()
}
