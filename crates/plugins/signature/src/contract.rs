//! `contract.ts` (with `token.ts` and `host-contract.ts`): the public
//! signature vocabulary. Engine DTOs (`FormFieldRef`, `SignatureSnapshot`,
//! `SignatureDTO`, `SignatureCompleteResult`, verdicts) stay the engine's
//! JSON, as `epdf-signature-core` keeps them.

use std::rc::Rc;
use std::sync::LazyLock;

use epdf_kernel::rt::Promise;
use epdf_kernel::{Error, Token};
use epdf_signature_core::{SignerPort, TrustPort};
use serde::Serialize;
use serde_json::{Value, json};

use crate::controller::SignatureCapability;

/// `SignatureToken`.
pub static SIGNATURE: LazyLock<Token<SignatureCapability>> = LazyLock::new(|| {
    Token::with_hint(
        "signature",
        "add signaturePlugin() from '@embedpdf/plugin-signature' to your plugins list",
    )
});

/// `SignatureMode`: what placing a mark on a signature field does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SignatureMode {
    /// Seal the field with the configured signer.
    Sign,
    /// Draw the mark into the field without sealing.
    Visual,
    /// Neither: `onSignRequested` fires and the chrome decides.
    Ask,
}

/// A key holder: a port, or a thunk that resolves one per signing
/// (`SignerPort | (() => Promise<SignerPort>)`).
#[derive(Clone)]
pub enum SignerSource {
    Port(SignerPort),
    Thunk(Rc<dyn Fn() -> Promise<SignerPort>>),
    /// A native host's signer slot, which may be empty: `resolve` per
    /// signing, and `available` says whether a signer is set now, which
    /// `getMode` and `canSign` read where the TypeScript reads `config.signer`.
    Host {
        resolve: Rc<dyn Fn() -> Promise<SignerPort>>,
        available: Rc<dyn Fn() -> bool>,
    },
}

impl SignerSource {
    /// Whether this source has a signer to offer now.
    pub fn is_available(&self) -> bool {
        match self {
            SignerSource::Host { available, .. } => available(),
            _ => true,
        }
    }
}

/// What the host supplies beside the config: the TypeScript's
/// `globalThis.console?.error`, which the plugin reports background
/// failures and failing listeners to.
#[derive(Clone, Default)]
pub struct SignatureHost {
    pub console_error: Option<Rc<dyn Fn(&str, &Error)>>,
}

/// `SignatureConfig`.
#[derive(Clone, Default)]
pub struct SignatureConfig {
    pub mode: Option<SignatureMode>,
    pub signer: Option<SignerSource>,
    /// Trust anchors for validation. None: every verdict tops out at
    /// `valid-untrusted`.
    pub trust: Option<Rc<dyn TrustPort>>,
    /// Let the UI offer a certification. Default false.
    pub allow_certify: Option<bool>,
    pub host: SignatureHost,
}

/// `Mark`: a stamp-plugin asset, or bytes the embedder brings (a
/// `BinarySource`, as JSON).
#[derive(Clone, Debug, PartialEq)]
pub enum Mark {
    Asset { asset_id: String },
    Source { source: Value },
}

impl Mark {
    /// The mark as the TypeScript prints it.
    pub fn to_json(&self) -> Value {
        match self {
            Mark::Asset { asset_id } => json!({ "assetId": asset_id }),
            Mark::Source { source } => json!({ "source": source }),
        }
    }
}

/// `SignFieldInput`. `field`, `attribution`, `certify` and `lock` stay the
/// engine's JSON.
#[derive(Clone)]
pub struct SignFieldInput {
    pub field: Value,
    pub mark: Option<Mark>,
    pub signer: Option<SignerSource>,
    pub attribution: Option<Value>,
    pub appearance: Option<Value>,
    pub certify: Option<Value>,
    pub lock: Option<Value>,
}

