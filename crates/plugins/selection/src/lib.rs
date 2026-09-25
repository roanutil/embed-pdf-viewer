//! `packages/plugin/selection` in Rust: text selection over the engine's
//! text geometry.
//!
//! Selection ranges live in character space (half-open [`TextRange`], the
//! space search hits address); geometry needs `doc.text.select`, text
//! extraction `doc.text.copy`, and neither implies the other.
//!
//! The TypeScript package has four entry points, and this crate exports
//! what each does:
//!
//! - the root: [`selection_plugin`], the token, the range vocabulary, and
//!   the handle policy ([`handles`]);
//! - `/contract`: the public half of [`SelectionCapability`];
//! - `/contract/host`: the same capability's host half (gesture verbs,
//!   warming, highlight visibility) and [`SelectionState`] /
//!   [`SelectionAction`], which sibling plugins (annotation markup,
//!   redaction) use;
//! - `/internal`: [`create_selection_capability`],
//!   [`create_text_select_handler`], the reducer and the coordinate seam
//!   ([`geometry`]).
//!
//! [`text_layout`] is the engine-core text layer the plugin is built on.

pub mod capability;
pub mod geometry;
pub mod handler;
pub mod handles;
pub mod reducer;
pub mod text_layout;
pub mod types;

#[cfg(feature = "harness")]
pub mod harness;

use std::rc::Rc;

use epdf_kernel::{AnyPlugin, define};
use epdf_plugin_interaction::{FEEDBACK, INTERACTION};

pub use capability::{MAX_SAFE_INTEGER, SelectionCapability, create_selection_capability};
pub use geometry::{
    SelectionPageGeometry, build_selection_page_geometry, content_point_to_pdf, to_content_segment,
    to_content_text_quad,
};
pub use handler::{TextSelectHandler, create_text_select_handler};
pub use handles::{
    HANDLE_BAR, HANDLE_HEAD, HANDLE_PAD, HandleRole, SelectionHandleDragSession,
    SelectionHandleEndpoint, SelectionHandleGeom, SelectionHandleTarget, SelectionHandleView,
    create_selection_handle_drag, selection_handle_geom,
};
pub use reducer::{initial_selection_state, selection_reducer};
pub use types::*;

/// `selectionPlugin()`: document-scoped, requires the interaction hub, and
/// uses platform feedback when a provider is registered. `init` registers
/// the pointer handler with the hub.
pub fn selection_plugin() -> AnyPlugin {
    define::<SelectionState, SelectionAction>("selection")
        .document_scoped()
        .requires(INTERACTION.any())
        .optional(FEEDBACK.any())
        .initial_state(initial_selection_state)
        .reduce(selection_reducer)
        .capability(&SELECTION, |ctx| Ok(create_selection_capability(ctx)))
        .init(|ctx| {
            let interaction = ctx.get(&INTERACTION)?;
            let selection = ctx.get(&SELECTION)?; // our own capability, built before init
            let feedback = ctx.try_get(&FEEDBACK)?;
            interaction.register_handler(
                Rc::new(create_text_select_handler(selection, interaction.clone(), feedback)),
                None,
            );
            Ok(None)
        })
        .build()
}
