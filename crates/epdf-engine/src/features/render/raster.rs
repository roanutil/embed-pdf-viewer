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
    RenderBackground, RenderTarget, RenderViewport, device_size, display_rect_to_device_matrix,
    pdf_rect_to_display_rect, resolve_target,
};
use crate::access::PageAccess;
use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};
use crate::geom::PdfRotation;

/// What to draw and how big.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RenderOptions {
    pub target: RenderTarget,
    pub viewport: RenderViewport,
    pub rotation: PdfRotation,
    pub background: RenderBackground,
    pub include_annotations: bool,
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
    pub max_output_pixels: Option<u64>,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            target: RenderTarget::default(),
            viewport: RenderViewport::default(),
            rotation: PdfRotation::None,
            background: RenderBackground::default(),
            // Annotations are drawn unless a caller says otherwise, because a
            // page without them is not what the document looks like.
            include_annotations: true,
            max_output_pixels: None,
        }
    }
}

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

/// Render a page region.
///
/// Cancellation is cooperative up to the draw and not inside it:
/// `FPDF_RenderPageBitmapWithMatrix` is one synchronous call that cannot be
/// interrupted, so a token set during it is observed on the far side. The
/// check after the draw exists so the caller is told rather than handed a
/// bitmap it no longer wants.
pub fn render_page(
    page: &dyn PageAccess,
    options: &RenderOptions,
    cancel: &CancelToken,
) -> Result<PageRaster, EngineError> {
    cancel.check()?;
    let view = page.view();
    let page_width = f64::from(view.width());
    let page_height = f64::from(view.height());

    let rect = resolve_target(options.target, page_width, page_height)?;
    let display_rect = pdf_rect_to_display_rect(rect, page_height);
    let (width, height) = device_size(display_rect, options.rotation, options.viewport)?;

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

    let matrix = display_rect_to_device_matrix(display_rect, options.rotation, width, height);
    cancel.check()?;
    let bitmap = view
        .render_rgba_with_matrix(
            width,
            height,
            matrix.to_array(),
            options.background == RenderBackground::White,
            options.include_annotations,
        )
        .map_err(|e| {
            EngineError::with_message(EngineErrorCode::RuntimeUnavailable, e.to_string())
        })?;
    cancel.check()?;

    Ok(PageRaster {
        width: bitmap.width,
        height: bitmap.height,
        stride: bitmap.stride,
        // Already RGBA: the render asked PDFium for reversed byte order, so
        // there is no pass over the pixels here.
        rgba: bitmap.bgra,
    })
}
