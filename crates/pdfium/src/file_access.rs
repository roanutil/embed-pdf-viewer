//! Feeding PDFium bytes it reads on demand.
//!
//! `FPDF_FILEACCESS` is a length plus a callback: PDFium asks for a range and
//! the callback fills it. Two things use it here. Layer deltas and artifacts
//! take one instead of a pointer, and a file-backed document can be read
//! without copying it into memory first, which for a large PDF is the
//! difference between a 200 MB allocation and a few pages of it.
//!
//! # The invariant, and how it is held
//!
//! PDFium stores the struct's address and calls back into it later, so the
//! struct must not move and the data behind `m_Param` must outlive every call.
//! Both are held by [`FileAccess`] owning its source and handing out a pointer
//! only through [`FileAccess::as_ptr`], which borrows. A `FileAccess` that has
//! been dropped cannot produce a pointer, and one that is alive cannot have
//! moved: the struct lives in a `Box`, so the address is stable even when the
//! `FileAccess` itself is moved.
//!
//! # Panics across the boundary
//!
//! The callback is `extern "C"` and a panic unwinding through it is undefined
//! behaviour. Every callback here is wrapped in `catch_unwind` and reports
//! failure to PDFium instead, which turns a would-be abort into a failed load.

use std::ffi::c_void;
use std::io::{Read, Seek, SeekFrom};
use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::PdfiumError;

/// Where the bytes come from.
enum Source {
    /// Owned bytes. Reads are memcpy and cannot fail.
    Memory(Vec<u8>),
    /// An open file, read on demand.
    ///
    /// `RefCell` because `m_GetBlock` hands back a `*mut c_void` that several
    /// calls share, and seeking is a mutation. PDFium calls it from the thread
    /// that owns the document, which is the only thread that can reach this,
    /// so there is no lock to take.
    File(std::cell::RefCell<std::fs::File>),
}

/// A byte source PDFium can read through.
///
/// Boxed internally so the `FPDF_FILEACCESS` address PDFium stores stays put.
pub struct FileAccess {
    /// Boxed so its address is stable across moves of `FileAccess` itself.
    access: Box<pdfium_sys::FPDF_FILEACCESS>,
    /// Boxed for the same reason: `m_Param` points at it.
    source: Box<Source>,
}

impl std::fmt::Debug for FileAccess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = match *self.source {
            Source::Memory(_) => "memory",
            Source::File(_) => "file",
        };
        f.debug_struct("FileAccess")
            .field("source", &kind)
            .field("len", &self.access.m_FileLen)
            .finish()
    }
}

impl FileAccess {
    /// Read from bytes this takes ownership of.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, PdfiumError> {
        let len = u32::try_from(bytes.len()).map_err(|_| {
            PdfiumError::InvalidArgument("file is larger than FPDF_FILEACCESS allows")
        })?;
        Self::build(Source::Memory(bytes), len)
    }

    /// Read from a file on demand, without loading it into memory.
    pub fn from_file(path: impl AsRef<std::path::Path>) -> Result<Self, PdfiumError> {
        let file = std::fs::File::open(path).map_err(|_| PdfiumError::Load)?;
        let len = file.metadata().map_err(|_| PdfiumError::Load)?.len();
        let len = u32::try_from(len).map_err(|_| {
            PdfiumError::InvalidArgument("file is larger than FPDF_FILEACCESS allows")
        })?;
        Self::build(Source::File(std::cell::RefCell::new(file)), len)
    }

    fn build(source: Source, len: u32) -> Result<Self, PdfiumError> {
        // `m_FileLen` is a c_ulong, which is 64-bit on Unix and 32-bit on
        // Windows. The length is checked against u32 above so the value fits
        // either way and the cast cannot truncate.
        let mut source = Box::new(source);
        let access = Box::new(pdfium_sys::FPDF_FILEACCESS {
            m_FileLen: len as std::os::raw::c_ulong,
            m_GetBlock: Some(get_block),
            m_Param: (&raw mut *source).cast::<c_void>(),
        });
        Ok(Self { access, source })
    }

    /// The pointer to hand PDFium, valid for this borrow.
    ///
    /// # Safety
    /// The returned pointer must not be used after this `FileAccess` drops.
    /// It is `*mut` because PDFium's signature says so; nothing here mutates
    /// through it.
    pub fn as_ptr(&self) -> *mut pdfium_sys::FPDF_FILEACCESS {
        // Casting away const on a pointer PDFium only reads. The struct is
        // boxed, so this address is stable for the life of `self`.
        std::ptr::from_ref(self.access.as_ref()).cast_mut()
    }

    pub fn len(&self) -> u64 {
        u64::from(self.access.m_FileLen as u32)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// PDFium's read callback.
///
/// Returns 1 on success and 0 on failure, which is what PDFium expects. Every
/// failure path returns 0 rather than reading a partial block: a short read
/// that reported success would hand PDFium uninitialized bytes as document
/// content.
unsafe extern "C" fn get_block(
    param: *mut c_void,
    position: std::os::raw::c_ulong,
    buffer: *mut u8,
    size: std::os::raw::c_ulong,
) -> std::os::raw::c_int {
    if param.is_null() || buffer.is_null() {
        return 0;
    }
    if size == 0 {
        return 1;
    }

    // A panic unwinding through an `extern "C"` boundary is undefined
    // behaviour, so anything that could panic is caught and reported as a
    // failed read instead.
    let result = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: `param` is the address of the `Box<Source>` this
        // `FPDF_FILEACCESS` was built with, which outlives every call because
        // the `FileAccess` that owns both is borrowed for as long as PDFium
        // holds the pointer.
        let source = unsafe { &*param.cast::<Source>() };
        // SAFETY: PDFium guarantees `buffer` is writable for `size` bytes.
        let out = unsafe { std::slice::from_raw_parts_mut(buffer, size as usize) };
        // c_ulong is 64-bit on Unix and 32-bit on Windows, so exactly one of
        // `as u64` and `u64::from` is a no-op on any given target and clippy
        // objects to whichever one it is. The conversion is load-bearing on
        // the other target, so it stays and the lint is silenced here rather
        // than the code being made wrong on Windows.
        #[allow(clippy::useless_conversion)]
        let position = u64::from(position);

        match source {
            Source::Memory(bytes) => {
                let start = usize::try_from(position).ok()?;
                let end = start.checked_add(out.len())?;
                // Refuse a read past the end rather than returning a short
                // one: PDFium asked for a specific range.
                let slice = bytes.get(start..end)?;
                out.copy_from_slice(slice);
                Some(())
            }
            Source::File(file) => {
                let mut file = file.try_borrow_mut().ok()?;
                file.seek(SeekFrom::Start(position)).ok()?;
                file.read_exact(out).ok()?;
                Some(())
            }
        }
    }));

    match result {
        Ok(Some(())) => 1,
        _ => 0,
    }
}
