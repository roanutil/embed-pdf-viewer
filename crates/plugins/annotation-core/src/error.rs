//! What the core throws: the engine's `EngineError` refusals the predictor
//! raises (`InvalidArg` with an optional `details`), and the plain `Error`
//! a session with too few object numbers raises. The harness prints each the
//! way `canonicalError` prints the TypeScript one.

use std::fmt;

use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub struct CoreError {
    /// The constructor's name: `EngineError`, `Error`, `TypeError`.
    pub name: &'static str,
    pub message: String,
    /// An `EngineError`'s code (`InvalidArg`).
    pub code: Option<&'static str>,
    /// An `EngineError`'s `details`.
    pub details: Option<Box<Value>>,
}

impl CoreError {
    /// `new EngineError(EngineErrorCode.InvalidArg, message)`.
    pub fn invalid(message: impl Into<String>) -> Self {
        CoreError {
            name: "EngineError",
            message: message.into(),
            code: Some("InvalidArg"),
            details: None,
        }
    }

    /// `new EngineError(InvalidArg, message, { details: { field } })`.
    pub fn invalid_field(message: impl Into<String>, field: &str) -> Self {
        let mut details = serde_json::Map::new();
        details.insert("field".into(), Value::String(field.into()));
        CoreError { details: Some(Box::new(Value::Object(details))), ..CoreError::invalid(message) }
    }

    /// `new Error(message)`.
    pub fn plain(message: impl Into<String>) -> Self {
        CoreError { name: "Error", message: message.into(), code: None, details: None }
    }
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.name, self.message)
    }
}

impl std::error::Error for CoreError {}

pub type CoreResult<T> = Result<T, CoreError>;
