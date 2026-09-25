//! `strategy.ts`: the render points a document actually uses, POLICY ∧
//! STRATEGY. A strategy value applies as written under `continuous` and
//! conforms to the advertised lattice when there is one.

use serde::Serialize;

use crate::js;
use crate::paint_plan::{Quantize, RenderFormat, ResolvedRenderOptions};
use crate::policy::EngineRenderPolicy;

/// `ResolvedStrategy`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedStrategy {
    /// Base render points; `None` is exact-up-to-budget.
    pub widths: Option<Vec<f64>>,
    /// No full-page raster is ever wider (device px).
    pub max_width: f64,
    /// Tile levels (device px per point); `None` is the exact scale at rest.
    pub pyramid: Option<Vec<f64>>,
    pub tile_size: f64,
    pub engage_at: f64,
    pub tile_max_scale: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<RenderFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<f64>,
}

/// `[...values].sort((a, b) => a - b)`: stable, NaN comparing equal.
pub(crate) fn ascending(values: &[f64]) -> Vec<f64> {
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    sorted
}

/// Rung list capped by the budget; always keeps at least the smallest rung.
fn cap_widths(widths: &[f64], max_width: f64) -> Vec<f64> {
    let sorted = ascending(widths);
    let kept: Vec<f64> = sorted.iter().copied().filter(|&w| w <= max_width).collect();
    if kept.is_empty() { sorted.into_iter().take(1).collect() } else { kept }
}

/// `resolveStrategy`.
pub fn resolve_strategy(
    policy: &EngineRenderPolicy,
    options: &ResolvedRenderOptions,
) -> ResolvedStrategy {
    let lattice = policy.lattice();

    let widths = match (lattice, &options.full_page.quantize) {
        (Some(lattice), _) if options.full_page.max_width_explicit => {
            Some(cap_widths(&lattice.full_page.widths, options.full_page.max_width))
        }
        (Some(lattice), _) => Some(ascending(&lattice.full_page.widths)),
        (None, Quantize::Exact) => None,
        (None, Quantize::Ladder(ladder)) => Some(cap_widths(ladder, options.full_page.max_width)),
    };

    let client_pyramid = match &options.tiles.quantize {
        Quantize::Exact => None,
        Quantize::Ladder(ladder) => Some(ascending(ladder)),
    };
    let pyramid = match lattice {
        Some(lattice) => match &lattice.tiles {
            Some(tiles) => Some(ascending(&tiles.scales)),
            None => Some(client_pyramid.unwrap_or_else(|| options.tiles.fallback_pyramid.clone())),
        },
        None => client_pyramid,
    };

    let tile_size = match lattice.and_then(|l| l.tiles.as_ref()) {
        Some(tiles) if !tiles.tile_sizes.is_empty() => {
            if tiles.tile_sizes.contains(&options.tiles.size) {
                options.tiles.size
            } else {
                tiles.tile_sizes[0]
            }
        }
        _ => options.tiles.size,
    };

    let engage_at = options.tiles.engage_at.unwrap_or(if widths.is_none() { 1.0 } else { 1.25 });

    let format = match (options.format, lattice) {
        (None, _) => None,
        (Some(requested), None) => Some(requested),
        (Some(requested), Some(lattice)) if lattice.formats.contains(&requested) => Some(requested),
        (Some(_), Some(lattice)) => lattice.formats.first().copied(),
    };

    ResolvedStrategy {
        widths,
        max_width: options.full_page.max_width,
        pyramid,
        tile_size,
        engage_at,
        tile_max_scale: options.tiles.max_scale,
        format,
        quality: options.quality,
    }
}

/// `baseAskWidth`: the one base-sizing function. Exact mode returns the
/// demand itself up to the budget; ladder mode snaps UP and caps at the top.
pub fn base_ask_width(strategy: &ResolvedStrategy, demand_width: f64) -> f64 {
    let wanted = js::max(1.0, js::round(demand_width));
    match &strategy.widths {
        None => js::min(wanted, strategy.max_width),
        Some(widths) => {
            widths.iter().copied().find(|&w| w >= wanted).unwrap_or(widths[widths.len() - 1])
        }
    }
}
