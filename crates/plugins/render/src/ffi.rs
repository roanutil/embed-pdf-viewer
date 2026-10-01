//! The render plugin through uniffi: page rasters as owned pixels, one
//! handle, one event enum. Every method hops to the kernel thread through
//! `KernelBound`.
//!
//! The public lens only: `render_page`, `render_thumbnail`, `render_pages`,
//! the epoch ledger and `invalidate`. The host lens (`render_source`, source
//! keys, view demands and tile plans) drives a tiled viewport the stage
//! plugin owns, and lands with it.
//!
//! A raster resolves to the engine's JSON, which on `epdf-viewer-host` is
//! the native host's raw pixels: `width`, `height`, `stride`, the pixel
//! format as `worker-protocol.ts`'s `PageRaster` spells it (`color: "rgba8"`,
//! `premultipliedAlpha: false`), `source: {kind: "pixels", $byteLength}`,
//! and a `$host` id naming the bytes in the host's table. The host hands
//! [`RenderHandle::new`] a [`PixelSource`] that reads that table (render
//! cannot depend on the host crate), and each export copies the bytes out
//! into an owned [`RenderedImage`]. The table entry lives as long as the
//! raster store's handle and goes with it, through
//! `RenderHost::release_image`. Anything else (an encoded image, a format
//! this does not name, bytes the table no longer holds) is an
//! `invalid-state` error, never a guess.

use std::rc::Rc;
use std::sync::Arc;

use epdf_ffi::{
    CancelToken, KernelBound, KernelPost, PageRef, PluginErrorInfo, Subscription, ViewerError,
};
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{Error, Promise, Unsubscribe};
use serde_json::Value;

use crate::RenderCapability;
use crate::contract::{
    InvalidateOptions, InvalidateScope, RenderPagesOptions, RenderThumbnailOptions,
};
use crate::paint_plan::{FullPageOptions, Quantize};
use crate::raster_store::PageImageHandle;

/// The bytes behind a raster's `$host` id, on the kernel thread.
pub type PixelSource = Rc<dyn Fn(u64) -> Option<Arc<Vec<u8>>>>;

/// How a [`RenderedImage`]'s bytes are laid out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RenderPixelFormat {
    /// Four bytes a pixel in memory order red, green, blue, alpha, with
    /// straight (not premultiplied) alpha. `PageRaster`'s `rgba8`. Opaque
    /// unless the page was rendered without a background.
    Rgba8,
}

/// A page raster the caller owns: the bytes outlive the viewer.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RenderedImage {
    pub width: u32,
    pub height: u32,
    /// Bytes per row. At least `width * 4`; address rows by it.
    pub stride: u32,
    /// Device pixels per PDF point across the page's width: `width` over
    /// the page's width in points.
    pub scale: f64,
    pub format: RenderPixelFormat,
    /// `stride * height` bytes.
    pub pixels: Vec<u8>,
}

/// `RenderPageOptions`, without the encode overrides (`format`, `quality`):
/// a native raster is raw pixels, never encoded.
#[derive(Clone, Copy, Debug, Default, PartialEq, uniffi::Record)]
pub struct RenderPageOptions {
    /// Exact output width in device pixels; the height keeps the aspect.
    #[uniffi(default = None)]
    pub width: Option<f64>,
    /// Device pixels per PDF point; ignored when `width` is given. `None`
    /// is 1.
    #[uniffi(default = None)]
    pub scale: Option<f64>,
    /// Bake annotations into the bitmap; `None` is true.
    #[uniffi(default = None)]
    pub include_annotations: Option<bool>,
}

/// The base plane's render points: the exact demand, or a ladder of widths.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum RenderQuantize {
    Exact,
    Ladder { widths: Vec<f64> },
}

/// `FullPageOptions`: the base plane's pixel budget and render points.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct RenderFullPageOptions {
    /// `None` is 640 device pixels.
    #[uniffi(default = None)]
    pub max_width: Option<f64>,
    /// `None` is `Exact`.
    #[uniffi(default = None)]
    pub quantize: Option<RenderQuantize>,
}

