//! `model.ts`: the stamp slice, serializable descriptors only. The bytes and
//! previews live in the binary sidecar and never enter the store.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::contract::{StampAsset, StampLibrary};

/// `StampState`. The TypeScript records are plain objects, so the maps keep
/// their key order: insertion order, integer-like keys first in numeric
/// order ([`sort_js_keys`]).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StampState {
    pub libraries: IndexMap<String, StampLibrary>,
    /// Library display order (insertion order).
    pub library_order: Vec<String>,
    pub assets: IndexMap<String, StampAsset>,
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
            sort_js_keys(&mut next.libraries);
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
            next.libraries.shift_remove(library_id);
            for id in &library.asset_ids {
                next.assets.shift_remove(id);
            }
            next.library_order.retain(|id| id != library_id);
        }
        StampAction::AssetAdded { asset } => {
            let library = state.libraries.get(&asset.library_id)?;
            next.assets.insert(asset.id.clone(), asset.clone());
            sort_js_keys(&mut next.assets);
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
            next.assets.shift_remove(asset_id);
            if let Some(library) = state.libraries.get(&asset.library_id) {
                let mut library = library.clone();
                library.asset_ids.retain(|id| id != asset_id);
                next.libraries.insert(library.id.clone(), library);
            }
        }
    }
    Some(next)
}

/// Puts a map's keys in the order `Object.keys` gives a plain object's:
/// array indices (canonical numeric strings below 2^32 - 1) first in
/// numeric order, then the rest in insertion order.
fn sort_js_keys<V>(object: &mut IndexMap<String, V>) {
    fn rank(key: &str) -> (u8, u32) {
        let canonical = key == "0" || (!key.starts_with('0') && !key.is_empty());
        let index = key
            .bytes()
            .all(|b| b.is_ascii_digit())
            .then(|| key.parse::<u64>().ok())
            .flatten()
            .filter(|&n| canonical && n < u64::from(u32::MAX));
        index.map_or((1, 0), |n| (0, n as u32))
    }
    object.sort_by(|a, _, b, _| rank(a).cmp(&rank(b)));
}
