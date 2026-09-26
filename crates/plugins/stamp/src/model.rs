//! `model.ts`: the stamp slice, serializable descriptors only. The bytes and
//! previews live in the binary sidecar and never enter the store.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::contract::{StampAsset, StampLibrary};

/// `StampState`. The TypeScript records are plain objects, which the vectors
/// compare unordered, so the maps sort.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StampState {
    pub libraries: BTreeMap<String, StampLibrary>,
    /// Library display order (insertion order).
    pub library_order: Vec<String>,
    pub assets: BTreeMap<String, StampAsset>,
}

/// `StampAction`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum StampAction {
    #[serde(rename = "LIBRARY_ADDED")]
    LibraryAdded { library: StampLibrary },
    #[serde(rename = "LIBRARY_UPDATED")]
    LibraryUpdated { library: StampLibrary },
    #[serde(rename = "LIBRARY_REMOVED", rename_all = "camelCase")]
    LibraryRemoved { library_id: String },
    #[serde(rename = "ASSET_ADDED")]
    AssetAdded { asset: StampAsset },
    #[serde(rename = "ASSET_UPDATED")]
    AssetUpdated { asset: StampAsset },
    #[serde(rename = "ASSET_REMOVED", rename_all = "camelCase")]
    AssetRemoved { asset_id: String },
}

/// `initialStampState()`.
pub fn initial_stamp_state() -> StampState {
    StampState::default()
}

/// `stampReducer(state, action)`. `None` is the TypeScript's `return state`.
pub fn stamp_reducer(state: &StampState, action: &StampAction) -> Option<StampState> {
    let mut next = state.clone();
    match action {
        StampAction::LibraryAdded { library } => {
            next.libraries.insert(library.id.clone(), library.clone());
            next.library_order.push(library.id.clone());
        }
        StampAction::LibraryUpdated { library } => {
            if !state.libraries.contains_key(&library.id) {
                return None;
            }
            next.libraries.insert(library.id.clone(), library.clone());
        }
        StampAction::LibraryRemoved { library_id } => {
            let library = state.libraries.get(library_id)?;
            next.libraries.remove(library_id);
            for id in &library.asset_ids {
                next.assets.remove(id);
            }
            next.library_order.retain(|id| id != library_id);
        }
        StampAction::AssetAdded { asset } => {
            let library = state.libraries.get(&asset.library_id)?;
            next.assets.insert(asset.id.clone(), asset.clone());
            let mut library = library.clone();
            library.asset_ids.push(asset.id.clone());
            next.libraries.insert(library.id.clone(), library);
        }
        StampAction::AssetUpdated { asset } => {
            if !state.assets.contains_key(&asset.id) {
                return None;
            }
            next.assets.insert(asset.id.clone(), asset.clone());
        }
        StampAction::AssetRemoved { asset_id } => {
            let asset = state.assets.get(asset_id)?;
            next.assets.remove(asset_id);
            if let Some(library) = state.libraries.get(&asset.library_id) {
                let mut library = library.clone();
                library.asset_ids.retain(|id| id != asset_id);
                next.libraries.insert(library.id.clone(), library);
            }
        }
    }
    Some(next)
}