/// `RenderConfig`, without what a native host cannot use yet: the encode
/// format and quality (its rasters are raw pixels) and the tile plane
/// (which the stage plugin drives, and lands with it). The base plane's
/// budget applies to the viewport renders that also land with stage;
/// `render_page` renders at exactly the size asked for.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct RenderConfig {
    #[uniffi(default = None)]
    pub full_page: Option<RenderFullPageOptions>,
    /// Log tile scheduling to stderr.
    #[uniffi(default = None)]
    pub debug: Option<bool>,
}

impl From<RenderConfig> for crate::RenderConfig {
    fn from(config: RenderConfig) -> Self {
        crate::RenderConfig {
            full_page: config.full_page.map(|full_page| FullPageOptions {
                max_width: full_page.max_width,
                quantize: full_page.quantize.map(|quantize| match quantize {
                    RenderQuantize::Exact => Quantize::Exact,
                    RenderQuantize::Ladder { widths } => Quantize::Ladder(widths),
                }),
            }),
            debug: config.debug,
            ..crate::RenderConfig::default()
        }
    }
}

/// One page of a batch.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RenderBatchEntry {
    pub page: PageRef,
    pub image: RenderedImage,
}

/// One page a batch could not render.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RenderBatchFailure {
    pub page: PageRef,
    pub error: PluginErrorInfo,
}

/// A batch, best effort per page, each list in input order.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RenderBatch {
    pub applied: Vec<RenderBatchEntry>,
    pub failed: Vec<RenderBatchFailure>,
}

/// `InvalidateScope`: `Content` (the page itself changed, every raster of
/// it is stale) or `Annotations` (only baked appearances changed).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RenderInvalidateScope {
    Content,
    Annotations,
}

impl From<InvalidateScope> for RenderInvalidateScope {
    fn from(scope: InvalidateScope) -> Self {
        match scope {
            InvalidateScope::Content => RenderInvalidateScope::Content,
            InvalidateScope::Annotations => RenderInvalidateScope::Annotations,
        }
    }
}

impl From<RenderInvalidateScope> for InvalidateScope {
    fn from(scope: RenderInvalidateScope) -> Self {
        match scope {
            RenderInvalidateScope::Content => InvalidateScope::Content,
            RenderInvalidateScope::Annotations => InvalidateScope::Annotations,
        }
    }
}

/// What the plugin announces. `Invalidated` is the plugin's
/// `RenderInvalidatedEvent` without its `ChangeOrigin`, which no foreign
/// caller can act on. `Completed` and `Failed` are a raster resolving or
/// failing (not an abort, not a close); `key` is its raster-store key.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum RenderEvent {
    Invalidated { pages: Vec<PageRef>, scope: RenderInvalidateScope },
    Completed { page: PageRef, key: String },
    Failed { page: PageRef, key: String, error: PluginErrorInfo },
}

#[uniffi::export(with_foreign)]
pub trait RenderEventListener: Send + Sync {
    fn on_event(&self, event: RenderEvent);
    /// The document closed: no event follows. Called once, and at once for a
    /// subscription made after the close.
    fn on_end(&self);
}

// ── conversion ────────────────────────────────────────────────────────────────

fn invalid_state(message: String) -> Error {
    Error::named("InvalidState", format!("[render] {message}")).with("code", "invalid-state")
}

fn invalid_input(message: String) -> Error {
    plugin_error(code::INVALID_INPUT, "render", message, None, None)
}

fn error_info(info: epdf_kernel::types::PluginErrorInfo) -> PluginErrorInfo {
    PluginErrorInfo { code: info.code, message: info.message, capability: info.capability }
}

/// A finite, positive size or scale, or `invalid-input`.
fn positive(value: Option<f64>, what: &str) -> Result<Option<f64>, Error> {
    match value {
        Some(v) if !(v.is_finite() && v > 0.0) => {
            Err(invalid_input(format!("{what} must be a positive number, not {v}")))
        }
        _ => Ok(value),
    }
}

