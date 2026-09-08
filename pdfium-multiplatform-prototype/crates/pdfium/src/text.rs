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
            return Err(PdfiumError::Text);
        }
        Ok(Self { handle, _page: PhantomData, _not_send: PhantomData })
    }

    pub fn char_count(&self) -> u32 {
        unsafe { pdfium_sys::FPDFText_CountChars(self.handle) }.max(0) as u32
    }

    /// EPDFText_GetTextFull is a fork extension: one call for the whole page,
    /// UTF-16, returning the code-unit count including the NUL terminator.
    pub fn text(&self) -> Result<String, PdfiumError> {
        let needed = unsafe { pdfium_sys::EPDFText_GetTextFull(self.handle, std::ptr::null_mut(), 0) };
        if needed <= 1 {
            return Ok(String::new());
        }

        let mut buf = vec![0u16; needed as usize];
        let written = unsafe {
            pdfium_sys::EPDFText_GetTextFull(self.handle, buf.as_mut_ptr(), needed)
        };
        if written <= 0 {
            return Err(PdfiumError::Text);
        }

        buf.truncate((written as usize).saturating_sub(1)); // drop the terminator
        String::from_utf16(&buf).map_err(|_| PdfiumError::Text)
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
                return Err(PdfiumError::Text);
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

impl Drop for TextPage<'_> {
    fn drop(&mut self) {
        unsafe { pdfium_sys::FPDFText_ClosePage(self.handle) };
    }
}
