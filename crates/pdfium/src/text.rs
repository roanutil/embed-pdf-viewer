use std::marker::PhantomData;

use crate::{MatrixF, PdfiumError, QuadF, RectF};

/// An `FPDF_TEXTPAGE`, closed on drop.
#[derive(Debug)]
pub struct TextPage<'a> {
    handle: pdfium_sys::FPDF_TEXTPAGE,
    _page: PhantomData<&'a ()>,
    _not_send: PhantomData<*const ()>,
}

/// Opaque identity of the text object a character belongs to.
///
/// This is `FPDFText_GetTextObject`'s pointer, used only for equality: it is
/// the run-grouping key, never dereferenced. Comparing pointers as integers
/// keeps that honest, and keeps a raw pointer out of the type that crosses
/// into `epdf-engine`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct TextObjectKey(pub usize);

/// `EPDF_CHAR_GEOMETRY::flags`.
///
/// Validity is explicit: never infer quad validity from `upright` or from
/// non-zero data, only from the matching `has_*` bit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CharGeometryFlags(pub u32);

impl CharGeometryFlags {
    pub fn has_tight_box(self) -> bool {
        self.0 & pdfium_sys::EPDF_CHARGEO_HAS_TIGHT_BOX != 0
    }
    pub fn has_loose_quad(self) -> bool {
        self.0 & pdfium_sys::EPDF_CHARGEO_HAS_LOOSE_QUAD != 0
    }
    pub fn has_tight_quad(self) -> bool {
        self.0 & pdfium_sys::EPDF_CHARGEO_HAS_TIGHT_QUAD != 0
    }
    pub fn upright(self) -> bool {
        self.0 & pdfium_sys::EPDF_CHARGEO_UPRIGHT != 0
    }
    pub fn space(self) -> bool {
        self.0 & pdfium_sys::EPDF_CHARGEO_SPACE != 0
    }
    pub fn empty(self) -> bool {
        self.0 & pdfium_sys::EPDF_CHARGEO_EMPTY != 0
    }
    /// An `/ActualText` piece: synthesized, but real selectable text.
    pub fn synthesized(self) -> bool {
        self.0 & pdfium_sys::EPDF_CHARGEO_SYNTHESIZED != 0
    }
}

/// One character's complete geometry, from a single `EPDFText_GetCharGeometry`
/// call.
///
/// All coordinates are PDF user space. Quad slots are frame-geometric in the
/// glyph's local upright frame: `(x1,y1)` upper-start, `(x2,y2)` upper-end,
/// `(x3,y3)` lower-start, `(x4,y4)` lower-end, where "start" is local
/// minimum-x and is deliberately not a bidi or reading-order statement.
/// A quad field is meaningful only when its flag says so.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CharGeometry {
    pub loose_box: RectF,
    pub tight_box: RectF,
    pub loose_quad: QuadF,
    pub tight_quad: QuadF,
    pub matrix: MatrixF,
    pub flags: CharGeometryFlags,
}

impl<'a> TextPage<'a> {
    pub(crate) fn load(page: pdfium_sys::FPDF_PAGE) -> Result<Self, PdfiumError> {
        let handle = unsafe { pdfium_sys::FPDFText_LoadPage(page) };
        if handle.is_null() {
            return Err(PdfiumError::TextLoad);
        }
        Ok(Self { handle, _page: PhantomData, _not_send: PhantomData })
    }

    /// The page's text in full UTF-16 fidelity.
    ///
    /// `EPDFText_GetTextFull` rather than `FPDFText_GetText`: the latter has a
    /// UCS-2 encoder that silently drops supplementary characters, so an emoji
    /// or a rare CJK ideograph would vanish from the extracted text while
    /// still occupying a character slot.
    ///
    /// One call, with the buffer sized at the maximum possible expansion of
    /// two units per character plus a terminator, so there is no measure
    /// round-trip.
    pub fn text_full(&self) -> Result<String, PdfiumError> {
        let char_count = self.char_count();
        if char_count == 0 {
            return Ok(String::new());
        }
        // Two units per character plus the NUL. The multiplication is on u64
        // because a pathological page could otherwise overflow the count.
        let units = u64::from(char_count) * 2 + 1;
        let units = i32::try_from(units)
            .map_err(|_| PdfiumError::InvalidArgument("page has too many characters"))?;

        let mut buffer = Vec::new();
        buffer.try_reserve_exact(units as usize).map_err(|_| PdfiumError::OutOfMemory)?;
        buffer.resize(units as usize, 0u16);

        // SAFETY: the buffer holds `units` u16 values, which is what the
        // length argument claims, and it outlives the call.
        let written =
            unsafe { pdfium_sys::EPDFText_GetTextFull(self.handle, buffer.as_mut_ptr(), units) };
        // The return counts units INCLUDING the terminator, so one or fewer
        // means no text.
        if written <= 1 {
            return Ok(String::new());
        }
        let produced = (written as usize - 1).min(buffer.len());

        // Lossy on purpose. A lone surrogate is a malformed pair PDFium
        // produced from a malformed font; refusing the whole page over one
        // would lose the other 5,000 characters, and the replacement
        // character is what JavaScript's own decoder would leave there.
        Ok(String::from_utf16_lossy(&buffer[..produced]))
    }

