//! `types.ts`: the serializable descriptors, the actions, the config, and
//! the capability's token.
//!
//! The plugin is a workspace-scoped asset substrate. An imported PDF library
//! keeps one canonical PDF whose catalog and page `/PieceInfo` carry its
//! metadata; per-asset PDFs and previews are derived caches used for
//! placement. Standalone PNG and JPEG assets stay loose.

use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::LazyLock;

use epdf_acrojs::{ScriptDiagnostic, ScriptExecutionError};
use epdf_geometry::Size;
use epdf_kernel::engine::Engine;
use epdf_kernel::rt::Promise;
use epdf_kernel::{Error, Token};
use epdf_plugin_form::{FormScriptingOptions, FormUiEffect};
use serde::{Deserialize, Serialize};

use crate::capability::StampCapability;

/// `StampAssetKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StampAssetKind {
    Stamp,
    Signature,
    Initials,
}

/// `StampAsset['format']`: what the asset bytes are.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StampAssetFormat {
    Pdf,
    Png,
    Jpeg,
}

/// `StampAsset`: one asset's serializable descriptor. The bytes and the
/// cached preview live in the capability, never in the store.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StampAsset {
    pub id: String,
    pub library_id: String,
    pub kind: StampAssetKind,
    pub name: String,
    /// Intrinsic size in PDF points.
    pub size: Size,
    pub format: StampAssetFormat,
    /// Canonical library page identity; present for PDF-backed libraries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_object_number: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub categories: Option<Vec<String>>,
}

/// `StampLibrary['storage']`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StampLibraryStorage {
    /// Owns canonical PDF bytes.
    #[serde(rename = "canonical-pdf")]
    CanonicalPdf,
    /// Raster or pre-sliced conveniences.
    #[serde(rename = "loose")]
    Loose,
}

/// `StampLibrary`: a named group of assets.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StampLibrary {
    pub id: String,
    pub name: String,
    pub storage: StampLibraryStorage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub categories: Option<Vec<String>>,
    /// Asset ids in display order.
    pub asset_ids: Vec<String>,
}

/// `StampState`. Member order is not observable (the TypeScript records are
/// plain objects and the vectors compare them unordered), so the maps sort.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StampState {
    pub libraries: BTreeMap<String, StampLibrary>,
    /// Library display order (insertion order).
    pub library_order: Vec<String>,
    pub assets: BTreeMap<String, StampAsset>,
}

/// `StampAction`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum StampAction {
    #[serde(rename = "LIBRARY_ADDED")]
    LibraryAdded { library: StampLibrary },
    #[serde(rename = "LIBRARY_REMOVED", rename_all = "camelCase")]
    LibraryRemoved { library_id: String },
    #[serde(rename = "ASSET_ADDED")]
    AssetAdded { asset: StampAsset },
    #[serde(rename = "ASSET_REMOVED", rename_all = "camelCase")]
    AssetRemoved { asset_id: String },
}

/// `StampAssetPreview`: a cached, paintable render of an asset. `bytes` is
/// `None` when the engine's render answered no bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct StampAssetPreview {
    pub bytes: Option<Vec<u8>>,
    pub mime_type: Option<String>,
}

/// What an asset-engine factory answers: `Engine | Promise<Engine>`.
#[derive(Clone)]
pub enum AssetEngineRef {
    Ready(Rc<dyn Engine>),
    Pending(Promise<Rc<dyn Engine>>),
}

/// `StampConfig['assetEngine']`: an engine used as is, or a factory called
/// (and memoized) on the first open and destroyed on teardown.
#[derive(Clone)]
pub enum AssetEngine {
    Instance(Rc<dyn Engine>),
    Factory(Rc<dyn Fn() -> Result<AssetEngineRef, Error>>),
}

/// An observer of a scripting result. An `Err` is a throw, which the plugin
/// reports to [`StampHost::console_error`] and which stops the remaining
/// observers.
pub type Observer<T> = Rc<dyn Fn(&T) -> Result<(), Error>>;

/// `StampScriptingOptions`: `FormScriptingOptions` plus the opt-in switch
/// and the observers. The form options carry the sandbox factory, which Rust
/// requires: a host with no sandbox has no scripting options at all, and the
/// plugin then behaves as the TypeScript does with scripting disabled.
#[derive(Clone)]
pub struct StampScriptingOptions {
    pub enabled: bool,
    pub form: FormScriptingOptions,
    pub on_ui_effect: Option<Observer<FormUiEffect>>,
    pub on_diagnostic: Option<Observer<ScriptDiagnostic>>,
    pub on_error: Option<Observer<ScriptExecutionError>>,
}

/// The ambient value the TypeScript reads: `console.error`, called when a
/// scripting observer throws. `None` drops the report.
#[derive(Clone, Default)]
pub struct StampHost {
    pub console_error: Option<Rc<dyn Fn(&str, &Error)>>,
}

/// `StampConfig`.
#[derive(Clone, Default)]
pub struct StampConfig {
    /// The asset engine port. `None` uses the kernel's engine.
    pub asset_engine: Option<AssetEngine>,
    /// Cached preview width in device pixels. Default 256.
    pub preview_width: Option<f64>,
    pub scripting: Option<StampScriptingOptions>,
    pub host: StampHost,
}

/// `ImportLibraryOptions`.
#[derive(Clone, Default)]
pub struct ImportLibraryOptions {
    /// Library display name. Default `'Stamps'`.
    pub name: Option<String>,
    pub categories: Option<Vec<String>>,
    pub kind: Option<StampAssetKind>,
    /// Per-page asset names by page index; `None` falls back.
    pub asset_name: Option<Rc<dyn Fn(f64) -> Option<String>>>,
}

/// `AddAssetInput`. `source` and `preview` are `BinarySource`s as canonical
/// JSON (`{"$bytes": ...}` or a `{data, mimeType, name}` payload).
#[derive(Clone, Debug, Default)]
pub struct AddAssetInput {
    pub library_id: Option<String>,
    pub name: String,
    pub kind: Option<StampAssetKind>,
    pub subject: Option<String>,
    pub categories: Option<Vec<String>>,
    pub source: serde_json::Value,
    pub preview: Option<serde_json::Value>,
    pub size: Option<Size>,
}

/// `armAsset`'s options.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ArmAssetOptions {
    pub target_width: Option<f64>,
}

/// `StampToken`.
pub static STAMP: LazyLock<Token<StampCapability>> = LazyLock::new(|| Token::new("stamp"));
