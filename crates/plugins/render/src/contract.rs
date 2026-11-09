//! `contract.ts`: the public render vocabulary. Page rasters for developers:
//! exact-size renders, thumbnails, batches, the deployment's render policy,
//! the settings, and invalidation in both directions (the `invalidate` verb
//! in, `onInvalidated` out). What a view layer needs to paint is the host
//! lens, [`crate::host_contract`].

use std::any::Any;
use std::rc::Rc;

use epdf_geometry::{PageRotation, PageTransform, Rect};
use epdf_kernel::geometry::PageRef;
use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::types::PluginErrorInfo;
use serde::ser::SerializeSeq;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{Value, json};

use crate::paint_plan::{
    FullPageSettings, PageViewDemand, PrefetchOptions, Quantize, RenderFormat, TilesSettings,
};
use crate::pictures::PictureReport;
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
    pub full_page: Option<FullPageSettings>,
    /// Tile plane strategy; `Off` disables the plane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tiles: Option<TilesSetting>,
    /// Encode format for both planes; `Some(None)` (`null`) is the engine
    /// default.
    #[serde(default, deserialize_with = "nullable", skip_serializing_if = "Option::is_none")]
    pub format: Option<Option<RenderFormat>>,
    /// WebP quality from 0 (smallest) to 1 (best); PNG and BMP ignore it.
    /// `Some(None)` (`null`) is the engine's.
    #[serde(default, deserialize_with = "nullable", skip_serializing_if = "Option::is_none")]
    pub quality: Option<Option<f64>>,
    /// How long a page waits for its first picture before its state turns
    /// `slow`, in milliseconds: the moment to show a loading indicator
    /// ([`crate::PictureState`]). `0` turns it on at once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slow_after_ms: Option<f64>,
    /// Diagnostic logging of tile scheduling and fetch outcomes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debug: Option<bool>,
    /// The `snapshot` tool: how big its pictures are, and how its box looks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<SnapshotConfig>,
}

/// A member present as `null` (`Some(None)`), apart from one left out
/// (`None`, through `#[serde(default)]`).
pub(crate) fn nullable<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

/// `SnapshotSettings`, and the part of it a config or a change names: the
/// snapshot tool's settings (see [`RenderConfig::snapshot`]).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SnapshotConfig {
    /// A snapshot's pixels per point: 2 is 144 DPI. A picture that would
    /// pass [`SNAPSHOT_MAX_PIXELS`], or the deployment's budget, is made
    /// smaller to fit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
    /// The look of the box you drag, one value: a change replaces it whole,
    /// and a color it leaves out is `null` again.
    #[serde(default, rename = "box", skip_serializing_if = "Option::is_none")]
    pub box_look: Option<SnapshotBoxLook>,
}

/// `SnapshotBoxLook`: the colors of the snapshot tool's box. A color left
/// `None` (`null`) follows the viewer's accent (the fill at 8%).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SnapshotBoxLook {
    #[serde(default)]
    pub fill: Option<String>,
    #[serde(default)]
    pub stroke: Option<String>,
}

/// `SNAPSHOT_MAX_PIXELS`: the most pixels a snapshot's picture has, 16
/// million, safe within a phone's memory.
pub const SNAPSHOT_MAX_PIXELS: f64 = 16_000_000.0;

/// `SNAPSHOT_TOOL`: the tool that takes snapshots, a box dragged on a page
/// or a click for the whole page.
pub const SNAPSHOT_TOOL: &str = "snapshot";

/// `RenderSettings`: the settings in effect.
pub type RenderSettings = RenderConfig;

/// `tiles?: TilesSettings | false`.
#[derive(Clone, Debug, PartialEq)]
pub enum TilesSetting {
    Off,
    On(TilesSettings),
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
            Options(TilesSettings),
        }
        match Raw::deserialize(d)? {
            Raw::Flag(false) => Ok(TilesSetting::Off),
            Raw::Flag(true) => Ok(TilesSetting::On(TilesSettings::default())),
            Raw::Options(options) => Ok(TilesSetting::On(options)),
        }
    }
}

/// `RENDER_DEFAULTS`: what the render settings are when the app registers
/// none.
pub fn render_defaults() -> RenderSettings {
    RenderConfig {
        full_page: Some(FullPageSettings {
            max_width: Some(640.0),
            quantize: Some(Quantize::Exact),
        }),
        tiles: Some(TilesSetting::On(TilesSettings {
            size: Some(512.0),
            quantize: Some(Quantize::Exact),
            max_scale: Some(128.0),
            bleed_px: Some(1.0),
            engage_ratio: None,
            prefetch: Some(PrefetchOptions { margin: Some(0.5), velocity_bias: Some(true) }),
            settle_ms: Some(150.0),
        })),
        format: Some(None),
        quality: Some(None),
        slow_after_ms: Some(400.0),
        debug: Some(false),
        snapshot: Some(SnapshotConfig {
            scale: Some(2.0),
            box_look: Some(SnapshotBoxLook { fill: None, stroke: None }),
        }),
    }
}

