//! The one place a PDF user-space region becomes a device raster.
//!
//! G4. A transcription of the pure half of
//! `services/src/features/render/deviceRaster.ts` and of engine-core's
//! `renderMatrix` (`geometry/pageTransform.ts`). Every rasterizer composes
//! these three pieces and supplies only its own draw call: the geometry is a
//! pure affine, and the bitmap lifecycle lives next door in [`super::raster`].
//!
//! Pure, so it is pinned to the TypeScript by
//! `tooling/engine-conformance/vectors/render/geometry.json` rather than by a
//! fixture. A matrix that is subtly wrong produces a picture that is subtly
//! wrong, which is the hardest kind of divergence to notice by looking.

use serde::{Deserialize, Serialize};

use crate::error::{EngineError, EngineErrorCode};
use crate::geom::{PageRect, PdfRect, PdfRotation};

/// A 2D affine: the same six numbers as CSS `matrix()` and `FS_MATRIX`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Mat2D {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Mat2D {
    /// The six numbers in the order every consumer writes them.
    pub fn to_array(self) -> [f64; 6] {
        [self.a, self.b, self.c, self.d, self.e, self.f]
    }
}

/// How much of the page to draw, and at what size.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RenderViewport {
    /// One PDF user-space unit becomes `scale` device pixels. A caller that
    /// cares about device pixel ratio folds it into this. Optional on the
    /// wire, and 1 when absent (`scale?: number`, `viewport.scale ?? 1`).
    Scale {
        #[serde(default = "unit_scale")]
        scale: f64,
    },
    /// An exact output width. Height preserves the region's aspect ratio
    /// after rotation.
    Width { width: f64 },
    /// A print picture's size (`usage: 'print'` only): `dpi` dots per inch
    /// of the paper the page prints on, A4 or Letter ([`print_scale`]). The
    /// wire's `PrintDpi` is 150 or 300; any other number is read, and
    /// refused by `check_render_usage` naming the `viewport`, as
    /// `checkRenderUsage` refuses it; so is a `dpi` that is missing or not
    /// a number, read as NaN.
    Dpi {
        #[serde(default = "no_dpi", deserialize_with = "dpi_or_nan")]
        dpi: f64,
    },
}

/// A `dpi` viewport's `dpi` when it names none: no DPI a print takes.
fn no_dpi() -> f64 {
    f64::NAN
}

/// A `dpi` of any JSON type: a number as itself, anything else NaN.
fn dpi_or_nan<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(serde_json::Value::deserialize(deserializer)?.as_f64().unwrap_or(f64::NAN))
}

/// The scale a `scale` viewport has when it names none.
fn unit_scale() -> f64 {
    1.0
}

impl Default for RenderViewport {
    fn default() -> Self {
        Self::Scale { scale: 1.0 }
    }
}

/// What the untouched pixels are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RenderBackground {
    /// Opaque white, which is what a viewer shows for a page with no
    /// background of its own.
    #[default]
    White,
    Transparent,
}

/// The paper a print picture is sized for, in points: the larger of A4
/// (595 by 842) and Letter (612 by 792) on each side (`PRINT_PAPER`).
pub const PRINT_PAPER_LONG: f64 = 842.0;
/// The short side of [`PRINT_PAPER_LONG`]'s paper.
pub const PRINT_PAPER_SHORT: f64 = 612.0;

/// The scale (pixels per point) of a print picture of a page `width` by
/// `height` points big at `dpi` dots per inch of paper (`printScale`). A
/// page that fits on the paper is drawn at its own size at that DPI; a
/// bigger one gets no more pixels than it needs shrunk to fit, since the
/// printer would throw the rest away. Portrait or landscape doesn't matter.
pub fn print_scale(width: f64, height: f64, dpi: f64) -> f64 {
    let long = width.max(height);
    let short = width.min(height);
    let fit = 1f64.min(PRINT_PAPER_LONG / long).min(PRINT_PAPER_SHORT / short);
    (dpi / 72.0) * fit
}

/// Device pixel size for a region under a rotation and viewport
/// (`renderSize`). A `dpi` viewport sizes a print picture for its paper
/// ([`print_scale`]).
///
/// Width swaps with height on a quarter turn. Both dimensions are at least 1:
/// a zero-pixel bitmap is not a smaller picture, it is no picture, and every
/// caller downstream would have to special-case it.
pub fn device_size(
    rect: PdfRect,
    rotation: PdfRotation,
    viewport: RenderViewport,
) -> Result<(u32, u32), EngineError> {
    let rect_width = rect.right - rect.left;
    let rect_height = rect.top - rect.bottom;
    let swap = matches!(rotation, PdfRotation::Clockwise90 | PdfRotation::Clockwise270);
    let base_width = if swap { rect_height } else { rect_width };
    let base_height = if swap { rect_width } else { rect_height };

    let scale = match viewport {
        RenderViewport::Width { width } => {
            if !width.is_finite() || width <= 0.0 {
                return Err(EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    "render viewport width must be positive",
                ));
            }
            let out_width = width.round().max(1.0);
            // Height from the ratio rather than from a second scale, so a
            // width request lands on exactly the width that was asked for.
            let out_height = (out_width * base_height / base_width).round().max(1.0);
            return Ok((clamp_dimension(out_width), clamp_dimension(out_height)));
        }
        RenderViewport::Scale { scale } => scale,
        // The area as it is, before the turn: a print picture is upright.
        RenderViewport::Dpi { dpi } => print_scale(rect_width, rect_height, dpi),
    };
    if !scale.is_finite() || scale <= 0.0 {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "render viewport scale must be positive",
        ));
    }
    Ok((
        clamp_dimension((base_width * scale).round().max(1.0)),
        clamp_dimension((base_height * scale).round().max(1.0)),
    ))
}

