//! `contract.ts`: the public stamp vocabulary. A library is a PDF: its
//! `/Title` is the name, its `/Names /Pages` registry the assets, its pages
//! the artwork, `/PieceInfo` only what has no standard home.
//!
//! The descriptors are plain objects in the TypeScript, built by object
//! literals and spreads, so their member order depends on which literal
//! built them and which later spread added a member. [`StampLibrary`],
//! [`StampAsset`] and [`crate::model::StampState`] carry that order
//! ([`Members`]) and serialize in it.

use std::rc::Rc;

use epdf_kernel::Error;
use epdf_kernel::engine::{DocumentHandle, Engine, EngineCall};
use epdf_kernel::geometry::PageRef;
use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use serde::de::Error as _;
use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub use crate::convention::DEFAULT_LIBRARY_KIND;
pub use crate::token::STAMP;

// ── member order ──────────────────────────────────────────────────────────────

/// The member order of a plain object: the order its literal named its
/// members, then each member a later assignment added. A member deleted
/// (`delete next.subject`) leaves the order; one set to `undefined` stays in
/// it and prints nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Members(Rc<Vec<&'static str>>);

impl Members {
    pub fn of(keys: &[&'static str]) -> Members {
        Members(Rc::new(keys.to_vec()))
    }

    pub fn keys(&self) -> &[&'static str] {
        &self.0
    }

    pub fn has(&self, key: &str) -> bool {
        self.0.contains(&key)
    }

    /// `object[key] = value`: a new member goes last.
    pub fn ensure(&mut self, key: &'static str) {
        if !self.has(key) {
            Rc::make_mut(&mut self.0).push(key);
        }
    }

    /// `delete object[key]`.
    pub fn remove(&mut self, key: &str) {
        if self.has(key) {
            Rc::make_mut(&mut self.0).retain(|k| *k != key);
        }
    }

    /// The order the members of `object` arrived in, among `known`.
    pub fn read(object: &Map<String, Value>, known: &[&'static str]) -> Members {
        Members(Rc::new(
            object
                .keys()
                .filter_map(|k| known.iter().find(|n| **n == k.as_str()).copied())
                .collect(),
        ))
    }
}

/// A JSON object member, in the order serde reads it.
fn object_of<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Map<String, Value>, D::Error> {
    match Value::deserialize(deserializer)? {
        Value::Object(map) => Ok(map),
        other => Err(D::Error::custom(format!("expected an object, got {other}"))),
    }
}

fn member<T: serde::de::DeserializeOwned, E: serde::de::Error>(
    object: &Map<String, Value>,
    key: &str,
) -> Result<Option<T>, E> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) if epdf_kernel::is_undefined(value) => Ok(None),
        Some(value) => serde_json::from_value(value.clone()).map(Some).map_err(E::custom),
    }
}

fn required<T: serde::de::DeserializeOwned, E: serde::de::Error>(
    object: &Map<String, Value>,
    key: &str,
) -> Result<T, E> {
    member(object, key)?.ok_or_else(|| E::custom(format!("missing member {key}")))
}

// ── descriptors ───────────────────────────────────────────────────────────────

/// `StampAssetKind`.
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StampAssetKind {
    Stamp,
    Signature,
    Initials,
}

/// `StampLibraryKind`: open; the plugin defines `stamps` (the default) and
/// `signatures`, an embedder may define its own.
pub type StampLibraryKind = String;

/// An asset's intrinsic size in PDF points.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct StampSize {
    pub width: f64,
    pub height: f64,
}

/// `StampAsset`: one asset's serializable descriptor. The bytes and the
/// cached preview live in the binaries resource, never in state.
#[derive(Clone, Debug, PartialEq)]
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
    pub size: StampSize,
    /// The library page that is this asset.
    pub page: PageRef,
    /// An explicit `/Subj` override (PieceInfo).
    pub subject: Option<String>,
    pub categories: Option<Vec<String>>,
    pub members: Members,
}

const ASSET_MEMBERS: [&str; 9] =
    ["id", "libraryId", "kind", "name", "label", "size", "page", "subject", "categories"];

impl StampAsset {
    /// The literal both `importLibrary` and `createAsset` write:
    /// `{ id, libraryId, kind, name, label, size, page, ...subject?,
    /// ...categories? }`, the two optional members present only when given.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: String,
        library_id: String,
        kind: StampAssetKind,
        name: String,
        label: String,
        size: StampSize,
        page: PageRef,
        subject: Option<String>,
        categories: Option<Vec<String>>,
    ) -> StampAsset {
        let mut members = Members::of(&ASSET_MEMBERS[..7]);
        if subject.is_some() {
            members.ensure("subject");
        }
        if categories.is_some() {
            members.ensure("categories");
        }
        StampAsset { id, library_id, kind, name, label, size, page, subject, categories, members }
    }
}

