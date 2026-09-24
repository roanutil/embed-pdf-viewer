//! The one place a PDF user-space region becomes a device raster.
//!
//! G4. A transcription of the pure half of
//! `services/src/features/render/deviceRaster.ts`. Every rasterizer composes
//! these three pieces and supplies only its own draw call: the geometry is a
//! pure affine, and the bitmap lifecycle lives next door in [`super::raster`].
//!
//! Pure, so it is pinned to the TypeScript by
//! `tooling/engine-conformance/vectors/render/geometry.json` rather than by a
//! fixture. A matrix that is subtly wrong produces a picture that is subtly
//! wrong, which is the hardest kind of divergence to notice by looking.

use serde::{Deserialize, Serialize};

use crate::error::{EngineError, EngineErrorCode};
use crate::geom::{PdfRect, PdfRotation};

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
    /// cares about device pixel ratio folds it into this.
    Scale { scale: f64 },
    /// An exact output width. Height preserves the region's aspect ratio
    /// after rotation.
    Width { width: f64 },
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

/// Device pixel size for a region under a rotation and viewport.
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

    match viewport {
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
            Ok((clamp_dimension(out_width), clamp_dimension(out_height)))
        }
        RenderViewport::Scale { scale } => {
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
    }
}

/// A dimension that does not fit in a `u32` is clamped rather than wrapped.
///
/// The allocation that follows is guarded by the pixel budget, so a clamped
/// dimension is refused there rather than attempted. Wrapping would turn an
/// absurd request into a small successful one, which is worse.
fn clamp_dimension(value: f64) -> u32 {
    if value >= f64::from(u32::MAX) { u32::MAX } else { value as u32 }
}

/// PDF user space (y-up, origin bottom-left) to display space (y-down).
///
/// PDFium's page renderer pre-applies `CPDF_Page::GetDisplayMatrix`, which has
/// already flipped the axis, so a caller matrix has to target the flipped
/// rect rather than the original one.
pub fn pdf_rect_to_display_rect(rect: PdfRect, page_height: f64) -> PdfRect {
    PdfRect {
        left: rect.left,
        right: rect.right,
        bottom: page_height - rect.top,
        top: page_height - rect.bottom,
    }
}

/// Map a display-space rect onto an `out_width` by `out_height` bitmap, baking
/// in the caller's rotation.
pub fn display_rect_to_device_matrix(
    rect: PdfRect,
    rotation: PdfRotation,
    out_width: u32,
    out_height: u32,
) -> Mat2D {
    let left = rect.left;
    let bottom = rect.bottom;
    let width = rect.right - rect.left;
    let height = rect.top - rect.bottom;
    let out_w = f64::from(out_width);
    let out_h = f64::from(out_height);
    let sx0 = out_w / width;
    let sy0 = out_h / height;
    let sx90 = out_w / height;
    let sy90 = out_h / width;

    match rotation {
        PdfRotation::Clockwise90 => Mat2D {
            a: 0.0,
            b: sy90,
            c: -sx90,
            d: 0.0,
            e: sx90 * (bottom + height),
            f: -sy90 * left,
        },
        PdfRotation::Clockwise180 => Mat2D {
            a: -sx0,
            b: 0.0,
            c: 0.0,
            d: -sy0,
            e: sx0 * (left + width),
            f: sy0 * (bottom + height),
        },
        PdfRotation::Clockwise270 => {
            Mat2D { a: 0.0, b: -sy90, c: sx90, d: 0.0, e: -sx90 * bottom, f: sy90 * (left + width) }
        }
        PdfRotation::None => {
            Mat2D { a: sx0, b: 0.0, c: 0.0, d: sy0, e: -sx0 * left, f: -sy0 * bottom }
        }
    }
}

/// What to draw: the whole page, or a region of it.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RenderTarget {
    #[default]
    Page,
    Rect {
        rect: PdfRect,
    },
}

/// Resolve a target to a normalized rect in PDF user space.
///
/// A region with no area is refused rather than rendered as an empty bitmap:
/// the matrix would divide by zero, and a caller asking for a zero-area region
/// has a bug the engine should name.
pub fn resolve_target(
    target: RenderTarget,
    page_width: f64,
    page_height: f64,
) -> Result<PdfRect, EngineError> {
    match target {
        RenderTarget::Page => {
            Ok(PdfRect { left: 0.0, bottom: 0.0, right: page_width, top: page_height })
        }
        RenderTarget::Rect { rect } => {
            let rect = crate::geom::normalize_pdf_rect(rect);
            if rect.right <= rect.left || rect.top <= rect.bottom {
                return Err(EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    "render rect must have positive area",
                ));
            }
            Ok(rect)
        }
    }
}