impl RenderPageOptions {
    fn validated(self) -> Result<crate::RenderPageOptions, Error> {
        Ok(crate::RenderPageOptions {
            width: positive(self.width, "width")?,
            scale: positive(self.scale, "scale")?,
            include_annotations: self.include_annotations,
            ..crate::RenderPageOptions::default()
        })
    }
}

/// A raster read out of its handle on the kernel thread, the bytes still
/// shared with the host's table. Cheap to clone; [`Raster::owned`] makes the
/// one copy the caller keeps.
#[derive(Clone)]
struct Raster {
    width: u32,
    height: u32,
    stride: u32,
    scale: f64,
    format: RenderPixelFormat,
    bytes: Arc<Vec<u8>>,
}

impl Raster {
    fn read(
        handle: &PageImageHandle,
        page_width: f64,
        pixels: &PixelSource,
    ) -> Result<Self, Error> {
        let json = handle.json();
        let whole = |key: &str| {
            json.get(key)
                .and_then(Value::as_u64)
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| invalid_state(format!("a raster's {key} is not a u32: {json}")))
        };
        let kind = json.pointer("/source/kind").and_then(Value::as_str);
        if kind != Some("pixels") {
            return Err(invalid_state(format!(
                "the engine answered an image this export cannot read (source kind {kind:?}); \
                 it reads raw pixels"
            )));
        }
        let format = match (
            json.get("color").and_then(Value::as_str),
            json.get("premultipliedAlpha").and_then(Value::as_bool),
        ) {
            (Some("rgba8"), Some(false)) => RenderPixelFormat::Rgba8,
            (color, premultiplied) => {
                return Err(invalid_state(format!(
                    "no pixel format for color {color:?} with premultipliedAlpha {premultiplied:?}"
                )));
            }
        };
        let (width, height, stride) = (whole("width")?, whole("height")?, whole("stride")?);
        let id = json
            .get("$host")
            .and_then(Value::as_u64)
            .ok_or_else(|| invalid_state(format!("a raster without its $host id: {json}")))?;
        let bytes = pixels(id)
            .ok_or_else(|| invalid_state(format!("the host holds no pixels for {id}")))?;
        let needed = u64::from(stride) * u64::from(height);
        if u64::from(stride) < u64::from(width) * 4 || (bytes.len() as u64) < needed {
            return Err(invalid_state(format!(
                "a {width}x{height} raster with stride {stride} cannot live in {} bytes",
                bytes.len()
            )));
        }
        Ok(Raster { width, height, stride, scale: f64::from(width) / page_width, format, bytes })
    }

    fn owned(self) -> RenderedImage {
        let length = self.stride as usize * self.height as usize;
        RenderedImage {
            width: self.width,
            height: self.height,
            stride: self.stride,
            scale: self.scale,
            format: self.format,
            pixels: self.bytes[..length].to_vec(),
        }
    }
}

/// A page's raster, read out once its handle resolves.
fn rendered(
    cap: &RenderCapability,
    pixels: &PixelSource,
    page: PageRef,
    image: Promise<PageImageHandle>,
) -> Result<Promise<Raster>, Error> {
    let page_width = cap.page_width(page.into())?;
    let pixels = pixels.clone();
    Ok(Promise::spawn(async move { Raster::read(&image.await?, page_width, &pixels) }))
}

/// A signal for the plugin's options, and the abort to hang on the promise
/// the export parks, so a cancelled `CancelToken` reaches the plugin's own
/// cancellation path (the raster store's consumer refcount, then the engine
/// call).
fn abortable() -> (AbortSignal, impl Fn(Option<Value>) + 'static) {
    let signal = AbortSignal::new();
    let aborter = signal.clone();
    (signal, move |reason| aborter.abort(Error::abort(reason)))
}

/// What lives in the kernel thread's slab for one handle.
struct Bound {
    cap: RenderCapability,
    pixels: PixelSource,
}

