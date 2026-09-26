//! `contract.ts`, `token.ts` and `host-contract.ts`: the public stamp
//! vocabulary, the token, and the configuration. A library IS a PDF: its
//! `/Title` is the name, its `/Names /Pages` registry the assets, its pages
//! the artwork, `/PieceInfo` only what has no standard home.

use std::rc::Rc;
use std::sync::LazyLock;

use epdf_geometry::Size;
use epdf_kernel::engine::Engine;
use epdf_kernel::geometry::PageRef;
use epdf_kernel::rt::Promise;
use epdf_kernel::{Error, Token};
use epdf_plugin_actions::page_ref_json;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::controller::StampCapability;

/// `StampToken`.
pub static STAMP: LazyLock<Token<StampCapability>> = LazyLock::new(|| {
    Token::with_hint(
        "stamp",
        "add stampPlugin() from '@embedpdf/plugin-stamp' to your plugins list",
    )
});

/// `StampAssetKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StampAssetKind {
    Stamp,
    Signature,
    Initials,
}

/// `StampLibraryKind`: open; the plugin defines `stamps` (the default) and
/// `signatures`.
pub type StampLibraryKind = String;

/// `StampAsset`: one asset's serializable descriptor. The bytes and the
/// cached preview live in the binary sidecar, never in the store.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StampAsset {
    /// Derived and stable: `${libraryId}:${name}`.
    pub id: String,
    pub library_id: String,
    pub kind: StampAssetKind,
    /// The identifier: the text before the first `=` in the library key and
    /// the placed annotation's `/Name`.
    pub name: String,
    /// What the picker shows and the placed annotation's default `/Subj`.
    pub label: String,
    /// Intrinsic size in PDF points.
    pub size: Size,
    /// The library page that is this asset.
    #[serde(with = "page_ref_json")]
    pub page: PageRef,
    /// An explicit `/Subj` override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub categories: Option<Vec<String>>,
}

/// `StampLibrary`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StampLibrary {
    /// PieceInfo `Id`: stable while the title is editable.
    pub id: String,
    pub name: String,
    /// PieceInfo `Kind`: `stamps`, `signatures`, or an embedder's own.
    pub kind: StampLibraryKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub categories: Option<Vec<String>>,
    /// Asset ids in display order.
    pub asset_ids: Vec<String>,
}

/// `StampLibraryChange['reason']`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StampLibraryChangeReason {
    Created,
    Imported,
    Updated,
    AssetAdded,
    AssetUpdated,
    AssetRemoved,
    Removed,
}

/// `StampLibraryChange`: why a library's canonical bytes changed.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StampLibraryChange {
    pub library_id: String,
    pub reason: StampLibraryChangeReason,
}

/// `StampLibraryEvent`: `library` is null once deleted.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StampLibraryEvent {
    pub library_id: String,
    pub library: Option<StampLibrary>,
}

/// `StampAssetEvent`: `asset` is null once deleted.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StampAssetEvent {
    pub asset_id: String,
    pub library_id: String,
    pub asset: Option<StampAsset>,
}

/// `StampArmChangedEvent`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StampArmChangedEvent {
    pub document_id: String,
    pub asset_id: Option<String>,
}

/// `StampAssetPreview`: a cached, paintable render. `bytes` is `None` when
/// the engine's render answered no bytes.
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

/// The ambient values the TypeScript reads. `console_error` and
/// `console_warn` receive `[stamp]` reports (`None` drops them); `random`
/// is `Math.random`, which a minted identifier draws from.
#[derive(Clone, Default)]
pub struct StampHost {
    pub console_error: Option<Rc<dyn Fn(&str, &Error)>>,
    pub console_warn: Option<Rc<dyn Fn(&str, Option<&Error>)>>,
    pub random: Option<Rc<dyn Fn() -> f64>>,
}

/// `StampConfig`.
#[derive(Clone, Default)]
pub struct StampConfig {
    /// The asset engine port. `None` uses the kernel's engine.
    pub asset_engine: Option<AssetEngine>,
    /// Cached preview width in device pixels. Default 256.
    pub preview_width: Option<f64>,
    /// Evaluate form-backed (dynamic) PDF assets on arm. Default `true`.
    /// Scripting itself is the target document's actions plugin's switch.
    pub dynamic: Option<bool>,
    pub host: StampHost,
}

/// `ImportLibraryOptions`.
#[derive(Clone, Default)]
pub struct ImportLibraryOptions {
    /// Fallback name when the PDF carries no `/Title`. Default `'Stamps'`.
    pub name: Option<String>,
    pub categories: Option<Vec<String>>,
    pub kind: Option<StampAssetKind>,
    pub library_kind: Option<StampLibraryKind>,
    /// Fallback per-page labels for a plain PDF, by page index.
    pub asset_name: Option<Rc<dyn Fn(f64) -> Option<String>>>,
}

/// `MarkSource`: how a mark is authored. Sources are `BinarySource`s as
/// canonical JSON.
#[derive(Clone, Debug, PartialEq)]
pub enum MarkSource {
    Ink { strokes: Vec<Vec<(f64, f64)>>, color: Option<String>, stroke_width: Option<f64> },
    Text { text: String, font_family: String, color: Option<String>, font_size: Option<f64> },
    Image { source: Value },
    Pdf { source: Value, page_index: Option<f64> },
}

/// `AddAssetInput`. `source` and `preview` are `BinarySource`s as canonical
/// JSON.
#[derive(Clone, Debug, Default)]
pub struct AddAssetInput {
    pub library_id: Option<String>,
    pub name: Option<String>,
    pub label: Option<String>,
    pub kind: Option<StampAssetKind>,
    pub subject: Option<String>,
    pub categories: Option<Vec<String>>,
    pub source: Option<Value>,
    pub mark: Option<MarkSource>,
    pub preview: Option<Value>,
    pub size: Option<Size>,
}

/// `createLibrary`'s options.
#[derive(Clone, Debug, Default)]
pub struct CreateLibraryOptions {
    pub id: Option<String>,
    pub kind: Option<StampLibraryKind>,
    pub categories: Option<Vec<String>>,
}

/// `updateLibrary`'s patch.
#[derive(Clone, Debug, Default)]
pub struct LibraryPatch {
    pub name: Option<String>,
    pub categories: Option<Vec<String>>,
}

/// `updateAsset`'s patch: `subject` is `Some(None)` for null (clear it).
#[derive(Clone, Debug, Default)]
pub struct AssetPatch {
    pub label: Option<String>,
    pub subject: Option<Option<String>>,
    pub categories: Option<Vec<String>>,
}

/// `StampLibraryFilter`: one kind or several.
#[derive(Clone, Debug, Default)]
pub struct StampLibraryFilter {
    pub kind: Option<Vec<String>>,
}

/// `StampAssetFilter`.
#[derive(Clone, Debug, Default)]
pub struct StampAssetFilter {
    pub library_id: Option<String>,
    pub kind: Option<StampAssetKind>,
    pub category: Option<String>,
}

/// One failed entry of a `BatchResult`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BatchFailure {
    #[serde(rename = "ref", with = "page_ref_json")]
    pub page: PageRef,
    pub error: Value,
}

/// `BatchResult<AnnotationRef, PageRef>`.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct BatchResult {
    pub applied: Vec<Value>,
    pub skipped: Vec<Value>,
    pub failed: Vec<BatchFailure>,
}
