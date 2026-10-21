//! Render targets and rendered areas in page space
//! (`core/src/pageSpace/rendering.ts`).

use super::{page_rect_of, pdf_rect_of};
use crate::features::render::geometry::RenderTarget;
use crate::features::render::raster::RenderOptions;
use crate::geom::{PageRect, PdfRect};

/// A page-space render target in the file's coordinates
/// (`pdfRenderTargetOf`). `visible` is the page's visible box.
pub fn pdf_render_target_of(target: RenderTarget<PageRect>, visible: PdfRect) -> RenderTarget {
    match target {
        RenderTarget::Page => RenderTarget::Page,
        RenderTarget::Rect { rect } => RenderTarget::Rect { rect: pdf_rect_of(rect, visible) },
    }
}

/// Render options in the file's coordinates (`renderOptionsInFileSpace`).
/// `visible` gives the page's visible box, read only when a target rect
/// needs it.
pub fn render_options_in_file_space<E>(
    options: RenderOptions<PageRect>,
    visible: impl FnOnce() -> Result<PdfRect, E>,
) -> Result<RenderOptions, E> {
    options.try_map_target(|rect| Ok(pdf_rect_of(rect, visible()?)))
}

/// The area a render shows, in page space: `pages.render`'s `area`
/// (`resultInPageSpace`, `pageBoxOf(payload.area, boxOf(payload.page))`).
pub fn page_render_area_of(area: PdfRect, visible: PdfRect) -> PageRect {
    page_rect_of(area, visible)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_target_needs_no_box_and_a_rect_is_measured_on_the_visible_box() {
        let visible = PdfRect { left: 50.0, bottom: 60.0, right: 562.0, top: 732.0 };
        assert_eq!(pdf_render_target_of(RenderTarget::Page, visible), RenderTarget::Page);
        let page = PageRect { x: 10.0, y: 20.0, width: 100.0, height: 50.0 };
        let pdf = PdfRect { left: 60.0, bottom: 662.0, right: 160.0, top: 712.0 };
        assert_eq!(
            pdf_render_target_of(RenderTarget::Rect { rect: page }, visible),
            RenderTarget::Rect { rect: pdf }
        );
        assert_eq!(page_render_area_of(pdf, visible), page);

        let whole: RenderOptions<PageRect> = RenderOptions::default();
        let converted = render_options_in_file_space(whole, || -> Result<PdfRect, ()> {
            panic!("a whole-page render reads no box")
        });
        assert_eq!(converted, Ok(RenderOptions::default()));
    }
}
