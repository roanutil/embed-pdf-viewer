//! `contract.ts` (with `token.ts`): the public signature vocabulary: the
//! act of signing (one-shot and two-phase), visual fills, validation, the
//! sign-here flow, and the facts about a document's signatures. Engine DTOs
//! (`FormFieldRef`, `SignatureSnapshot`, `SignatureDTO`,
//! `SignatureCompleteResult`, verdicts) stay the engine's JSON, as
//! `epdf-signature-core` keeps them.

use std::rc::Rc;
use std::sync::LazyLock;

use epdf_kernel::Token;
use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use epdf_plugin_stamp::{StampAsset, StampLibrary};
use epdf_signature_core::{SignerPort, TrustPort, ValidationTime};
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
    /// Seal the field with the configured key (the mark is the appearance).
    Sign,
    /// Draw the mark into the field without sealing.
    Visual,
    /// Neither: `onSignRequested` fires so the chrome can decide.
    Ask,
}

impl SignatureMode {
    pub fn as_str(self) -> &'static str {
        match self {
            SignatureMode::Sign => "sign",
            SignatureMode::Visual => "visual",
            SignatureMode::Ask => "ask",
        }
    }

    /// The mode a settings string names; anything else is no mode.
    pub fn from_name(name: &str) -> Option<SignatureMode> {
        match name {
            "sign" => Some(SignatureMode::Sign),
            "visual" => Some(SignatureMode::Visual),
            "ask" => Some(SignatureMode::Ask),
            _ => None,
        }
    }
}

/// `SignatureKey`: the key that signs, a signer port or a function that
/// returns one when someone signs (`SignerPort | (() => SignerPort |
/// Promise<SignerPort>)`). The port is held by `Rc`: the key setting is one
/// value, and a clone of it is the same value, as a JavaScript object is.
#[derive(Clone)]
pub enum SignatureKey {
    Port(Rc<SignerPort>),
    Thunk(Rc<dyn Fn() -> Promise<SignerPort>>),
}

impl SignatureKey {
    /// A port as the key setting.
    pub fn port(port: SignerPort) -> SignatureKey {
        SignatureKey::Port(Rc::new(port))
    }

