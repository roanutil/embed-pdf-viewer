//! `contract.ts`: the public render vocabulary. Page rasters for developers:
//! exact-size renders, thumbnails, batches, the deployment's render policy,
//! the settings, and invalidation in both directions (the `invalidate` verb
//! in, `onInvalidated` out). What a view layer needs to paint is the host
//! lens, [`crate::host_contract`].

use epdf_kernel::geometry::PageRef;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::types::PluginErrorInfo;
use serde::ser::SerializeSeq;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{Value, json};

use crate::paint_plan::{FullPageOptions, PrefetchOptions, Quantize, RenderFormat, TilesOptions};
use crate::raster_store::PageImageHandle;

// ── settings ──────────────────────────────────────────────────────────────────

/// `RenderSettings`, and `RenderConfig` (`DeepPartial<RenderSettings>`):
/// the render strategy, what the viewer chooses to spend. A strategy value
/// applies as written under a `continuous` policy, and the advertised
/// lattice wins when there is one. The settings belong to the plugin as
/// registered: `update_settings` changes every document.
///
/// One type serves both: the settings in effect have every member the
/// defaults give ([`render_defaults`]); a config or a change leaves out what
/// it does not set.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderConfig {
    /// Base plane: the pixel budget and render points.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_page: Option<FullPageOptions>,
    /// Tile plane strategy; `Off` disables the plane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tiles: Option<TilesSetting>,
    /// Encode format for both planes; `None` is the engine default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<RenderFormat>,
    /// WebP quality from 0 (smallest) to 1 (best); PNG and BMP ignore it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<f64>,
    /// Diagnostic logging of tile scheduling and fetch outcomes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debug: Option<bool>,
}

/// `RenderSettings`: the settings in effect.
pub type RenderSettings = RenderConfig;

/// `tiles?: TilesOptions | false`.
#[derive(Clone, Debug, PartialEq)]
pub enum TilesSetting {
    Off,
    On(TilesOptions),
}

impl Serialize for TilesSetting {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            TilesSetting::Off => s.serialize_bool(false),
            TilesSetting::On(options) => options.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for TilesSetting {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Flag(bool),
            Options(TilesOptions),
        }
        match Raw::deserialize(d)? {
            Raw::Flag(false) => Ok(TilesSetting::Off),
            Raw::Flag(true) => Ok(TilesSetting::On(TilesOptions::default())),
            Raw::Options(options) => Ok(TilesSetting::On(options)),
        }
    }
}

/// `RENDER_DEFAULTS`: what the render settings are when the app registers
/// none.
pub fn render_defaults() -> RenderSettings {
    RenderConfig {
        full_page: Some(FullPageOptions {
            max_width: Some(640.0),
            quantize: Some(Quantize::Exact),
        }),
        tiles: Some(TilesSetting::On(TilesOptions {
            size: Some(512.0),
            quantize: Some(Quantize::Exact),
            max_scale: Some(128.0),
            bleed: Some(1.0),
            engage_at: None,
            prefetch: Some(PrefetchOptions { margin: Some(0.5), velocity_bias: Some(true) }),
            settle_ms: Some(150.0),
        })),
        format: None,
        quality: None,
        debug: Some(false),
    }
}

/// [`render_defaults`] as the kernel keeps settings: JSON, with the two
/// members the TypeScript spells `undefined` present as undefined.
pub(crate) fn render_defaults_json() -> Value {
    let defaults = render_defaults();
    json!({
        "fullPage": defaults.full_page,
        "tiles": defaults.tiles,
        "format": epdf_kernel::undefined(),
        "quality": epdf_kernel::undefined(),
        "debug": defaults.debug,
    })
}

/// The settings JSON as [`RenderConfig`] reads it: an undefined member is
/// one left out.
pub(crate) fn config_of(settings: &Value) -> Result<RenderConfig, serde_json::Error> {
    fn defined(value: &Value) -> Value {
        match value {
            Value::Object(members) => Value::Object(
                members
                    .iter()
                    .filter(|(_, v)| !epdf_kernel::is_undefined(v))
                    .map(|(k, v)| (k.clone(), defined(v)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }
    serde_json::from_value(defined(settings))
}

// ── pages ─────────────────────────────────────────────────────────────────────

/// A page argument as the plugin's calls take it: `PageRef | number`, the
/// ref or a zero-based index into the document's page list.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PageArg {
    Ref(PageRef),
    Index(f64),
}

impl From<PageRef> for PageArg {
    fn from(page: PageRef) -> Self {
        PageArg::Ref(page)
    }
}

impl From<usize> for PageArg {
    fn from(index: usize) -> Self {
        PageArg::Index(index as f64)
    }
}

impl Serialize for PageArg {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            PageArg::Ref(page) => page.to_json().serialize(s),
            PageArg::Index(index) => index.serialize(s),
        }
    }
}

// ── the raster vocabulary ─────────────────────────────────────────────────────

/// `PageLayerOptions`: what a picture draws beside the page content. An
/// option left out draws what the user may read: the annotations with
/// `doc.annotate.read`, the form fields with `doc.forms.read`, and no form
/// fields when `include_annotations` is `false`. `true` for something the
/// user may not read is refused (`permission-denied`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PageLayerOptions {
    /// Draw the annotations (form fields' widgets excepted).
    pub include_annotations: Option<bool>,
    /// Draw the form fields, each in the state its field shows.
    pub include_form_fields: Option<bool>,
}

/// `PageImage`: an encoded page image.
pub type PageImage = PageImageHandle;

/// `RenderPageOptions`.
#[derive(Clone, Default)]
pub struct RenderPageOptions {
    /// Exact output width in device pixels (height keeps the aspect).
    pub width: Option<f64>,
    /// Device pixels per PDF point; ignored when `width` is given. `None` is 1.
    pub scale: Option<f64>,
    pub layers: PageLayerOptions,
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
    pub layers: PageLayerOptions,
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

/// One failed entry of a batch render: the page as it was given, and why.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PageRenderFailure {
    #[serde(rename = "ref")]
    pub page: PageArg,
    pub error: PluginErrorInfo,
}

/// `BatchResult<PageRender, PageRef | number>`. Render batches skip
/// nothing, so `skipped` is always empty.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RenderBatchResult {
    pub applied: Vec<PageRender>,
    pub skipped: Vec<PageArg>,
    pub failed: Vec<PageRenderFailure>,
}

/// `InvalidateScope`: every pixel-changing fact is one of them.
/// `annotations`: pictures without annotations keep their pixels.
/// `fields`: pictures without form fields keep theirs. `content`: the page
/// itself changed, and every picture of it is stale.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InvalidateScope {
    Content,
    Annotations,
    Fields,
}

/// `InvalidateOptions`: `pages: None` is every page, `scope: None` is
/// `content`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InvalidateOptions {
    pub pages: Option<Vec<PageArg>>,
    pub scope: Option<InvalidateScope>,
}

// ── events ────────────────────────────────────────────────────────────────────

/// `RenderInvalidatedEvent`: pixels changed on these pages, from a confirmed
/// document mutation (own or remote) or an `invalidate` call. `origin` is
/// the event's own `origin` member as it came, and `null` for the verb or
/// an event without one.
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
