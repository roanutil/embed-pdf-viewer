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
        // Plain `serde_json`, not `to_json`: only `type` is read, and a
        // member `serde_json` refuses (a map with non-string keys) must not
        // panic here. An action without a string `type` has the type "".
        match serde_json::to_value(self) {
            Ok(Value::Object(mut members)) => match members.remove("type") {
                Some(Value::String(t)) => t,
                _ => String::new(),
            },
            _ => String::new(),
        }
    }
    fn to_json(&self) -> Value {
        // NaN and the infinities as canonical JSON spells them, not `null`.
        crate::js::to_value(self)
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
