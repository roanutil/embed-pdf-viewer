//! `packages/plugin/redaction` in Rust: the two-stage redaction model.
//! Marking is annotation authoring (a mark is a `redact` annotation made
//! through the annotation plugin, from the text selection, an area, a whole
//! page or search matches); applying is the destructive document mutation
//! over the engine's optional `redaction.apply`. The plugin owns no mark
//! state: the pending view is a live projection of the annotation plane.
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

use std::rc::Rc;

use epdf_kernel::plugin::ConnectFn;
use epdf_kernel::{AnyPlugin, define};
use epdf_plugin_annotation::ANNOTATION;
use epdf_plugin_search::SEARCH;
use epdf_plugin_selection::SELECTION;

pub use contract::*;
pub use controller::{RedactionCapability, create_redaction_controller};
pub use model::{RedactionAction, RedactionState, initial_redaction_state, redaction_reducer};
pub use read::pending::{estimate_collateral, regions_of};
pub use services::store::APPLY_CAPABILITIES;
pub use token::REDACTION;

/// `RedactionHostCapability`: the host lens equals the public one today.
pub type RedactionHostCapability = RedactionCapability;

/// `redactionPlugin(config)`: document-scoped, requires the annotation
/// plane, and uses the selection and search plugins when present. `connect`
/// subscribes the remote-apply feed and the pending-change feed.
pub fn redaction_plugin(config: RedactionConfig) -> AnyPlugin {
    define::<RedactionState, RedactionAction>("redaction")
        .document_scoped()
        .requires(ANNOTATION.any())
        .optional(SELECTION.any())
        .optional(SEARCH.any())
        .initial_state(initial_redaction_state)
        .reduce(redaction_reducer)
        .create(&REDACTION, move |cx| {
            let api = create_redaction_controller(cx, config.clone())?;
            let connected = api.clone();
            let connect: ConnectFn = Rc::new(move || {
                connected.subscribe_changes();
                Ok(())
            });
            Ok((api, Some(connect)))
        })
        .build()
}
