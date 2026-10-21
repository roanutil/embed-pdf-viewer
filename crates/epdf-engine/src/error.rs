//! G1. Engine errors.
//!
//! Replaces `core/src/errors/{EngineError,EngineErrorCode}.ts`. The code values
//! are byte-identical to `EngineErrorCode.ts` and the serialized envelope is
//! byte-identical to `serializeError`'s output, because both cross the wire to
//! TypeScript callers that match on them.

use serde::{Deserialize, Serialize};

/// The one error vocabulary every platform speaks.
///
/// Ported from `packages/engine/core/src/errors/EngineErrorCode.ts`. The
/// variant names and their serialized strings are the same 19 values in the
/// same spelling; a rename here is a wire break.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EngineErrorCode {
    Unknown,
    InvalidArg,
    DocNotOpen,
    DocOpenFailed,
    DocPasswordRequired,
    DocPasswordIncorrect,
    /// A share-token exchange needs the grant's passphrase: none was supplied,
    /// or the supplied one was rejected, indistinguishable by design because
    /// the server does not distinguish them either. Distinct from
    /// `DocPasswordRequired`, which is about the PDF's own encryption.
    SharePasswordRequired,
    Aborted,
    Network,
    Unauthenticated,
    Forbidden,
    NotFound,
    WireFormat,
    RuntimeUnavailable,
    /// An annotation or page reference did not resolve: no annotation with
    /// that object number or `/NM` on the addressed page, a `RevisionToken`
    /// that does not match the page's current generation, or an index past the
    /// page's annotation count.
    InvalidReference,
    /// A weak-annotation structural edit was attempted without sole active
    /// edit presence for the page. A collaboration guard for index-addressed
    /// annotations only.
    WeakAnnotationSessionConflict,
    /// A layer write lost its optimistic-concurrency check and the server
    /// exhausted its rebase-and-retry budget. Retryable: the operation was NOT
    /// applied.
    LayerVersionConflict,
    /// Typed but not yet wired in this engine version. The wire shape stays
    /// valid, so clients can detect it and degrade.
    NotImplemented,
    /// PDFium parsed the file, but it violates a structural invariant the
    /// engine relies on: a direct (non-indirect) page dictionary in the
    /// `/Pages` tree, which `EPDFPage_GetObjectNumber` reports as `0`. Engine
    /// creation paths always produce indirect objects, so this never describes
    /// engine-produced state. Distinct from `DocOpenFailed` (PDFium refused
    /// the bytes) and `NotFound` (a well-formed thing that does not exist).
    MalformedPdf,
    /// A signing candidate is parked on the session: every mutation is
    /// refused until it completes or is aborted.
    SigningPending,
    /// The signing candidate expired before it completed.
    SigningExpired,
    /// `complete` named a version the candidate was not built on.
    SigningVersionMismatch,
    /// The engine refused to author the signature (a signed field, a seed
    /// value it cannot satisfy, a second certification).
    SignatureRefused,
    /// A document's signatures forbid the change.
    ProtectedDocument,
    /// The base the caller named is not the one the session is on.
    StaleBase,
    /// The document would pass the highest object number a file should have
    /// (`OBJECT_NUMBER_CEILING`), or a session asked for numbers past
    /// `OBJECT_NUMBER_ISSUE_LIMIT`. `details.lastObjectNumber` is the
    /// layer's last number.
    LayerFull,
    /// An op of a change found the document not holding what its `expect`
    /// said it would: `details` names the op (`opIndex`) and the `fields`.
    /// The whole change is refused.
    ChangeConflict,
    /// `{ undoOf }` named a change that can no longer be undone:
    /// `details.reason` is `final-change` or `expired`.
    UndoUnavailable,
    /// An `opId` that already answered one change was sent with another.
    /// Upstream's other new codes (`PayloadTooLarge`,
    /// `ObjectNumberUnavailable`) come with Task 8.27.
    IdempotencyKeyReused,
}

