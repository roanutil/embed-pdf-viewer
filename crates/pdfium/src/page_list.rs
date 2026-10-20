//! Page geometry without loading a page.
//!
//! The `...ByIndex` family reads a page's boxes, size, rotation, user unit and
//! label straight from the page tree, parsing nothing. That is what keeps
//! listing a 500-page document cheap: no page object is built, so none has to
//! be closed.
//!
//! Every one of these takes an index rather than an object number, which is
//! the one place in the engine that is correct: the caller is enumerating, so
//! it has the index already and the object number is what it is producing.

use crate::{Document, PdfiumError, RectF};

/// `EPDF_PAGE_BOX_TYPE` from `public/fpdfview.h`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageBoxType {
    Media = 0,
    Crop = 1,
    Bleed = 2,
    Trim = 3,
    Art = 4,
}

impl Document {
    /// One of a page's boxes, or `None` when the optional box is absent.
    ///
    /// Media always resolves through page-tree inheritance and a US Letter
    /// default, and crop falls back to media, so those two are always present
    /// for a page that exists. Bleed, trim and art are genuinely optional,
    /// and are read from the page dictionary only: since this pin they are no
    /// longer inherited through `/Parent` (ISO 32000 never allowed it), where
    /// the old fork inherited them like media and crop.
    ///
    /// The defaults live here because the fork stopped applying them:
    /// `EPDF_GetPageBoxByIndex` now reports a box only as the file writes it
    /// (runtime 3920846f3). [`Document::written_page_box_by_index`] is that
    /// raw read.
    pub fn page_box_by_index(&self, index: u32, which: PageBoxType) -> Option<RectF> {
        if index >= self.page_count() {
            return None;
        }
        match which {
            PageBoxType::Media => Some(self.effective_media_box(index)),
            PageBoxType::Crop => Some(
                self.written_page_box_by_index(index, PageBoxType::Crop)
                    .unwrap_or_else(|| self.effective_media_box(index)),
            ),
            PageBoxType::Bleed | PageBoxType::Trim | PageBoxType::Art => {
                self.written_page_box_by_index(index, which)
            }
        }
    }

    /// One of a page's boxes exactly as the file writes it, corners in order.
    ///
    /// `None` when the box is absent, empty or not four numbers, or the page
    /// does not exist. Media and crop resolve through page-tree inheritance;
    /// no default and no clipping to the media box is applied.
    pub fn written_page_box_by_index(&self, index: u32, which: PageBoxType) -> Option<RectF> {
        let mut rect = pdfium_sys::FS_RECTF { left: 0.0, top: 0.0, right: 0.0, bottom: 0.0 };
        // SAFETY: the handle is live and `rect` is an initialized local.
        let ok = unsafe {
            pdfium_sys::EPDF_GetPageBoxByIndex(self.handle, index as i32, which as u32, &mut rect)
        };
        (ok != 0).then_some(RectF {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        })
    }

    /// The media box as the fork used to resolve it: the written box, or
    /// US Letter when there is none.
    fn effective_media_box(&self, index: u32) -> RectF {
        self.written_page_box_by_index(index, PageBoxType::Media).unwrap_or(RectF {
            left: 0.0,
            top: 792.0,
            right: 612.0,
            bottom: 0.0,
        })
    }

    /// A page's size with rotation normalized away.
    ///
    /// The crop box, or the media box when there is none, cut to the media
    /// box, and never swapped for `/Rotate`. That is what the removed
    /// `EPDF_GetPageSizeByIndexNormalized` computed natively; it is computed
    /// here now from the written boxes, with the same defaults.
    pub fn page_size_by_index(&self, index: u32) -> Option<(f32, f32)> {
        let media = self.page_box_by_index(index, PageBoxType::Media)?;
        let crop = self.page_box_by_index(index, PageBoxType::Crop)?;
        Some(intersection_size(media, crop))
    }

    /// A page's `/Rotate` as the file writes it, resolved through page-tree
    /// inheritance: 0 when absent, and not normalized, so 450 or 45 comes
    /// back as written. `None` when the page does not exist.
    pub fn page_rotate_by_index(&self, index: u32) -> Option<i32> {
        let mut rotate = 0i32;
        // SAFETY: the handle is live and `rotate` is an initialized local.
        let ok = unsafe {
            pdfium_sys::EPDF_GetPageRotateByIndex(self.handle, index as i32, &raw mut rotate)
        };
        (ok != 0).then_some(rotate)
    }

    /// A page's `/Rotate`, in quarter turns clockwise.
    ///
    /// A page that cannot be read becomes zero here: a page whose rotation
    /// could not be read is upright as far as anything downstream can tell,
    /// and there is no rotation value that means "unknown". The written value
    /// is reduced the way the removed `EPDF_GetPageRotationByIndex` did:
    /// truncated to whole quarter turns, then taken modulo four.
    pub fn page_rotation_quarter_turns(&self, index: u32) -> u8 {
        quarter_turns_of(self.page_rotate_by_index(index).unwrap_or(0))
    }