impl Serialize for StampAsset {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut out = serializer.serialize_map(None)?;
        for key in self.members.keys() {
            match *key {
                "id" => out.serialize_entry(key, &self.id)?,
                "libraryId" => out.serialize_entry(key, &self.library_id)?,
                "kind" => out.serialize_entry(key, &self.kind)?,
                "name" => out.serialize_entry(key, &self.name)?,
                "label" => out.serialize_entry(key, &self.label)?,
                "size" => out.serialize_entry(key, &self.size)?,
                "page" => out.serialize_entry(key, &self.page)?,
                "subject" => {
                    if let Some(subject) = &self.subject {
                        out.serialize_entry(key, subject)?;
                    }
                }
                "categories" => {
                    if let Some(categories) = &self.categories {
                        out.serialize_entry(key, categories)?;
                    }
                }
                _ => {}
            }
        }
        out.end()
    }
}

impl<'de> Deserialize<'de> for StampAsset {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let object = object_of(deserializer)?;
        Ok(StampAsset {
            id: required(&object, "id")?,
            library_id: required(&object, "libraryId")?,
            kind: required(&object, "kind")?,
            name: required(&object, "name")?,
            label: required(&object, "label")?,
            size: required(&object, "size")?,
            page: required(&object, "page")?,
            subject: member(&object, "subject")?,
            categories: member(&object, "categories")?,
            members: Members::read(&object, &ASSET_MEMBERS),
        })
    }
}

/// `StampLibrary`. `createLibrary` writes `{ id, name, kind, assetIds,
/// ...categories? }`; `importLibrary` writes `{ id, name, kind, ...locale?,
/// categories, assetIds }`, where `categories` is a member even when it is
/// `undefined`. Later copies spread the original, so a library keeps its
/// literal's order for life.
#[derive(Clone, Debug, PartialEq)]
pub struct StampLibrary {
    /// PieceInfo `Id`: stable while the title is editable.
    pub id: String,
    pub name: String,
    /// PieceInfo `Kind`: `stamps`, `signatures`, or an embedder's own.
    pub kind: StampLibraryKind,
    /// PieceInfo `Locale`: the language of the labels.
    pub locale: Option<String>,
    pub categories: Option<Vec<String>>,
    /// Asset ids in display order.
    pub asset_ids: Vec<String>,
    pub members: Members,
}

const LIBRARY_MEMBERS: [&str; 6] = ["id", "name", "kind", "locale", "categories", "assetIds"];

impl StampLibrary {
    /// `createLibrary`'s literal.
    pub fn created(
        id: String,
        name: String,
        kind: StampLibraryKind,
        categories: Option<Vec<String>>,
    ) -> StampLibrary {
        let mut members = Members::of(&["id", "name", "kind", "assetIds"]);
        if categories.is_some() {
            members.ensure("categories");
        }
        StampLibrary { id, name, kind, locale: None, categories, asset_ids: Vec::new(), members }
    }

    /// `importLibrary`'s literal.
    pub fn imported(
        id: String,
        name: String,
        kind: StampLibraryKind,
        locale: Option<String>,
        categories: Option<Vec<String>>,
    ) -> StampLibrary {
        let members = if locale.is_some() {
            Members::of(&["id", "name", "kind", "locale", "categories", "assetIds"])
        } else {
            Members::of(&["id", "name", "kind", "categories", "assetIds"])
        };
        StampLibrary { id, name, kind, locale, categories, asset_ids: Vec::new(), members }
    }
}

impl Serialize for StampLibrary {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut out = serializer.serialize_map(None)?;
        for key in self.members.keys() {
            match *key {
                "id" => out.serialize_entry(key, &self.id)?,
                "name" => out.serialize_entry(key, &self.name)?,
                "kind" => out.serialize_entry(key, &self.kind)?,
                "locale" => {
                    if let Some(locale) = &self.locale {
                        out.serialize_entry(key, locale)?;
                    }
                }
                "categories" => {
                    if let Some(categories) = &self.categories {
                        out.serialize_entry(key, categories)?;
                    }
                }
                "assetIds" => out.serialize_entry(key, &self.asset_ids)?,
                _ => {}
            }
        }
        out.end()
    }
}

impl<'de> Deserialize<'de> for StampLibrary {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let object = object_of(deserializer)?;
        Ok(StampLibrary {
            id: required(&object, "id")?,
            name: required(&object, "name")?,
            kind: required(&object, "kind")?,
            locale: member(&object, "locale")?,
            categories: member(&object, "categories")?,
            asset_ids: member(&object, "assetIds")?.unwrap_or_default(),
            members: Members::read(&object, &LIBRARY_MEMBERS),
        })
    }
}

