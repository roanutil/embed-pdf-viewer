//! The stamp plugin through uniffi: workspace-scoped, so the handle needs
//! no document and every placement names one.
//!
//! A `BinarySource` crosses as bytes and becomes `{"$bytes": base64}`
//! here; the plugin's batch results and engine errors are JSON and decode
//! into typed records, and a value that does not decode is
//! `invalid-state`.

use std::rc::Rc;
use std::sync::{Arc, Mutex};

use epdf_annotation_core as ac;
use epdf_ffi::{
    AnnotationRef, CancelToken, KernelBound, KernelPost, PageRef, Point, Size, Subscription,
    ViewerError,
};
use epdf_kernel::bytes::Bytes;
use epdf_kernel::rt::Promise;
use epdf_kernel::{Error, Unsubscribe};
use serde_json::Value;

use crate::persistence::{
    PersistOptions, StampLibraryStore as CoreStore, StoredLibrary, persist_stamp_libraries,
    restore_stamp_libraries,
};
use crate::{
    AddAssetInput, AssetPatch, CreateLibraryOptions, ImportLibraryOptions, LibraryPatch,
    MarkSource, StampAssetKind, StampCapability, StampLibraryChangeReason, StampLibraryFilter,
};

fn invalid_state(what: &str, e: impl std::fmt::Display) -> Error {
    Error::named("InvalidState", format!("[stamp] {what} does not decode: {e}"))
        .with("code", "invalid-state")
}

fn source(bytes: Vec<u8>) -> Value {
    Bytes(bytes).to_json()
}

fn annotation_out(r: &ac::AnnotationRef) -> Result<AnnotationRef, ViewerError> {
    AnnotationRef::try_from(r).map_err(ViewerError::from)
}

// ── config ──────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct StampConfig {
    /// Cached preview width in device pixels. `None` is 256.
    #[uniffi(default = None)]
    pub preview_width: Option<f64>,
    /// Evaluate form-backed assets on arm. `None` is true; scripting is the
    /// target document's actions plugin's switch.
    #[uniffi(default = None)]
    pub dynamic: Option<bool>,
}

impl StampConfig {
    /// The plugin's config with the host's asset engine.
    pub fn into_core(self, asset_engine: Option<crate::AssetEngine>) -> crate::StampConfig {
        crate::StampConfig {
            asset_engine,
            preview_width: self.preview_width,
            dynamic: self.dynamic,
            host: crate::StampHost::default(),
        }
    }
}

// ── records ─────────────────────────────────────────────────────────────────

/// A library: a PDF whose pages are its assets.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct StampLibrary {
    /// Stable while the title is editable.
    pub id: String,
    pub name: String,
    /// `stamps`, `signatures`, or an embedder's own.
    pub kind: String,
    /// Asset ids in display order.
    pub asset_ids: Vec<String>,
    pub locale: Option<String>,
    pub categories: Option<Vec<String>>,
}

