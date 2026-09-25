//! `packages/plugin/redaction` in Rust: the destructive half of the
//! two-stage redaction model. Marking is annotation-plane work (the composed
//! `redact` tool ships with the annotation plugin); this plugin wraps the
//! engine's optional `redaction.apply`, projects the pending queue out of
//! annotation state, and estimates collateral for confirm dialogs. It owns
//! no mark state of its own.
//!
//! The file split follows the TypeScript package: [`types`], [`reducer`],
//! [`capability`], and the wiring of `redaction.plugin.ts` in
//! [`redaction_plugin`]. `/contract` is [`REDACTION`] and the types.
//!
//! Trust boundary: on a layered document, applying rewrites the layer's
//! bytes; the immutable base keeps the original.

// Callback-shaped types are the port of JavaScript closures, as in the kernel.
#![allow(clippy::type_complexity)]

pub mod capability;
pub mod reducer;
pub mod types;

#[cfg(feature = "harness")]
pub mod harness;

use epdf_kernel::{AnyPlugin, define};
use epdf_plugin_annotation::ANNOTATION;
use epdf_plugin_interaction::INTERACTION;
use epdf_plugin_selection::SELECTION;

pub use capability::{
    APPLY_CAPABILITIES, PdfRect, RedactionCapability, collect_pending, create_redaction_capability,
    estimate_collateral, id_of, intersects, regions_of,
};
pub use reducer::{initial_redaction_state, redaction_reducer};
pub use types::*;

/// `redactionPlugin()`: document-scoped, requires the annotation plane, and
/// uses the interaction hub and the selection plugin when present. `host`
/// carries the one ambient value the TypeScript reads (`console.error`).
pub fn redaction_plugin(host: RedactionHost) -> AnyPlugin {
    define::<RedactionState, RedactionAction>("redaction")
        .document_scoped()
        .requires(ANNOTATION.any())
        .optional(INTERACTION.any())
        .optional(SELECTION.any())
        .initial_state(initial_redaction_state)
        .reduce(redaction_reducer)
        .capability(&REDACTION, move |ctx| create_redaction_capability(ctx, host.clone()))
        .build()
}