impl EngineErrorCode {
    /// Every code, in `EngineErrorCode.ts` declaration order.
    pub const ALL: [EngineErrorCode; 29] = [
        Self::Unknown,
        Self::InvalidArg,
        Self::DocNotOpen,
        Self::DocOpenFailed,
        Self::DocPasswordRequired,
        Self::DocPasswordIncorrect,
        Self::SharePasswordRequired,
        Self::Aborted,
        Self::Network,
        Self::Unauthenticated,
        Self::Forbidden,
        Self::NotFound,
        Self::WireFormat,
        Self::RuntimeUnavailable,
        Self::InvalidReference,
        Self::WeakAnnotationSessionConflict,
        Self::LayerVersionConflict,
        Self::NotImplemented,
        Self::MalformedPdf,
        Self::SigningPending,
        Self::SigningExpired,
        Self::SigningVersionMismatch,
        Self::SignatureRefused,
        Self::ProtectedDocument,
        Self::StaleBase,
        Self::LayerFull,
        Self::ChangeConflict,
        Self::UndoUnavailable,
        Self::IdempotencyKeyReused,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "Unknown",
            Self::InvalidArg => "InvalidArg",
            Self::DocNotOpen => "DocNotOpen",
            Self::DocOpenFailed => "DocOpenFailed",
            Self::DocPasswordRequired => "DocPasswordRequired",
            Self::DocPasswordIncorrect => "DocPasswordIncorrect",
            Self::SharePasswordRequired => "SharePasswordRequired",
            Self::Aborted => "Aborted",
            Self::Network => "Network",
            Self::Unauthenticated => "Unauthenticated",
            Self::Forbidden => "Forbidden",
            Self::NotFound => "NotFound",
            Self::WireFormat => "WireFormat",
            Self::RuntimeUnavailable => "RuntimeUnavailable",
            Self::InvalidReference => "InvalidReference",
            Self::WeakAnnotationSessionConflict => "WeakAnnotationSessionConflict",
            Self::LayerVersionConflict => "LayerVersionConflict",
            Self::NotImplemented => "NotImplemented",
            Self::MalformedPdf => "MalformedPdf",
            Self::SigningPending => "SigningPending",
            Self::SigningExpired => "SigningExpired",
            Self::SigningVersionMismatch => "SigningVersionMismatch",
            Self::SignatureRefused => "SignatureRefused",
            Self::ProtectedDocument => "ProtectedDocument",
            Self::StaleBase => "StaleBase",
            Self::LayerFull => "LayerFull",
            Self::ChangeConflict => "ChangeConflict",
            Self::UndoUnavailable => "UndoUnavailable",
            Self::IdempotencyKeyReused => "IdempotencyKeyReused",
        }
    }
}

impl std::fmt::Display for EngineErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The single error type crossing the engine boundary.
///
/// `details` is `serde_json::Value` for parity with TypeScript's
/// `details?: Record<string, unknown>`. A typed detail enum per code is the
/// later shape; matching that now would change the wire before anything reads
/// it.
///
/// Serializes as [`EngineError::to_wire`], `{name: "EngineError", code,
/// message, details?}`, which is what `serializeError` puts in a TypeScript
/// `reject` envelope. Deserializing ignores `name`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct EngineError {
    pub code: EngineErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl EngineError {
    /// A code with no message uses the code's own name, exactly as
    /// `new EngineError(code)` does (`EngineError.ts:20`, `message ?? code`).
    pub fn new(code: EngineErrorCode) -> Self {
        Self { code, message: code.as_str().to_string(), details: None }
    }

    pub fn with_message(code: EngineErrorCode, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), details: None }
    }

    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }

    /// `PermissionDenied` (`core/src/auth/scope/errors.ts`): `Forbidden`,
    /// naming what the caller needed in the message and in
    /// `details.required`, with the `context` it was refused in when there
    /// is one, and `details.anyOf` when any of several would have done
    /// (`required` is then the first).
    pub fn permission_denied(
        required: &str,
        context: Option<&str>,
        any_of: Option<&[&str]>,
    ) -> Self {
        let mut message = String::from("permission denied");
        if let Some(context) = context {
            message.push_str(&format!(" ({context})"));
        }
        match any_of {
            Some(any_of) => message.push_str(&format!(": one of {}", any_of.join(", "))),
            None => message.push_str(&format!(": {required}")),
        }
        let mut details = serde_json::json!({ "required": required });
        if let Some(context) = context {
            details["context"] = serde_json::json!(context);
        }
        if let Some(any_of) = any_of {
            details["anyOf"] = serde_json::json!(any_of);
        }
        Self::with_message(EngineErrorCode::Forbidden, message).with_details(details)
    }

    pub fn is(&self, code: EngineErrorCode) -> bool {
        self.code == code
    }

    /// The `SerializedEngineError` envelope, which carries a literal
    /// `name: "EngineError"` that TypeScript's `EngineError.is` sniffs for on
    /// values that crossed a structured-clone boundary and lost their
    /// prototype.
    pub fn to_wire(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        map.insert("name".into(), serde_json::Value::String("EngineError".into()));
        map.insert("code".into(), serde_json::Value::String(self.code.as_str().into()));
        map.insert("message".into(), serde_json::Value::String(self.message.clone()));
        if let Some(details) = &self.details {
            map.insert("details".into(), details.clone());
        }
        serde_json::Value::Object(map)
    }
}

