//! `paint-plan.ts`: the demand a page host supplies, the plan a tile plane
//! paints, and the plugin options with their resolved defaults.

use std::rc::Rc;

use epdf_geometry::Rect;
use serde::de::{self, Deserializer};
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};

use crate::raster_store::PageImageHandle;

/// Scroll velocity in page points per second: prefetch direction only.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Velocity {
    pub dx: f64,
    pub dy: f64,
}

/// `PageViewDemand`: what the page host asks of the tile plane. An absent
/// `visible_rect` means the whole page is visible.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageViewDemand {
    /// Device pixels across the page's unrotated content width.
    pub desired_device_width: f64,
    /// Visible page region, y-down page points.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_rect: Option<Rect>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub velocity: Option<Velocity>,
}

/// `TilePaintSource`: one tile the layer keeps in the DOM.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TilePaintSource {
    /// Reconciliation identity, stable across plan recomputes.
    pub key: String,
    pub scale: f64,
    /// Placement rect (the bled one), y-down page points.
    pub rect: Rect,
    /// Stacking rank among the scales present: coarse under fine.
    pub z: f64,
    pub handle: PageImageHandle,
}

/// `TilePaintPlan`. Plans are shared as `Rc`: the tile manager returns the
/// same `Rc` until something changes, which is the TypeScript's memo
/// identity (`Rc::ptr_eq` where a layer compared with `Object.is`).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TilePaintPlan {
    pub engaged: bool,
    pub paint: Vec<TilePaintSource>,
    /// Want-set keys still in flight.
    pub fetching: Vec<String>,
    pub stamp: String,
}

thread_local! {
    static EMPTY: Rc<TilePaintPlan> = Rc::new(TilePaintPlan {
        engaged: false,
        paint: Vec::new(),
        fetching: Vec::new(),
        stamp: "empty".into(),
    });
}

/// `EMPTY_TILE_PLAN`: one shared object, so every not-engaged answer is the
/// same `Rc`.
pub fn empty_tile_plan() -> Rc<TilePaintPlan> {
    EMPTY.with(Rc::clone)
}

/// Render points: the exact demand, or a ladder.
#[derive(Clone, Debug, PartialEq)]
pub enum Quantize {
    Exact,
    Ladder(Vec<f64>),
}

impl Serialize for Quantize {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Quantize::Exact => s.serialize_str("exact"),
            Quantize::Ladder(values) => values.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for Quantize {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Name(String),
            Ladder(Vec<f64>),
        }
        match Raw::deserialize(d)? {
            Raw::Name(name) if name == "exact" => Ok(Quantize::Exact),
            Raw::Name(other) => Err(de::Error::custom(format!("unknown quantize \"{other}\""))),
            Raw::Ladder(values) => Ok(Quantize::Ladder(values)),
        }
    }
}

/// The encoded formats (`PageImageOptions['format']`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RenderFormat {
    Png,
    Webp,
    Bmp,
}

impl RenderFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            RenderFormat::Png => "png",
            RenderFormat::Webp => "webp",
            RenderFormat::Bmp => "bmp",
        }
    }
}

/// `FullPageOptions`: the base-plane budget and render points.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FullPageOptions {
    #[serde(default)]
    pub max_width: Option<f64>,
    #[serde(default)]
    pub quantize: Option<Quantize>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrefetchOptions {
    #[serde(default)]
    pub margin: Option<f64>,
    #[serde(default)]
    pub velocity_bias: Option<bool>,
}

/// `TilesOptions`: the tile-plane strategy.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TilesOptions {
    #[serde(default)]
    pub size: Option<f64>,
    #[serde(default)]
    pub quantize: Option<Quantize>,
    #[serde(default)]
    pub max_scale: Option<f64>,
    #[serde(default)]
    pub bleed: Option<f64>,
    #[serde(default)]
    pub engage_at: Option<f64>,
    #[serde(default)]
    pub prefetch: Option<PrefetchOptions>,
    #[serde(default)]
    pub settle_ms: Option<f64>,
    #[serde(default)]
    pub fade_ms: Option<f64>,
}

