//! The signature model: every signature field, its dictionary, its byte
//! range and contents, and the revisions they seal.
//!
//! Wraps `EPDFSig_*` from `epdf_signature.h` and the revision and loaded
//! byte calls beside them. The accessors here are thin: one method per
//! native getter, with the fork's integer codes left as integers. What they
//! mean (`readSignatureModel.ts`) is the engine's to decide.
//!
//! A model is a detached snapshot of the document when it was built, closed
//! on drop. `!Send`, like every PDFium handle.

use std::marker::PhantomData;
use std::os::raw::{c_int, c_ulong};

use crate::{Document, PdfiumError};

/// `EPDF_SIG_STRING_*`, the keys of [`SignatureModel::string`].
pub mod string_key {
    pub const FILTER: i32 = 0;
    pub const SUBFILTER: i32 = 1;
    pub const NAME: i32 = 2;
    pub const REASON: i32 = 3;
    pub const LOCATION: i32 = 4;
    pub const CONTACT_INFO: i32 = 5;
    pub const M: i32 = 6;
}

/// `EPDF_SIG_FIELDS_*`: which field-name list a lock reads.
pub mod field_list {
    pub const FIELDMDP: i32 = 0;
    pub const LOCK: i32 = 1;
}

/// The digest algorithms `EPDFSig_DigestByteRange` computes, by the codes
/// `SignatureReader.ts` `DIGEST_CODE` passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DigestAlgorithm {
    Sha1 = 0,
    Sha256 = 1,
    Sha384 = 2,
    Sha512 = 3,
}

impl DigestAlgorithm {
    pub fn len(self) -> usize {
        match self {
            Self::Sha1 => 20,
            Self::Sha256 => 32,
            Self::Sha384 => 48,
            Self::Sha512 => 64,
        }
    }
}

pub struct SignatureModel<'doc> {
    raw: pdfium_sys::EPDF_SIGNATURE_MODEL,
    _document: PhantomData<&'doc Document>,
    _not_send: PhantomData<*const ()>,
}

impl Drop for SignatureModel<'_> {
    fn drop(&mut self) {
        // SAFETY: built by `EPDFSig_LoadModel` and closed once, here.
        unsafe { pdfium_sys::EPDFSig_CloseModel(self.raw) };
    }
}