// ── settings ──────────────────────────────────────────────────────────────────

/// A local engine: one stamp libraries can be sliced and tagged on. The
/// TypeScript brands it (`LOCAL_ENGINE_BRAND`) and checks the brand with
/// `isLocalEngine`; here the type is the brand. Whoever wraps an engine in
/// it vouches that its documents are local ones.
#[derive(Clone)]
pub struct LocalEngine(Rc<dyn Engine>);

impl LocalEngine {
    pub fn new(engine: Rc<dyn Engine>) -> LocalEngine {
        LocalEngine(engine)
    }

    pub fn open(&self, input: Value, options: Option<Value>) -> EngineCall<Rc<dyn DocumentHandle>> {
        self.0.open(input, options)
    }

    pub fn destroy(&self) -> EngineCall<()> {
        self.0.destroy()
    }

    pub fn engine(&self) -> &Rc<dyn Engine> {
        &self.0
    }

    /// The same engine.
    pub fn same(&self, other: &LocalEngine) -> bool {
        std::ptr::addr_eq(Rc::as_ptr(&self.0), Rc::as_ptr(&other.0))
    }
}

/// What an asset-engine factory answers: `LocalEngine | Promise<LocalEngine>`.
#[derive(Clone)]
pub enum MadeAssetEngine {
    Ready(LocalEngine),
    Pending(Promise<LocalEngine>),
}

/// An asset-engine factory. `Err` is a synchronous throw.
pub type AssetEngineFactory = Rc<dyn Fn() -> Result<MadeAssetEngine, Error>>;

/// `StampAssetEngine`: the local engine stamp libraries open in, or a
/// function that makes one. A function is called on the first import, not
/// at startup, and its engine is destroyed with the plugin.
#[derive(Clone)]
pub enum StampAssetEngine {
    Engine(LocalEngine),
    Factory(AssetEngineFactory),
}

impl StampAssetEngine {
    /// The same setting value (`===`).
    pub fn same(&self, other: &StampAssetEngine) -> bool {
        match (self, other) {
            (StampAssetEngine::Engine(a), StampAssetEngine::Engine(b)) => a.same(b),
            (StampAssetEngine::Factory(a), StampAssetEngine::Factory(b)) => {
                std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
            }
            _ => false,
        }
    }
}

/// `StampSettings`.
#[derive(Clone)]
pub struct StampSettings {
    /// The engine stamp libraries open in. `None` uses the viewer's own
    /// engine, which must then be local.
    pub asset_engine: Option<StampAssetEngine>,
    /// The width of a stamp's thumbnail, in device pixels.
    pub preview_width: f64,
    /// Fill in a dynamic stamp's form fields when it's armed or placed.
    pub dynamic: bool,
}

/// `STAMP_DEFAULTS`.
pub fn stamp_defaults() -> StampSettings {
    StampSettings { asset_engine: None, preview_width: 256.0, dynamic: true }
}

/// `StampConfig`: any of the settings, over the defaults. `asset_engine`
/// is `Some(None)` for an explicit `null`.
#[derive(Clone, Default)]
pub struct StampConfig {
    pub asset_engine: Option<Option<StampAssetEngine>>,
    pub preview_width: Option<f64>,
    pub dynamic: Option<bool>,
}

/// `onSettingsChanged`'s event.
#[derive(Clone)]
pub struct StampSettingsChanged {
    pub settings: StampSettings,
    pub changed: Vec<String>,
}

// ── verbs ─────────────────────────────────────────────────────────────────────

/// `OperationOptions`.
#[derive(Clone, Default)]
pub struct OperationOptions {
    pub signal: Option<AbortSignal>,
}

/// `StampDocumentOptions`: which document a placing verb acts on; left out,
/// the active one.
#[derive(Clone, Default)]
pub struct StampDocumentOptions {
    pub document_id: Option<String>,
    pub signal: Option<AbortSignal>,
}

/// `StampArmOptions`.
#[derive(Clone, Default)]
pub struct StampArmOptions {
    pub document_id: Option<String>,
    /// Placed width in PDF points; the stamp's own width by default.
    pub target_width: Option<f64>,
    pub signal: Option<AbortSignal>,
}

/// `CreateLibraryOptions`.
#[derive(Clone, Default)]
pub struct CreateLibraryOptions {
    /// The library's id; a fresh one when left out or taken.
    pub id: Option<String>,
    /// What the library is for. Default `'stamps'`.
    pub kind: Option<StampLibraryKind>,
    pub categories: Option<Vec<String>>,
    pub signal: Option<AbortSignal>,
}

