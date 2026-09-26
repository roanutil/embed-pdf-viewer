//! `model.ts`: pure state and transitions. `revision` bumps on every
//! confirmed value.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{DocumentMetadata, ResourceStatus};

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MetadataState {
    pub status: ResourceStatus,
    /// `null` until the first read lands, or while reading is forbidden.
    pub value: Option<DocumentMetadata>,
    pub revision: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum MetadataAction {
    Loading,
    Set { metadata: DocumentMetadata },
    Error { forbidden: bool },
}

/// `initialMetadataState()`.
pub fn initial_metadata_state() -> MetadataState {
    MetadataState { status: ResourceStatus::Idle, value: None, revision: 0 }
}

/// `reduceMetadata(state, action)`. `None` is `return state`.
pub fn reduce_metadata(state: &MetadataState, action: &MetadataAction) -> Option<MetadataState> {
    match action {
        MetadataAction::Loading => {
            if state.status == ResourceStatus::Loading {
                return None;
            }
            Some(MetadataState { status: ResourceStatus::Loading, ..state.clone() })
        }
        MetadataAction::Set { metadata } => Some(MetadataState {
            status: ResourceStatus::Ready,
            value: Some(metadata.clone()),
            revision: state.revision + 1,
        }),
        MetadataAction::Error { forbidden } => Some(MetadataState {
            status: if *forbidden { ResourceStatus::Forbidden } else { ResourceStatus::Error },
            ..state.clone()
        }),
    }
}

/// `changedKeys(previous, next)`: the keys of `next` whose values differ
/// from `previous` (every key when there is no previous). `custom` compares
/// by its JSON text, so reordered entries count as a change; every other key
/// by `!==`, under which two objects are never equal.
pub fn changed_keys(previous: Option<&DocumentMetadata>, next: &DocumentMetadata) -> Vec<String> {
    let Some(next) = next.as_object() else { return Vec::new() };
    let keys = next.keys().cloned();
    let Some(previous) = previous else { return keys.collect() };
    keys.filter(|key| {
        let a = previous.get(key.as_str());
        let b = next.get(key.as_str());
        if key == "custom" {
            return a.map(json_text) != b.map(json_text);
        }
        !strict_equals(a, b)
    })
    .collect()
}

/// `JSON.stringify(value)` for the JSON values metadata carries.
fn json_text(value: &Value) -> String {
    value.to_string()
}

/// JavaScript `===` over two members, `None` being `undefined`.
fn strict_equals(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(Value::Object(_) | Value::Array(_)), _)
        | (_, Some(Value::Object(_) | Value::Array(_))) => false,
        (Some(Value::Number(x)), Some(Value::Number(y))) => x.as_f64() == y.as_f64(),
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}
