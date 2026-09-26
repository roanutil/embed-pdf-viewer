//! `packages/plugin/render` in Rust: document-scoped page rasters over the
//! engine handle.
//!
//! Policy conformance (the one consumer of the deployment's render lattice),
//! per-page raster versioning fed by the document event stream and the
//! `invalidate` verb, and the tile paint-plan machinery. Tiling is a strategy
//! inside this plugin, not a sibling.
//!
//! The module split follows the TypeScript package: [`contract`] and
//! [`host_contract`] (the two lenses), [`model`], [`invalidation`],
//! [`paint_plan`], [`strategy`], [`tiles`], [`raster_store`],
//! [`tile_manager`], [`controller`], and the wiring in [`render_plugin`].
//! [`policy`] is `engine-core`'s render-policy snaps, which the plugin's
//! arithmetic rests on. Names mirror the TypeScript (`getSourceKey` is
//! [`RenderCapability::get_source_key`]).
//!
//! Everything a browser would supply (`requestAnimationFrame`, the console)
//! comes in through [`RenderHost`].

// Callback-shaped types (`Rc<dyn Fn(...)>`) are the port of JavaScript
// closures.
#![allow(clippy::type_complexity)]

use std::sync::LazyLock;

use epdf_kernel::{AnyPlugin, Controller, Token, define};

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

#[cfg(feature = "harness")]
pub mod harness;

pub use contract::{
    InvalidateOptions, InvalidateScope, PageImage, PageRender, PageRenderFailure,
    RenderBatchResult, RenderConfig, RenderInvalidatedEvent, RenderPageOptions, RenderPagesOptions,
    RenderThumbnailOptions,
};
pub use controller::{RenderCapability, ViewDemand, create_render_controller};
pub use host_contract::{
    PaintSettings, RenderCompletedEvent, RenderFailedEvent, RenderHost, RenderSourceOptions,
};
pub use invalidation::annotated_pons;
pub use model::{RenderAction, RenderState, initial_render_state, reduce_render};
pub use paint_plan::{
    DEFAULT_TILE_PYRAMID, FullPageOptions, PageViewDemand, PrefetchOptions, Quantize, RenderFormat,
    ResolvedRenderOptions, TilePaintPlan, TilePaintSource, TilesOptions, TilesSetting, Velocity,
    empty_tile_plan, resolve_render_options,
};
pub use policy::{
    CONTINUOUS_RENDER_POLICY, EngineRenderPolicy, LatticePolicy, PageRenderViewport, SnapOptions,
    snap_appearance_scale, snap_full_page_viewport, snap_tile_scale,
};
pub use raster_store::{PageImageHandle, RasterStore};
pub use strategy::{ResolvedStrategy, base_ask_width, resolve_strategy};
pub use tile_manager::{TileManager, TileManagerDeps, TileStats};
pub use tiles::{PageSizePt, TileCoord, TileGrid};

/// `RenderToken`.
pub static RENDER: LazyLock<Token<RenderCapability>> = LazyLock::new(|| Token::new("render"));

/// `renderPlugin(config)`, with the default [`RenderHost`].
pub fn render_plugin(config: RenderConfig) -> AnyPlugin {
    render_plugin_with_host(config, RenderHost::default())
}

/// `renderPlugin(config)` on a host's frame scheduler and console.
pub fn render_plugin_with_host(config: RenderConfig, host: RenderHost) -> AnyPlugin {
    define::<RenderState, RenderAction>("render")
        .document_scoped()
        .initial_state(initial_render_state)
        .reduce(reduce_render)
        .create(&RENDER, move |cx: Controller<RenderState, RenderAction>| {
            create_render_controller(cx, &config, host.clone())
        })
        .build()
}