    /// The character-to-text anchors, or empty when the two spaces agree.
    ///
    /// Measure then fill. Zero anchors is the common page and skips the
    /// second call entirely.
    pub fn char_to_text_map(&self) -> Result<Vec<(u32, u32)>, PdfiumError> {
        // SAFETY: a null buffer with zero length is the documented measure
        // call.
        let count =
            unsafe { pdfium_sys::EPDFText_GetCharToTextMap(self.handle, std::ptr::null_mut(), 0) };
        if count < 0 {
            return Err(PdfiumError::TextRead);
        }
        if count == 0 {
            return Ok(Vec::new());
        }

        let mut anchors = Vec::new();
        anchors.try_reserve_exact(count as usize).map_err(|_| PdfiumError::OutOfMemory)?;
        anchors.resize(
            count as usize,
            pdfium_sys::EPDF_CHAR_MAP_ANCHOR { char_index: 0, text_offset: 0 },
        );

        // SAFETY: the buffer holds `count` anchors, which is what the length
        // argument claims, and it outlives the call.
        let filled = unsafe {
            pdfium_sys::EPDFText_GetCharToTextMap(self.handle, anchors.as_mut_ptr(), count)
        };
        if filled < 0 {
            return Err(PdfiumError::TextRead);
        }
        anchors.truncate(filled.min(count) as usize);

        // Negative indices would mean PDFium contradicted itself; refuse
        // rather than cast them into enormous unsigned values that would then
        // fail an invariant check with a confusing message.
        anchors
            .into_iter()
            .map(|a| match (u32::try_from(a.char_index), u32::try_from(a.text_offset)) {
                (Ok(char_index), Ok(text_offset)) => Ok((char_index, text_offset)),
                _ => Err(PdfiumError::TextRead),
            })
            .collect()
    }

    pub fn char_count(&self) -> u32 {
        unsafe { pdfium_sys::FPDFText_CountChars(self.handle) }.max(0) as u32
    }

    /// Boxes, oriented cells, effective matrix and selection flags for one
    /// character, in a single call.
    ///
    /// `None` means PDFium refused the index: on failure it leaves the output
    /// struct unmodified, so a caller must not read a partially written one.
    /// This is the call the whole geometry read is built on, and it is why
    /// `pages.geometry` needs no per-glyph host crossing.
    pub fn char_geometry(&self, index: u32) -> Option<CharGeometry> {
        let index = i32::try_from(index).ok()?;
        let mut raw = pdfium_sys::EPDF_CHAR_GEOMETRY::default();
        // SAFETY: `raw` is a live, correctly aligned EPDF_CHAR_GEOMETRY for
        // the duration of the call, and the handle is open.
        let ok = unsafe { pdfium_sys::EPDFText_GetCharGeometry(self.handle, index, &mut raw) };
        if ok == 0 {
            return None;
        }
        Some(CharGeometry {
            loose_box: raw.loose_box.into(),
            tight_box: raw.tight_box.into(),
            loose_quad: raw.loose_quad.into(),
            tight_quad: raw.tight_quad.into(),
            matrix: raw.matrix.into(),
            flags: CharGeometryFlags(raw.flags),
        })
    }

    /// Identity of the text object a character belongs to, for run grouping.
    pub fn text_object(&self, index: u32) -> TextObjectKey {
        let Ok(index) = i32::try_from(index) else {
            return TextObjectKey(0);
        };
        let ptr = unsafe { pdfium_sys::FPDFText_GetTextObject(self.handle, index) };
        TextObjectKey(ptr as usize)
    }

    /// The font size in effect at a character, in PDF text-space units.
    pub fn font_size(&self, index: u32) -> f64 {
        let Ok(index) = i32::try_from(index) else {
            return 0.0;
        };
        unsafe { pdfium_sys::FPDFText_GetFontSize(self.handle, index) }
    }

    /// The whole page's text as one UTF-16-derived `String`.
    ///
    /// `EPDFText_GetTextFull` is a fork extension: one call for the whole
    /// page, true UTF-16 (supplementary-plane characters survive as surrogate
    /// pairs), returning the code-unit count INCLUDING the NUL terminator.
    ///
    /// The fork extracts and re-encodes the whole page on every call, sizing
    /// call included, so the classic two-call pattern does that work twice.
    /// Every character contributes at most two UTF-16 units, so
    /// `2 * CountChars + 1` is a true upper bound and one call normally
    /// suffices; the resize-and-retry below is only for a fork that stops
    /// honouring that bound.
    pub fn text(&self) -> Result<String, PdfiumError> {
        let capacity = (self.char_count() as usize) * 2 + 1;
        let capacity = i32::try_from(capacity).map_err(|_| PdfiumError::TextRead)?;
        let mut buf = vec![0u16; capacity as usize];
        let mut required =
            unsafe { pdfium_sys::EPDFText_GetTextFull(self.handle, buf.as_mut_ptr(), capacity) };
        if required > capacity {
            buf.resize(required as usize, 0);
            required = unsafe {
                pdfium_sys::EPDFText_GetTextFull(self.handle, buf.as_mut_ptr(), required)
            };
            if required as usize > buf.len() {
                return Err(PdfiumError::TextRead);
            }
        }
        if required <= 0 {
            return Err(PdfiumError::TextRead);
        }

        buf.truncate((required as usize).saturating_sub(1)); // drop the terminator
        Ok(decode_text(&buf))
    }

