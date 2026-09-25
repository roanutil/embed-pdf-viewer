//! `packages/plugin/interaction` in Rust: the pointer hub every interactive
//! plugin rides on (one active tool, one cursor, one priority-ordered
//! handler list) and the platform feedback seam.
//!
//! The file split follows the TypeScript package: `types`, `reducer`,
//! `capability`, `feedback`, and the wiring in [`interaction_plugin`].
//! `callbacks` holds the JavaScript `Set` semantics the callback lists need,
//! shared with the selection plugin.
//!
//! Downstream plugins (selection, annotation, form, stage) implement
//! [`InteractionHandler`] and register it with
//! [`InteractionCapability::register_handler`] from their `init`, as the
//! TypeScript does.

pub mod callbacks;
pub mod capability;
pub mod feedback;
pub mod reducer;
pub mod types;

#[cfg(feature = "harness")]
pub mod harness;

use epdf_kernel::{AnyPlugin, define};

pub use callbacks::{CallbackSet, Unregister, Unsubscriber, VoidCallback};
pub use capability::{InteractionCapability, create_interaction_capability};
pub use feedback::{
    FEEDBACK, Feedback, FeedbackPluginOptions, ImpactWeight, NotifyKind, PlatformFeedback,
    feedback_plugin,
};
pub use reducer::{initial_interaction_state, interaction_reducer};
pub use types::*;

/// The two built-in tools. `enables` is the composition seam: `pointer`
/// turns on text selection and annotation editing, `pan` scrolling; both
/// carry `form-fill` and `link-nav`, the resting state of a viewer.
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
    define::<InteractionState, InteractionAction>("interaction")
        .document_scoped()
        .initial_state(move || initial_interaction_state(&config))
        .reduce(interaction_reducer)
        .capability(&INTERACTION, |ctx| Ok(create_interaction_capability(ctx, builtin_tools())))
        .build()
}
