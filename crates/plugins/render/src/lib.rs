//! `packages/plugin/render` in Rust: the document-scoped render capability.
//!
//! Policy conformance (the one consumer of the deployment's render lattice),
//! per-page raster versioning fed by the document event stream and the
//! `invalidate` verb, and the tile paint-plan machinery. Tiling is a strategy
//! inside this plugin, not a sibling.
//!
//! The module split follows the TypeScript package: [`types`],
//! [`paint_plan`], [`strategy`], [`tiles`], [`raster_store`],
//! [`tile_manager`], [`reducer`], [`effects`], [`capability`], and the
//! wiring in [`render_plugin`]. [`policy`] is `engine-core`'s render-policy
//! snaps, which the plugin's arithmetic rests on. Names mirror the
//! TypeScript (`renderSourceKey` is [`RenderCapability::render_source_key`]).
//!
//! Everything a browser would supply (`requestAnimationFrame`, the console)
//! comes in through [`RenderHost`].

// Callback-shaped types (`Rc<dyn Fn(...)>`) are the port of JavaScript
// closures.
#![allow(clippy::type_complexity)]

use std::sync::LazyLock;

use epdf_kernel::{AnyPlugin, Token, define};

pub mod capability;
pub mod effects;
pub mod js;
pub mod paint_plan;
pub mod policy;
pub mod raster_store;
pub mod reducer;
pub mod strategy;
pub mod tile_manager;
pub mod tiles;
pub mod types;

#[cfg(feature = "harness")]
pub mod harness;

pub use capability::{RenderCapability, ViewTiles, create_render_capability};
pub use effects::{annotated_pons, register_render_effects};
pub use paint_plan::{
    DEFAULT_TILE_PYRAMID, FullPageOptions, PageViewDemand, PrefetchOptions, Quantize, RenderFormat,
    RenderPluginOptions, ResolvedRenderOptions, TilePaintPlan, TilePaintSource, TilesOptions,
    TilesSetting, Velocity, empty_tile_plan, resolve_render_options,
};
pub use policy::{
    CONTINUOUS_RENDER_POLICY, EngineRenderPolicy, LatticePolicy, PageRenderViewport, SnapOptions,
    snap_appearance_scale, snap_full_page_viewport, snap_tile_scale,
};
pub use raster_store::{PageImageHandle, RasterStore};
pub use reducer::{initial_render_state, render_reducer};
pub use strategy::{ResolvedStrategy, base_ask_width, resolve_strategy};
pub use tile_manager::{TileManager, TileManagerDeps, TileStats};
pub use tiles::{PageSizePt, PdfRect, TileCoord, TileGrid};
pub use types::{
    InvalidateOptions, InvalidateScope, PaintSettings, RenderAction, RenderHost, RenderPageOptions,
    RenderState,
};

/// `RenderToken`.
pub static RENDER: LazyLock<Token<RenderCapability>> = LazyLock::new(|| Token::new("render"));

/// `renderPlugin(options)`, with the default [`RenderHost`].
pub fn render_plugin(options: RenderPluginOptions) -> AnyPlugin {
    render_plugin_with_host(options, RenderHost::default())
}

/// `renderPlugin(options)` on a host's frame scheduler and console.
pub fn render_plugin_with_host(options: RenderPluginOptions, host: RenderHost) -> AnyPlugin {
    define::<RenderState, RenderAction>("render")
        .document_scoped()
        .initial_state(initial_render_state)
        .reduce(render_reducer)
        .capability(&RENDER, move |ctx| Ok(create_render_capability(ctx, &options, host.clone())))
        .effects(register_render_effects)
        .build()
}
