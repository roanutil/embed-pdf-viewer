//! `packages/plugin/link` in Rust: the navigation plane of PDF link
//! annotations. This plugin owns clicking links: per-page clickable areas
//! and one `activate()` entry point (goto reveals through the stage, the
//! action engine takes a payload tree, a URI goes back to the framework
//! opener, everything executable-shaped is reported and never executed).
//! The annotation plugin owns authoring them. The active tool's `link-nav`
//! tag decides which plane owns a click.
//!
//! The file split follows the TypeScript package: [`types`], [`reducer`],
//! [`source`], [`capability`], [`effects`], and the wiring of
//! `link.plugin.ts` in [`link_plugin`]. `/contract` is [`LINK`] and the
//! types.

// Callback-shaped types are the port of JavaScript closures, as in the kernel.
#![allow(clippy::type_complexity)]

pub mod capability;
pub mod effects;
pub mod reducer;
pub mod source;
pub mod types;

#[cfg(feature = "harness")]
pub mod harness;

use std::rc::Rc;

use epdf_kernel::{AnyPlugin, define};
use epdf_plugin_actions::ACTIONS;
use epdf_plugin_annotation::{ANNOTATION, Behavior};
use epdf_plugin_interaction::{INTERACTION, InteractionCapability};
use epdf_plugin_stage::STAGE;

pub use capability::{LinkCapability, create_link_capability};
pub use effects::register_link_effects;
pub use reducer::{initial_link_state, link_reducer};
pub use source::{load_links_page, project_links};
pub use types::*;

/// The Behavior `init` registers with the annotation plugin: links stay
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
        self.interaction.active_tool().enables("link-nav")
    }
}

/// `linkPlugin(config)`: document-scoped, requires only the interaction
/// hub, and uses the stage, the annotation plugin and the action engine
/// when present. With the annotation plugin its folded model becomes the
/// data source and the `link-nav` behavior hands links to the editing
/// plane while a tool without that tag is active.
pub fn link_plugin(config: LinkPluginConfig) -> AnyPlugin {
    define::<LinkState, LinkAction>("link")
        .document_scoped()
        .requires(INTERACTION.any())
        .optional(STAGE.any())
        .optional(ANNOTATION.any())
        .optional(ACTIONS.any())
        .initial_state(initial_link_state)
        .reduce(link_reducer)
        .capability(&LINK, move |ctx| Ok(create_link_capability(ctx, config.clone())))
        .effects(register_link_effects)
        .init(|ctx| {
            let interaction = ctx.get(&INTERACTION)?;
            if let Some(annotation) = ctx.try_get(&ANNOTATION)? {
                // The TypeScript drops the unregister function too: the
                // behavior lives as long as the annotation capability.
                annotation.register_behavior(Rc::new(LinkNavBehavior { interaction }));
            }
            Ok(None)
        })
        .build()
}
