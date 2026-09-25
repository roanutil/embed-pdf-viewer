//! The one error shape, as JavaScript would see it: a `name`, a `message`,
//! and whatever own members the thrower attached (`code` and `details` on an
//! `EngineError`, `required` and `context` on a `PermissionDenied`, `reason`
//! on an `AbortError`).
//!
//! The kernel and the plugins throw plain `Error`s whose messages are part of
//! their behaviour (a UI shows them, a caller matches on them), so the
//! message is carried verbatim and compared verbatim.

use std::fmt;

use serde_json::{Map, Value};

#[derive(Clone, Debug, PartialEq)]
pub struct Error {
    pub name: String,
    pub message: String,
    /// Own enumerable members beyond `name` and `message`, in insertion order.
    pub members: Map<String, Value>,
}

impl Error {
    /// `new Error(message)`.
    pub fn new(message: impl Into<String>) -> Self {
        Error::named("Error", message)
    }

    pub fn named(name: impl Into<String>, message: impl Into<String>) -> Self {
        Error { name: name.into(), message: message.into(), members: Map::new() }
    }

    /// Attach an own member, as `Object.assign(error, {key: value})` would.
    pub fn with(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.members.insert(key.into(), value.into());
        self
    }

    pub fn member(&self, key: &str) -> Option<&Value> {
        self.members.get(key)
    }

    /// `EngineError(code, message)`.
    pub fn engine(code: impl Into<String>, message: impl Into<String>) -> Self {
        Error::named("EngineError", message).with("code", code.into())
    }

    /// The code an `EngineError` carries.
    pub fn code(&self) -> Option<&str> {
        self.members.get("code").and_then(Value::as_str)
    }

    /// `PermissionDenied(required, context?)`: the refusal shape of the
    /// permissions convention, identical to the engine's.
    pub fn permission_denied(required: impl Into<String>, context: Option<&str>) -> Self {
        let required = required.into();
        let message = match context {
            Some(context) => format!("permission denied ({context}): {required}"),
            None => format!("permission denied: {required}"),
        };
        let mut error = Error::named("PermissionDenied", message).with("required", required);
        if let Some(context) = context {
            error = error.with("context", context);
        }
        error
    }

    /// `new AbortError(reason)`: the message rules of `AbortError.ts`.
    pub fn abort(reason: Option<Value>) -> Self {
        let message = match &reason {
            None | Some(Value::Null) => "aborted".to_string(),
            Some(Value::String(s)) => s.clone(),
            Some(Value::Object(o)) if o.contains_key("$error") => o["$error"]
                .get("message")
                .and_then(Value::as_str)
                .filter(|m| !m.is_empty())
                .unwrap_or("aborted")
                .to_string(),
            Some(other) => other.to_string(),
        };
        let mut error = Error::named("AbortError", message);
        if let Some(reason) = reason {
            error = error.with("reason", reason);
        }
        error
    }

    pub fn is_abort(&self) -> bool {
        self.name == "AbortError"
    }

    /// The canonical JSON form (`{"$error": {...}}`).
    pub fn to_canonical(&self) -> Value {
        let mut out = Map::new();
        out.insert("name".into(), Value::String(self.name.clone()));
        out.insert("message".into(), Value::String(self.message.clone()));
        for (key, value) in &self.members {
            out.insert(key.clone(), value.clone());
        }
        let mut wrapper = Map::new();
        wrapper.insert("$error".into(), Value::Object(out));
        Value::Object(wrapper)
    }

    /// Read an error back out of its canonical form, or out of a bare
    /// `{name, message, ...}` spec as a vector writes one.
    pub fn from_spec(spec: &Value) -> Self {
        let spec = spec.get("$error").unwrap_or(spec);
        let mut error = Error::named(
            spec.get("name").and_then(Value::as_str).unwrap_or("Error"),
            spec.get("message").and_then(Value::as_str).unwrap_or(""),
        );
        if let Some(object) = spec.as_object() {
            for (key, value) in object {
                if key != "name" && key != "message" {
                    error.members.insert(key.clone(), value.clone());
                }
            }
        }
        error
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.name, self.message)
    }
}

impl std::error::Error for Error {}