/// The capability, if its document is still open. The plugin keeps
/// answering from its last state after a close; a foreign handle that
/// outlived the close gets `instance-closed` instead.
fn open(bound: &Bound) -> Result<&RenderCapability, Error> {
    if bound.cap.lifetime().aborted() {
        return Err(plugin_error(
            code::INSTANCE_CLOSED,
            "render",
            "the document this capability belongs to is closed",
            None,
            None,
        ));
    }
    Ok(&bound.cap)
}

#[derive(uniffi::Object)]
pub struct RenderHandle {
    bound: KernelBound<Bound>,
    post: KernelPost,
}

impl RenderHandle {
    /// On the kernel thread. `pixels` answers a raster's `$host` id with its
    /// bytes.
    pub fn new(cap: RenderCapability, post: KernelPost, pixels: PixelSource) -> Arc<Self> {
        Arc::new(RenderHandle {
            bound: KernelBound::new(Bound { cap, pixels }, post.clone()),
            post,
        })
    }
}

#[uniffi::export]
impl RenderHandle {
    /// Whether page rasters would be served to this session (`doc.render`).
    pub async fn can_render(&self) -> Result<bool, ViewerError> {
        self.bound.call(|b| Ok(open(b)?.can_render())).await
    }

    /// Render one page at exactly the size asked for. Rejects
    /// `invalid-input` for a width or scale that is not a positive number,
    /// `not-found` for a page not in this document, `permission-denied`
    /// without `doc.render`, and `operation-cancelled` when cancelled.
    pub async fn render_page(
        &self,
        page: PageRef,
        options: RenderPageOptions,
        cancel: Arc<CancelToken>,
    ) -> Result<RenderedImage, ViewerError> {
        self.bound
            .call_promise_cancellable(
                cancel,
                move |b| {
                    let cap = open(b)?;
                    let mut options = options.validated()?;
                    let (signal, abort) = abortable();
                    options.signal = Some(signal);
                    let image = cap.render_page(page.into(), options);
                    Ok(rendered(cap, &b.pixels, page, image)?.with_abort(abort))
                },
                Raster::owned,
            )
            .await
    }

    /// A small raster `max_width` device pixels wide.
    pub async fn render_thumbnail(
        &self,
        page: PageRef,
        max_width: f64,
        include_annotations: Option<bool>,
        cancel: Arc<CancelToken>,
    ) -> Result<RenderedImage, ViewerError> {
        self.bound
            .call_promise_cancellable(
                cancel,
                move |b| {
                    let cap = open(b)?;
                    let max_width = positive(Some(max_width), "max_width")?.unwrap_or_default();
                    let (signal, abort) = abortable();
                    let options = RenderThumbnailOptions {
                        max_width,
                        include_annotations,
                        signal: Some(signal),
                    };
                    let image = cap.render_thumbnail(page.into(), options);
                    Ok(rendered(cap, &b.pixels, page, image)?.with_abort(abort))
                },
                Raster::owned,
            )
            .await
    }

