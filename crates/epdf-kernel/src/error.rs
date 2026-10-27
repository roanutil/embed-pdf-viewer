//! The one error shape, as JavaScript would see it: a `name`, a `message`,
//! and whatever own members the thrower attached (`code` and `details` on an
//! `EngineError`, `required` and `context` on a `PermissionDenied`, `reason`
//! on an `AbortError`).
//!
//! The kernel and the plugins throw plain `Error`s whose messages are part of
//! their behaviour (a UI shows them, a caller matches on them), so the
//! message is carried verbatim and compared verbatim.

use std::fmt;

use serde_json::{Map, Value, json};

/// The class an error was constructed from, where the TypeScript tells
/// errors apart with `instanceof` rather than by name. A table or a vector
/// can only make plain errors: one named `PermissionDenied` that did not
/// come from that class is, to `toPluginError`, a plain failure.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ErrorClass {
    #[default]
    Plain,
    /// The kernel's `CancelledError`.
    Cancelled,
    /// `engine-core`'s `PermissionDenied`.
    PermissionDenied,
    /// The kernel's `PluginError`.
    Plugin,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Error {
    pub name: String,
    pub message: String,
    /// Own enumerable members beyond `name` and `message`, in insertion order.
    /// Boxed so an `Error` stays small enough to return by value.
    pub members: Box<Map<String, Value>>,
    pub class: ErrorClass,
}

impl Error {
    /// `new Error(message)`.
    pub fn new(message: impl Into<String>) -> Self {
        Error::named("Error", message)
    }

    pub fn named(name: impl Into<String>, message: impl Into<String>) -> Self {
        Error {
            name: name.into(),
            message: message.into(),
            members: Box::default(),
            class: ErrorClass::Plain,
        }
    }

    pub fn of_class(mut self, class: ErrorClass) -> Self {
        self.class = class;
        self
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
        let mut details = json!({ "required": required });
        if let Some(context) = context {
            details["context"] = json!(context);
        }
        let mut error = Error::named("PermissionDenied", message)
            .with("code", "Forbidden")
            .with("details", details)
            .with("required", required)
            .of_class(ErrorClass::PermissionDenied);
        if let Some(context) = context {
            error = error.with("context", context);
        }
        error
    }

