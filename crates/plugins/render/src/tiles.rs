//! `tiles.ts`: pure tile-grid math over an aligned pyramid (one origin at
//! the page's top-left, a fixed tile edge in device px).
//!
//! Tile coords live on the device grid at their level's scale; paint rects
//! are y-down page points; engine rects are y-up PDF user space. Coords are
//! `f64` because the TypeScript's loop bounds come out of `Math.floor` and
//! `Math.ceil` and can be NaN; an integer type would change which rects
//! yield no tiles.

use epdf_geometry::Rect;
use serde::{Deserialize, Serialize};

pub use epdf_geometry::intersect_rects;

use crate::js;
use crate::paint_plan::Velocity;
use crate::strategy::ascending;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PageSizePt {
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TileCoord {
    pub ix: f64,
    pub iy: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TileGrid {
    /// Device px per PDF point at this level.
    pub scale: f64,
    /// Tile edge in device px.
    pub tile_size: f64,
    pub cols: f64,
    pub rows: f64,
}

/// `PdfRect`: y-up PDF user space, top > bottom.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PdfRect {
    pub left: f64,
    pub right: f64,
    pub top: f64,
    pub bottom: f64,
}

pub fn tile_grid(page: PageSizePt, scale: f64, tile_size: f64) -> TileGrid {
    TileGrid {
        scale,
        tile_size,
        cols: js::max(1.0, (page.width * scale / tile_size).ceil()),
        rows: js::max(1.0, (page.height * scale / tile_size).ceil()),
    }
}

/// One tile's edge in page points at its level.
fn tile_span_pt(grid: &TileGrid) -> f64 {
    grid.tile_size / grid.scale
}

/// The tiles of `grid` intersecting a y-down page-point rect, clamped to the
/// page, in row-major order.
pub fn tiles_in_rect(grid: &TileGrid, _page: PageSizePt, rect: Rect) -> Vec<TileCoord> {
    let span = tile_span_pt(grid);
    let x0 = js::max(0.0, (rect.x / span).floor());
    let y0 = js::max(0.0, (rect.y / span).floor());
    let x1 = js::min(grid.cols - 1.0, ((rect.x + rect.width) / span).ceil() - 1.0);
    let y1 = js::min(grid.rows - 1.0, ((rect.y + rect.height) / span).ceil() - 1.0);
    let mut out = Vec::new();
    let mut iy = y0;
    while iy <= y1 {
        let mut ix = x0;
        while ix <= x1 {
            out.push(TileCoord { ix, iy });
            ix += 1.0;
        }
        iy += 1.0;
    }
    out
}

/// Paint rect: y-down page points, clamped to the page edge.
pub fn tile_paint_rect(grid: &TileGrid, page: PageSizePt, c: TileCoord) -> Rect {
    let span = tile_span_pt(grid);
    let x = c.ix * span;
    let y = c.iy * span;
    Rect { x, y, width: js::min(span, page.width - x), height: js::min(span, page.height - y) }
}

/// Engine target rect for an arbitrary y-down page-point rect.
pub fn to_engine_rect(page: PageSizePt, rect: Rect) -> PdfRect {
    PdfRect {
        left: rect.x,
        right: rect.x + rect.width,
        top: page.height - rect.y,
        bottom: page.height - (rect.y + rect.height),
    }
}

pub fn tile_engine_rect(grid: &TileGrid, page: PageSizePt, c: TileCoord) -> PdfRect {
    to_engine_rect(page, tile_paint_rect(grid, page, c))
}

/// Expand a paint rect by `pt` points per side, clamped to the page: the
/// tile bleed.
pub fn bleed_rect(rect: Rect, pt: f64, page: PageSizePt) -> Rect {
    let x = js::max(0.0, rect.x - pt);
    let y = js::max(0.0, rect.y - pt);
    Rect {
        x,
        y,
        width: js::min(page.width, rect.x + rect.width + pt) - x,
        height: js::min(page.height, rect.y + rect.height + pt) - y,
    }
}

/// Does the painted set cover `region`? Stops at the first unpainted tile.
pub fn region_covered(
    grid: &TileGrid,
    page: PageSizePt,
    region: Rect,
    mut is_painted: impl FnMut(TileCoord) -> bool,
) -> bool {
    // `width <= 0 || height <= 0`: a NaN extent is not empty.
    if region.width <= 0.0 || region.height <= 0.0 {
        return true;
    }
    tiles_in_rect(grid, page, region).into_iter().all(&mut is_painted)
}

/// Inflate a visible rect by a prefetch margin (fractions of its own size
/// per side), stretched toward the direction of travel.
pub fn inflate_rect(rect: Rect, margin: f64, velocity: Option<Velocity>) -> Rect {
    let mx = rect.width * margin;
    let my = rect.height * margin;
    let (mut left, mut right, mut up, mut down) = (mx, mx, my, my);
    if let Some(v) = velocity {
        if v.dx > 0.0 {
            right = mx * 2.0;
            left = mx / 2.0;
        } else if v.dx < 0.0 {
            left = mx * 2.0;
            right = mx / 2.0;
        }
        if v.dy > 0.0 {
            down = my * 2.0;
            up = my / 2.0;
        } else if v.dy < 0.0 {
            up = my * 2.0;
            down = my / 2.0;
        }
    }
    Rect {
        x: rect.x - left,
        y: rect.y - up,
        width: rect.width + left + right,
        height: rect.height + up + down,
    }
}

/// Snap UP through a pyramid's sorted scales; cap at the top.
pub fn snap_to_pyramid(scales: &[f64], needed: f64) -> f64 {
    let sorted = ascending(scales);
    sorted.iter().copied().find(|&s| s >= needed).unwrap_or(sorted[sorted.len() - 1])
}
