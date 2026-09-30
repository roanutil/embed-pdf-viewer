//! `services/geometry.ts`: the crop lookup and the view environment.

use epdf_annotation_core::{PageObjectNumber, PdfRect, Rect, Size, ViewEnv};
use serde_json::Value;

use crate::controller::AnnotationCapability;
use crate::value::get;

/// `pageSizeOf(crop)`: a page's size in content units.
pub fn page_size_of(crop: PdfRect) -> Size {
    Size { width: crop.right - crop.left, height: crop.top - crop.bottom }
}

/// `viewEnv(zoom, rotation)`: the core's view environment, or none.
pub fn view_env(zoom: Option<f64>, rotation: Option<f64>) -> Option<ViewEnv> {
    (zoom.is_some() || rotation.is_some())
        .then(|| ViewEnv { zoom: zoom.unwrap_or(1.0), rotation: rotation.unwrap_or(0.0) })
}

impl AnnotationCapability {
    /// `cropOf(pon)`: the page's crop box from the document registry.
    pub(crate) fn crop_of(&self, pon: PageObjectNumber) -> Option<PdfRect> {
        let meta = self.cx().document()?;
        let page = meta.pages.as_array()?.iter().find(|p| {
            get(p, "ref").and_then(|r| get(r, "pageObjectNumber")).and_then(Value::as_f64)
                == Some(f64::from(pon))
        })?;
        let crop = get(page, "boxes").and_then(|b| get(b, "crop")).filter(|c| c.is_object())?;
        Some(crate::value::pdf_rect_of(Some(crop)))
    }

    /// `pageBoxOf(pon)`: the page's box in content space.
    pub(crate) fn page_box_of(&self, pon: PageObjectNumber) -> Option<Rect> {
        let size = page_size_of(self.crop_of(pon)?);
        Some(Rect { x: 0.0, y: 0.0, width: size.width, height: size.height })
    }

    /// Whether the document registry names this page.
    pub(crate) fn has_page(&self, pon: PageObjectNumber) -> bool {
        self.cx().document().is_some_and(|d| {
            d.pages.as_array().into_iter().flatten().any(|p| {
                get(p, "ref").and_then(|r| get(r, "pageObjectNumber")).and_then(Value::as_f64)
                    == Some(f64::from(pon))
            })
        })
    }
}
