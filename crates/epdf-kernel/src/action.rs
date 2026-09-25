//! Actions: plain, serializable, with a `type`.
//!
//! Every serde type is an action, and its type is the `type` member of its
//! JSON form. A plugin's action enum is `#[serde(tag = "type")]`, which is
//! the shape the TypeScript union already has, so the JSON a trace records
//! is the same on both sides.

use std::any::Any;

use serde::Serialize;
use serde_json::Value;

pub trait Action: Any {
    /// The action's `type`.
    fn action_type(&self) -> String;
    /// The action as the TypeScript would print it.
    fn to_json(&self) -> Value;
    fn as_any(&self) -> &dyn Any;
}

impl<T: Serialize + 'static> Action for T {
    fn action_type(&self) -> String {
        match self.to_json().get("type") {
            Some(Value::String(t)) => t.clone(),
            _ => String::new(),
        }
    }
    fn to_json(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl dyn Action {
    /// The action as `T`, when that is what it is.
    pub fn downcast<T: 'static>(&self) -> Option<&T> {
        self.as_any().downcast_ref::<T>()
    }
}

/// An action with only a type: what the kernel's lifecycle emits.
#[derive(Clone, Debug, Serialize)]
pub struct CoreAction {
    #[serde(rename = "type")]
    pub kind: &'static str,
}
