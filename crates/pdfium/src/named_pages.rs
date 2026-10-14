//! The catalog's `/Names /Pages` and `/Names /Templates` trees.
//!
//! Wraps the fork's `epdf_named_pages.h`. Names are layout: setting or
//! removing one changes no page's identity or order. The key text is never
//! interpreted here; Acrobat's `identifier=label` convention belongs to the
//! stamp plugin.

use std::os::raw::{c_int, c_uint, c_ulong};

use crate::{Document, PdfiumError};

/// Which tree an entry belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NamedPageTree {
    Pages,
    Templates,
}

impl NamedPageTree {
    /// `EPDF_NAMED_PAGE_TREE_*`.
    fn code(self) -> c_int {
        match self {
            NamedPageTree::Pages => 0,
            NamedPageTree::Templates => 1,
        }
    }
}

/// What an entry's value resolves to (`EPDF_NAMED_PAGE_KIND_*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NamedPageTarget {
    /// A page in the page tree, by object number.
    Page(u32),
    /// A `/Type /Template` dictionary outside the page tree.
    Template(u32),
    /// Null, missing, or neither a page nor a template.
    Dangling,
}

/// One registration, in tree (key-sorted) order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamedPage {
    pub name: String,
    pub target: NamedPageTarget,
}

fn wide(what: &'static str, s: &str) -> Result<Vec<u16>, PdfiumError> {
    if s.is_empty() {
        return Err(PdfiumError::InvalidArgument(what));
    }
    Ok(s.encode_utf16().chain(std::iter::once(0)).collect())
}

impl Document {
    /// Every entry of `tree` in key order. An entry whose key can't be read
    /// is skipped, as `PagesReader.ts` skips it. A tree the fork can't read
    /// reads as empty.
    pub fn named_pages(&self, tree: NamedPageTree) -> Result<Vec<NamedPage>, PdfiumError> {
        // SAFETY: the handle is live for the borrow.
        let doc = unsafe { self.as_raw() };
        let count = unsafe { pdfium_sys::EPDFDoc_GetNamedPageCount(doc, tree.code()) };
        let mut out = Vec::new();
        for index in 0..count.max(0) {
            let mut object_number: c_uint = 0;
            let mut kind: c_int = 0;
            let mut read = |buffer: *mut u16, len: c_ulong| -> c_ulong {
                // SAFETY: `buffer` is null or holds `len` bytes; the out
                // pointers are live locals.
                unsafe {
                    pdfium_sys::EPDFDoc_GetNamedPageAt(
                        doc,
                        tree.code(),
                        index,
                        buffer,
                        len,
                        &mut object_number,
                        &mut kind,
                    )
                }
            };
            let Some(name) = read_key(&mut read)? else { continue };
            let target = match kind {
                0 => NamedPageTarget::Page(object_number),
                1 => NamedPageTarget::Template(object_number),
                _ => NamedPageTarget::Dangling,
            };
            out.push(NamedPage { name, target });
        }
        Ok(out)
    }

    /// Register `name` for the page, replacing any registration with the
    /// same key. `Ok(false)` when the fork refuses: the page isn't in the
    /// page tree, or the catalog can't be written.
    pub fn set_named_page(&self, name: &str, page_object_number: u32) -> Result<bool, PdfiumError> {
        let key = wide("a named page needs a non-empty name", name)?;
        // SAFETY: the handle is live and `key` is NUL-terminated.
        let ok = unsafe {
            pdfium_sys::EPDFDoc_SetNamedPage(self.as_raw(), key.as_ptr(), page_object_number)
        };
        Ok(ok != 0)
    }

    /// Whether a registration was there to remove.
    pub fn remove_named_page(&self, name: &str) -> Result<bool, PdfiumError> {
        let key = wide("a named page needs a non-empty name", name)?;
        // SAFETY: the handle is live and `key` is NUL-terminated.
        Ok(unsafe { pdfium_sys::EPDFDoc_RemoveNamedPage(self.as_raw(), key.as_ptr()) } != 0)
    }
}

/// `readUtf16String(..., '')`: a length of zero is absent, a lone
/// terminator is the empty key.
fn read_key(
    call: &mut impl FnMut(*mut u16, c_ulong) -> c_ulong,
) -> Result<Option<String>, PdfiumError> {
    let bytes = call(std::ptr::null_mut(), 0);
    if bytes == 0 {
        return Ok(None);
    }
    if bytes <= 2 {
        return Ok(Some(String::new()));
    }
    let units = (bytes as usize).div_ceil(2);
    let mut buffer: Vec<u16> = Vec::new();
    buffer.try_reserve_exact(units).map_err(|_| PdfiumError::OutOfMemory)?;
    buffer.resize(units, 0u16);
    let written = call(buffer.as_mut_ptr(), bytes);
    if written == 0 {
        return Ok(None);
    }
    let produced = ((written as usize) / 2).saturating_sub(1).min(buffer.len());
    Ok(Some(String::from_utf16_lossy(&buffer[..produced])))
}
