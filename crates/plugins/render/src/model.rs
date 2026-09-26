//! `model.ts`: the render ledger, per-page raster versions and the tile
//! wake-up counter.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::contract::InvalidateScope;

/// `RenderState`: per-page versions of the two raster products, and the
/// tile wake-up counter. Keys are page object numbers.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderState {
    /// Base-raster versions, bumped by content facts.
    pub content_epochs: BTreeMap<u32, u64>,
    /// Appearance versions, bumped by annotation facts.
    pub annotated_epochs: BTreeMap<u32, u64>,
    /// Tile paint-plan wake-ups; the value carries no meaning. The tile
    /// manager's state lives outside the store, so a re-plan bumps this to
    /// make subscribed layers read `getPlan` again.
    pub paint_versions: BTreeMap<u32, u64>,
}

/// `RenderAction`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum RenderAction {
    Invalidate { scope: InvalidateScope, pages: Vec<u32> },
    PaintAdvanced { page: u32 },
}

pub fn initial_render_state() -> RenderState {
    RenderState::default()
}

fn bump(epochs: &BTreeMap<u32, u64>, pons: &[u32]) -> BTreeMap<u32, u64> {
    let mut next = epochs.clone();
    for &pon in pons {
        *next.entry(pon).or_insert(0) += 1;
    }
    next
}

/// `reduceRender`. Pure. `None` is the TypeScript returning `state` (an
/// invalidation of no pages). A content fact bumps only the content ledger;
/// annotated readers sum both, which is how content invalidation reaches
/// them too.
pub fn reduce_render(state: &RenderState, action: &RenderAction) -> Option<RenderState> {
    match action {
        RenderAction::PaintAdvanced { page } => Some(RenderState {
            paint_versions: bump(&state.paint_versions, &[*page]),
            ..state.clone()
        }),
        RenderAction::Invalidate { pages, .. } if pages.is_empty() => None,
        RenderAction::Invalidate { scope: InvalidateScope::Content, pages } => Some(RenderState {
            content_epochs: bump(&state.content_epochs, pages),
            ..state.clone()
        }),
        RenderAction::Invalidate { scope: InvalidateScope::Annotations, pages } => {
            Some(RenderState {
                annotated_epochs: bump(&state.annotated_epochs, pages),
                ..state.clone()
            })
        }
    }
}
