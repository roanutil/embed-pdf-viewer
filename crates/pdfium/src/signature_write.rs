//! Authoring a signature: the fork's candidate calls `SignatureMutator.ts`
//! makes. `EPDFSig_Prepare` writes the `/V` dictionary on a candidate,
//! `EPDFSig_SaveCandidateToOwnedBuffer` saves it incrementally and reports
//! where the dictionary landed, `EPDFSig_Seal` patches `/ByteRange` and
//! hashes, and `EPDFSig_WriteContents` puts the CMS into the hole.

use std::ffi::CString;
use std::os::raw::{c_int, c_ulong};

use crate::{DigestAlgorithm, Document, PdfiumError};

/// `EPDF_SIG_PREPARE`, owned. Codes are the header's.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SignaturePrepareOptions {
    /// `EPDF_SIG_SUBFILTER_*`.
    pub subfilter: i32,
    /// [`DigestAlgorithm`] code.
    pub digest: i32,
    pub contents_size: u64,
    pub name: Option<String>,
    pub reason: Option<String>,
    pub location: Option<String>,
    pub contact_info: Option<String>,
    /// `/M` as a PDF date; `None` is now.
    pub signing_time: Option<String>,
    pub docmdp_permission: i32,
    /// `EPDF_SIG_FIELD_ACTION_*`.
    pub fieldmdp_action: i32,
    pub fieldmdp_fields: Vec<String>,
    pub lock_permission: i32,
}

/// What `EPDFSig_SaveCandidateToOwnedBuffer` produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedCandidate {
    pub bytes: Vec<u8>,
    pub object_offset: u64,
    pub object_length: u64,
}

/// What `EPDFSig_Seal` reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedCandidate {
    pub byte_range: [u64; 4],
    pub contents_offset: u64,
    pub contents_hex_length: u64,
    pub digest: Vec<u8>,
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

impl Document {
    /// `EPDFSig_Prepare`: the `/V` object number, 0 when refused.
    pub fn prepare_signature(
        &self,
        field_object_number: u32,
        opts: &SignaturePrepareOptions,
    ) -> Result<u32, PdfiumError> {
        let name = opts.name.as_deref().map(wide);
        let reason = opts.reason.as_deref().map(wide);
        let location = opts.location.as_deref().map(wide);
        let contact = opts.contact_info.as_deref().map(wide);
        let time = opts
            .signing_time
            .as_deref()
            .map(CString::new)
            .transpose()
            .map_err(|_| PdfiumError::InvalidArgument("signing time contains NUL"))?;
        let fields: Vec<Vec<u16>> = opts.fieldmdp_fields.iter().map(|f| wide(f)).collect();
        let field_ptrs: Vec<pdfium_sys::FPDF_WIDESTRING> =
            fields.iter().map(|f| f.as_ptr()).collect();
        let ptr = |v: &Option<Vec<u16>>| v.as_ref().map_or(std::ptr::null(), |v| v.as_ptr());
        let raw = pdfium_sys::EPDF_SIG_PREPARE {
            subfilter: opts.subfilter,
            digest: opts.digest,
            contents_size: opts.contents_size as c_ulong,
            name: ptr(&name),
            reason: ptr(&reason),
            location: ptr(&location),
            contact_info: ptr(&contact),
            signing_time: time.as_ref().map_or(std::ptr::null(), |t| t.as_ptr()),
            docmdp_permission: opts.docmdp_permission,
            fieldmdp_action: opts.fieldmdp_action,
            fieldmdp_fields: if field_ptrs.is_empty() {
                std::ptr::null()
            } else {
                field_ptrs.as_ptr()
            },
            fieldmdp_field_count: field_ptrs.len() as c_int,
            lock_permission: opts.lock_permission,
        };
        // SAFETY: every pointer in `raw` points into a buffer alive for the call.
        Ok(unsafe { pdfium_sys::EPDFSig_Prepare(self.as_raw(), field_object_number, &raw) })
    }