/// `StampLibraryPatch`.
#[derive(Clone, Debug, Default)]
pub struct StampLibraryPatch {
    pub name: Option<String>,
    pub categories: Option<Vec<String>>,
}

/// `ImportLibraryOptions`.
#[derive(Clone, Default)]
pub struct ImportLibraryOptions {
    /// Fallback name when the PDF carries no `/Title`. Default `'Stamps'`.
    pub name: Option<String>,
    pub categories: Option<Vec<String>>,
    /// Kind stamped onto every imported asset.
    pub kind: Option<StampAssetKind>,
    /// What the file is for; overrides what the file says.
    pub library_kind: Option<StampLibraryKind>,
    /// Fallback per-page labels for a plain PDF, by page index. `Ok(None)`
    /// is the function answering `undefined`; `Err` a throw.
    pub asset_name: Option<Rc<dyn Fn(f64) -> Result<Option<String>, Error>>>,
    pub signal: Option<AbortSignal>,
}

/// `MarkSource`: how a mark is authored. Sources are `BinarySource`s as
/// canonical JSON. Ink points are in points, from the top-left, y down.
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
    /// Target library; omitted, a new single-asset library named after the
    /// asset.
    pub library_id: Option<String>,
    /// The identifier (placed `/Name`); omitted, a fresh Acrobat-style one.
    pub name: Option<String>,
    /// Picker text and default `/Subj`. Defaults to `name`.
    pub label: Option<String>,
    pub kind: Option<StampAssetKind>,
    pub subject: Option<String>,
    pub categories: Option<Vec<String>>,
    pub source: Option<Value>,
    pub mark: Option<MarkSource>,
    pub preview: Option<Value>,
    pub size: Option<StampSize>,
}

/// `StampAssetPatch`: `subject` is `Some(None)` for `null` (drop the
/// override).
#[derive(Clone, Debug, Default)]
pub struct StampAssetPatch {
    pub label: Option<String>,
    pub subject: Option<Option<String>>,
    pub categories: Option<Vec<String>>,
}

/// `StampAssetPreview`: a cached, paintable render. `bytes` is `None` when
/// the render answered none (the TypeScript's `undefined`), which prints
/// nothing.
#[derive(Clone, Debug, PartialEq)]
pub struct StampAssetPreview {
    pub bytes: Option<Value>,
    pub mime_type: Option<Value>,
}

impl StampAssetPreview {
    /// `{ bytes, mimeType }` as JSON, absent members left out.
    pub fn to_json(&self) -> Value {
        let mut out = Map::new();
        if let Some(bytes) = &self.bytes {
            out.insert("bytes".into(), bytes.clone());
        }
        if let Some(mime_type) = &self.mime_type {
            out.insert("mimeType".into(), mime_type.clone());
        }
        Value::Object(out)
    }

    /// The bytes as a byte array, when they are one.
    pub fn byte_vec(&self) -> Option<Vec<u8>> {
        self.bytes.as_ref().and_then(epdf_kernel::bytes::Bytes::from_json).map(|b| b.0)
    }
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

/// A page argument: a ref, or a display index.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PageTarget {
    Ref(PageRef),
    Index(f64),
}

/// `placeAssetOnPages`' pages: a list, or `'all'`.
#[derive(Clone, Debug, PartialEq)]
pub enum PageSelection {
    All,
    Pages(Vec<PageTarget>),
}

/// One failed page of a `BatchResult`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BatchFailure {
    /// The page's ref, as the registry holds it.
    #[serde(rename = "ref")]
    pub page: Value,
    /// `PluginErrorInfo`.
    pub error: Value,
}

/// `BatchResult<Annotation, PageRef>`.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct BatchResult {
    pub applied: Vec<Value>,
    pub skipped: Vec<Value>,
    pub failed: Vec<BatchFailure>,
}

// ── events ────────────────────────────────────────────────────────────────────

/// `StampLibraryChangedEvent['reason']`.
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
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

/// `StampLibraryChangedEvent`: a library's bytes changed, and why.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StampLibraryChangedEvent {
    pub library_id: String,
    pub reason: StampLibraryChangeReason,
}

/// `StampLibraryCreatedEvent`, `StampLibraryUpdatedEvent` and
/// `StampLibraryDeletedEvent` (`library: null`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StampLibraryEvent {
    pub library_id: String,
    pub library: Option<StampLibrary>,
}

/// `StampAssetCreatedEvent`, `StampAssetUpdatedEvent` and
/// `StampAssetDeletedEvent` (`asset: null`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StampAssetEvent {
    pub asset_id: String,
    pub library_id: String,
    pub asset: Option<StampAsset>,
}

/// `StampArmChangedEvent`: armed, or disarmed (`asset: null`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StampArmChangedEvent {
    pub document_id: String,
    pub asset: Option<StampAsset>,
}
