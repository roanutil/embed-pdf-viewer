//! `reducer.ts`: pure and serializable. Descriptors only; the bytes and
//! previews live in the capability. `None` is the TypeScript's
//! `return state`.

use crate::types::{StampAction, StampState};

/// `initialStampState()`.
pub fn initial_stamp_state() -> StampState {
    StampState::default()
}

/// `stampReducer(state, action)`.
pub fn stamp_reducer(state: &StampState, action: &StampAction) -> Option<StampState> {
    match action {
        StampAction::LibraryAdded { library } => {
            let mut next = state.clone();
            next.libraries.insert(library.id.clone(), library.clone());
            next.library_order.push(library.id.clone());
            Some(next)
        }
        StampAction::LibraryRemoved { library_id } => {
            let library = state.libraries.get(library_id)?;
            let mut next = state.clone();
            next.libraries.remove(library_id);
            for id in &library.asset_ids {
                next.assets.remove(id);
            }
            next.library_order.retain(|id| id != library_id);
            Some(next)
        }
        StampAction::AssetAdded { asset } => {
            let library = state.libraries.get(&asset.library_id)?;
            let mut next = state.clone();
            next.assets.insert(asset.id.clone(), asset.clone());
            let mut library = library.clone();
            library.asset_ids.push(asset.id.clone());
            next.libraries.insert(library.id.clone(), library);
            Some(next)
        }
        StampAction::AssetRemoved { asset_id } => {
            let asset = state.assets.get(asset_id)?;
            let mut next = state.clone();
            next.assets.remove(asset_id);
            if let Some(library) = state.libraries.get(&asset.library_id) {
                let mut library = library.clone();
                library.asset_ids.retain(|id| id != asset_id);
                next.libraries.insert(library.id.clone(), library);
            }
            Some(next)
        }
    }
}
