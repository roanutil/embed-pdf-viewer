//! `packages/plugin/interaction` in Rust: the pointer hub every interactive
//! plugin rides on (one active tool, one cursor, one priority-ordered
//! handler list) and the platform feedback seam.
//!
//! The file split follows the TypeScript package: [`contract`] (with the
//! host lens folded into [`InteractionCapability`]), [`model`],
//! [`controller`], [`feedback`], and the wiring in [`interaction_plugin`].
//! `callbacks` holds `Unregister`, what the registries return.
//!
//! Downstream plugins (selection, annotation, form, stage) implement
//! [`InteractionHandler`] and register it with
//! [`InteractionCapability::register_handler`], as the TypeScript does
//! through the host contract.

pub mod callbacks;
pub mod contract;
pub mod controller;
pub mod feedback;
pub mod model;

#[cfg(feature = "harness")]
pub mod harness;

use epdf_kernel::{AnyPlugin, define};

pub use callbacks::Unregister;
pub use contract::*;
pub use controller::{InteractionCapability, create_interaction_controller};
pub use feedback::{
    FEEDBACK, Feedback, FeedbackPluginOptions, ImpactWeight, NotifyKind, PlatformFeedback,
    feedback_plugin,
};
pub use model::{
    InteractionAction, InteractionState, initial_interaction_state, reduce_interaction,
};

/// `InteractionHostCapability`: the same object as the public capability.
pub type InteractionHostCapability = InteractionCapability;

/// The two built-in tools. `enables` is the composition seam: `pointer`
/// turns on text selection, annotation editing and marquee selection, `pan`
/// scrolling and annotation editing; both carry `form-fill` and `link-nav`,
/// the resting state of a viewer.
pub fn builtin_tools() -> Vec<Tool> {
    vec![
        Tool::new(
            "pointer",
            "default",
            ["text-select", "annotation-edit", "annotation-marquee", "form-fill", "link-nav"],
        ),
        // pan works anywhere: the open hand doesn't stop at page edges.
        Tool::new("pan", "grab", ["scroll", "annotation-edit", "form-fill", "link-nav"])
            .with_gap_cursor("grab"),
    ]
}

/// `interactionPlugin(config)`: document-scoped, depends on nothing.
pub fn interaction_plugin(config: InteractionConfig) -> AnyPlugin {
    let initial = config.clone();
    define::<InteractionState, InteractionAction>("interaction")
        .document_scoped()
        .initial_state(move || initial_interaction_state(&initial))
        .reduce(reduce_interaction)
        .create(&INTERACTION, move |cx| {
            create_interaction_controller(cx, builtin_tools(), config.tools.clone())
        })
        .build()
}