/// A dimension that does not fit in a `u32` is clamped rather than wrapped.
///
/// The allocation that follows is guarded by the pixel budget, so a clamped
/// dimension is refused there rather than attempted. Wrapping would turn an
/// absurd request into a small successful one, which is worse.
fn clamp_dimension(value: f64) -> u32 {
    if value >= f64::from(u32::MAX) { u32::MAX } else { value as u32 }
}

/// A PDF user-space rect in display space: from the page box's top-left, y
/// down (`pdfRectToDisplayRect`).
///
/// PDFium's page renderer pre-applies `CPDF_Page::GetDisplayMatrix`, which
/// has already moved the page box's top-left to the origin and flipped the
/// axis, so a caller matrix has to target the moved, flipped rect rather than
/// the original one. `page` is the page's display box
/// ([`pdfium::PageView::bounding_box`], `readPageBox`).
pub fn pdf_rect_to_display_rect(rect: PdfRect, page: PdfRect) -> PdfRect {
    PdfRect {
        left: rect.left - page.left,
        right: rect.right - page.left,
        bottom: page.top - rect.top,
        top: page.top - rect.bottom,
    }
}

/// A display rect (edges, y down) as a page-space box
/// (`pageBoxOfDisplayRect`).
pub fn page_rect_of_display_rect(rect: PdfRect) -> PageRect {
    PageRect {
        x: rect.left,
        y: rect.bottom,
        width: rect.right - rect.left,
        height: rect.top - rect.bottom,
    }
}

/// The matrix that draws `area` of the page, turned clockwise by `rotation`,
/// onto `width` by `height` pixels (`renderMatrix`, 771342379). The page's
/// display matrix already turned PDF space into page space, so this is the
/// matrix a page transform reports.
pub fn render_matrix(area: PageRect, rotation: PdfRotation, width: u32, height: u32) -> Mat2D {
    let PageRect { x, y, width: w, height: h } = area;
    let width = f64::from(width);
    let height = f64::from(height);
    match rotation {
        PdfRotation::None => Mat2D {
            a: width / w,
            b: 0.0,
            c: 0.0,
            d: height / h,
            e: (-width / w) * x,
            f: (-height / h) * y,
        },
        PdfRotation::Clockwise90 => Mat2D {
            a: 0.0,
            b: height / w,
            c: -width / h,
            d: 0.0,
            e: (width / h) * (y + h),
            f: (-height / w) * x,
        },
        PdfRotation::Clockwise180 => Mat2D {
            a: -width / w,
            b: 0.0,
            c: 0.0,
            d: -height / h,
            e: (width / w) * (x + w),
            f: (height / h) * (y + h),
        },
        PdfRotation::Clockwise270 => Mat2D {
            a: 0.0,
            b: -height / w,
            c: width / h,
            d: 0.0,
            e: (-width / h) * y,
            f: (height / w) * (x + w),
        },
    }
}

/// What to draw: the whole page, or a region of it (`PageRenderTarget<C>`).
///
/// `R` is the region's space: [`PdfRect`] as the renderer takes it, or
/// [`PageRect`] as the wire sends it, which the boundary converts with
/// `crate::page_space::rendering::pdf_render_target_of`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RenderTarget<R = PdfRect> {
    Page,
    Rect { rect: R },
}

/// Resolve a target to a normalized rect in PDF user space: the page box,
/// or a sub-rect in the page's own coordinates, as annotation rects are
/// (`resolveTarget`).
///
/// A region with no area is refused rather than rendered as an empty bitmap:
/// the matrix would divide by zero, and a caller asking for a zero-area region
/// has a bug the engine should name. A region that isn't numbers is refused
/// the same way.
pub fn resolve_target(target: RenderTarget, page: PdfRect) -> Result<PdfRect, EngineError> {
    match target {
        RenderTarget::Page => Ok(page),
        RenderTarget::Rect { rect } => {
            let rect = crate::geom::normalize_pdf_rect(rect);
            if !(rect.right > rect.left && rect.top > rect.bottom) {
                return Err(EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    "render rect must have positive area",
                ));
            }
            Ok(rect)
        }
    }
}