    /// Render several pages, `concurrency` at a time (`None` is 4), best
    /// effort per page: a page that fails is in `failed` with its error.
    /// Cancelling rejects the whole batch with `operation-cancelled`.
    pub async fn render_pages(
        &self,
        pages: Vec<PageRef>,
        options: RenderPageOptions,
        concurrency: Option<u32>,
        cancel: Arc<CancelToken>,
    ) -> Result<RenderBatch, ViewerError> {
        self.bound
            .call_promise_cancellable(
                cancel,
                move |b| {
                    let cap = open(b)?;
                    let mut page = options.validated()?;
                    let (signal, abort) = abortable();
                    page.signal = Some(signal.clone());
                    let batch = cap.render_pages(
                        pages.into_iter().map(Into::into).collect(),
                        RenderPagesOptions { page, concurrency: concurrency.map(f64::from) },
                    );
                    let (cap, pixels) = (cap.clone(), b.pixels.clone());
                    Ok(Promise::spawn(async move {
                        let batch = batch.await?;
                        if signal.aborted() {
                            return Err(plugin_error(
                                code::OPERATION_CANCELLED,
                                "render",
                                "render.renderPages was cancelled",
                                None,
                                None,
                            ));
                        }
                        let mut applied = Vec::with_capacity(batch.applied.len());
                        for entry in batch.applied {
                            let page = PageRef::from(entry.page);
                            let width = cap.page_width(entry.page)?;
                            applied.push((page, Raster::read(&entry.image, width, &pixels)?));
                        }
                        let failed = batch
                            .failed
                            .into_iter()
                            .map(|failure| RenderBatchFailure {
                                page: failure.page.into(),
                                error: error_info(failure.error),
                            })
                            .collect::<Vec<_>>();
                        Ok((applied, failed))
                    })
                    .with_abort(abort))
                },
                |(applied, failed): (Vec<(PageRef, Raster)>, Vec<RenderBatchFailure>)| {
                    RenderBatch {
                        applied: applied
                            .into_iter()
                            .map(|(page, raster)| RenderBatchEntry { page, image: raster.owned() })
                            .collect(),
                        failed,
                    }
                },
            )
            .await
    }

    /// The version of the raster these options would produce. It goes up
    /// when the page's pixels change (see `invalidate` and
    /// `RenderEvent::Invalidated`). `include_annotations: None` is true.
    pub async fn render_epoch(
        &self,
        page: PageRef,
        include_annotations: Option<bool>,
    ) -> Result<u64, ViewerError> {
        self.bound
            .call(move |b| {
                let epoch =
                    open(b)?.get_render_epoch(page.into(), include_annotations.unwrap_or(true));
                if !(epoch.is_finite() && epoch >= 0.0 && epoch.fract() == 0.0) {
                    return Err(invalid_state(format!("an epoch that is not a count: {epoch}")));
                }
                Ok(epoch as u64)
            })
            .await
    }

    /// Declare that page pixels changed: `pages: None` is every page,
    /// `scope: None` is `Content`.
    pub async fn invalidate(
        &self,
        pages: Option<Vec<PageRef>>,
        scope: Option<RenderInvalidateScope>,
    ) -> Result<(), ViewerError> {
        self.bound
            .call(move |b| {
                open(b)?.invalidate(InvalidateOptions {
                    pages: pages.map(|pages| pages.into_iter().map(Into::into).collect()),
                    scope: scope.map(Into::into),
                })
            })
            .await
    }

    /// The listener is registered on the kernel thread after this returns;
    /// a cancel before then still takes effect. It gets `on_end` when the
    /// document closes (at once if it already has).
    pub fn subscribe(&self, listener: Arc<dyn RenderEventListener>) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.bound.fire(move |b| {
            let cap = &b.cap;
            let ending = listener.clone();
            slot.fill_until_end(
                cap.lifetime(),
                || {
                    let offs: Vec<Unsubscribe> = vec![
                        {
                            let listener = listener.clone();
                            cap.on_invalidated(move |e| {
                                listener.on_event(RenderEvent::Invalidated {
                                    pages: e.pages.iter().copied().map(PageRef::from).collect(),
                                    scope: e.scope.into(),
                                });
                                Ok(())
                            })
                        },
                        {
                            let listener = listener.clone();
                            cap.on_render_completed(move |e| {
                                listener.on_event(RenderEvent::Completed {
                                    page: e.page.into(),
                                    key: e.key.clone(),
                                });
                                Ok(())
                            })
                        },
                        {
                            let listener = listener.clone();
                            cap.on_render_failed(move |e| {
                                listener.on_event(RenderEvent::Failed {
                                    page: e.page.into(),
                                    key: e.key.clone(),
                                    error: error_info(e.error.clone()),
                                });
                                Ok(())
                            })
                        },
                    ];
                    Unsubscribe::new(move || offs.iter().for_each(Unsubscribe::call))
                },
                move || ending.on_end(),
            );
        });
        subscription
    }
}