impl Document {
    /// Build the signature model of the document as it is now.
    pub fn signature_model(&self) -> Result<SignatureModel<'_>, PdfiumError> {
        // SAFETY: the document is live for the borrow the model keeps.
        let raw = unsafe { pdfium_sys::EPDFSig_LoadModel(self.as_raw()) };
        if raw.is_null() {
            return Err(PdfiumError::InvalidArgument("failed to build signature model"));
        }
        Ok(SignatureModel { raw, _document: PhantomData, _not_send: PhantomData })
    }

    /// `EPDFDoc_GetRevisionCount`: 0 when the chain is not valid.
    pub fn revision_count(&self) -> i32 {
        // SAFETY: live document.
        unsafe { pdfium_sys::EPDFDoc_GetRevisionCount(self.as_raw()) }
    }

    /// `EPDFDoc_GetRevision`: `(end, xref_offset)` of revision `index`.
    pub fn revision(&self, index: i32) -> Option<(u64, u64)> {
        let (mut end, mut xref) = (0u64, 0u64);
        // SAFETY: live document, two u64 out-params.
        let ok =
            unsafe { pdfium_sys::EPDFDoc_GetRevision(self.as_raw(), index, &mut end, &mut xref) };
        (ok != 0).then_some((end, xref))
    }

    /// The size of the bytes the document was loaded from (base plus the
    /// loaded delta for a layer).
    pub fn loaded_bytes_size(&self) -> u64 {
        // SAFETY: live document.
        unsafe { pdfium_sys::EPDFDoc_GetLoadedBytesSize(self.as_raw()) }
    }

    /// The size of the base bytes (the whole file for a plain document).
    pub fn base_bytes_size(&self) -> u64 {
        // SAFETY: live document.
        unsafe { pdfium_sys::EPDFDoc_GetBaseBytesSize(self.as_raw()) }
    }

    /// `[offset, offset + len)` of the loaded bytes.
    pub fn read_loaded_bytes(&self, offset: u64, len: u64) -> Result<Vec<u8>, PdfiumError> {
        if len == 0 {
            return Ok(Vec::new());
        }
        let size = usize::try_from(len).map_err(|_| PdfiumError::OutOfMemory)?;
        let mut out: Vec<u8> = Vec::new();
        out.try_reserve_exact(size).map_err(|_| PdfiumError::OutOfMemory)?;
        out.resize(size, 0);
        // SAFETY: `out` holds `len` writable bytes.
        let read = unsafe {
            pdfium_sys::EPDFDoc_ReadLoadedBytes(
                self.as_raw(),
                offset,
                out.as_mut_ptr().cast(),
                len as c_ulong,
            )
        };
        if read as u64 != len {
            return Err(PdfiumError::InvalidArgument("failed to read the loaded bytes"));
        }
        Ok(out)
    }

    /// `EPDFSig_DigestByteRange`: hash `[r0, r0 + r1) ∪ [r2, r2 + r3)` of the
    /// loaded bytes. `None` when the range is not within them.
    pub fn digest_byte_range(
        &self,
        range: [u64; 4],
        algorithm: DigestAlgorithm,
    ) -> Option<Vec<u8>> {
        let mut out = vec![0u8; algorithm.len()];
        let mut len = out.len() as c_ulong;
        // SAFETY: four u64s in, `len` writable bytes out.
        let ok = unsafe {
            pdfium_sys::EPDFSig_DigestByteRange(
                self.as_raw(),
                range.as_ptr(),
                algorithm as c_int,
                out.as_mut_ptr(),
                &mut len,
            )
        };
        if ok == 0 {
            return None;
        }
        out.truncate(len as usize);
        Some(out)
    }

    /// `EPDFLayer_GetBaseSha256`: the base hash of a layer document.
    pub fn layer_base_sha256(&self) -> Option<[u8; 32]> {
        let mut out = [0u8; 32];
        // SAFETY: 32 writable bytes.
        let ok = unsafe { pdfium_sys::EPDFLayer_GetBaseSha256(self.as_raw(), out.as_mut_ptr()) };
        (ok != 0).then_some(out)
    }
}

