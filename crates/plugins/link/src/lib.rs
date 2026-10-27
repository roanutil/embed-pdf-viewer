//! `packages/plugin/link` in Rust: clickable link regions and their
//! activation. Two sources for the areas: the annotation plugin's folded
//! model when it is installed, else a page mirror of this plugin's own
//! per-page reads. Activation is pure resolution (`resolve`) plus the one
//! side effect this plugin owns, a stage reveal; a link with an `/A` tree
//! dispatches through the actions plugin, and everything else is reported
//! to the host, which keeps the user gesture.
//!
//! The files follow the TypeScript package: [`contract`] (with
//! `host-contract.ts`), [`source`], [`controller`], `connect`, [`token`],
//! and the wiring of `link.plugin.ts` in [`link_plugin`].

// Callback-shaped types are the port of JavaScript closures, as in the kernel.
#![allow(clippy::type_complexity)]

mod connect;
pub mod contract;
pub mod controller;
pub mod source;
pub mod token;

#[cfg(feature = "harness")]
pub mod harness;

#[cfg(feature = "uniffi")]
pub mod ffi;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

use std::rc::Rc;

use epdf_kernel::plugin::ConnectFn;
use epdf_kernel::{AnyPlugin, ControllerContext, define, undefined};
use epdf_plugin_actions::ACTIONS;
use epdf_plugin_annotation::ANNOTATION;
use epdf_plugin_interaction::INTERACTION;
use epdf_plugin_stage::STAGE;
use serde_json::Value;

pub use connect::LinkNavBehavior;
pub use contract::*;
pub use controller::{LinkCapability, create_link_controller, pages_changed_by};
pub use source::links_of;
pub use token::LINK;

/// `LinkHostCapability`: the host lens adds `is_navigation_engaged` and
/// `register_uri_opener`, which the one Rust type carries.
pub type LinkHostCapability = LinkCapability;

/// `linkPlugin()`: document-scoped and stateless; requires the interaction
/// hub (the navigation tool tag), and uses the stage (reveal), the
/// annotation plugin (the folded link model) and actions (`/A` trees) when
/// they are installed.
pub fn link_plugin() -> AnyPlugin {
    build_link_plugin(None)
}

/// `linkPlugin()` with a hook that sees the capability as soon as it is
/// built, before `connect`: the harness subscribes its scripted
/// `onActivated` listener there, as the generator's factory does.
pub(crate) fn build_link_plugin(
    on_created: Option<Rc<dyn Fn(&LinkCapability, &ControllerContext)>>,
) -> AnyPlugin {
    define::<Value, Value>("link")
        .document_scoped()
        .initial_state(undefined)
        .requires(INTERACTION.any())
        .optional(STAGE.any())
        .optional(ANNOTATION.any())
        .optional(ACTIONS.any())
        .create(&LINK, move |cx| {
            let (api, connect) = create_link_controller(cx.cx.clone())?;
            if let Some(hook) = &on_created {
                hook(&api, &cx.cx);
            }
            Ok((api, Some(connect as ConnectFn)))
        })
        .build()
}