/// `tiles?: TilesOptions | false`.
#[derive(Clone, Debug, PartialEq)]
pub enum TilesSetting {
    Off,
    On(TilesOptions),
}

impl<'de> Deserialize<'de> for TilesSetting {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Flag(bool),
            Options(TilesOptions),
        }
        match Raw::deserialize(d)? {
            Raw::Flag(false) => Ok(TilesSetting::Off),
            Raw::Flag(true) => Ok(TilesSetting::On(TilesOptions::default())),
            Raw::Options(options) => Ok(TilesSetting::On(options)),
        }
    }
}

/// `RenderPluginOptions`: the render strategy.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderPluginOptions {
    #[serde(default)]
    pub full_page: Option<FullPageOptions>,
    #[serde(default)]
    pub tiles: Option<TilesSetting>,
    #[serde(default)]
    pub format: Option<RenderFormat>,
    #[serde(default)]
    pub quality: Option<f64>,
    #[serde(default)]
    pub debug: Option<bool>,
}

/// `DEFAULT_TILE_PYRAMID`: the ×2 client pyramid for lattices without a
/// tiles block.
pub const DEFAULT_TILE_PYRAMID: [f64; 6] = [1.0, 2.0, 4.0, 8.0, 16.0, 32.0];

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedFullPage {
    pub max_width: f64,
    /// Only an embedder-set budget also filters an advertised ladder.
    pub max_width_explicit: bool,
    pub quantize: Quantize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedTiles {
    pub enabled: bool,
    pub size: f64,
    pub quantize: Quantize,
    pub max_scale: f64,
    pub bleed_px: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engage_at: Option<f64>,
    pub prefetch_margin: f64,
    pub velocity_bias: bool,
    pub settle_ms: f64,
    pub fade_ms: f64,
    pub fallback_pyramid: Vec<f64>,
}

/// `ResolvedRenderOptions`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedRenderOptions {
    pub full_page: ResolvedFullPage,
    pub tiles: ResolvedTiles,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<RenderFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<f64>,
    pub debug: bool,
}

/// `resolveRenderOptions`.
pub fn resolve_render_options(options: &RenderPluginOptions) -> ResolvedRenderOptions {
    let tiles = match &options.tiles {
        Some(TilesSetting::On(tiles)) => Some(tiles),
        Some(TilesSetting::Off) | None => None,
    };
    let full_page = options.full_page.as_ref();
    let prefetch = tiles.and_then(|t| t.prefetch.as_ref());
    ResolvedRenderOptions {
        full_page: ResolvedFullPage {
            max_width: full_page.and_then(|f| f.max_width).unwrap_or(640.0),
            max_width_explicit: full_page.and_then(|f| f.max_width).is_some(),
            quantize: full_page.and_then(|f| f.quantize.clone()).unwrap_or(Quantize::Exact),
        },
        tiles: ResolvedTiles {
            enabled: options.tiles != Some(TilesSetting::Off),
            size: tiles.and_then(|t| t.size).unwrap_or(512.0),
            quantize: tiles.and_then(|t| t.quantize.clone()).unwrap_or(Quantize::Exact),
            max_scale: tiles.and_then(|t| t.max_scale).unwrap_or(128.0),
            bleed_px: tiles.and_then(|t| t.bleed).unwrap_or(1.0),
            engage_at: tiles.and_then(|t| t.engage_at),
            prefetch_margin: prefetch.and_then(|p| p.margin).unwrap_or(0.5),
            velocity_bias: prefetch.and_then(|p| p.velocity_bias).unwrap_or(true),
            settle_ms: tiles.and_then(|t| t.settle_ms).unwrap_or(150.0),
            fade_ms: tiles.and_then(|t| t.fade_ms).unwrap_or(0.0),
            fallback_pyramid: DEFAULT_TILE_PYRAMID.to_vec(),
        },
        format: options.format,
        quality: options.quality,
        debug: options.debug.unwrap_or(false),
    }
}