impl SignatureModel<'_> {
    pub fn count(&self) -> i32 {
        // SAFETY: live model; likewise for every accessor below.
        unsafe { pdfium_sys::EPDFSig_Count(self.raw) }
    }

    pub fn is_revision_chain_valid(&self) -> bool {
        unsafe { pdfium_sys::EPDFSig_IsRevisionChainValid(self.raw) != 0 }
    }

    /// -1 when no field has that object number.
    pub fn index_by_field_object_number(&self, field: u32) -> i32 {
        unsafe { pdfium_sys::EPDFSig_GetIndexByFieldObjNum(self.raw, field) }
    }

    pub fn field_object_number(&self, index: i32) -> u32 {
        unsafe { pdfium_sys::EPDFSig_GetFieldObjNum(self.raw, index) }
    }

    pub fn widget_object_number(&self, index: i32) -> u32 {
        unsafe { pdfium_sys::EPDFSig_GetWidgetObjNum(self.raw, index) }
    }

    pub fn widget_page_object_number(&self, index: i32) -> u32 {
        unsafe { pdfium_sys::EPDFSig_GetWidgetPageObjNum(self.raw, index) }
    }

    pub fn value_object_number(&self, index: i32) -> u32 {
        unsafe { pdfium_sys::EPDFSig_GetValueObjNum(self.raw, index) }
    }

    pub fn field_name(&self, index: i32) -> Result<Option<String>, PdfiumError> {
        read_wide(|buf, len| unsafe { pdfium_sys::EPDFSig_GetFieldName(self.raw, index, buf, len) })
    }

    pub fn is_signed(&self, index: i32) -> bool {
        unsafe { pdfium_sys::EPDFSig_IsSigned(self.raw, index) != 0 }
    }

    /// `EPDF_SIG_KIND_*`.
    pub fn kind(&self, index: i32) -> i32 {
        unsafe { pdfium_sys::EPDFSig_GetKind(self.raw, index) }
    }

    pub fn byte_range(&self, index: i32) -> Option<[u64; 4]> {
        let mut range = [0u64; 4];
        let ok = unsafe { pdfium_sys::EPDFSig_GetByteRange(self.raw, index, range.as_mut_ptr()) };
        (ok != 0).then_some(range)
    }

    /// `EPDF_SIG_COVERAGE_*`.
    pub fn coverage(&self, index: i32) -> i32 {
        unsafe { pdfium_sys::EPDFSig_GetCoverage(self.raw, index) }
    }

    /// -1 unless the coverage is whole-revision.
    pub fn revision_index(&self, index: i32) -> i32 {
        unsafe { pdfium_sys::EPDFSig_GetRevisionIndex(self.raw, index) }
    }

    /// The DER length of `/Contents`, 0 when unsigned or malformed.
    pub fn contents_len(&self, index: i32) -> u64 {
        unsafe { pdfium_sys::EPDFSig_GetContents(self.raw, index, std::ptr::null_mut(), 0) as u64 }
    }

    /// The DER `/Contents`, padding stripped. Empty when there is none.
    pub fn contents(&self, index: i32) -> Result<Vec<u8>, PdfiumError> {
        let len = self.contents_len(index);
        if len == 0 {
            return Ok(Vec::new());
        }
        let size = usize::try_from(len).map_err(|_| PdfiumError::OutOfMemory)?;
        let mut out: Vec<u8> = Vec::new();
        out.try_reserve_exact(size).map_err(|_| PdfiumError::OutOfMemory)?;
        out.resize(size, 0);
        let written = unsafe {
            pdfium_sys::EPDFSig_GetContents(
                self.raw,
                index,
                out.as_mut_ptr().cast(),
                len as c_ulong,
            )
        };
        if written as u64 != len {
            return Err(PdfiumError::InvalidArgument("failed to read signature contents"));
        }
        Ok(out)
    }

    /// A `/V` string by [`string_key`].
    pub fn string(&self, index: i32, key: i32) -> Result<Option<String>, PdfiumError> {
        read_wide(|buf, len| unsafe {
            pdfium_sys::EPDFSig_GetString(self.raw, index, key, buf, len)
        })
    }

    pub fn doc_mdp_permission(&self, index: i32) -> i32 {
        unsafe { pdfium_sys::EPDFSig_GetDocMDPPermission(self.raw, index) }
    }

    pub fn is_catalog_certification(&self, index: i32) -> bool {
        unsafe { pdfium_sys::EPDFSig_IsCatalogCertification(self.raw, index) != 0 }
    }

    /// `EPDF_SIG_FIELD_ACTION_*` of the signature's FieldMDP transform.
    pub fn field_mdp_action(&self, index: i32) -> i32 {
        unsafe { pdfium_sys::EPDFSig_GetFieldMDPAction(self.raw, index) }
    }

    /// `EPDF_SIG_FIELD_ACTION_*` of the field's `/Lock`.
    pub fn lock_action(&self, index: i32) -> i32 {
        unsafe { pdfium_sys::EPDFSig_GetLockAction(self.raw, index) }
    }

    pub fn lock_permission(&self, index: i32) -> i32 {
        unsafe { pdfium_sys::EPDFSig_GetLockPermission(self.raw, index) }
    }

    /// The names of [`field_list`] `which`.
    pub fn field_names(&self, index: i32, which: i32) -> Result<Vec<String>, PdfiumError> {
        let count = unsafe { pdfium_sys::EPDFSig_GetFieldNameCount(self.raw, index, which) };
        (0..count.max(0))
            .map(|n| {
                read_wide(|buf, len| unsafe {
                    pdfium_sys::EPDFSig_GetFieldNameAt(self.raw, index, which, n, buf, len)
                })
                .map(Option::unwrap_or_default)
            })
            .collect()
    }

    pub fn has_seed_value(&self, index: i32) -> bool {
        unsafe { pdfium_sys::EPDFSig_HasSeedValue(self.raw, index) != 0 }
    }

    pub fn seed_value_required_flags(&self, index: i32) -> u32 {
        unsafe { pdfium_sys::EPDFSig_GetSeedValueRequiredFlags(self.raw, index) }
    }

    pub fn seed_value_present_flags(&self, index: i32) -> u32 {
        unsafe { pdfium_sys::EPDFSig_GetSeedValuePresentFlags(self.raw, index) }
    }

    pub fn seed_value_mdp(&self, index: i32) -> i32 {
        unsafe { pdfium_sys::EPDFSig_GetSeedValueMDP(self.raw, index) }
    }

    pub fn seed_value_version(&self, index: i32) -> i32 {
        unsafe { pdfium_sys::EPDFSig_GetSeedValueVersion(self.raw, index) }
    }

    pub fn seed_value_filter(&self, index: i32) -> Result<Option<String>, PdfiumError> {
        read_wide(|buf, len| unsafe {
            pdfium_sys::EPDFSig_GetSeedValueFilter(self.raw, index, buf, len)
        })
    }

    /// `EPDF_SIG_SV_LIST_*` list `which`.
    pub fn seed_value_list(&self, index: i32, which: i32) -> Result<Vec<String>, PdfiumError> {
        let count = unsafe { pdfium_sys::EPDFSig_GetSeedValueListCount(self.raw, index, which) };
        (0..count.max(0))
            .map(|n| {
                read_wide(|buf, len| unsafe {
                    pdfium_sys::EPDFSig_GetSeedValueListAt(self.raw, index, which, n, buf, len)
                })
                .map(Option::unwrap_or_default)
            })
            .collect()
    }
}

