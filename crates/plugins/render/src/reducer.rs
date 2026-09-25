//! `reducer.ts`: the per-page ledger.

use std::collections::BTreeMap;

use crate::types::{InvalidateScope, RenderAction, RenderState};

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

/// Pure. `None` is the TypeScript returning `state` (no change): an
/// invalidation of no pages. A content fact bumps only the content ledger;
/// annotated readers sum both.
pub fn render_reducer(state: &RenderState, action: &RenderAction) -> Option<RenderState> {
    match action {
        RenderAction::PaintAdvanced { pon } => Some(RenderState {
            paint_versions: bump(&state.paint_versions, &[*pon]),
            ..state.clone()
        }),
        RenderAction::Invalidate { pons, .. } if pons.is_empty() => None,
        RenderAction::Invalidate { scope: InvalidateScope::Content, pons } => {
            Some(RenderState { content_epochs: bump(&state.content_epochs, pons), ..state.clone() })
        }
        RenderAction::Invalidate { scope: InvalidateScope::Annotations, pons } => {
            Some(RenderState {
                annotated_epochs: bump(&state.annotated_epochs, pons),
                ..state.clone()
            })
        }
    }
}
