//! `packages/plugin/stage` in Rust: the viewport lens. One camera, scene
//! and flat settings bag per document: zoom, scroll, spreads, layout,
//! motion over the host's clock, responsive settings, page-space
//! destinations, and the pan handler for the interaction hub.
//!
//! The module split follows the TypeScript package: [`contract`],
//! [`host_contract`] and [`token`] (the lenses), [`model`], [`settings`],
//! [`motion`], [`responsive`], [`destination`], [`controller`] with its
//! areas (`services`, `read`, `camera`, `navigation`, `settings::area`,
//! `view`), [`connect`] and [`working_set`], [`state`], [`scroll_handler`],
//! and the wiring in [`stage_plugin`] (`stage.plugin.ts`). Names mirror the
//! TypeScript contract (`getPageFrame` is
//! [`StageCapability::get_page_frame`]). The spatial math is
//! `epdf-stage-core`.

// Callback-shaped types (`Rc<dyn Fn(...)>`) are the port of JavaScript
// closures.
#![allow(clippy::type_complexity)]

pub mod connect;
pub mod contract;
pub mod controller;
pub mod destination;
pub mod host_contract;
pub mod model;
pub mod motion;
pub mod responsive;
pub mod scroll_handler;
pub mod settings;
pub mod state;
pub mod token;
pub mod working_set;

mod prelude;

/// `camera/`: the write path, motion and direct manipulation.
pub mod camera {
    pub mod animation;
    pub mod gestures;
    pub mod write;
}

/// `navigation/`: arrivals, reveals and the zoom verbs.
pub mod navigation {
    pub mod arrive;
    pub mod reveal;
    pub mod zoom;
}

/// `read/`: the page reads.
pub mod read {
    pub mod pages;
}

pub mod services;

/// `view/`: the view lifecycle.
pub mod view {
    pub mod lifecycle;
}

#[cfg(feature = "harness")]
pub mod harness;

#[cfg(feature = "uniffi")]
pub mod ffi;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

use epdf_kernel::{AnyPlugin, define};
use epdf_plugin_actions::ACTIONS;
use serde_json::Value;

pub use connect::DEFAULT_LENS_ID;
pub use contract::*;
pub use controller::StageCapability;
pub use destination::reveal_of_destination;
pub use model::{
    CameraObject, CameraOrder, MotionCause, StageState, initial_stage_state, mark_placed,
    patch_settings, set_active_rules, set_camera, set_camera_resting, set_cursor, set_dpr,
    set_motion_cause, set_viewport,
};
pub use responsive::{box_of, eq_setting, matches_query, merge_settings, resolve_responsive};
pub use scroll_handler::{ScrollHandlerOptions, create_scroll_handler};
pub use settings::{default_responsive, default_settings, settings_equal};
pub use state::stage_state;
pub use token::STAGE;
pub use working_set::{WorkingSetPage, WorkingSetRole, working_set_of};

/// `stagePlugin(options)`: one lens's camera, layout, zoom and navigation
/// over a document. Document-scoped, with actions as an optional
/// dependency: with it, the main lens feeds page state to it and interprets
/// GoTo and Named page actions.
pub fn stage_plugin(options: StagePluginOptions) -> AnyPlugin {
    let id = options.id.unwrap_or_else(|| DEFAULT_LENS_ID.to_string());
    let token = options.token.unwrap_or_else(|| STAGE.clone());
    let config = options.config;
    let settings = config.settings.clone();
    define::<StageState, Value>(id)
        .document_scoped()
        .optional(ACTIONS.any())
        .initial_state(move || initial_stage_state(&settings))
        .create(&token, move |cx| {
            let stage = controller::create_stage_controller(cx.clone(), config.clone());
            let connected = stage.clone();
            Ok((stage, Some(std::rc::Rc::new(move || connect::connect_stage(&cx, &connected)))))
        })
        .build()
}
