//! Page geometry without loading a page.
//!
//! The `...ByIndex` family reads a page's boxes, rotation, tab order, user unit
//! and label straight from the page tree, parsing nothing. That is what keeps
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
    /// One of a page's boxes exactly as the file writes it, its corners put
    /// in order by PDFium.
    ///
    /// `None` when the box is absent, empty or not four numbers, or the page
    /// does not exist. Media and crop resolve through page-tree inheritance;
    /// bleed, trim and art are read from the page dictionary only, since ISO
    /// 32000 never let them inherit. Nothing is defaulted and nothing is cut
    /// to the media box: since runtime 3920846f3 `EPDF_GetPageBoxByIndex`
    /// reports a box only as the file writes it, and the engine applies ISO
    /// 32000's defaults itself (`epdf_engine::geom::page_boxes_of`, upstream's
    /// `pageBoxesOf`).
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

    /// A page's `/Rotate` as the file writes it, resolved through page-tree
    /// inheritance: 0 when absent, and not reduced, so 450 or 45 comes back
    /// as written. `None` when the page does not exist. The engine reads it
    /// by upstream's `pageRotationOf` rule (`epdf_engine::geom::
    /// page_rotation_of`).
    pub fn page_rotate_by_index(&self, index: u32) -> Option<i32> {
        let mut rotate = 0i32;
        // SAFETY: the handle is live and `rotate` is an initialized local.
        let ok = unsafe {
            pdfium_sys::EPDF_GetPageRotateByIndex(self.handle, index as i32, &raw mut rotate)
        };
        (ok != 0).then_some(rotate)
    }

    /// A page's `/Tabs` name as the file writes it (`R`, `C`, `S`, or PDF
    /// 2.0's `A` and `W`), read without parsing the page
    /// (`EPDF_GetPageTabOrderByIndex`). `None` when the page sets none or does
    /// not exist. `/Tabs` is not inherited.
    pub fn page_tab_order_by_index(&self, index: u32) -> Result<Option<String>, PdfiumError> {
        // SAFETY: measure-then-fill with a null probe; the buffer holds the
        // length passed with it.
        crate::action::read_utf8(|buf, len| unsafe {
            pdfium_sys::EPDF_GetPageTabOrderByIndex(self.handle, index as i32, buf, len)
        })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Library;

    /// A PDF whose page tree holds `pages`, each a page dictionary's body
    /// without `/Type` and `/Parent`. `tree` goes into the root `/Pages`
    /// dictionary, so a page can inherit from it. The cross-reference
    /// offsets are computed, so PDFium reads the file as written.
    fn pdf(tree: &str, pages: &[&str]) -> Vec<u8> {
        let first = 3;
        let kids: Vec<String> = (0..pages.len()).map(|i| format!("{} 0 R", first + i)).collect();
        let mut objects = vec![
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            format!("<< /Type /Pages /Kids [{}] /Count {} {tree} >>", kids.join(" "), pages.len()),
        ];
        objects.extend(pages.iter().map(|body| format!("<< /Type /Page /Parent 2 0 R {body} >>")));
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (i, object) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
        for offset in offsets {
            out.extend(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        out
    }

    fn open(bytes: &[u8]) -> Document {
        Document::from_bytes(Library::acquire(), bytes, None).expect("the test PDF opens")
    }

    fn rect(left: f32, bottom: f32, right: f32, top: f32) -> RectF {
        RectF { left, top, right, bottom }
    }

    #[test]
    fn a_box_the_file_leaves_out_is_absent_and_none_is_defaulted() {
        // No media box anywhere: the old wrapper answered US Letter here.
        let doc = open(&pdf("", &[""]));
        for which in [
            PageBoxType::Media,
            PageBoxType::Crop,
            PageBoxType::Bleed,
            PageBoxType::Trim,
            PageBoxType::Art,
        ] {
            assert_eq!(doc.written_page_box_by_index(0, which), None, "{which:?}");
        }
        assert_eq!(doc.written_page_box_by_index(1, PageBoxType::Media), None, "past the end");
    }

    #[test]
    fn a_box_reads_as_written_and_is_not_cut_to_the_media_box() {
        let doc = open(&pdf(
            "",
            &["/MediaBox [0 0 612 792] /CropBox [700 800 -10 -10] /TrimBox [100 100 10 10]"],
        ));
        assert_eq!(
            doc.written_page_box_by_index(0, PageBoxType::Media),
            Some(rect(0.0, 0.0, 612.0, 792.0))
        );
        // Past the media box, as written: the engine does the cutting now.
        // PDFium puts the corners in order.
        assert_eq!(
            doc.written_page_box_by_index(0, PageBoxType::Crop),
            Some(rect(-10.0, -10.0, 700.0, 800.0))
        );
        assert_eq!(
            doc.written_page_box_by_index(0, PageBoxType::Trim),
            Some(rect(10.0, 10.0, 100.0, 100.0))
        );
        // Bleed is absent: it does not default to crop here.
        assert_eq!(doc.written_page_box_by_index(0, PageBoxType::Bleed), None);
    }

    #[test]
    fn media_and_crop_inherit_and_the_other_three_do_not() {
        let doc = open(&pdf(
            "/MediaBox [0 0 300 400] /CropBox [10 10 290 390] /BleedBox [0 0 300 400]",
            &[""],
        ));
        assert_eq!(
            doc.written_page_box_by_index(0, PageBoxType::Media),
            Some(rect(0.0, 0.0, 300.0, 400.0))
        );
        assert_eq!(
            doc.written_page_box_by_index(0, PageBoxType::Crop),
            Some(rect(10.0, 10.0, 290.0, 390.0))
        );
        assert_eq!(doc.written_page_box_by_index(0, PageBoxType::Bleed), None);
    }

    #[test]
    fn rotate_reads_as_written_inherited_and_unreduced() {
        let doc = open(&pdf(
            "/Rotate 270",
            &[
                "/MediaBox [0 0 10 10]",
                "/MediaBox [0 0 10 10] /Rotate 45",
                "/MediaBox [0 0 10 10] /Rotate -90",
                "/MediaBox [0 0 10 10] /Rotate 450",
            ],
        ));
        // The old wrapper truncated 45 to 0 and wrapped -90 to three quarter
        // turns; the raw read hands both back for `pageRotationOf`.
        assert_eq!(doc.page_rotate_by_index(0), Some(270), "inherited");
        assert_eq!(doc.page_rotate_by_index(1), Some(45));
        assert_eq!(doc.page_rotate_by_index(2), Some(-90));
        assert_eq!(doc.page_rotate_by_index(3), Some(450));
        assert_eq!(doc.page_rotate_by_index(4), None, "past the end");
    }

    #[test]
    fn tabs_reads_the_name_the_page_writes() {
        let doc =
            open(&pdf("/Tabs /R", &["/MediaBox [0 0 10 10] /Tabs /S", "/MediaBox [0 0 10 10]"]));
        assert_eq!(doc.page_tab_order_by_index(0).unwrap().as_deref(), Some("S"));
        // Not inherited from the tree.
        assert_eq!(doc.page_tab_order_by_index(1).unwrap(), None);
        assert_eq!(doc.page_tab_order_by_index(2).unwrap(), None, "past the end");
    }
}
