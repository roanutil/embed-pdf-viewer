//! Turning a page and a request into pixels.
//!
//! G4. The Rust side of `PageRenderReader.render` plus the bitmap half of
//! `rasterize`. The geometry it composes lives in [`super::geometry`].
//!
//! `pages.render`, and the internal entry point G15 consumes for the encoded
//! render kinds. Encoding stays a host concern: this produces RGBA and nothing
//! decides here whether that becomes a PNG.

use serde::{Deserialize, Serialize};

use super::geometry::{
    RenderBackground, RenderTarget, RenderViewport, device_size, page_rect_of_display_rect,
    pdf_rect_to_display_rect, render_matrix, resolve_target,
};
use crate::access::PageAccess;
use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};
use crate::geom::{PdfRect, PdfRotation, normalize_pdf_rect};

/// What a picture is for (`PageFlattenUsage` as `PageRenderOptions.usage`
/// takes it): the screen, the default, or paper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RenderUsage {
    Display,
    Print,
    /// Any other spelling: read, so that [`check_render_usage`] refuses it
    /// naming the `usage` (`InvalidArg`), as `checkRenderUsage` does, rather
    /// than the request failing to parse.
    #[serde(other)]
    Unknown,
}

/// What to draw and how big (`PageRenderOptions<C>`). `R` is the target
/// region's space, as in [`RenderTarget`]: the renderer takes [`PdfRect`];
/// the wire sends `PageRect`, which the boundary converts.
///
/// Every option is optional, as on the wire: whether a print picture was
/// given a `target`, `rotation` or `background` is what
/// [`check_render_usage`] reads, and a left-out layer option takes
/// [`resolve_page_layers`]' default.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RenderOptions<R = PdfRect> {
    /// The whole page when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<RenderTarget<R>>,
    /// Scale 1 when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub viewport: Option<RenderViewport>,
    /// Upright when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation: Option<PdfRotation>,
    /// White when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<RenderBackground>,
    /// Draw the page's annotations, from their appearances.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_annotations: Option<bool>,
    /// Draw the form fields' widgets too. Absent, `false` when
    /// `include_annotations` is `false`, otherwise `true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_form_fields: Option<bool>,
    /// The screen when absent. A print picture follows the print rules, and
    /// takes a `dpi` viewport and no `target`, `rotation` or `background`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<RenderUsage>,
    /// Output-pixel budget. The request is refused BEFORE anything is
    /// allocated when the computed size exceeds it.
    ///
    /// PDF page space is effectively unbounded, so a width-bounded request can
    /// still explode vertically on a degenerate page: a 1 by 14,400 point page
    /// at width 1,000 is 14.4 million pixels. The guard therefore lives where
    /// the allocation happens rather than on the width.
    ///
    /// `None` is unbudgeted, which is the local default: exactness is the
    /// local product's promise. A server render carries its deployment's
    /// policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_pixels: Option<u64>,
}

impl<R> RenderOptions<R> {
    /// The same options with the target's region converted.
    pub fn try_map_target<S, E>(
        self,
        convert: impl FnOnce(R) -> Result<S, E>,
    ) -> Result<RenderOptions<S>, E> {
        Ok(RenderOptions {
            target: match self.target {
                None => None,
                Some(RenderTarget::Page) => Some(RenderTarget::Page),
                Some(RenderTarget::Rect { rect }) => {
                    Some(RenderTarget::Rect { rect: convert(rect)? })
                }
            },
            viewport: self.viewport,
            rotation: self.rotation,
            background: self.background,
            include_annotations: self.include_annotations,
            include_form_fields: self.include_form_fields,
            usage: self.usage,
            max_output_pixels: self.max_output_pixels,
        })
    }
}

impl<R> Default for RenderOptions<R> {
    fn default() -> Self {
        Self {
            target: None,
            viewport: None,
            rotation: None,
            background: None,
            include_annotations: None,
            include_form_fields: None,
            usage: None,
            max_output_pixels: None,
        }
    }
}

/// `InvalidArg` naming `field` in `details.field`.
fn invalid(field: &str, message: impl Into<String>) -> EngineError {
    EngineError::with_message(EngineErrorCode::InvalidArg, message)
        .with_details(serde_json::json!({ "field": field }))
}

