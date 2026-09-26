//! `packages/plugin/link` in Rust: clickable link regions and their
//! activation. Two sources for the areas: the annotation plugin's folded
//! model when it is installed, else this plugin's own per-page reads.
//! Activation is pure resolution (`resolve`) plus the one side effect this
//! plugin owns, a stage reveal; a link with an `/A` tree dispatches through
//! the actions plugin, and everything else is reported to the host, which
//! keeps the user gesture.
//!
//! The files follow the TypeScript package: [`contract`] (with
//! `host-contract.ts`), [`model`], [`source`], [`controller`], [`token`],
//! and the wiring of `link.plugin.ts` in [`link_plugin`].

// Callback-shaped types are the port of JavaScript closures, as in the kernel.
#![allow(clippy::type_complexity)]

pub mod contract;
pub mod controller;
pub mod model;
pub mod source;
pub mod token;

#[cfg(feature = "harness")]
pub mod harness;

use std::rc::Rc;

use epdf_kernel::plugin::ConnectFn;
use epdf_kernel::{AnyPlugin, define};
use epdf_plugin_actions::ACTIONS;
use epdf_plugin_annotation::{ANNOTATION, Behavior};
use epdf_plugin_interaction::{INTERACTION, InteractionCapability};
use epdf_plugin_stage::STAGE;

pub use contract::*;
pub use controller::{LinkCapability, create_link_controller};
pub use model::{LinkAction, LinkState, initial_link_state, link_reducer};
pub use source::{load_links_page, project_links};
pub use token::LINK;

/// `LinkHostCapability`: the host lens adds `is_navigation_engaged`, which
/// the one Rust type carries.
pub type LinkHostCapability = LinkCapability;

/// The behavior `connect` registers with the annotation plugin: links stay
/// navigation-owned while the active tool enables `link-nav`.
pub struct LinkNavBehavior {
    interaction: InteractionCapability,
}

impl Behavior for LinkNavBehavior {
    fn id(&self) -> &str {
        "link-nav"
    }

    fn matches(&self, subtype: &str, _ref: Option<&AnnotationRef>) -> bool {
        subtype == "link"
    }

    fn engaged(&self) -> bool {
        self.interaction.get_active_tool().enables("link-nav")
    }
}

/// `linkPlugin()`: document-scoped, requires the interaction hub, and uses
/// the stage, the annotation plugin and the action engine when present.
/// `config.host` carries what the TypeScript reads from the global scope.
pub fn link_plugin(config: LinkConfig) -> AnyPlugin {
    build_link_plugin(config, None)
}

/// `linkPlugin()` with a hook that sees the capability as soon as it is
/// built, before `connect`: the harness subscribes its scripted
/// `onActivated` listener there, as the generator's factory does.
pub(crate) fn build_link_plugin(
    config: LinkConfig,
    on_created: Option<Rc<dyn Fn(&LinkCapability)>>,
) -> AnyPlugin {
    define::<LinkState, LinkAction>("link")
        .document_scoped()
        .requires(INTERACTION.any())
        .optional(STAGE.any())
        .optional(ANNOTATION.any())
        .optional(ACTIONS.any())
        .initial_state(initial_link_state)
        .reduce(link_reducer)
        .create(&LINK, move |cx| {
            let api = create_link_controller(cx.clone(), &config);
            if let Some(hook) = &on_created {
                hook(&api);
            }
            let connected = api.clone();
            let connect: ConnectFn = Rc::new(move || {
                connected.connect();
                // Links are annotations: while a navigation tool is active
                // the link plane owns their pixels, so the annotation plane
                // stands down. The TypeScript drops the unregister function
                // too: the behavior lives as long as the annotation plugin.
                let interaction = cx.get(&INTERACTION)?;
                if let Some(annotation) = cx.try_get(&ANNOTATION)? {
                    let _ = annotation.register_behavior(Rc::new(LinkNavBehavior { interaction }));
                }
                Ok(())
            });
            Ok((api, Some(connect)))
        })
        .build()
}