/// A UTF-16LE getter's two-call protocol, as `readUtf16String` reads it:
/// `None` when absent (length 0), `""` for a lone terminator.
pub(crate) fn read_wide<F>(mut call: F) -> Result<Option<String>, PdfiumError>
where
    F: FnMut(*mut u16, c_ulong) -> c_ulong,
{
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
    let end = buffer.iter().position(|&u| u == 0).unwrap_or(buffer.len());
    Ok(Some(String::from_utf16_lossy(&buffer[..end])))
}

/// A document opened over another's bytes: one revision prefix
/// (`EPDFDoc_OpenRevision`) or a layer's base with a working delta
/// (`EPDFDoc_OpenBaseOverlay`). It reads through `'parent`, so it closes
/// before the document it was opened from.
pub struct DerivedDocument<'parent> {
    document: Document,
    _parent: PhantomData<&'parent Document>,
}

impl std::ops::Deref for DerivedDocument<'_> {
    type Target = Document;
    fn deref(&self) -> &Document {
        &self.document
    }
}

impl Document {
    /// `EPDFDoc_OpenRevision`: the revision ending at `end` (a revision end
    /// [`Document::revision`] reported) as its own read-only document.
    pub fn open_revision(&self, end: u64) -> Option<DerivedDocument<'_>> {
        // SAFETY: live document.
        let raw = unsafe { pdfium_sys::EPDFDoc_OpenRevision(self.as_raw(), end) };
        // SAFETY: non-null, closed once by the wrapper, and the lifetime keeps
        // the parent it reads through open longer.
        (!raw.is_null()).then(|| DerivedDocument {
            document: unsafe { Document::from_raw_layer(raw, self.library()) },
            _parent: PhantomData,
        })
    }

    /// `EPDFDoc_OpenBaseOverlay`: a layer's base followed by `delta`
    /// (copied), read-only. `None` for a plain document, an empty delta, or
    /// one that does not compose.
    pub fn open_base_overlay(&self, delta: &[u8]) -> Option<DerivedDocument<'_>> {
        // SAFETY: `delta` is readable for its length; the runtime copies it.
        let raw = unsafe {
            pdfium_sys::EPDFDoc_OpenBaseOverlay(
                self.as_raw(),
                delta.as_ptr().cast(),
                delta.len() as c_ulong,
            )
        };
        // SAFETY: as for `open_revision`.
        (!raw.is_null()).then(|| DerivedDocument {
            document: unsafe { Document::from_raw_layer(raw, self.library()) },
            _parent: PhantomData,
        })
    }

    /// A fresh layer over this layer's base: the document when every edit
    /// brought the layer back to its base. `None` when it does not open.
    pub fn open_fresh_layer_over_base(
        &self,
        password: Option<&str>,
    ) -> Option<DerivedDocument<'_>> {
        let password = std::ffi::CString::new(password.unwrap_or("")).ok()?;
        // SAFETY: live layer document.
        let base = unsafe { pdfium_sys::EPDFLayer_GetBaseDocument(self.as_raw()) };
        if base.is_null() {
            return None;
        }
        let mut status: pdfium_sys::EPDFLayerOpenStatus = u32::MAX as _;
        // SAFETY: the base outlives the layer it backs; a null file access
        // opens the layer with no delta.
        let raw = unsafe {
            pdfium_sys::EPDFLayer_OpenLayer(
                base,
                std::ptr::null_mut(),
                password.as_ptr(),
                &mut status,
            )
        };
        if raw.is_null() || status != 0 {
            if !raw.is_null() {
                // SAFETY: opened above, closed once.
                unsafe { pdfium_sys::FPDF_CloseDocument(raw) };
            }
            return None;
        }
        // SAFETY: as for `open_revision`.
        Some(DerivedDocument {
            document: unsafe { Document::from_raw_layer(raw, self.library()) },
            _parent: PhantomData,
        })
    }

    /// `EPDFDoc_GetStructureObjectNumbers`: the catalog, `/AcroForm` (0 when
    /// direct or absent) and page tree root.
    pub fn structure_object_numbers(&self) -> (u32, u32, u32) {
        let (mut root, mut acro, mut pages) = (0u32, 0u32, 0u32);
        // SAFETY: live document, three out-params.
        unsafe {
            pdfium_sys::EPDFDoc_GetStructureObjectNumbers(
                self.as_raw(),
                &mut root,
                &mut acro,
                &mut pages,
            )
        };
        (root, acro, pages)
    }

    /// `EPDFDoc_CompareRevisions(self, newer)`: the exact object diff from
    /// this revision to `newer`. `None` when they share no byte history.
    pub fn compare_revisions(&self, newer: &Document) -> Option<ObjectDiff> {
        // SAFETY: both documents live while the diff is built; it is
        // detached afterwards.
        let raw = unsafe { pdfium_sys::EPDFDoc_CompareRevisions(self.as_raw(), newer.as_raw()) };
        (!raw.is_null()).then_some(ObjectDiff { raw, _not_send: PhantomData })
    }
}

