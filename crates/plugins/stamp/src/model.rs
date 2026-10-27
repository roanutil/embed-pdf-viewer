//! `model.ts`: the stamp plugin's session state. Libraries, their display
//! order and assets, as serializable descriptors, and the asset armed on
//! each document. The bytes and previews they describe live in the binaries
//! resource (`services/binaries.rs`) and never enter state. Every function
//! below is a pure transition; the areas apply them with `ctx.state.update`
//! (the kernel's `update_state`). A transition that changes nothing hands
//! back the same `Rc`, as the TypeScript hands back the same object.

use std::rc::Rc;

use indexmap::IndexMap;
use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::contract::{Members, StampAsset, StampLibrary};

/// A `Record<string, T>`: a plain object's members, in the order
/// `Object.keys` gives them.
pub type Record<T> = IndexMap<String, T>;

/// `StampState`.
#[derive(Clone, Debug, PartialEq)]
pub struct StampState {
    pub libraries: Record<StampLibrary>,
    /// Library display order (insertion order).
    pub library_order: Vec<String>,
    pub assets: Record<StampAsset>,
    /// Document id to the id of the asset armed there.
    pub armed: Record<String>,
    /// The member order of the state object itself, which every spread
    /// keeps.
    pub members: Members,
}

const STATE_MEMBERS: [&str; 4] = ["libraries", "libraryOrder", "assets", "armed"];

impl Default for StampState {
    fn default() -> Self {
        initial_stamp_state()
    }
}

impl Serialize for StampState {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut out = serializer.serialize_map(Some(4))?;
        for key in self.members.keys() {
            match *key {
                "libraries" => out.serialize_entry(key, &self.libraries)?,
                "libraryOrder" => out.serialize_entry(key, &self.library_order)?,
                "assets" => out.serialize_entry(key, &self.assets)?,
                "armed" => out.serialize_entry(key, &self.armed)?,
                _ => {}
            }
        }
        out.end()
    }
}

impl<'de> Deserialize<'de> for StampState {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let object = match Value::deserialize(deserializer)? {
            Value::Object(map) => map,
            other => return Err(D::Error::custom(format!("expected a state object, got {other}"))),
        };
        let read = |key: &str| object.get(key).cloned().unwrap_or(Value::Object(Map::new()));
        let mut members = Members::read(&object, &STATE_MEMBERS);
        for key in STATE_MEMBERS {
            members.ensure(key);
        }
        Ok(StampState {
            libraries: serde_json::from_value(read("libraries")).map_err(D::Error::custom)?,
            library_order: serde_json::from_value(
                object.get("libraryOrder").cloned().unwrap_or(Value::Array(Vec::new())),
            )
            .map_err(D::Error::custom)?,
            assets: serde_json::from_value(read("assets")).map_err(D::Error::custom)?,
            armed: serde_json::from_value(read("armed")).map_err(D::Error::custom)?,
            members,
        })
    }
}

/// Whether `key` is an array index, which a plain object lists first, in
/// numeric order: a canonical decimal string below 2^32 - 1.
fn array_index(key: &str) -> Option<u32> {
    if key.is_empty() || !key.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if key.len() > 1 && key.starts_with('0') {
        return None;
    }
    key.parse::<u64>().ok().filter(|n| *n < u64::from(u32::MAX)).map(|n| n as u32)
}

/// `object[key] = value` on a plain object: an existing member keeps its
/// place, a new array index goes among the indices in numeric order, any
/// other new member goes last.
pub fn assign<T>(object: &mut Record<T>, key: &str, value: T) {
    if let Some(slot) = object.get_mut(key) {
        *slot = value;
        return;
    }
    let Some(index) = array_index(key) else {
        object.insert(key.to_string(), value);
        return;
    };
    let at = object
        .keys()
        .position(|k| array_index(k).is_none_or(|other| other > index))
        .unwrap_or(object.len());
    object.shift_insert(at, key.to_string(), value);
}

/// `initialStampState()`.
pub fn initial_stamp_state() -> StampState {
    StampState {
        libraries: Record::new(),
        library_order: Vec::new(),
        assets: Record::new(),
        armed: Record::new(),
        members: Members::of(&STATE_MEMBERS),
    }
}