impl Serialize for EngineError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("name", "EngineError")?;
        map.serialize_entry("code", &self.code)?;
        map.serialize_entry("message", &self.message)?;
        if let Some(details) = &self.details {
            map.serialize_entry("details", details)?;
        }
        map.end()
    }
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for EngineError {}

#[cfg(test)]
mod tests {
    use super::*;

    /// The literal list from `packages/engine/core/src/errors/EngineErrorCode.ts`.
    /// A value that changes on either side has to change in both, and this is
    /// where the two are compared until the generated-DTO parity harness
    /// covers it.
    const TYPESCRIPT_CODES: [&str; 29] = [
        "Unknown",
        "InvalidArg",
        "DocNotOpen",
        "DocOpenFailed",
        "DocPasswordRequired",
        "DocPasswordIncorrect",
        "SharePasswordRequired",
        "Aborted",
        "Network",
        "Unauthenticated",
        "Forbidden",
        "NotFound",
        "WireFormat",
        "RuntimeUnavailable",
        "InvalidReference",
        "WeakAnnotationSessionConflict",
        "LayerVersionConflict",
        "NotImplemented",
        "MalformedPdf",
        "SigningPending",
        "SigningExpired",
        "SigningVersionMismatch",
        "SignatureRefused",
        "ProtectedDocument",
        "StaleBase",
        "LayerFull",
        "ChangeConflict",
        "UndoUnavailable",
        "IdempotencyKeyReused",
    ];

    #[test]
    fn codes_are_byte_identical_to_the_typescript_enum() {
        let ours: Vec<&str> = EngineErrorCode::ALL.iter().map(|c| c.as_str()).collect();
        assert_eq!(ours, TYPESCRIPT_CODES);
    }

    #[test]
    fn codes_serialize_as_their_own_names() {
        for (code, name) in EngineErrorCode::ALL.iter().zip(TYPESCRIPT_CODES) {
            assert_eq!(serde_json::to_value(code).unwrap(), serde_json::json!(name));
            let back: EngineErrorCode = serde_json::from_value(serde_json::json!(name)).unwrap();
            assert_eq!(back, *code);
        }
    }

    #[test]
    fn a_message_free_error_uses_the_code_as_its_message() {
        let err = EngineError::new(EngineErrorCode::DocNotOpen);
        assert_eq!(err.message, "DocNotOpen");
        assert_eq!(
            err.to_wire(),
            serde_json::json!({
                "name": "EngineError",
                "code": "DocNotOpen",
                "message": "DocNotOpen",
            })
        );
    }

    #[test]
    fn details_are_omitted_rather_than_serialized_as_null() {
        let plain = EngineError::with_message(EngineErrorCode::InvalidArg, "bad page");
        assert_eq!(plain.to_wire().get("details"), None);

        let detailed = plain.clone().with_details(serde_json::json!({ "pageIndex": 4 }));
        assert_eq!(detailed.to_wire()["details"], serde_json::json!({ "pageIndex": 4 }));
    }

    /// `serializeError` (`EngineError.ts`): `{name: "EngineError", code,
    /// message, details?}`, with no `cause`. Serde writes the same object as
    /// `to_wire`, so a response envelope's `error` has the TypeScript shape.
    #[test]
    fn serde_writes_the_serialized_engine_error_shape() {
        let plain = EngineError::with_message(EngineErrorCode::NotFound, "no such page");
        assert_eq!(
            serde_json::to_string(&plain).unwrap(),
            r#"{"name":"EngineError","code":"NotFound","message":"no such page"}"#
        );
        let detailed = plain.with_details(serde_json::json!({ "pageIndex": 4 }));
        assert_eq!(serde_json::to_value(&detailed).unwrap(), detailed.to_wire());
        let back: EngineError = serde_json::from_value(detailed.to_wire()).unwrap();
        assert_eq!(back, detailed);
    }
}