impl From<crate::StampLibrary> for StampLibrary {
    fn from(l: crate::StampLibrary) -> Self {
        StampLibrary {
            id: l.id,
            name: l.name,
            kind: l.kind,
            asset_ids: l.asset_ids,
            locale: l.locale,
            categories: l.categories,
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct StampAsset {
    /// `libraryId:name`, stable.
    pub id: String,
    pub library_id: String,
    pub kind: StampAssetKind,
    pub name: String,
    pub label: String,
    /// Intrinsic size in PDF points.
    pub size: Size,
    /// The library page that is this asset.
    pub page: PageRef,
    pub subject: Option<String>,
    pub categories: Option<Vec<String>>,
}

impl From<crate::StampAsset> for StampAsset {
    fn from(a: crate::StampAsset) -> Self {
        StampAsset {
            id: a.id,
            library_id: a.library_id,
            kind: a.kind,
            name: a.name,
            label: a.label,
            size: a.size.into(),
            page: a.page.into(),
            subject: a.subject,
            categories: a.categories,
        }
    }
}

/// A cached, paintable render. `bytes` is `None` when the engine answered
/// none.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct StampAssetPreview {
    pub bytes: Option<Vec<u8>>,
    pub mime_type: Option<String>,
}

impl From<crate::StampAssetPreview> for StampAssetPreview {
    fn from(p: crate::StampAssetPreview) -> Self {
        StampAssetPreview { bytes: p.bytes, mime_type: p.mime_type }
    }
}

/// How a new asset's artwork is authored.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum StampMark {
    /// Strokes in PDF points.
    Ink {
        strokes: Vec<Vec<Point>>,
        color: Option<String>,
        stroke_width: Option<f64>,
    },
    Text {
        text: String,
        font_family: String,
        color: Option<String>,
        font_size: Option<f64>,
    },
    /// PNG or JPEG bytes.
    Image {
        bytes: Vec<u8>,
    },
    /// One page of a PDF.
    Pdf {
        bytes: Vec<u8>,
        page_index: Option<u32>,
    },
}

impl From<StampMark> for MarkSource {
    fn from(m: StampMark) -> Self {
        match m {
            StampMark::Ink { strokes, color, stroke_width } => MarkSource::Ink {
                strokes: strokes
                    .into_iter()
                    .map(|s| s.into_iter().map(|p| (p.x, p.y)).collect())
                    .collect(),
                color,
                stroke_width,
            },
            StampMark::Text { text, font_family, color, font_size } => {
                MarkSource::Text { text, font_family, color, font_size }
            }
            StampMark::Image { bytes } => MarkSource::Image { source: source(bytes) },
            StampMark::Pdf { bytes, page_index } => {
                MarkSource::Pdf { source: source(bytes), page_index: page_index.map(f64::from) }
            }
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct StampAddAssetInput {
    #[uniffi(default = None)]
    pub library_id: Option<String>,
    #[uniffi(default = None)]
    pub name: Option<String>,
    #[uniffi(default = None)]
    pub label: Option<String>,
    #[uniffi(default = None)]
    pub kind: Option<StampAssetKind>,
    #[uniffi(default = None)]
    pub subject: Option<String>,
    #[uniffi(default = None)]
    pub categories: Option<Vec<String>>,
    /// The artwork as bytes (PDF, PNG or JPEG). Give this or `mark`.
    #[uniffi(default = None)]
    pub source: Option<Vec<u8>>,
    #[uniffi(default = None)]
    pub mark: Option<StampMark>,
    #[uniffi(default = None)]
    pub preview: Option<Vec<u8>>,
    #[uniffi(default = None)]
    pub size: Option<Size>,
}

impl From<StampAddAssetInput> for AddAssetInput {
    fn from(i: StampAddAssetInput) -> Self {
        AddAssetInput {
            library_id: i.library_id,
            name: i.name,
            label: i.label,
            kind: i.kind,
            subject: i.subject,
            categories: i.categories,
            source: i.source.map(source),
            mark: i.mark.map(Into::into),
            preview: i.preview.map(source),
            size: i.size.map(Into::into),
        }
    }
}

/// `asset_name` (per-page labels from a closure) is not exported; a plain
/// PDF's assets are labelled from their page index.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct StampImportOptions {
    /// Fallback name when the PDF has no `/Title`. `None` is `Stamps`.
    #[uniffi(default = None)]
    pub name: Option<String>,
    #[uniffi(default = None)]
    pub categories: Option<Vec<String>>,
    #[uniffi(default = None)]
    pub kind: Option<StampAssetKind>,
    #[uniffi(default = None)]
    pub library_kind: Option<String>,
}

impl From<StampImportOptions> for ImportLibraryOptions {
    fn from(o: StampImportOptions) -> Self {
        ImportLibraryOptions {
            name: o.name,
            categories: o.categories,
            kind: o.kind,
            library_kind: o.library_kind,
            asset_name: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct StampCreateLibraryOptions {
    #[uniffi(default = None)]
    pub id: Option<String>,
    #[uniffi(default = None)]
    pub kind: Option<String>,
    #[uniffi(default = None)]
    pub categories: Option<Vec<String>>,
}

#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct StampLibraryPatch {
    #[uniffi(default = None)]
    pub name: Option<String>,
    #[uniffi(default = None)]
    pub categories: Option<Vec<String>>,
}

/// `clear_subject` removes the subject override; `subject` sets it.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct StampAssetPatch {
    #[uniffi(default = None)]
    pub label: Option<String>,
    #[uniffi(default = None)]
    pub subject: Option<String>,
    #[uniffi(default = false)]
    pub clear_subject: bool,
    #[uniffi(default = None)]
    pub categories: Option<Vec<String>>,
}

#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct StampAssetFilter {
    #[uniffi(default = None)]
    pub library_id: Option<String>,
    #[uniffi(default = None)]
    pub kind: Option<StampAssetKind>,
    #[uniffi(default = None)]
    pub category: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct StampErrorInfo {
    pub code: String,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct StampBatchFailure {
    pub page: PageRef,
    pub error: StampErrorInfo,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct StampBatchResult {
    pub applied: Vec<AnnotationRef>,
    pub skipped: Vec<PageRef>,
    pub failed: Vec<StampBatchFailure>,
}

impl TryFrom<crate::BatchResult> for StampBatchResult {
    type Error = Error;

    fn try_from(r: crate::BatchResult) -> Result<Self, Error> {
        let annotation = |v: Value| {
            let core: ac::AnnotationRef =
                serde_json::from_value(v).map_err(|e| invalid_state("a placed annotation", e))?;
            AnnotationRef::try_from(&core)
        };
        let page = |v: Value| {
            epdf_kernel::geometry::PageRef::from_json(&v)
                .map(PageRef::from)
                .ok_or_else(|| invalid_state("a skipped page", &v))
        };
        let failure = |f: crate::BatchFailure| {
            let code = f.error.get("code").and_then(Value::as_str);
            let message = f.error.get("message").and_then(Value::as_str);
            match (code, message) {
                (Some(code), Some(message)) => Ok(StampBatchFailure {
                    page: f.page.into(),
                    error: StampErrorInfo { code: code.into(), detail: message.into() },
                }),
                _ => Err(invalid_state("a placement error", &f.error)),
            }
        };
        Ok(StampBatchResult {
            applied: r.applied.into_iter().map(annotation).collect::<Result<_, _>>()?,
            skipped: r.skipped.into_iter().map(page).collect::<Result<_, _>>()?,
            failed: r.failed.into_iter().map(failure).collect::<Result<_, _>>()?,
        })
    }
}

// ── events ──────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum StampEvent {
    /// A library's canonical bytes changed: persist it.
    LibraryChanged {
        library_id: String,
        reason: StampLibraryChangeReason,
    },
    LibraryCreated {
        library_id: String,
        library: Option<StampLibrary>,
    },
    LibraryUpdated {
        library_id: String,
        library: Option<StampLibrary>,
    },
    LibraryDeleted {
        library_id: String,
    },
    AssetCreated {
        asset_id: String,
        library_id: String,
        asset: Option<StampAsset>,
    },
    AssetUpdated {
        asset_id: String,
        library_id: String,
        asset: Option<StampAsset>,
    },
    AssetDeleted {
        asset_id: String,
        library_id: String,
    },
    ArmChanged {
        document_id: String,
        asset_id: Option<String>,
    },
}

/// Workspace events never end on their own: the stream finishes when the
/// viewer is destroyed.
#[uniffi::export(with_foreign)]
pub trait StampEventListener: Send + Sync {
    fn on_event(&self, event: StampEvent);
}

// ── persistence port ────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct StoredStampLibrary {
    pub id: String,
    pub bytes: Vec<u8>,
}

/// Where custom libraries live: bytes by library id. Swift implements it
/// over files. Called on the kernel thread, synchronously; keep it to
/// local disk.
#[uniffi::export(with_foreign)]
pub trait StampLibraryStore: Send + Sync {
    fn list(&self) -> Result<Vec<StoredStampLibrary>, ViewerError>;
    fn put(&self, id: String, bytes: Vec<u8>) -> Result<(), ViewerError>;
    fn delete(&self, id: String) -> Result<(), ViewerError>;
}

/// The Rust store over the foreign one.
struct ForeignStore(Arc<dyn StampLibraryStore>);

fn settled<T: Clone + 'static>(result: Result<T, ViewerError>) -> Promise<T> {
    match result {
        Ok(value) => Promise::resolved(value),
        Err(ViewerError::Kernel { name, code, detail }) => {
            let mut error = Error::named(name, detail);
            if let Some(code) = code {
                error = error.with("code", code);
            }
            Promise::rejected(error)
        }
        Err(ViewerError::Closed) => Promise::rejected(Error::new("[stamp] the store is closed")),
    }
}

impl CoreStore for ForeignStore {
    fn list(&self) -> Promise<Vec<StoredLibrary>> {
        settled(self.0.list().map(|rows| {
            rows.into_iter().map(|r| StoredLibrary { id: r.id, bytes: r.bytes }).collect()
        }))
    }
    fn put(&self, id: &str, bytes: Vec<u8>) -> Promise<()> {
        settled(self.0.put(id.to_string(), bytes))
    }
    fn delete(&self, id: &str) -> Promise<()> {
        settled(self.0.delete(id.to_string()))
    }
}

/// A running `persist`. `stop` is idempotent; it flushes pending writes.
#[derive(uniffi::Object)]
pub struct StampPersistence {
    stop: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

#[uniffi::export]
impl StampPersistence {
    pub fn stop(&self) {
        let stop = self.stop.lock().unwrap().take();
        if let Some(stop) = stop {
            stop();
        }
    }
}

// ── the handle ──────────────────────────────────────────────────────────────

#[derive(uniffi::Object)]
pub struct StampHandle {
    cap: KernelBound<StampCapability>,
    post: KernelPost,
}

impl StampHandle {
    /// On the kernel thread.
    pub fn new(cap: StampCapability, post: KernelPost) -> Arc<Self> {
        Arc::new(StampHandle { cap: KernelBound::new(cap, post.clone()), post })
    }
}

#[uniffi::export]
impl StampHandle {
    /// Every library, or those of the given kinds (`stamps`, `signatures`).
    pub async fn libraries(
        &self,
        kinds: Option<Vec<String>>,
    ) -> Result<Vec<StampLibrary>, ViewerError> {
        self.cap
            .call(move |c| {
                let filter = kinds.map(|kind| StampLibraryFilter { kind: Some(kind) });
                Ok(c.list_libraries(filter.as_ref()).into_iter().map(Into::into).collect())
            })
            .await
    }

    pub async fn library(&self, id: String) -> Result<Option<StampLibrary>, ViewerError> {
        self.cap.call(move |c| Ok(c.get_library(&id).map(Into::into))).await
    }

    pub async fn assets(
        &self,
        filter: Option<StampAssetFilter>,
    ) -> Result<Vec<StampAsset>, ViewerError> {
        self.cap
            .call(move |c| {
                let filter = filter.map(|f| crate::StampAssetFilter {
                    library_id: f.library_id,
                    kind: f.kind,
                    category: f.category,
                });
                Ok(c.list_assets(filter.as_ref()).into_iter().map(Into::into).collect())
            })
            .await
    }

    pub async fn asset(&self, id: String) -> Result<Option<StampAsset>, ViewerError> {
        self.cap.call(move |c| Ok(c.get_asset(&id).map(Into::into))).await
    }

    /// The cached preview, made at import. `None` before then.
    pub async fn asset_preview(
        &self,
        id: String,
    ) -> Result<Option<StampAssetPreview>, ViewerError> {
        self.cap.call(move |c| Ok(c.get_asset_preview(&id).map(Into::into))).await
    }

    /// A fresh render `width` device pixels wide.
    pub async fn render_asset_preview(
        &self,
        id: String,
        width: f64,
        cancel: Arc<CancelToken>,
    ) -> Result<Option<StampAssetPreview>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(c.render_asset_preview(&id, width)),
                |p: Option<crate::StampAssetPreview>| p.map(Into::into),
            )
            .await
    }

    /// The asset's own one-page PDF.
    pub async fn read_asset_bytes(&self, id: String) -> Result<Option<Vec<u8>>, ViewerError> {
        self.cap.call(move |c| Ok(c.read_asset_bytes(&id))).await
    }

    /// The library's canonical PDF.
    pub async fn export_library(
        &self,
        id: String,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<u8>, ViewerError> {
        self.cap.call_promise_cancellable(cancel, move |c| Ok(c.export_library(&id)), |b| b).await
    }

    /// Whether `document_id` (or the active document) can take a stamp.
    pub async fn can_place(&self, document_id: Option<String>) -> Result<bool, ViewerError> {
        self.cap.call(move |c| Ok(c.can_place(document_id.as_deref()))).await
    }

    pub async fn can_import(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(c.can_import())).await
    }

    /// Returns the new library's id.
    pub async fn create_library(
        &self,
        name: String,
        options: StampCreateLibraryOptions,
        cancel: Arc<CancelToken>,
    ) -> Result<String, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let opts = CreateLibraryOptions {
                        id: options.id,
                        kind: options.kind,
                        categories: options.categories,
                    };
                    Ok(c.create_library(&name, opts))
                },
                |id| id,
            )
            .await
    }

    pub async fn update_library(
        &self,
        id: String,
        patch: StampLibraryPatch,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let patch = LibraryPatch { name: patch.name, categories: patch.categories };
                    Ok(c.update_library(&id, patch))
                },
                |()| (),
            )
            .await
    }

    /// Import a library PDF. Returns the new library's id. `InvalidArg` for
    /// bytes that aren't a PDF.
    pub async fn import_library(
        &self,
        bytes: Vec<u8>,
        options: StampImportOptions,
        cancel: Arc<CancelToken>,
    ) -> Result<String, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(c.import_library(source(bytes), options.into())),
                |id| id,
            )
            .await
    }

    pub async fn delete_library(
        &self,
        id: String,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap.call_promise_cancellable(cancel, move |c| Ok(c.delete_library(&id)), |()| ()).await
    }

    /// Returns the new asset's id.
    pub async fn create_asset(
        &self,
        input: StampAddAssetInput,
        cancel: Arc<CancelToken>,
    ) -> Result<String, ViewerError> {
        self.cap
            .call_promise_cancellable(cancel, move |c| Ok(c.create_asset(input.into())), |id| id)
            .await
    }

    /// Needs `page.annotations.exportAppearance`, which the native engine
    /// has not mapped: answers `unsupported` there.
    pub async fn create_asset_from_annotations(
        &self,
        document_id: String,
        page: PageRef,
        annotations: Vec<AnnotationRef>,
        input: StampAddAssetInput,
        cancel: Arc<CancelToken>,
    ) -> Result<String, ViewerError> {
        let created = self
            .cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let refs = annotations
                        .into_iter()
                        .map(|a| epdf_kernel::js::to_value(&ac::AnnotationRef::from(a)))
                        .collect();
                    Ok(c.create_asset_from_annotations(
                        &document_id,
                        page.into(),
                        refs,
                        input.into(),
                    ))
                },
                |id| id,
            )
            .await;
        // The plugin says `NotImplemented` when the document's engine can't
        // export appearances; through this host that is always the case.
        created.map_err(|error| match error {
            ViewerError::Kernel { name, code, detail }
                if code.as_deref() == Some("NotImplemented") =>
            {
                ViewerError::Kernel { name, code: Some("unsupported".into()), detail }
            }
            other => other,
        })
    }

    pub async fn update_asset(
        &self,
        id: String,
        patch: StampAssetPatch,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let subject =
                        if patch.clear_subject { Some(None) } else { patch.subject.map(Some) };
                    let patch =
                        AssetPatch { label: patch.label, subject, categories: patch.categories };
                    Ok(c.update_asset(&id, patch))
                },
                |()| (),
            )
            .await
    }

    pub async fn delete_asset(
        &self,
        id: String,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap.call_promise_cancellable(cancel, move |c| Ok(c.delete_asset(&id)), |()| ()).await
    }

    /// Returns the asset's new id (ids carry the library).
    pub async fn move_asset(
        &self,
        id: String,
        to_library_id: String,
        cancel: Arc<CancelToken>,
    ) -> Result<String, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(c.move_asset(&id, &to_library_id)),
                |id| id,
            )
            .await
    }

    pub async fn duplicate_asset(
        &self,
        id: String,
        label: Option<String>,
        cancel: Arc<CancelToken>,
    ) -> Result<String, ViewerError> {
        self.cap
            .call_promise_cancellable(cancel, move |c| Ok(c.duplicate_asset(&id, label)), |id| id)
            .await
    }

    /// Arm `asset_id` for the next tap in `document_id`.
    pub async fn arm_asset(
        &self,
        document_id: String,
        asset_id: String,
        target_width: Option<f64>,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(c.arm_asset(&document_id, &asset_id, target_width)),
                |()| (),
            )
            .await
    }

    /// Place `asset_id` on `page` of `document_id` at `at` (page space).
    #[allow(clippy::too_many_arguments)]
    pub async fn place_asset(
        &self,
        document_id: String,
        asset_id: String,
        page: PageRef,
        at: Point,
        target_width: Option<f64>,
        rotation: Option<f64>,
        cancel: Arc<CancelToken>,
    ) -> Result<AnnotationRef, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let placement = epdf_plugin_annotation::StampPlacement {
                        page: page.into(),
                        at: at.into(),
                        target_width,
                        rotation,
                    };
                    Ok(c.place_asset(&document_id, &asset_id, placement))
                },
                |placed: ac::AnnotationRef| annotation_out(&placed),
            )
            .await?
    }

    /// The same placement on every page (`pages` is `None`) or those listed.
    #[allow(clippy::too_many_arguments)]
    pub async fn place_asset_on_pages(
        &self,
        document_id: String,
        asset_id: String,
        pages: Option<Vec<PageRef>>,
        at: Point,
        target_width: Option<f64>,
        rotation: Option<f64>,
        cancel: Arc<CancelToken>,
    ) -> Result<StampBatchResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let pages = pages.map(|p| p.into_iter().map(Into::into).collect());
                    Ok(c.place_asset_on_pages(
                        &document_id,
                        &asset_id,
                        pages,
                        at.into(),
                        target_width,
                        rotation,
                    ))
                },
                |r: crate::BatchResult| StampBatchResult::try_from(r).map_err(ViewerError::from),
            )
            .await?
    }

    pub async fn disarm(&self, document_id: String) -> Result<(), ViewerError> {
        self.cap.call(move |c| c.disarm(&document_id)).await
    }

    pub async fn armed_asset(
        &self,
        document_id: String,
    ) -> Result<Option<StampAsset>, ViewerError> {
        self.cap.call(move |c| Ok(c.get_armed_asset(&document_id).map(Into::into))).await
    }

    /// Load every library `store` holds. Returns the restored ids. A stored
    /// library that fails to import is skipped.
    pub async fn restore(
        &self,
        store: Arc<dyn StampLibraryStore>,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<String>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(restore_stamp_libraries(c, Rc::new(ForeignStore(store)), None)),
                |ids| ids,
            )
            .await
    }

    /// Write each changed library to `store`, debounced (`None` is 250 ms),
    /// except the ids in `except`. Runs until `stop`.
    pub fn persist(
        &self,
        store: Arc<dyn StampLibraryStore>,
        except: Vec<String>,
        debounce_ms: Option<f64>,
    ) -> Arc<StampPersistence> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.cap.fire(move |c| {
            let stop = persist_stamp_libraries(
                c,
                Rc::new(ForeignStore(store)),
                PersistOptions { except, debounce_ms },
                None,
            );
            slot.fill(Unsubscribe::new(move || stop()));
        });
        Arc::new(StampPersistence {
            stop: Mutex::new(Some(Box::new(move || subscription.cancel()))),
        })
    }

    /// Every hook in one registration.
    pub fn subscribe(&self, listener: Arc<dyn StampEventListener>) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.cap.fire(move |c| {
            let offs = subscribe_all(c, listener);
            slot.fill(Unsubscribe::new(move || offs.iter().for_each(Unsubscribe::call)));
        });
        subscription
    }
}

