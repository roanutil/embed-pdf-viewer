//! The engine's render-parameter policy and the three snaps that conform a
//! request to it: `engine-core`'s `DocumentRenderService.ts`, ported here
//! because the plugin's arithmetic rests on it and `engine-core` itself is
//! not a Rust crate.
//!
//! `continuous`: any viewport renders exactly as requested (the local engine
//! always answers this). `lattice`: the deployment serves a finite set of
//! render points; full pages quantize on width, tiles and annotation
//! appearances on scale. The snaps are pure and explicit: identity under
//! `continuous`, snap UP and cap at the largest point under a lattice.

use epdf_kernel::Error;
use serde::{Deserialize, Serialize};

use crate::paint_plan::RenderFormat;
use crate::strategy::ascending;

/// `EngineRenderPolicy`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum EngineRenderPolicy {
    Continuous,
    Lattice(LatticePolicy),
}

/// The `lattice` member of [`EngineRenderPolicy`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LatticePolicy {
    pub full_page: FullPageLattice,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tiles: Option<TileLattice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearances: Option<AppearanceLattice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_render_pixels: Option<f64>,
    #[serde(default)]
    pub formats: Vec<RenderFormat>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enforced: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FullPageLattice {
    pub widths: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TileLattice {
    pub tile_sizes: Vec<f64>,
    pub scales: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AppearanceLattice {
    pub scales: Vec<f64>,
}

/// `CONTINUOUS_RENDER_POLICY`.
pub const CONTINUOUS_RENDER_POLICY: EngineRenderPolicy = EngineRenderPolicy::Continuous;

impl EngineRenderPolicy {
    /// The policy the kernel materialized on `DocumentMeta.renderPolicy`.
    /// A value this port cannot read (a format it does not know, a missing
    /// ladder) is `continuous`, the kernel's own fallback.
    pub fn from_json(value: &serde_json::Value) -> EngineRenderPolicy {
        serde_json::from_value(value.clone()).unwrap_or(CONTINUOUS_RENDER_POLICY)
    }

    pub fn lattice(&self) -> Option<&LatticePolicy> {
        match self {
            EngineRenderPolicy::Lattice(lattice) => Some(lattice),
            EngineRenderPolicy::Continuous => None,
        }
    }
}

/// `PageRenderViewport`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum PageRenderViewport {
    /// One PDF unit as `scale` device pixels; absent is 1.
    Scale {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scale: Option<f64>,
    },
    /// Exact output width in device pixels.
    Width { width: f64 },
}

/// The options `snapFullPageViewport` reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapOptions {
    #[serde(default)]
    pub page_width: Option<f64>,
}

/// Snap UP a sorted list; cap at the largest.
fn snap_up(sorted: &[f64], wanted: f64) -> f64 {
    sorted.iter().copied().find(|&v| v >= wanted).unwrap_or_else(|| sorted[sorted.len() - 1])
}

/// `snapFullPageViewport`: conform a full-page viewport. Under a lattice the
/// answer is always width-kind; a scale viewport converts through
/// `opts.page_width`, and without a positive one it is an `InvalidArg`.
pub fn snap_full_page_viewport(
    policy: &EngineRenderPolicy,
    viewport: PageRenderViewport,
    opts: Option<SnapOptions>,
) -> Result<PageRenderViewport, Error> {
    let EngineRenderPolicy::Lattice(lattice) = policy else { return Ok(viewport) };
    if lattice.full_page.widths.is_empty() {
        return Err(Error::engine("InvalidArg", "render policy has an empty width ladder"));
    }
    let widths = ascending(&lattice.full_page.widths);
    let requested = match viewport {
        PageRenderViewport::Width { width } => width,
        PageRenderViewport::Scale { scale } => {
            let page_width = opts.and_then(|o| o.page_width);
            match page_width {
                Some(page_width) if page_width > 0.0 => scale.unwrap_or(1.0) * page_width,
                _ => {
                    return Err(Error::engine(
                        "InvalidArg",
                        "snapping a scale viewport to a width lattice requires opts.pageWidth",
                    ));
                }
            }
        }
    };
    Ok(PageRenderViewport::Width { width: snap_up(&widths, requested) })
}

/// `snapAppearanceScale`: identity on continuous and without an appearance
/// block.
pub fn snap_appearance_scale(policy: &EngineRenderPolicy, scale: f64) -> Result<f64, Error> {
    let Some(appearances) = policy.lattice().and_then(|l| l.appearances.as_ref()) else {
        return Ok(scale);
    };
    if appearances.scales.is_empty() {
        return Err(Error::engine(
            "InvalidArg",
            "render policy has an empty appearance-scale lattice",
        ));
    }
    Ok(snap_up(&ascending(&appearances.scales), scale))
}

/// `snapTileScale`: identity on continuous and without a tiles block.
pub fn snap_tile_scale(policy: &EngineRenderPolicy, scale: f64) -> Result<f64, Error> {
    let Some(tiles) = policy.lattice().and_then(|l| l.tiles.as_ref()) else {
        return Ok(scale);
    };
    if tiles.scales.is_empty() {
        return Err(Error::engine("InvalidArg", "render policy has an empty tile-scale lattice"));
    }
    Ok(snap_up(&ascending(&tiles.scales), scale))
}