    /// The same setting value, by identity: a clone of the key is the same
    /// key, and a port rebuilt from the same parts is another. The
    /// TypeScript store compares with `sameValue`
    /// (`packages/core/main/src/settings.ts:204-218`), which deep-compares a
    /// plain port object, so a CMS port rebuilt over the same `sign` is no
    /// change there. A Rust-only divergence, as interaction's tools are
    /// (README).
    pub fn same(&self, other: &SignatureKey) -> bool {
        match (self, other) {
            (SignatureKey::Thunk(a), SignatureKey::Thunk(b)) => {
                std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
            }
            (SignatureKey::Port(a), SignatureKey::Port(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

/// `SignatureSettings`: what `signaturePlugin(config)` registers over
/// [`signature_defaults`], and `updateSettings()` changes for every
/// document. A key and a trust port are one value each: a change replaces
/// them whole.
#[derive(Clone, Default)]
pub struct SignatureSettings {
    /// The key: a raw signer (the CMS is built here) or a CMS signer (a
    /// service builds it); `None` for none.
    pub key: Option<SignatureKey>,
    /// What a mark does in a field; `None` is `sign` with a key, else
    /// `visual`.
    pub mode: Option<SignatureMode>,
    /// The certificates you trust, for checking signatures. Without them a
    /// verdict is at best `valid-untrusted`.
    pub trust: Option<Rc<dyn TrustPort>>,
    /// Let a first signature be a certification (still needs
    /// `doc.sign.certify`).
    pub allow_certify: bool,
}

/// `SIGNATURE_DEFAULTS`.
pub fn signature_defaults() -> SignatureSettings {
    SignatureSettings::default()
}

/// `SignatureConfig = Partial<SignatureSettings>`: any of the settings.
/// The outer `Option` is "not given"; `Some(None)` is an explicit `null`.
#[derive(Clone, Default)]
pub struct SignatureConfig {
    pub key: Option<Option<SignatureKey>>,
    pub mode: Option<Option<SignatureMode>>,
    pub trust: Option<Option<Rc<dyn TrustPort>>>,
    pub allow_certify: Option<bool>,
}

/// `onSettingsChanged`'s event.
#[derive(Clone)]
pub struct SignatureSettingsChanged {
    pub settings: SignatureSettings,
    pub changed: Vec<String>,
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

/// `OperationOptions`.
#[derive(Clone, Default)]
pub struct OperationOptions {
    pub signal: Option<AbortSignal>,
}

/// `SignFieldInput`. `field`, `signer`, `certify` and `lock` stay the
/// engine's JSON (`FormFieldRef`, `SignatureSignerInput`, `{permission}`,
/// `FieldLockSpec`); `appearance` is a `BinarySource`.
#[derive(Clone)]
pub struct SignFieldInput {
    pub field: Value,
    pub mark: Option<Mark>,
    /// A per-call key; the `key` setting otherwise.
    pub key: Option<SignatureKey>,
    /// What the signature dictionary says about the signer; the name
    /// defaults to the certificate's subject.
    pub signer: Option<Value>,
    /// Instead of the mark: a ready one-page appearance PDF.
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
            key: None,
            signer: None,
            appearance: None,
            certify: None,
            lock: None,
        }
    }
}

/// `PrepareSignatureInput`: two-phase signing, step one: everything but the
/// key.
#[derive(Clone, Default)]
pub struct PrepareSignatureInput {
    pub field: Value,
    pub mark: Option<Mark>,
    pub appearance: Option<Value>,
    pub signer: Option<Value>,
    pub certify: Option<Value>,
    pub lock: Option<Value>,
    pub sub_filter: Option<Value>,
    pub digest: Option<Value>,
}

/// `SIGNATURES_LIBRARY_KIND`: the stamp-library kind that holds people's
/// marks, one library per person.
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

/// `SignerRow`: one person in a signatures picker, a library of kind
/// `signatures` with its marks split by role.
#[derive(Clone, Debug, PartialEq)]
pub struct SignerRow {
    pub library_id: String,
    pub name: String,
    pub library: StampLibrary,
    /// Every full signature of the person, in the library's order.
    pub signatures: Vec<StampAsset>,
    /// The person's initials, or `None` when the library has none.
    pub initials: Option<StampAsset>,
}

/// `signerRowsOf(libraries, assets)`: one row per library of kind
/// `signatures`, in the order of `libraries`. A pure derivation over the
/// stamp plugin's `listLibraries` and `listAssets`.
pub fn signer_rows_of(libraries: &[StampLibrary], assets: &[StampAsset]) -> Vec<SignerRow> {
    libraries
        .iter()
        .filter(|library| library.kind == SIGNATURES_LIBRARY_KIND)
        .map(|library| {
            let own: Vec<&StampAsset> =
                assets.iter().filter(|asset| asset.library_id == library.id).collect();
            SignerRow {
                library_id: library.id.clone(),
                name: library.name.clone(),
                library: library.clone(),
                signatures: own
                    .iter()
                    .filter(|asset| mark_role_of(&asset.name) == MarkRole::Signature)
                    .map(|asset| (*asset).clone())
                    .collect(),
                initials: own
                    .iter()
                    .find(|asset| mark_role_of(&asset.name) == MarkRole::Initials)
                    .map(|asset| (*asset).clone()),
            }
        })
        .collect()
}

/// `SignaturePending`: a signature prepared in two steps and not completed
/// yet.
#[derive(Clone, Debug, PartialEq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignaturePending {
    pub signing_id: String,
    pub field: Value,
}

/// `PlaceMarkResult`: sealed, drawn, handed to the chrome (mode `ask`), or
/// placed as a stamp.
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

/// Where `placeMark` puts a mark: into a field (`{ field }`), or freely (a
/// `StampPlacement` as JSON: `page`, `center`, `targetWidth`, ...).
#[derive(Clone, Debug, PartialEq)]
pub enum PlaceTarget {
    Field(Value),
    Free(Value),
}

// ── events ──

/// `SignatureSignedEvent`: a signing completed, in this session or another:
/// the confirmed `signatures.completed` fact.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SignatureSignedEvent {
    /// The sealed field, by its durable object-number ref.
    pub field: Value,
    pub result: Value,
    pub origin: Value,
}

/// `SignatureFilledEvent`: this session's `fillField()` drew a mark into a
/// field.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SignatureFilledEvent {
    pub field: Value,
}

/// `SignatureClearedEvent`: this session's `clearField()` took a drawn mark
/// out of a field.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SignatureClearedEvent {
    pub field: Value,
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

/// `SignatureInvalidationPredictedEvent`: an unsaved edit just turned a
/// signature that held into one a save would invalidate. Once per edge.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SignatureInvalidationPredictedEvent {
    pub field: Value,
    pub detail: String,
}

/// `SignatureTargetChangedEvent`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SignatureTargetChangedEvent {
    pub field: Value,
}

/// `SignatureSignRequestedEvent`: mode `ask`, a mark met a field and the
/// chrome decides.
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

/// `SignatureInspectionRequestedEvent`: a signed field was activated.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SignatureInspectionRequestedEvent {
    pub field: Value,
}

/// `SignatureValidateOptions`: the moment the certificates are judged at
/// (now by default), and `persisted` or `working-copy` (the default).
#[derive(Clone, Default)]
pub struct SignatureValidateOptions {
    pub at: Option<ValidationTime>,
    pub until: Option<String>,
    pub signal: Option<AbortSignal>,
}