/// `armAsset(state, documentId, assetId)`: remember the asset armed on a
/// document.
pub fn arm_asset(state: &Rc<StampState>, document_id: &str, asset_id: &str) -> Rc<StampState> {
    if state.armed.get(document_id).is_some_and(|armed| armed == asset_id) {
        return state.clone();
    }
    let mut next = (**state).clone();
    assign(&mut next.armed, document_id, asset_id.to_string());
    Rc::new(next)
}

/// `disarmDocuments(state, documentIds)`: forget what was armed on these
/// documents.
pub fn disarm_documents(state: &Rc<StampState>, document_ids: &[String]) -> Rc<StampState> {
    if !document_ids.iter().any(|id| state.armed.contains_key(id)) {
        return state.clone();
    }
    let mut next = (**state).clone();
    for id in document_ids {
        next.armed.shift_remove(id);
    }
    Rc::new(next)
}

/// `addLibrary(state, library)`: append a library to the display order.
pub fn add_library(state: &Rc<StampState>, library: &StampLibrary) -> Rc<StampState> {
    let mut next = (**state).clone();
    assign(&mut next.libraries, &library.id, library.clone());
    next.library_order.push(library.id.clone());
    Rc::new(next)
}

/// `setLibrary(state, library)`: replace a known library's descriptor; an
/// unknown id changes nothing.
pub fn set_library(state: &Rc<StampState>, library: &StampLibrary) -> Rc<StampState> {
    if !state.libraries.contains_key(&library.id) {
        return state.clone();
    }
    let mut next = (**state).clone();
    assign(&mut next.libraries, &library.id, library.clone());
    Rc::new(next)
}

/// `removeLibrary(state, libraryId)`: remove a library together with its
/// assets.
pub fn remove_library(state: &Rc<StampState>, library_id: &str) -> Rc<StampState> {
    let Some(library) = state.libraries.get(library_id) else { return state.clone() };
    let mut next = (**state).clone();
    next.libraries.shift_remove(library_id);
    for id in &library.asset_ids {
        next.assets.shift_remove(id);
    }
    next.library_order.retain(|id| id != library_id);
    Rc::new(next)
}

/// `addAsset(state, asset)`: add an asset to the end of its library; an
/// asset of an unknown library changes nothing.
pub fn add_asset(state: &Rc<StampState>, asset: &StampAsset) -> Rc<StampState> {
    let Some(library) = state.libraries.get(&asset.library_id) else { return state.clone() };
    let mut library = library.clone();
    library.asset_ids.push(asset.id.clone());
    let mut next = (**state).clone();
    assign(&mut next.assets, &asset.id, asset.clone());
    assign(&mut next.libraries, &library.id.clone(), library);
    Rc::new(next)
}

/// `setAsset(state, asset)`: replace a known asset's descriptor; an unknown
/// id changes nothing.
pub fn set_asset(state: &Rc<StampState>, asset: &StampAsset) -> Rc<StampState> {
    if !state.assets.contains_key(&asset.id) {
        return state.clone();
    }
    let mut next = (**state).clone();
    assign(&mut next.assets, &asset.id, asset.clone());
    Rc::new(next)
}

/// `removeAsset(state, assetId)`: remove an asset and its entry in its
/// library's order.
pub fn remove_asset(state: &Rc<StampState>, asset_id: &str) -> Rc<StampState> {
    let Some(asset) = state.assets.get(asset_id) else { return state.clone() };
    let library = state.libraries.get(&asset.library_id).cloned();
    let mut next = (**state).clone();
    next.assets.shift_remove(asset_id);
    if let Some(mut library) = library {
        library.asset_ids.retain(|id| id != asset_id);
        assign(&mut next.libraries, &library.id.clone(), library);
    }
    Rc::new(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn array_index_members_list_first_in_numeric_order() {
        let mut object: Record<u8> = Record::new();
        for key in ["b", "10", "a", "2", "02", "4294967295", "0"] {
            assign(&mut object, key, 0);
        }
        let keys: Vec<&str> = object.keys().map(String::as_str).collect();
        assert_eq!(keys, ["0", "2", "10", "b", "a", "02", "4294967295"]);
    }

    #[test]
    fn an_existing_member_keeps_its_place() {
        let mut object: Record<u8> = Record::new();
        assign(&mut object, "x", 1);
        assign(&mut object, "y", 2);
        assign(&mut object, "x", 3);
        assert_eq!(
            object.iter().map(|(k, v)| (k.as_str(), *v)).collect::<Vec<_>>(),
            [("x", 3), ("y", 2)]
        );
    }
}