/// [`render_defaults`] as the kernel keeps settings: JSON.
pub(crate) fn render_defaults_json() -> Value {
    json!(render_defaults())
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
    /// Exact output width in device pixels (height keeps the aspect of what
    /// is drawn).
    pub width: Option<f64>,
    /// Device pixels per PDF point; ignored when `width` is given. `None` is 1.
    pub scale: Option<f64>,
    /// Draw only this part of the page, in page space: from the page's
    /// top-left, y down, in points. `width` and `scale` then size the part.
    /// Cut at the page's edge; a rect that misses the page is refused
    /// (`invalid-input`).
    pub rect: Option<Rect>,
    /// Turn the picture clockwise. `None` (and 0) is the page as stored.
    pub rotation: Option<PageRotation>,
    pub layers: PageLayerOptions,
    /// Per-call encode overrides; default to the plugin's strategy.
    pub format: Option<RenderFormat>,
    pub quality: Option<f64>,
    pub signal: Option<AbortSignal>,
}

/// `RenderThumbnailOptions`: the page's thumbnail is the whole page at the
/// engine's thumbnail size, so it takes no size.
#[derive(Clone, Default)]
pub struct RenderThumbnailOptions {
    pub layers: PageLayerOptions,
    /// Per-call encode overrides; default to the plugin's strategy.
    pub format: Option<RenderFormat>,
    pub quality: Option<f64>,
    pub signal: Option<AbortSignal>,
}

/// `RenderPagesOptions`: a batch's per-page options and its pool size. A
/// batch renders whole pages: the TypeScript type leaves `rect` out, and
/// one given anyway reaches every page as it would there.
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

// ── snapshots ─────────────────────────────────────────────────────────────────

/// `Snapshot`: part of a page taken as a picture with the
/// [`SNAPSHOT_TOOL`], held until another is taken, the tool changes or it
/// is let go (`clear_snapshot`).
#[derive(Clone)]
pub struct Snapshot {
    /// The page it was taken on.
    pub page: PageRef,
    /// The box, in page space: from the page's top-left, y down, in points.
    pub rect: Rect,
    /// Its picture, as PNG, turned as the page was shown. Rejects when it
    /// can't be rendered, and with `operation-cancelled` once the snapshot is
    /// let go first.
    pub image: Promise<PageImage>,
}

/// `SnapshotChangedEvent`: a snapshot was taken, or let go (`None`).
#[derive(Clone)]
pub struct SnapshotChangedEvent {
    pub snapshot: Option<Snapshot>,
}

// ── another plugin painting the pictures ──────────────────────────────────────

/// `PicturePage`: the page a painter paints, as every framework's page layer
/// knows it.
#[derive(Clone)]
pub struct PicturePage {
    /// The page's address, the same for as long as the page is shown.
    pub page: PageRef,
    /// Its place in the document, from 0.
    pub page_index: u32,
    /// The view that shows it: a Stage's lens id.
    pub view: String,
    /// Its size, zoom and turn on screen, and its device pixels.
    pub transform: PageTransform,
    /// The part of it on screen, read when asked; `None` for a view without
    /// a camera.
    pub get_view_demand: Option<Rc<dyn Fn() -> PageViewDemand>>,
}

/// `PaintedPicture`: a picture a painter is painting.
pub trait PaintedPicture {
    /// The page's geometry changed (a zoom, a turn, a resize): `page` is how
    /// it is now.
    fn update(&self, page: &PicturePage);
    /// The page left its view, or the painter was given back: take the
    /// picture away.
    fn destroy(&self);
}

/// `PicturePainter`: something that paints the pages' pictures instead of
/// the render plugin ([`RenderCapability::hand_over_pictures`]). `host` is
/// the box a page layer lends it, whatever a platform's layer is.
///
/// [`RenderCapability::hand_over_pictures`]: crate::RenderCapability::hand_over_pictures
pub trait PicturePainter {
    /// Whether it paints the pictures of this view; a view it doesn't paint
    /// keeps the render plugin's.
    fn paints(&self, view: &str) -> bool;
    /// Paint `page`'s picture into `host`, an empty box over the page's
    /// content, until `destroy()`. The page is `loading` until
    /// `report.shown()`: call it once the first picture is on screen.
    fn paint(
        &self,
        host: &dyn Any,
        page: &PicturePage,
        report: Rc<dyn PictureReport>,
    ) -> Box<dyn PaintedPicture>;
    /// The painter itself, for a host that handed over its own type.
    fn as_any(&self) -> &dyn Any;
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