    /// A page's `/UserUnit`, defaulting to 1.
    ///
    /// A non-positive or non-finite value also becomes 1. The unit scales
    /// every measurement on the page, so a zero there would collapse the page
    /// to nothing rather than reporting an error anybody could act on.
    pub fn page_user_unit_by_index(&self, index: u32) -> f32 {
        let mut unit = 0f32;
        // SAFETY: the handle is live and `unit` is an initialized local.
        let ok = unsafe {
            pdfium_sys::EPDF_GetPageUserUnitByIndex(self.handle, index as i32, &mut unit)
        };
        if ok != 0 && unit.is_finite() && unit > 0.0 { unit } else { 1.0 }
    }

    /// A page's `/PageLabel`, or `None`.
    ///
    /// An empty label reads as absent. The two are indistinguishable to a
    /// consumer that is going to display it, and the wire type says `null`
    /// rather than `""` for "this page has no label".
    pub fn page_label_by_index(&self, index: u32) -> Result<Option<String>, PdfiumError> {
        // SAFETY: a null buffer with zero length is the documented measure
        // call, returning the byte length including the terminator.
        let bytes = unsafe {
            pdfium_sys::FPDF_GetPageLabel(self.handle, index as i32, std::ptr::null_mut(), 0)
        };
        if bytes <= 2 {
            return Ok(None);
        }

        let units = (bytes as usize).div_ceil(2);
        let mut buffer: Vec<u16> = Vec::new();
        buffer.try_reserve_exact(units).map_err(|_| PdfiumError::OutOfMemory)?;
        buffer.resize(units, 0u16);

        // SAFETY: the buffer holds `bytes` bytes.
        let written = unsafe {
            pdfium_sys::FPDF_GetPageLabel(
                self.handle,
                index as i32,
                buffer.as_mut_ptr().cast(),
                bytes,
            )
        };
        if written == 0 {
            return Ok(None);
        }
        let produced = ((written as usize) / 2).saturating_sub(1).min(buffer.len());
        let label = String::from_utf16_lossy(&buffer[..produced]);
        Ok((!label.is_empty()).then_some(label))
    }

    /// A page's indirect object number, by index.
    pub fn page_object_number_by_index(&self, index: u32) -> u32 {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::EPDFDoc_GetPageObjectNumberByIndex(self.handle, index as i32) }
    }
}

/// The size of the overlap of two ordered rects, or nothing.
///
/// Either side at or below zero means they do not overlap, and the whole
/// result is then zero, as `CFX_FloatRect::Intersect` and the TypeScript's
/// `pdfRectIntersection` both have it. Clamping each axis separately would
/// report a strip of positive width and zero height.
fn intersection_size(a: RectF, b: RectF) -> (f32, f32) {
    let width = a.right.min(b.right) - a.left.max(b.left);
    let height = a.top.min(b.top) - a.bottom.max(b.bottom);
    if width <= 0.0 || height <= 0.0 { (0.0, 0.0) } else { (width, height) }
}

/// The removed `EPDF_GetPageRotationByIndex` reduction: whole quarter turns,
/// modulo four, made non-negative.
fn quarter_turns_of(written: i32) -> u8 {
    ((written / 90) % 4).rem_euclid(4) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: f32, bottom: f32, right: f32, top: f32) -> RectF {
        RectF { left, top, right, bottom }
    }

    #[test]
    fn a_crop_inside_the_media_is_its_own_size() {
        let media = rect(0.0, 0.0, 612.0, 792.0);
        assert_eq!(intersection_size(media, rect(10.0, 20.0, 110.0, 220.0)), (100.0, 200.0));
    }

    #[test]
    fn a_crop_past_the_media_is_cut_to_it() {
        let media = rect(0.0, 0.0, 612.0, 792.0);
        assert_eq!(intersection_size(media, rect(-10.0, -10.0, 700.0, 800.0)), (612.0, 792.0));
    }

    #[test]
    fn a_miss_on_one_axis_is_no_size_at_all() {
        let media = rect(0.0, 0.0, 612.0, 792.0);
        // Overlaps horizontally, sits above the media vertically.
        assert_eq!(intersection_size(media, rect(100.0, 800.0, 200.0, 900.0)), (0.0, 0.0));
        // Overlaps vertically, sits right of the media horizontally.
        assert_eq!(intersection_size(media, rect(700.0, 100.0, 800.0, 200.0)), (0.0, 0.0));
        // Touching edges is zero area too.
        assert_eq!(intersection_size(media, rect(612.0, 0.0, 700.0, 792.0)), (0.0, 0.0));
    }

    #[test]
    fn rotation_truncates_to_whole_quarter_turns() {
        assert_eq!(quarter_turns_of(0), 0);
        assert_eq!(quarter_turns_of(90), 1);
        assert_eq!(quarter_turns_of(45), 0);
        assert_eq!(quarter_turns_of(100), 1);
        assert_eq!(quarter_turns_of(270), 3);
        assert_eq!(quarter_turns_of(450), 1);
    }

    #[test]
    fn negative_rotation_wraps_to_the_same_turn() {
        assert_eq!(quarter_turns_of(-90), 3);
        assert_eq!(quarter_turns_of(-180), 2);
        assert_eq!(quarter_turns_of(-270), 1);
        assert_eq!(quarter_turns_of(-360), 0);
        assert_eq!(quarter_turns_of(-45), 0);
    }
}