/// `EPDF_OBJECT_DIFF`, closed on drop. Detached from both documents.
pub struct ObjectDiff {
    raw: pdfium_sys::EPDF_OBJECT_DIFF,
    _not_send: PhantomData<*const ()>,
}

impl Drop for ObjectDiff {
    fn drop(&mut self) {
        // SAFETY: built by `EPDFDoc_CompareRevisions`, closed once.
        unsafe { pdfium_sys::EPDFObjectDiff_Close(self.raw) };
    }
}

/// One `EPDFObjectDiff_GetEntry`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectDiffEntry {
    pub object_number: u32,
    /// `EPDF_DIFF_ADDED` / `MODIFIED` / `FREED`.
    pub change: i32,
    /// `EPDF_DIFF_OBJ_*`.
    pub kind: i32,
    pub old_generation: i32,
    pub new_generation: i32,
    pub stream_data_changed: bool,
}

/// `EPDFObjectDiff_GetRevisionHealth`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffRevisionHealth {
    pub sparse_xref: bool,
    pub bare_references: u32,
    pub referrers_complete: bool,
}

/// Bytes one to one as chars, as the TypeScript's `String.fromCharCode`
/// per byte reads them: the serialisation may carry binary string data.
fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

impl ObjectDiff {
    pub fn count(&self) -> i32 {
        // SAFETY: live diff; likewise below.
        unsafe { pdfium_sys::EPDFObjectDiff_GetCount(self.raw) }
    }

