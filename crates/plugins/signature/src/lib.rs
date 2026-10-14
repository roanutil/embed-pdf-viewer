//! `packages/plugin/signature` in Rust: the act of signing, document-scoped.
//! It owns no mark: marks are stamp-library assets (or bytes the embedder
//! brings), and the mark IS the appearance the engine draws into the field.
//! Signing goes through `epdf-signature-core` with the configured signer
//! port; the engine seals.
//!
//! With the interaction hub and the stamp plugin present, an armed mark
//! from a `signatures` library dropped over an unsigned signature field goes
//! into the field (sign, visual fill or ask, by mode) instead of onto the
//! page.
//!
//! The files follow the TypeScript package: [`contract`], [`model`],
//! [`blank_page`], [`controller`] (the composition root), and the areas
//! under [`services`], [`read`], [`sync`], [`write`] and [`tools`], each an
//! `impl` block on the one [`SignatureCapability`].

// Callback-shaped types are the port of JavaScript closures, as in the kernel.
#![allow(clippy::type_complexity)]

pub mod blank_page;
pub mod contract;
pub mod controller;
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

use epdf_kernel::plugin::ConnectFn;
use epdf_kernel::{AnyPlugin, define};
use epdf_plugin_annotation::ANNOTATION;
use epdf_plugin_form::FORM;
use epdf_plugin_interaction::{INTERACTION, RegisterHandlerOptions};
use epdf_plugin_stamp::STAMP;

pub use blank_page::blank_page_pdf;
pub use contract::*;
pub use controller::{
    SignatureCapability, SignatureHostCapability, create_signature_capability,
    create_signature_controller,
};
pub use model::{
    ResourceStatus, SignatureAction, SignatureState, initial_signature_state, signature_reducer,
};
pub use services::store::same_ref;
pub use tools::armed_mark::ArmedMarkHandler;
pub use write::sign::is_target;

/// `signaturePlugin(config)`: document-scoped; requires the form plugin,
/// and takes the interaction hub, the stamp plugin and the annotation plugin
/// when they are there.
pub fn signature_plugin(config: SignatureConfig) -> AnyPlugin {
    define::<SignatureState, SignatureAction>("signature")
        .document_scoped()
        .requires(FORM.any())
        .optional(INTERACTION.any())
        .optional(STAMP.any())
        .optional(ANNOTATION.any())
        .initial_state(initial_signature_state)
        .reduce(signature_reducer)
        .create(&SIGNATURE, move |cx| {
            let (api, connect) = create_signature_controller(cx.clone(), config.clone())?;
            let registering = api.clone();
            let connect: ConnectFn = Rc::new(move || {
                connect()?;
                let interaction = cx.try_get(&INTERACTION)?;
                let stamp = cx.try_get(&STAMP)?;
                if let (Some(interaction), Some(stamp), Some(document_id)) =
                    (interaction, stamp, cx.document_id())
                {
                    let handler = ArmedMarkHandler::new(
                        document_id.to_string(),
                        registering.clone(),
                        cx.get(&FORM)?,
                        stamp,
                    );
                    let unregister = interaction
                        .register_handler(Rc::new(handler), RegisterHandlerOptions::default());
                    cx.defer(Box::new(move || {
                        unregister.call();
                        Ok(None)
                    }));
                }
                Ok(())
            });
            Ok((api, Some(connect)))
        })
        .build()
}