    /// Merged rectangles covering a character range.
    ///
    /// `FPDFText_CountRects` has to run before `FPDFText_GetRect`: it is what
    /// populates the rect cache `GetRect` then indexes into.
    pub fn rects(&self, char_index: u32, char_count: u32) -> Result<Vec<[f64; 4]>, PdfiumError> {
        let char_index =
            i32::try_from(char_index).map_err(|_| PdfiumError::InvalidArgument("char_index"))?;
        let char_count =
            i32::try_from(char_count).map_err(|_| PdfiumError::InvalidArgument("char_count"))?;
        let count =
            unsafe { pdfium_sys::FPDFText_CountRects(self.handle, char_index, char_count) }.max(0);

        let mut out = Vec::new();
        out.try_reserve(count as usize).map_err(|_| PdfiumError::OutOfMemory)?;
        for i in 0..count {
            let (mut left, mut top, mut right, mut bottom) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
            let ok = unsafe {
                pdfium_sys::FPDFText_GetRect(
                    self.handle,
                    i,
                    &mut left,
                    &mut top,
                    &mut right,
                    &mut bottom,
                )
            };
            if ok == 0 {
                return Err(PdfiumError::TextRect);
            }
            out.push([left, top, right, bottom]);
        }
        Ok(out)
    }

    /// Literal search over the page, returning `(char_index, char_count)` per
    /// hit in document order.
    pub fn find(&self, query: &str, case_sensitive: bool) -> Result<Vec<(u32, u32)>, PdfiumError> {
        // PDFium's empty-word search can fail to advance on non-whitespace.
        if query.is_empty() {
            return Ok(Vec::new());
        }
        // The native API takes a NUL-terminated string, not a length. Do not
        // silently truncate a query, including turning it into an empty one.
        if query.contains('\0') {
            return Err(PdfiumError::InvalidArgument("query contains NUL"));
        }
        let needle: Vec<u16> = query.encode_utf16().chain(std::iter::once(0)).collect();
        let flags = if case_sensitive { pdfium_sys::FPDF_MATCHCASE } else { 0 };

        let search =
            unsafe { pdfium_sys::FPDFText_FindStart(self.handle, needle.as_ptr(), flags as _, 0) };
        if search.is_null() {
            return Err(PdfiumError::Search);
        }

        let mut hits = Vec::new();
        unsafe {
            while pdfium_sys::FPDFText_FindNext(search) != 0 {
                let index = pdfium_sys::FPDFText_GetSchResultIndex(search).max(0) as u32;
                let count = pdfium_sys::FPDFText_GetSchCount(search).max(0) as u32;
                hits.push((index, count));
            }
            pdfium_sys::FPDFText_FindClose(search);
        }
        Ok(hits)
    }
}

/// The fork preserves lone surrogate units. Replace those units rather than
/// discarding all the otherwise usable text on the page.
fn decode_text(units: &[u16]) -> String {
    String::from_utf16_lossy(units)
}

impl Drop for TextPage<'_> {
    fn drop(&mut self) {
        // SAFETY: loaded by FPDFText_LoadPage and closed exactly once.
        unsafe { pdfium_sys::FPDFText_ClosePage(self.handle) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_text_around_unpaired_surrogates() {
        assert_eq!(
            decode_text(&[0x41, 0xd800, 0x42, 0xdc00, 0xd83d, 0xde00]),
            "A\u{fffd}B\u{fffd}\u{1f600}"
        );
    }

    #[test]
    fn flag_accessors_read_the_documented_bits() {
        let flags = CharGeometryFlags(
            pdfium_sys::EPDF_CHARGEO_HAS_TIGHT_BOX | pdfium_sys::EPDF_CHARGEO_UPRIGHT,
        );
        assert!(flags.has_tight_box());
        assert!(flags.upright());
        assert!(!flags.has_loose_quad());
        assert!(!flags.has_tight_quad());
        assert!(!flags.space());
        assert!(!flags.empty());
        assert!(!flags.synthesized());
    }

    #[test]
    fn errors_identify_the_failed_operation() {
        assert_eq!(PdfiumError::TextLoad.to_string(), "FPDFText_LoadPage failed");
        assert_eq!(PdfiumError::TextRead.to_string(), "EPDFText_GetTextFull failed");
        assert_eq!(PdfiumError::TextRect.to_string(), "FPDFText_GetRect failed");
    }
}
