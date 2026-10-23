//! `packages/plugin/render` in Rust: document-scoped page rasters over the
//! engine handle.
//!
//! Policy conformance (the one consumer of the deployment's render lattice),
//! per-page raster versions for content, annotations and form fields fed by
//! the document event stream and the `invalidate` verb, the owners'
//! appearance pictures, and the tile paint-plan machinery. Tiling is a
//! strategy inside this plugin, not a sibling.
//!
//! The module split follows the TypeScript package: [`contract`] and
//! [`host_contract`] (the two lenses), [`model`], [`invalidation`],
//! [`paint_plan`], [`strategy`], [`tiles`], [`raster_store`],
//! [`appearance_store`], [`tile_manager`], [`controller`], and the wiring in
//! [`render_plugin`]. [`policy`] is `engine-core`'s render-policy snaps,
//! which the plugin's arithmetic rests on. Names mirror the TypeScript
//! (`getSourceKey` is [`RenderCapability::get_source_key`]).
//!
//! The console a browser would supply comes in through [`RenderHost`];
//! timers and frames are the kernel's clock.

// Callback-shaped types (`Rc<dyn Fn(...)>`) are the port of JavaScript
// closures.
#![allow(clippy::type_complexity)]

use std::sync::LazyLock;

use epdf_kernel::settings::SettingsDeclaration;
use epdf_kernel::{AnyPlugin, Token, define};
use serde_json::Value;

pub mod appearance_store;
pub mod contract;
pub mod controller;
pub mod host_contract;
pub mod invalidation;
pub mod js;
pub mod model;
pub mod paint_plan;
pub mod policy;
pub mod raster_store;
pub mod strategy;
pub mod tile_manager;
pub mod tiles;

#[cfg(feature = "uniffi")]
pub mod ffi;
#[cfg(feature = "harness")]
pub mod harness;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

pub use appearance_store::{AppearanceStore, ITEMS_KEPT, RecentPictures, SETS_KEPT};
pub use contract::{
    InvalidateOptions, InvalidateScope, PageArg, PageImage, PageLayerOptions, PageRender,
    PageRenderFailure, RenderBatchResult, RenderConfig, RenderInvalidatedEvent, RenderPageOptions,
    RenderPagesOptions, RenderSettings, RenderThumbnailOptions, TilesSetting, render_defaults,
};
pub use controller::{
    RenderCapability, RenderSettingsChanged, ViewDemand, create_render_controller,
};
pub use host_contract::{
    AppearanceFamily, AppearanceItem, AppearancePicture, AppearancesOptions, PaintSettings,
    RenderCompletedEvent, RenderFailedEvent, RenderHost, RenderSourceOptions,
};
pub use invalidation::{PixelChange, pixel_change_of};
pub use model::{
    PageLayerRights, PageRenderLayers, RenderState, initial_render_state, invalidate_pages,
    layers_key_of, render_epoch_of, resolve_page_layers,
};
pub use paint_plan::{
    DEFAULT_TILE_PYRAMID, FullPageOptions, PageViewDemand, PrefetchOptions, Quantize, RenderFormat,
    ResolvedRenderOptions, TilePaintPlan, TilePaintSource, TilesOptions, Velocity, empty_tile_plan,
    resolve_render_options, same_page_view_demand,
};
pub use policy::{
    CONTINUOUS_RENDER_POLICY, EngineRenderPolicy, LatticePolicy, PageRenderViewport, SnapOptions,
    snap_appearance_scale, snap_full_page_viewport, snap_tile_scale,
};
pub use raster_store::{DEFAULT_MAX_COST, PageImageHandle, RasterStore, ReleaseImage};
pub use strategy::{ResolvedStrategy, base_ask_width, resolve_strategy};
pub use tile_manager::{TileManager, TileManagerDeps, TileStats};
pub use tiles::{PageSizePt, TileCoord, TileGrid};

/// `RenderToken`.
pub static RENDER: LazyLock<Token<RenderCapability>> = LazyLock::new(|| Token::new("render"));

/// `renderPlugin(config)`, with the default [`RenderHost`].
pub fn render_plugin(config: RenderConfig) -> AnyPlugin {
    render_plugin_with_host(config, RenderHost::default())
}

/// `renderPlugin(config)` on a host's console. `config` is the settings the
/// app registers, merged over [`render_defaults`].
pub fn render_plugin_with_host(config: RenderConfig, host: RenderHost) -> AnyPlugin {
    define::<RenderState, Value>("render")
        .document_scoped()
        .initial_state(initial_render_state)
        .settings(SettingsDeclaration {
            defaults: contract::render_defaults_json(),
            registered: Some(serde_json::to_value(&config).expect("plain data")),
            whole: Vec::new(),
        })
        .create(&RENDER, move |cx| create_render_controller(cx, host.clone()))
        .build()
}
