use std::marker::PhantomData;

use crate::PdfiumError;

pub struct TextPage<'a> {
    handle: pdfium_sys::FPDF_TEXTPAGE,
    _page: PhantomData<&'a ()>,
    _not_send: PhantomData<*const ()>,
}

impl<'a> TextPage<'a> {
    pub(crate) fn load(page: pdfium_sys::FPDF_PAGE) -> Result<Self, PdfiumError> {
        let handle = unsafe { pdfium_sys::FPDFText_LoadPage(page) };
        if handle.is_null() {
            return Err(PdfiumError::TextLoad);
        }
        Ok(Self { handle, _page: PhantomData, _not_send: PhantomData })
    }

    pub fn char_count(&self) -> u32 {
        unsafe { pdfium_sys::FPDFText_CountChars(self.handle) }.max(0) as u32
    }

    /// EPDFText_GetTextFull is a fork extension: one call for the whole page,
    /// UTF-16, returning the code-unit count including the NUL terminator.
    ///
    /// The fork extracts and re-encodes the whole page on every call, sizing
    /// call included (fpdf_text.cpp, EPDFText_GetTextFull), so the classic
    /// two-call pattern does that work twice. Every character contributes at
    /// most two UTF-16 units, so `2 * CountChars + 1` is a true upper bound
    /// and one call normally suffices; the resize-and-retry path below is
    /// only for a fork that stops honouring that bound.
    pub fn text(&self) -> Result<String, PdfiumError> {
        let capacity = (self.char_count() as usize) * 2 + 1;
        let capacity = i32::try_from(capacity).map_err(|_| PdfiumError::TextRead)?;
        let mut buf = vec![0u16; capacity as usize];
        let mut required = unsafe {
            pdfium_sys::EPDFText_GetTextFull(self.handle, buf.as_mut_ptr(), capacity)
        };
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
        // The fork preserves lone surrogate units. Replace those units rather
        // than discarding all the otherwise usable text on the page.
        Ok(decode_text(&buf))
    }

    /// FPDFText_CountRects has to run before FPDFText_GetRect: it is what
    /// populates the rect cache GetRect then indexes into.
    pub fn rects(&self, char_index: u32, char_count: u32) -> Result<Vec<[f64; 4]>, PdfiumError> {
        let count = unsafe {
            pdfium_sys::FPDFText_CountRects(self.handle, char_index as i32, char_count as i32)
        }
        .max(0);

        let mut out = Vec::with_capacity(count as usize);
        for i in 0..count {
            let mut left = 0.0f64;
            let mut top = 0.0f64;
            let mut right = 0.0f64;
            let mut bottom = 0.0f64;
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

    /// Returns `(char_index, char_count)` per hit, in document order.
    pub fn find(&self, query: &str, case_sensitive: bool) -> Result<Vec<(u32, u32)>, PdfiumError> {
        // PDFium's empty-word search can fail to advance on non-whitespace.
        if query.is_empty() {
            return Ok(Vec::new());
        }
        // The native API accepts a NUL-terminated string, not a length. Do not
        // silently truncate a query (including turning it into an empty one).
        if query.contains('\0') {
            return Err(PdfiumError::Search);
        }
        let needle: Vec<u16> = query.encode_utf16().chain(std::iter::once(0)).collect();
        let flags = if case_sensitive { pdfium_sys::FPDF_MATCHCASE } else { 0 };

        let search = unsafe {
            pdfium_sys::FPDFText_FindStart(self.handle, needle.as_ptr(), flags as _, 0)
        };
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

fn decode_text(units: &[u16]) -> String {
    String::from_utf16_lossy(units)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_text_around_unpaired_surrogates() {
        assert_eq!(decode_text(&[0x41, 0xd800, 0x42, 0xdc00, 0xd83d, 0xde00]), "A\u{fffd}B\u{fffd}\u{1f600}");
    }

    #[test]
    fn text_errors_identify_the_failed_operation() {
        assert_eq!(PdfiumError::TextLoad.to_string(), "FPDFText_LoadPage failed");
        assert_eq!(PdfiumError::TextRead.to_string(), "EPDFText_GetTextFull failed");
        assert_eq!(PdfiumError::TextRect.to_string(), "FPDFText_GetRect failed");
    }
}

impl Drop for TextPage<'_> {
    fn drop(&mut self) {
        unsafe { pdfium_sys::FPDFText_ClosePage(self.handle) };
    }
}