    pub fn entry(&self, index: i32) -> Option<ObjectDiffEntry> {
        let (mut num, mut change, mut kind, mut old_gen, mut new_gen) =
            (0u32, 0i32, 0i32, 0i32, 0i32);
        let mut stream: pdfium_sys::FPDF_BOOL = 0;
        let ok = unsafe {
            pdfium_sys::EPDFObjectDiff_GetEntry(
                self.raw,
                index,
                &mut num,
                &mut change,
                &mut kind,
                &mut old_gen,
                &mut new_gen,
                &mut stream,
            )
        };
        (ok != 0).then_some(ObjectDiffEntry {
            object_number: num,
            change,
            kind,
            old_generation: old_gen,
            new_generation: new_gen,
            stream_data_changed: stream != 0,
        })
    }

    /// The shallow serialisation on side `which` (0 old, 1 new), as
    /// latin-1 text, and whether it was truncated. `None` text when absent.
    pub fn value(&self, index: i32, which: i32) -> (Option<String>, bool) {
        let mut truncated: pdfium_sys::FPDF_BOOL = 0;
        let length = unsafe {
            pdfium_sys::EPDFObjectDiff_GetValue(
                self.raw,
                index,
                which,
                std::ptr::null_mut(),
                0,
                &mut truncated,
            )
        };
        // A value past the cap reports length 0 with the flag set.
        let probe = truncated != 0;
        if length == 0 {
            return (None, probe);
        }
        let mut buffer = vec![0u8; length as usize];
        truncated = 0;
        let written = unsafe {
            pdfium_sys::EPDFObjectDiff_GetValue(
                self.raw,
                index,
                which,
                buffer.as_mut_ptr().cast(),
                length,
                &mut truncated,
            )
        };
        if written == 0 {
            return (None, truncated != 0);
        }
        let end = (written as usize).saturating_sub(1).min(buffer.len());
        (Some(latin1(&buffer[..end])), truncated != 0)
    }

    /// `EPDF_DIFF_READ_*` of side `which`, -1 for bad arguments.
    pub fn read_status(&self, index: i32, which: i32) -> i32 {
        unsafe { pdfium_sys::EPDFObjectDiff_GetReadStatus(self.raw, index, which) }
    }

    pub fn revision_health(&self, which: i32) -> Option<DiffRevisionHealth> {
        let (mut sparse, mut complete): (pdfium_sys::FPDF_BOOL, pdfium_sys::FPDF_BOOL) = (0, 0);
        let mut bare = 0u32;
        let ok = unsafe {
            pdfium_sys::EPDFObjectDiff_GetRevisionHealth(
                self.raw,
                which,
                &mut sparse,
                &mut bare,
                &mut complete,
            )
        };
        (ok != 0).then_some(DiffRevisionHealth {
            sparse_xref: sparse != 0,
            bare_references: bare,
            referrers_complete: complete != 0,
        })
    }

    /// Every inbound reference to `object_number` on side `which`, as
    /// `(parent, label)`.
    pub fn referrers(&self, which: i32, object_number: u32) -> Vec<(u32, String)> {
        let count =
            unsafe { pdfium_sys::EPDFObjectDiff_GetReferrerCount(self.raw, which, object_number) };
        let mut out = Vec::new();
        for k in 0..count.max(0) {
            let mut parent = 0u32;
            let length = unsafe {
                pdfium_sys::EPDFObjectDiff_GetReferrer(
                    self.raw,
                    which,
                    object_number,
                    k,
                    &mut parent,
                    std::ptr::null_mut(),
                    0,
                )
            };
            let label = if length == 0 {
                String::new()
            } else {
                let mut buffer = vec![0u8; length as usize];
                let written = unsafe {
                    pdfium_sys::EPDFObjectDiff_GetReferrer(
                        self.raw,
                        which,
                        object_number,
                        k,
                        &mut parent,
                        buffer.as_mut_ptr().cast(),
                        length,
                    )
                };
                if written == 0 {
                    String::new()
                } else {
                    latin1(&buffer[..(written as usize).saturating_sub(1).min(buffer.len())])
                }
            };
            out.push((parent, label));
        }
        out
    }
}