impl SignFieldInput {
    /// A field and its mark, nothing else.
    pub fn new(field: Value, mark: Mark) -> Self {
        SignFieldInput {
            field,
            mark: Some(mark),
            signer: None,
            attribution: None,
            appearance: None,
            certify: None,
            lock: None,
        }
    }
}

/// `PrepareSignatureInput`.
#[derive(Clone, Default)]
pub struct PrepareSignatureInput {
    pub field: Value,
    pub mark: Option<Mark>,
    pub appearance: Option<Value>,
    pub attribution: Option<Value>,
    pub certify: Option<Value>,
    pub lock: Option<Value>,
    pub sub_filter: Option<Value>,
    pub digest: Option<Value>,
}

/// `SIGNATURES_LIBRARY_KIND`: the stamp-library kind that holds people's
/// marks.
pub const SIGNATURES_LIBRARY_KIND: &str = "signatures";
/// `SIGNATURE_MARK_NAME`.
pub const SIGNATURE_MARK_NAME: &str = "signature";
/// `INITIALS_MARK_NAME`.
pub const INITIALS_MARK_NAME: &str = "initials";

/// `MarkRole`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MarkRole {
    Signature,
    Initials,
}

/// `markRoleOf(asset)`: anything not named `initials` is a signature.
pub fn mark_role_of(name: &str) -> MarkRole {
    if name == INITIALS_MARK_NAME { MarkRole::Initials } else { MarkRole::Signature }
}

/// `SignaturePending`.
#[derive(Clone, Debug, PartialEq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignaturePending {
    pub signing_id: String,
    pub field: Value,
}

/// `PlaceMarkResult`.
#[derive(Clone, Debug, PartialEq)]
pub enum PlaceMarkResult {
    Signed { field: Value, result: Value },
    Filled { field: Value },
    Requested { field: Value },
    Placed { annotation: Value },
}

impl PlaceMarkResult {
    pub fn to_json(&self) -> Value {
        match self {
            PlaceMarkResult::Signed { field, result } => {
                json!({ "kind": "signed", "field": field, "result": result })
            }
            PlaceMarkResult::Filled { field } => json!({ "kind": "filled", "field": field }),
            PlaceMarkResult::Requested { field } => json!({ "kind": "requested", "field": field }),
            PlaceMarkResult::Placed { annotation } => {
                json!({ "kind": "placed", "annotation": annotation })
            }
        }
    }
}

/// Where `placeMark` puts a mark: into a field, or freely (a stamp).
#[derive(Clone, Debug, PartialEq)]
pub enum PlaceTarget {
    Field(Value),
    Free(epdf_plugin_annotation::StampPlacement),
}

// ── events ──

/// `SignatureSignedEvent`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SignatureSignedEvent {
    pub field: Value,
    pub result: Value,
    pub origin: Value,
}

/// `SignatureFieldEvent`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SignatureFieldEvent {
    pub field: Value,
    pub origin: Value,
}

/// `SignatureValidatedEvent`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SignatureValidatedEvent {
    pub verdicts: Value,
}

/// `SignatureProtectionChangedEvent`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SignatureProtectionChangedEvent {
    pub protection: Value,
}

/// `SignatureInvalidatingEvent`: an unsaved edit just turned a signature
/// that held into one a save would invalidate. Once per edge.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SignatureInvalidatingEvent {
    pub field: Value,
    pub detail: String,
}

/// `SignatureTargetChangedEvent`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SignatureTargetChangedEvent {
    pub field: Value,
}

/// `SignatureSignRequestedEvent`.
#[derive(Clone, Debug, PartialEq)]
pub struct SignatureSignRequestedEvent {
    pub field: Value,
    pub mark: Mark,
}

impl Serialize for SignatureSignRequestedEvent {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        json!({ "field": self.field, "mark": self.mark.to_json() }).serialize(s)
    }
}

/// `SignatureInspectionRequestedEvent`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SignatureInspectionRequestedEvent {
    pub field: Value,
}
