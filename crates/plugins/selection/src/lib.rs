//! `packages/plugin/selection` in Rust: text selection over the engine's
//! text geometry.
//!
//! Selection ranges live in character space (half-open [`TextRange`], the
//! space search hits address); geometry needs `doc.text.select`, text
//! extraction `doc.text.copy`, and neither implies the other.
//!
//! The file split follows the TypeScript package: [`contract`] (with
//! `token.ts`), [`model`], [`controller`] (whose [`SelectionCapability`] is
//! the public lens and `host-contract.ts`'s host lens in one type),
//! [`handler`], [`handles`], the coordinate seam in [`geometry`], and the
//! wiring in [`selection_plugin`]. `/internal` exports the controller, the
//! handler, the model and the seam; here they are public modules.
//!
//! [`text_layout`] is the engine-core text layer the plugin is built on.

pub mod contract;
pub mod controller;
pub mod geometry;
pub mod handler;
pub mod handles;
pub mod model;
pub mod text_layout;

#[cfg(feature = "harness")]
pub mod harness;

#[cfg(feature = "uniffi")]
pub mod ffi;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

use std::rc::Rc;

use epdf_kernel::{AnyPlugin, define};
use epdf_plugin_interaction::{FEEDBACK, INTERACTION, RegisterHandlerOptions};

pub use contract::*;
pub use controller::{
    OPEN_END, OperationOptions, SelectionCapability, create_selection_controller,
};
pub use geometry::{
    SelectionPageGeometry, build_selection_page_geometry, content_point_to_pdf, to_content_segment,
    to_content_text_quad,
};
pub use handler::{TextSelectHandler, TextSelectHandlerOptions, create_text_select_handler};
pub use handles::{
    HANDLE_BAR, HANDLE_HEAD, HANDLE_PAD, HandleRole, SelectionHandleDragSession,
    SelectionHandleEndpoint, SelectionHandleGeom, SelectionHandleTarget, SelectionHandleView,
    create_selection_handle_drag, selection_handle_geom,
};
pub use model::{
    GlyphPointer, PageObjectNumber, SelectionAction, SelectionRange, SelectionState,
    initial_selection_state, reduce_selection,
};

/// `SelectionHostCapability`: the same object as the public capability.
pub type SelectionHostCapability = SelectionCapability;

/// `selectionPlugin(config)`: document-scoped, requires the interaction hub,
/// and uses platform feedback when a provider is registered. Its `connect`
/// registers the pointer handler with the hub.
pub fn selection_plugin(config: SelectionConfig) -> AnyPlugin {
    define::<SelectionState, SelectionAction>("selection")
        .document_scoped()
        .requires(INTERACTION.any())
        .optional(FEEDBACK.any())
        .initial_state(initial_selection_state)
        .reduce(reduce_selection)
        .create(&SELECTION, move |cx| {
            let (api, connect) = create_selection_controller(cx.clone());
            let selection = api.clone();
            let connect: epdf_kernel::plugin::ConnectFn = Rc::new(move || {
                connect()?;
                let interaction = cx.get(&INTERACTION)?;
                let feedback = cx.try_get(&FEEDBACK)?;
                interaction.register_handler(
                    Rc::new(create_text_select_handler(
                        selection.clone(),
                        interaction.clone(),
                        feedback,
                        TextSelectHandlerOptions { drag_threshold: config.drag_threshold },
                    )),
                    RegisterHandlerOptions::default(),
                );
                Ok(())
            });
            Ok((api, Some(connect)))
        })
        .build()
}