    /// `EPDFSig_SaveCandidateToOwnedBuffer`.
    pub fn save_signature_candidate(
        &self,
        sig_object_number: u32,
    ) -> Result<SavedCandidate, PdfiumError> {
        let (mut size, mut offset, mut length) = (0u64, 0u64, 0u64);
        // SAFETY: live document, three out-params.
        let ptr = unsafe {
            pdfium_sys::EPDFSig_SaveCandidateToOwnedBuffer(
                self.as_raw(),
                sig_object_number,
                &mut size,
                &mut offset,
                &mut length,
            )
        };
        if ptr.is_null() {
            return Err(PdfiumError::Save);
        }
        let len = usize::try_from(size).map_err(|_| PdfiumError::OutOfMemory);
        let bytes = len.and_then(|len| {
            let mut out: Vec<u8> = Vec::new();
            out.try_reserve_exact(len).map_err(|_| PdfiumError::OutOfMemory)?;
            // SAFETY: the save reported `len` readable bytes at `ptr`.
            out.extend_from_slice(unsafe { std::slice::from_raw_parts(ptr.cast::<u8>(), len) });
            Ok(out)
        });
        // SAFETY: allocated by the save, freed once.
        unsafe { pdfium_sys::EPDF_FreeBuffer(ptr) };
        Ok(SavedCandidate { bytes: bytes?, object_offset: offset, object_length: length })
    }

    /// `EPDFAnnot_SetAppearanceFromPage` then `EPDFAnnot_UpdateAppearanceToRect`
    /// (contain): draw page `page_index` of `artwork` into the widget
    /// `annot_object_number` on page `page_object_number`.
    /// `Err` names the step that refused.
    pub fn bake_widget_appearance(
        &self,
        page_object_number: u32,
        annot_object_number: u32,
        artwork: &Document,
        page_index: i32,
    ) -> Result<(), &'static str> {
        let page = self.page_by_object_number(page_object_number).map_err(|_| "page")?;
        let annotation = page.annotation_by_object_number(annot_object_number).ok_or("widget")?;
        if !annotation.set_appearance_from_page(artwork, page_index) {
            return Err("draw");
        }
        if !annotation.update_appearance_to_rect(crate::StampFit::Contain) {
            return Err("fit");
        }
        Ok(())
    }
}

/// `EPDFSig_Seal` over a saved candidate, in place.
pub fn seal_signature_candidate(
    saved: &mut SavedCandidate,
    algorithm: DigestAlgorithm,
) -> Option<SealedCandidate> {
    let mut range = [0u64; 4];
    let (mut contents_offset, mut hex_length) = (0u64, 0u64);
    let mut digest = vec![0u8; 64];
    let mut digest_len: c_ulong = 64;
    // SAFETY: the buffer is `bytes.len()` writable bytes; every out-param is valid.
    let ok = unsafe {
        pdfium_sys::EPDFSig_Seal(
            saved.bytes.as_mut_ptr(),
            saved.bytes.len() as u64,
            saved.object_offset,
            saved.object_length,
            algorithm as c_int,
            range.as_mut_ptr(),
            &mut contents_offset,
            &mut hex_length,
            digest.as_mut_ptr(),
            &mut digest_len,
        )
    };
    if ok == 0 {
        return None;
    }
    digest.truncate(digest_len as usize);
    Some(SealedCandidate {
        byte_range: range,
        contents_offset,
        contents_hex_length: hex_length,
        digest,
    })
}

/// `EPDFSig_WriteContents`: hex-encode `cms` into the `/Contents` hole.
/// False when it is not one DER object that fits.
pub fn write_signature_contents(
    bytes: &mut [u8],
    contents_offset: u64,
    contents_hex_length: u64,
    cms: &[u8],
) -> bool {
    // SAFETY: the buffer and the CMS are valid for their lengths.
    unsafe {
        pdfium_sys::EPDFSig_WriteContents(
            bytes.as_mut_ptr(),
            bytes.len() as u64,
            contents_offset,
            contents_hex_length,
            cms.as_ptr(),
            cms.len() as c_ulong,
        ) != 0
    }
}