/// Check what a render is for (`checkRenderUsage(options, null)`, as the
/// worker checks it). A print picture takes a `{kind: 'dpi', dpi}` viewport
/// with a DPI of 150 or 300, and no `target`, `rotation` or `background`:
/// it is the whole page, upright, on white. A `dpi` viewport is for print
/// pictures only. Anything else is `InvalidArg`, naming the option in
/// `details.field`.
///
/// Who may print, at what DPI (`doc.print`, `doc.print.high`), is the
/// engine's caller's check, made before a render reaches this host.
pub fn check_render_usage<R>(options: &RenderOptions<R>) -> Result<(), EngineError> {
    let is_dpi = matches!(options.viewport, Some(RenderViewport::Dpi { .. }));
    match options.usage {
        None | Some(RenderUsage::Display) => {
            if is_dpi {
                return Err(invalid(
                    "viewport",
                    "a dpi viewport is only for a print picture (usage: \"print\")",
                ));
            }
            return Ok(());
        }
        Some(RenderUsage::Unknown) => {
            return Err(invalid("usage", "usage must be \"display\" or \"print\""));
        }
        Some(RenderUsage::Print) => {}
    }
    if !matches!(options.viewport, Some(RenderViewport::Dpi { dpi }) if dpi == 150.0 || dpi == 300.0)
    {
        return Err(invalid(
            "viewport",
            "a print picture needs viewport: { kind: 'dpi', dpi: 150 or 300 }",
        ));
    }
    for (field, given) in [
        ("target", options.target.is_some()),
        ("rotation", options.rotation.is_some()),
        ("background", options.background.is_some()),
    ] {
        if given {
            return Err(invalid(
                field,
                format!(
                    "a print picture takes no {field}: it is the whole page, upright, on white"
                ),
            ));
        }
    }
    Ok(())
}

/// What a caller may read, for [`resolve_page_layers`] (`PageLayerRights`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageLayerRights {
    /// `doc.annotate.read`.
    pub annotations: bool,
    /// `doc.forms.read`.
    pub form_fields: bool,
}

impl PageLayerRights {
    /// Everything: what the worker draws with (`{annotations: true,
    /// formFields: true}`), since who may see what is checked before a
    /// render gets here.
    pub const ALL: Self = Self { annotations: true, form_fields: true };
}

/// What a page picture draws beside the page content (`PageRenderLayers`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageRenderLayers {
    pub include_annotations: bool,
    pub include_form_fields: bool,
}

/// What a page picture draws (`resolvePageLayers`): the options as passed,
/// and for each one left out, what the caller may read. A left-out
/// `include_form_fields` is `false` when `include_annotations` is `false`.
/// `true` for something the caller may not read is refused
/// (`PermissionDenied`, naming the option).
pub fn resolve_page_layers(
    include_annotations: Option<bool>,
    include_form_fields: Option<bool>,
    may: PageLayerRights,
) -> Result<PageRenderLayers, EngineError> {
    if include_annotations == Some(true) && !may.annotations {
        return Err(EngineError::permission_denied(
            "doc.annotate.read",
            Some("includeAnnotations"),
            None,
        ));
    }
    if include_form_fields == Some(true) && !may.form_fields {
        return Err(EngineError::permission_denied(
            "doc.forms.read",
            Some("includeFormFields"),
            None,
        ));
    }
    Ok(PageRenderLayers {
        include_annotations: include_annotations.unwrap_or(may.annotations),
        include_form_fields: include_form_fields.unwrap_or(if include_annotations == Some(false) {
            false
        } else {
            may.form_fields
        }),
    })
}

/// How long a render works before the cancel token is checked again, in
/// milliseconds (`DEFAULT_SLICE_MS`).
pub const DEFAULT_SLICE_MS: u32 = 8;

/// Raw renderer output. RGBA8, straight alpha.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageRaster {
    pub width: u32,
    pub height: u32,
    /// Bytes per row. May exceed `width * 4`, and rows must be addressed by it.
    pub stride: u32,
    pub rgba: Vec<u8>,
}

impl PageRaster {
    /// The pixel at `(x, y)` as `(r, g, b, a)`, or `None` when out of bounds.
    ///
    /// Addresses by stride rather than by `width * 4`, which is the mistake
    /// this method exists to stop a caller making.
    pub fn pixel(&self, x: u32, y: u32) -> Option<(u8, u8, u8, u8)> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let offset = (y as usize) * (self.stride as usize) + (x as usize) * 4;
        let px = self.rgba.get(offset..offset + 4)?;
        Some((px[0], px[1], px[2], px[3]))
    }
}