    /// `controller.abort()` with no reason: the platform's `DOMException`,
    /// named `AbortError`, with Node's and the browsers' message.
    pub fn dom_abort() -> Self {
        Error::named("AbortError", "This operation was aborted")
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
        for (key, value) in self.members.iter() {
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

// ── the plugin error vocabulary (`errors.ts`) ─────────────────────────────────

/// `PluginErrorCode`.
pub mod code {
    pub const PERMISSION_DENIED: &str = "permission-denied";
    pub const UNSUPPORTED: &str = "unsupported";
    pub const NOT_FOUND: &str = "not-found";
    pub const NOT_READY: &str = "not-ready";
    pub const INVALID_INPUT: &str = "invalid-input";
    pub const CONFLICT: &str = "conflict";
    pub const INSTANCE_CLOSED: &str = "instance-closed";
    pub const OPERATION_CANCELLED: &str = "operation-cancelled";
    pub const OPERATION_FAILED: &str = "operation-failed";

    /// Every `PluginErrorCode`.
    pub const ALL: [&str; 9] = [
        PERMISSION_DENIED,
        UNSUPPORTED,
        NOT_FOUND,
        NOT_READY,
        INVALID_INPUT,
        CONFLICT,
        INSTANCE_CLOSED,
        OPERATION_CANCELLED,
        OPERATION_FAILED,
    ];
}

/// `new PluginError(code, capability, message, {cause, details})`. The own
/// members are `code`, `capability`, then `details` and `cause` when given,
/// as the class's fields and constructor leave them.
pub fn plugin_error(
    code: &str,
    capability: &str,
    message: impl Into<String>,
    cause: Option<Value>,
    details: Option<Value>,
) -> Error {
    let mut error = Error::named("PluginError", message)
        .with("code", code)
        .with("capability", capability)
        .with(
            "permission",
            if code == code::PERMISSION_DENIED {
                details
                    .as_ref()
                    .and_then(|d| d.get("required"))
                    .filter(|v| v.is_string())
                    .cloned()
                    .unwrap_or(Value::Null)
            } else {
                Value::Null
            },
        )
        .of_class(ErrorClass::Plugin);
    if let Some(details) = details.filter(|d| !crate::is_undefined(d)) {
        error = error.with("details", details);
    }
    if let Some(cause) = cause {
        error = error.with("cause", cause);
    }
    error
}

impl Error {
    /// `isPluginError(value, code?)`.
    pub fn is_plugin_error(&self, code: Option<&str>) -> bool {
        self.class == ErrorClass::Plugin && code.is_none_or(|c| self.plugin_code() == Some(c))
    }

    /// A `PluginError`'s code.
    pub fn plugin_code(&self) -> Option<&str> {
        if self.class != ErrorClass::Plugin {
            return None;
        }
        self.members.get("code").and_then(Value::as_str)
    }

    /// `toPluginErrorInfo(error)` (`errors.ts:58-65`): `{code, message,
    /// capability, permission}`, a member the error lacks as `null`. The one
    /// converter: [`to_plugin_error_info`] reads this into the typed struct.
    pub fn plugin_error_info(&self) -> Value {
        serde_json::json!({
            "code": self.members.get("code").cloned().unwrap_or(Value::Null),
            "message": self.message,
            "capability": self.members.get("capability").cloned().unwrap_or(Value::Null),
            "permission": self.members.get("permission").cloned().unwrap_or(Value::Null),
        })
    }
}

/// [`Error::plugin_error_info`] as the typed `PluginErrorInfo` a contract
/// carries. A `PluginError` (`to_plugin_error`'s answer) always has `code`
/// and `capability`; on another error the struct's strings read a missing
/// one as empty, where the JSON says `null`.
pub fn to_plugin_error_info(error: &Error) -> crate::types::PluginErrorInfo {
    let info = error.plugin_error_info();
    let text = |key: &str| info[key].as_str().unwrap_or_default().to_string();
    crate::types::PluginErrorInfo {
        code: text("code"),
        message: text("message"),
        capability: text("capability"),
        permission: info["permission"].as_str().map(str::to_string),
    }
}

/// The engine codes `toPluginError` maps; anything else is `operation-failed`.
fn engine_code(code: &str) -> Option<&'static str> {
    Some(match code {
        "InvalidArg" | "MalformedPdf" | "PayloadTooLarge" => code::INVALID_INPUT,
        "DocNotOpen" => code::NOT_READY,
        "DocPasswordRequired"
        | "DocPasswordIncorrect"
        | "SharePasswordRequired"
        | "Unauthenticated"
        | "Forbidden"
        | "ProtectedDocument" => code::PERMISSION_DENIED,
        "NotFound" => code::NOT_FOUND,
        "NotImplemented" => code::UNSUPPORTED,
        "IdempotencyKeyReused" => code::INVALID_INPUT,
        "Aborted" => code::OPERATION_CANCELLED,
        "ObjectNumberUnavailable"
        | "LayerFull"
        | "ChangeConflict"
        | "UndoUnavailable"
        | "LayerVersionConflict"
        | "StaleBase"
        | "SigningPending"
        | "SigningExpired"
        | "SigningVersionMismatch" => code::CONFLICT,
        _ => return None,
    })
}

/// The `PluginErrorCode` of `error` as a plugin-level caller sees it: the
/// code `to_plugin_error` gives it, without building the error. A code that
/// already is one is kept, and an engine code on any error maps as an
/// `EngineError`'s does, so a host that raised `NotFound` itself still
/// answers `not-found`.
pub fn plugin_code_of(error: &Error) -> &'static str {
    let own = error.code();
    if let Some(known) = own.and_then(|c| code::ALL.into_iter().find(|k| *k == c)) {
        return known;
    }
    match error.class {
        ErrorClass::PermissionDenied => return code::PERMISSION_DENIED,
        ErrorClass::Cancelled => return code::OPERATION_CANCELLED,
        ErrorClass::Plugin | ErrorClass::Plain => {}
    }
    if error.name == "AbortError" {
        return code::OPERATION_CANCELLED;
    }
    own.and_then(engine_code).unwrap_or(code::OPERATION_FAILED)
}

/// `toPluginError(capability, error)`: the one boundary mapping from what an
/// engine call threw to a `PluginError`. Idempotent.
pub fn to_plugin_error(capability: &str, error: &Error) -> Error {
    match error.class {
        ErrorClass::Plugin => return error.clone(),
        ErrorClass::PermissionDenied => {
            // TS passes `error.details` through whole (`anyOf`, `refs`
            // included), and `permission` reads `details.required`.
            let details = error.members.get("details").cloned();
            return plugin_error(
                code::PERMISSION_DENIED,
                capability,
                error.message.clone(),
                Some(error.to_canonical()),
                details,
            );
        }
        ErrorClass::Cancelled => {
            return plugin_error(
                code::OPERATION_CANCELLED,
                capability,
                "operation cancelled",
                Some(error.to_canonical()),
                None,
            );
        }
        ErrorClass::Plain => {}
    }
    if error.name == "AbortError" {
        return plugin_error(
            code::OPERATION_CANCELLED,
            capability,
            "operation cancelled",
            Some(error.to_canonical()),
            None,
        );
    }
    if error.name == "EngineError" {
        let mapped = error.code().and_then(engine_code).unwrap_or(code::OPERATION_FAILED);
        return plugin_error(
            mapped,
            capability,
            error.message.clone(),
            Some(error.to_canonical()),
            error.members.get("details").cloned(),
        );
    }
    plugin_error(
        code::OPERATION_FAILED,
        capability,
        error.message.clone(),
        Some(error.to_canonical()),
        None,
    )
}

/// `toPluginError` of something thrown that is not an `Error`: its message
/// is `String(value)` and the value itself is the cause.
pub fn to_plugin_error_of_value(capability: &str, message: &str, value: Value) -> Error {
    plugin_error(code::OPERATION_FAILED, capability, message, Some(value), None)
}
