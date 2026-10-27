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
//! `connect`, [`handler`], [`handles`], [`geometry`], [`state`], and the
//! wiring in [`selection_plugin`].
//!
//! [`text_layout`] is the engine-core text layer the plugin is built on: a
//! pure copy of `crates/epdf-engine`'s, since that crate links PDFium and
//! this one also builds for wasm32 (Ruling13.15).

mod connect;
pub mod contract;
pub mod controller;
pub mod geometry;
pub mod handler;
pub mod handles;
pub mod model;
pub mod state;
pub mod text_layout;

#[cfg(feature = "harness")]
pub mod harness;

#[cfg(feature = "uniffi")]
pub mod ffi;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

use epdf_kernel::settings::SettingsDeclaration;
use epdf_kernel::{AnyPlugin, define};
use epdf_plugin_interaction::{FEEDBACK, INTERACTION};
use serde_json::Value;

pub use contract::*;
pub use controller::{OPEN_END, OperationOptions, SelectionCapability};
pub use handler::{TextSelectHandler, TextSelectHandlerOptions, create_text_select_handler};
pub use handles::{
    ArmedSelectionHandle, HANDLE_BAR, HANDLE_HEAD, HANDLE_PAD, HandleRole,
    SelectionHandleDragSession, SelectionHandleEndpoint, SelectionHandleEndpoints,
    SelectionHandleGeom, SelectionHandleStage, SelectionHandleTarget, SelectionHandleView,
    arm_selection_handle, create_selection_handle_drag, selection_handle_endpoints_of,
    selection_handle_geom, selection_handle_view_of,
};
pub use model::{
    GlyphPosition, PageObjectNumber, SegmentsByPage, SelectionRange, SelectionState,
    initial_selection_state,
};
pub use state::{SelectionStateView, selection_state};

/// `SelectionHostCapability`: the same object as the public capability.
pub type SelectionHostCapability = SelectionCapability;

/// `selectionPlugin(config)`: document-scoped, requires the interaction hub
/// (whose pointer stream drives the selection's own handler) and uses
/// platform feedback when a provider is registered. `config` is the
/// settings the app registers, over [`selection_defaults`].
pub fn selection_plugin(config: SelectionConfig) -> AnyPlugin {
    define::<SelectionState, Value>("selection")
        .document_scoped()
        .requires(INTERACTION.any())
        .optional(FEEDBACK.any())
        .initial_state(initial_selection_state)
        .settings(SettingsDeclaration {
            defaults: serde_json::to_value(selection_defaults()).expect("plain data"),
            registered: Some(config.to_json()),
            whole: Vec::new(),
        })
        .create(&SELECTION, |cx| {
            let (api, connect) = controller::create_selection_controller(cx)?;
            Ok((api, Some(connect)))
        })
        .build()
}