/// The page's display box in PDF user space (`readPageBox`): the box
/// PDFium's display matrix maps to the page's pixels, the crop box of a page
/// loaded normalized. A rect in PDF user space, as annotation rects and
/// render targets are, is placed relative to it.
pub fn read_page_box(page: &pdfium::PageView<'_>) -> PdfRect {
    match page.bounding_box() {
        Some(r) => normalize_pdf_rect(PdfRect {
            left: f64::from(r.left),
            bottom: f64::from(r.bottom),
            right: f64::from(r.right),
            top: f64::from(r.top),
        }),
        None => PdfRect {
            left: 0.0,
            bottom: 0.0,
            right: f64::from(page.width()),
            top: f64::from(page.height()),
        },
    }
}

/// A page render: its pixels, and the area of the page they show, in PDF
/// space (`{raster, area}` from `PageRenderReader.render`).
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedPage {
    pub raster: PageRaster,
    pub area: PdfRect,
}

/// Render a page region.
///
/// In slices of [`DEFAULT_SLICE_MS`], the cancel token checked between two
/// of them ([`render_page_area_in_slices`]).
pub fn render_page(
    page: &dyn PageAccess,
    options: &RenderOptions,
    cancel: &CancelToken,
) -> Result<PageRaster, EngineError> {
    render_page_area(page, options, cancel).map(|rendered| rendered.raster)
}

/// [`render_page`], with the area of the page the pixels show: the resolved
/// target, in PDF space. `pages.render` answers it, in page space.
pub fn render_page_area(
    page: &dyn PageAccess,
    options: &RenderOptions,
    cancel: &CancelToken,
) -> Result<RenderedPage, EngineError> {
    render_page_area_in_slices(page, options, cancel, DEFAULT_SLICE_MS)
}

/// [`render_page_area`] in slices of `slice_ms` (0 pauses at every chance),
/// as `PageRenderReader.render` draws: PDFium works for a slice, and the
/// cancel token is checked before the next, so a cancelled render stops at
/// the next slice. However finely sliced, the pixels are those of a render
/// in one slice. `Host::call` is synchronous, so nothing else runs between
/// slices; the token is the observable part (its owner thread or a server
/// can set it while the render runs).
pub fn render_page_area_in_slices(
    page: &dyn PageAccess,
    options: &RenderOptions,
    cancel: &CancelToken,
    slice_ms: u32,
) -> Result<RenderedPage, EngineError> {
    cancel.check()?;
    // The worker draws what it's asked: who may see or print what is checked
    // before a render gets here, so a left-out option draws everything. A
    // print picture is the whole page, upright, sized for its paper by its
    // `dpi` viewport.
    check_render_usage(options)?;
    let print = options.usage == Some(RenderUsage::Print);
    let view = page.view();
    let page_box = read_page_box(view);

    let rect = resolve_target(options.target.unwrap_or(RenderTarget::Page), page_box)?;
    let rotation = options.rotation.unwrap_or(PdfRotation::None);
    let viewport = options.viewport.unwrap_or_default();
    let layers = resolve_page_layers(
        options.include_annotations,
        options.include_form_fields,
        PageLayerRights::ALL,
    )?;
    let flags = pdfium::RenderFlags {
        annotations: layers.include_annotations,
        widgets: layers.include_form_fields,
        printing: print,
    };

    let display_rect = pdf_rect_to_display_rect(rect, page_box);
    let (width, height) = device_size(display_rect, rotation, viewport)?;

    if let Some(budget) = options.max_output_pixels {
        let pixels = u64::from(width) * u64::from(height);
        if pixels > budget {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!(
                    "render output {width}x{height} exceeds the {budget}-pixel budget; \
                     request a smaller width"
                ),
            ));
        }
    }

    // The page's display matrix already turned PDF space into page space
    // (the display rect), so this is the matrix a page transform reports.
    let matrix = render_matrix(page_rect_of_display_rect(display_rect), rotation, width, height);
    cancel.check()?;
    let bitmap = view
        .render_rgba_with_matrix_in_slices(
            width,
            height,
            matrix.to_array(),
            options.background != Some(RenderBackground::Transparent),
            flags,
            slice_ms,
            || cancel.check(),
        )
        .map_err(|e| match e {
            pdfium::SlicedRender::Stopped(e) => e,
            pdfium::SlicedRender::Pdfium(e) => {
                EngineError::with_message(EngineErrorCode::RuntimeUnavailable, e.to_string())
            }
        })?;
    // A token set during the last slice is answered rather than a bitmap
    // nobody wants.
    cancel.check()?;

    Ok(RenderedPage {
        raster: PageRaster {
            width: bitmap.width,
            height: bitmap.height,
            stride: bitmap.stride,
            // Already RGBA: the render asked PDFium for reversed byte order,
            // so there is no pass over the pixels here.
            rgba: bitmap.bgra,
        },
        area: rect,
    })
}
