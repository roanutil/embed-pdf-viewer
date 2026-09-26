//! `contract.ts`: the public render vocabulary. Page rasters for developers:
//! exact-size renders, thumbnails, batches, the deployment's render policy,
//! and invalidation in both directions (the `invalidate` verb in,
//! `onInvalidated` out). What a view layer needs to paint is the host lens,
//! [`crate::host_contract`].

use epdf_kernel::geometry::PageRef;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::types::PluginErrorInfo;
use serde::ser::SerializeSeq;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;

use crate::paint_plan::{FullPageOptions, RenderFormat, TilesSetting};
use crate::raster_store::PageImageHandle;

// ── configuration ─────────────────────────────────────────────────────────────

/// `RenderConfig`: the render strategy, what the viewer chooses to spend. A
/// strategy value applies as written under a `continuous` policy, and the
/// advertised lattice wins when there is one.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderConfig {
    /// Base plane: the pixel budget and render points.
    #[serde(default)]
    pub full_page: Option<FullPageOptions>,
    /// Tile plane strategy; `Off` disables the plane.
    #[serde(default)]
    pub tiles: Option<TilesSetting>,
    /// Encode format for both planes; `None` is the engine default.
    #[serde(default)]
    pub format: Option<RenderFormat>,
    /// Encoder quality (webp/png).
    #[serde(default)]
    pub quality: Option<f64>,
    /// Diagnostic logging of tile scheduling (`console.debug`).
    #[serde(default)]
    pub debug: Option<bool>,
}

// ── the raster vocabulary ─────────────────────────────────────────────────────

/// `PageImage`: an encoded page image.
pub type PageImage = PageImageHandle;

/// `RenderPageOptions`.
#[derive(Clone, Default)]
pub struct RenderPageOptions {
    /// Exact output width in device pixels (height keeps the aspect).
    pub width: Option<f64>,
    /// Device pixels per PDF point; ignored when `width` is given. `None` is 1.
    pub scale: Option<f64>,
    /// Bake annotations into the bitmap; `None` is true.
    pub include_annotations: Option<bool>,
    /// Per-call encode overrides; default to the plugin's strategy.
    pub format: Option<RenderFormat>,
    pub quality: Option<f64>,
    pub signal: Option<AbortSignal>,
}

/// `RenderThumbnailOptions`.
#[derive(Clone, Default)]
pub struct RenderThumbnailOptions {
    /// The thumbnail's width in device pixels.
    pub max_width: f64,
    /// Bake annotations; `None` is true.
    pub include_annotations: Option<bool>,
    pub signal: Option<AbortSignal>,
}

/// `RenderPagesOptions`: a batch's per-page options and its pool size.
#[derive(Clone, Default)]
pub struct RenderPagesOptions {
    pub page: RenderPageOptions,
    /// Renders in flight at once; `None` is 4, and anything below 1 is 1.
    pub concurrency: Option<f64>,
}

/// `PageRender`: one entry of a batch render.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PageRender {
    #[serde(serialize_with = "ser_page")]
    pub page: PageRef,
    pub image: PageImage,
}

/// One failed entry of a batch render.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PageRenderFailure {
    #[serde(rename = "ref", serialize_with = "ser_page")]
    pub page: PageRef,
    pub error: PluginErrorInfo,
}

/// `BatchResult<PageRender, PageRef>`. Render batches skip nothing, so
/// `skipped` is always empty.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RenderBatchResult {
    pub applied: Vec<PageRender>,
    #[serde(serialize_with = "ser_pages")]
    pub skipped: Vec<PageRef>,
    pub failed: Vec<PageRenderFailure>,
}

/// `InvalidateScope`: `content` (the page itself changed, everything is
/// stale) or `annotations` (only baked appearances changed).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InvalidateScope {
    Content,
    Annotations,
}

/// `InvalidateOptions`: `pages: None` is every page, `scope: None` is
/// `content`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InvalidateOptions {
    pub pages: Option<Vec<PageRef>>,
    pub scope: Option<InvalidateScope>,
}

// ── events ────────────────────────────────────────────────────────────────────

/// `RenderInvalidatedEvent`: pixels changed on these pages, from a confirmed
/// document mutation (own or remote) or an `invalidate` call. `origin` is a
/// `ChangeOrigin`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RenderInvalidatedEvent {
    #[serde(serialize_with = "ser_pages")]
    pub pages: Vec<PageRef>,
    pub scope: InvalidateScope,
    pub origin: Value,
}

pub(crate) fn ser_page<S: Serializer>(page: &PageRef, s: S) -> Result<S::Ok, S::Error> {
    page.to_json().serialize(s)
}

pub(crate) fn ser_pages<S: Serializer>(pages: &[PageRef], s: S) -> Result<S::Ok, S::Error> {
    let mut seq = s.serialize_seq(Some(pages.len()))?;
    for page in pages {
        seq.serialize_element(&page.to_json())?;
    }
    seq.end()
}