fn subscribe_all(c: &StampCapability, l: Arc<dyn StampEventListener>) -> Vec<Unsubscribe> {
    let library = |l: Option<&crate::StampLibrary>| l.cloned().map(StampLibrary::from);
    let asset = |a: Option<&crate::StampAsset>| a.cloned().map(StampAsset::from);
    vec![
        c.on_library_changed({
            let l = l.clone();
            move |e| {
                let (library_id, reason) = (e.library_id.clone(), e.reason);
                l.on_event(StampEvent::LibraryChanged { library_id, reason });
                Ok(())
            }
        }),
        c.on_library_created({
            let l = l.clone();
            move |e| {
                let (library_id, library) = (e.library_id.clone(), library(e.library.as_ref()));
                l.on_event(StampEvent::LibraryCreated { library_id, library });
                Ok(())
            }
        }),
        c.on_library_updated({
            let l = l.clone();
            move |e| {
                let (library_id, library) = (e.library_id.clone(), library(e.library.as_ref()));
                l.on_event(StampEvent::LibraryUpdated { library_id, library });
                Ok(())
            }
        }),
        c.on_library_deleted({
            let l = l.clone();
            move |e| {
                l.on_event(StampEvent::LibraryDeleted { library_id: e.library_id.clone() });
                Ok(())
            }
        }),
        c.on_asset_created({
            let l = l.clone();
            move |e| {
                l.on_event(StampEvent::AssetCreated {
                    asset_id: e.asset_id.clone(),
                    library_id: e.library_id.clone(),
                    asset: asset(e.asset.as_ref()),
                });
                Ok(())
            }
        }),
        c.on_asset_updated({
            let l = l.clone();
            move |e| {
                l.on_event(StampEvent::AssetUpdated {
                    asset_id: e.asset_id.clone(),
                    library_id: e.library_id.clone(),
                    asset: asset(e.asset.as_ref()),
                });
                Ok(())
            }
        }),
        c.on_asset_deleted({
            let l = l.clone();
            move |e| {
                l.on_event(StampEvent::AssetDeleted {
                    asset_id: e.asset_id.clone(),
                    library_id: e.library_id.clone(),
                });
                Ok(())
            }
        }),
        c.on_arm_changed(move |e| {
            l.on_event(StampEvent::ArmChanged {
                document_id: e.document_id.clone(),
                asset_id: e.asset_id.clone(),
            });
            Ok(())
        }),
    ]
}
