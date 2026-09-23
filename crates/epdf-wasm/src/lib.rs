//! Browser and Node worker binding layer, built for `wasm32-unknown-emscripten`.
//!
//! Stage A, and only Stage A. TypeScript owns the document session; every
//! export here borrows a page pointer the host already holds, reads it, and
//! returns. Nothing is retained between calls, nothing is closed, and there is
//! no state in this crate at all. The semantics live in `epdf-engine`; this
//! file is the C ABI and the buffer handoff.
//!
//! # Why borrowing is safe here, and only here
//!
//! Rust and PDFium share one Emscripten module, so a page pointer is an offset
//! into the same linear memory on both sides: no marshalling, no second
//! address space. The host acquires through
//! `document-session/pages/PagePtrPool.acquire` and releases in a `finally`,
//! which is what keeps the page alive across the call. Rust must not call
//! `FPDF_LoadPage` itself, because that would bypass the pool's normalized
//! rotation, its shared refcounts and its structural-mutation leak checks.
//!
//! Every export therefore has the same unwritten precondition: the pointer was
//! acquired by the owner, on this thread, and outlives the call.
//!
//! # Toolchain note, an exit criterion rather than a footnote
//!
//! `-O3` makes `em++` run `wasm-opt`, emsdk 3.1.72's bundled Binaryen v119
//! rejects two WASM features present in `rustc` 1.98.0's precompiled std, and
//! Emscripten takes Binaryen from its config rather than `PATH`. The build
//! selects it explicitly through `EM_BINARYEN_ROOT` and asserts the version it
//! got.

use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};

use epdf_engine::access::BorrowedPage;
use epdf_engine::cancel::CancelToken;
use epdf_engine::error::{EngineError, EngineErrorCode};
use epdf_engine::features::geometry::read_page_geometry;

/// One JSON envelope for both outcomes, so the host has one parse and one
/// shape to branch on.
///
/// Untagged with a `Result`-shaped pair rather than a tag field, because the
/// TypeScript side already discriminates on the presence of `error` in the
/// worker protocol and a second convention would be a second thing to get
/// wrong.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Envelope<T> {
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<EngineError>,
}

impl<T> Envelope<T> {
    fn ok(value: T) -> Self {
        Self { result: Some(value), error: None }
    }

    fn err(error: EngineError) -> Self {
        Self { result: None, error: Some(error) }
    }
}

/// Encodes an envelope and hands the host an owned buffer.
///
/// The buffer is leaked deliberately: it belongs to the host until it calls
/// [`epdf_buffer_free`] with the same pointer and length. Length comes back
/// through an out-parameter rather than a NUL terminator because the payload is
/// UTF-8 JSON that may legitimately contain any byte a string can.
fn hand_off<T: serde::Serialize>(envelope: &Envelope<T>, out_len: *mut u32) -> *mut u8 {
    // An envelope that will not serialize is a bug in this crate, not a
    // document problem, so it reports as one rather than as a PDF error.
    let json = serde_json::to_vec(envelope).unwrap_or_else(|e| {
        format!(r#"{{"error":{{"code":"Unknown","message":"envelope encode failed: {e}"}}}}"#)
            .into_bytes()
    });
    let mut boxed = json.into_boxed_slice();
    let ptr = boxed.as_mut_ptr();
    let len = boxed.len();
    std::mem::forget(boxed);
    // SAFETY: the host passes a pointer to a live u32 it allocated. A null one
    // means the caller is broken, and dropping the length would leak the
    // buffer silently, so this is checked rather than assumed.
    if out_len.is_null() {
        // Nothing can be returned safely without a length, so free and refuse.
        unsafe { drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len))) };
        return std::ptr::null_mut();
    }
    unsafe { *out_len = len as u32 };
    ptr
}

/// Runs a borrowed-page read, converting every failure mode into an envelope.
///
/// `catch_unwind` is not decoration. The workspace deliberately does not set
/// `panic = "abort"`, so a panic here would otherwise unwind across the C ABI,
/// which is undefined behaviour. A panic is reported as `Unknown` and the
/// session is the host's to decide about.
fn with_borrowed_page<T: serde::Serialize>(
    page: *mut c_void,
    out_len: *mut u32,
    read: impl FnOnce(&BorrowedPage<'_>) -> Result<T, EngineError>,
) -> *mut u8 {
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: the host acquired this page and keeps it open across the
        // call. `BorrowedPage::from_raw` rejects null rather than dereferencing
        // it; nothing else about the pointer can be checked from here.
        let borrowed = unsafe { BorrowedPage::from_raw(page) }?;
        read(&borrowed)
    }));

    match outcome {
        Ok(Ok(value)) => hand_off(&Envelope::ok(value), out_len),
        Ok(Err(error)) => hand_off(&Envelope::<T>::err(error), out_len),
        Err(_) => hand_off(
            &Envelope::<T>::err(EngineError::with_message(
                EngineErrorCode::Unknown,
                "the Rust engine panicked; the session may be poisoned",
            )),
            out_len,
        ),
    }
}

/// Reads one page's geometry. G2, `pages.geometry`.
///
/// `page` is a live `FPDF_PAGE` the host acquired and will release. Writes the
/// JSON byte length to `out_len` and returns the buffer, which the caller owns
/// and frees with [`epdf_buffer_free`]. Returns null only when `out_len` is
/// null, which no correct caller does.
///
/// # Safety
/// `page` must be a page pointer acquired by the session owner, on this
/// thread, still open for the duration of this call. `out_len` must point to a
/// writable `u32`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn epdf_page_geometry(page: *mut c_void, out_len: *mut u32) -> *mut u8 {
    // Cancellation is the host's job in Stage A: the whole read is one
    // synchronous call inside one worker message, so there is no second thread
    // to set a flag. `CancelToken::never` says that rather than implying a
    // token exists.
    with_borrowed_page(page, out_len, |borrowed| {
        read_page_geometry(borrowed, &CancelToken::never())
    })
}

/// Frees a buffer handed out by this module.
///
/// # Safety
/// `ptr` and `len` must be exactly what an export of this module returned and
/// wrote, and must not have been freed already.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn epdf_buffer_free(ptr: *mut u8, len: u32) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: the contract above. Reconstructing the same Box the handoff
    // forgot is what makes the allocation balance.
    unsafe { drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len as usize))) };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reading the envelope the way the host does, so the tests assert the
    /// wire rather than the Rust types.
    fn take(ptr: *mut u8, len: u32) -> serde_json::Value {
        assert!(!ptr.is_null());
        let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) }.to_vec();
        unsafe { epdf_buffer_free(ptr, len) };
        serde_json::from_slice(&bytes).expect("the envelope is JSON")
    }

    #[test]
    fn a_null_page_reports_invalid_arg_rather_than_dereferencing_it() {
        let mut len = 0u32;
        let value = take(unsafe { epdf_page_geometry(std::ptr::null_mut(), &mut len) }, len);
        assert_eq!(value["error"]["code"], "InvalidArg");
        assert!(value.get("result").is_none(), "an error envelope carries no result");
    }

    #[test]
    fn a_null_length_pointer_refuses_rather_than_leaking_the_buffer() {
        // Returning a pointer whose length the caller cannot learn would leak
        // it forever, so the only safe answer is null.
        let ptr = unsafe { epdf_page_geometry(std::ptr::null_mut(), std::ptr::null_mut()) };
        assert!(ptr.is_null());
    }

    #[test]
    fn freeing_a_null_buffer_is_a_no_op() {
        unsafe { epdf_buffer_free(std::ptr::null_mut(), 0) };
    }
}

/// The Stage B bridge spike's exports. Throwaway, deleted with the spike.
///
/// Stage A borrows a page pointer from a TypeScript-owned session. These go
/// the other way: Rust owns the document and the host leases a pointer into
/// it. That is the direction where a protocol mistake is a use-after-free
/// rather than a wrong answer, which is why it gets proven before `G10` moves
/// `DocumentSession` behind it.
///
/// One session, in a thread-local, because the spike needs exactly one and a
/// handle table would be the part that outlives the spike. PDFium handles
/// never cross threads, so thread-local is also the only correct place for it.
pub mod spike {
    use std::cell::RefCell;

    use epdf_engine::error::{EngineError, EngineErrorCode};
    use epdf_engine::spike::lease::{LeaseOutcome, LeaseToken, SpikeSession};
    use pdfium::{Document, Library};

    thread_local! {
        static SESSION: RefCell<Option<SpikeSession>> = const { RefCell::new(None) };
    }

    #[derive(serde::Serialize, Default)]
    #[serde(rename_all = "camelCase")]
    struct SpikeResult {
        /// Non-zero only for checkout, where it is the leased document pointer.
        document: u32,
        token: u64,
        mutation_sequence: u64,
        form_model_invalidated: bool,
        failed: bool,
        /// Whether `close` actually released the document, or is waiting for a
        /// lease to come home.
        released: bool,
        annotations: u32,
        page_count: u32,
    }


    fn respond(
        out_len: *mut u32,
        run: impl FnOnce(&mut Option<SpikeSession>) -> Result<SpikeResult, EngineError>,
    ) -> *mut u8 {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            SESSION.with(|cell| run(&mut cell.borrow_mut()))
        }));
        match outcome {
            Ok(Ok(value)) => super::hand_off(&super::Envelope::ok(value), out_len),
            Ok(Err(error)) => super::hand_off(&super::Envelope::<SpikeResult>::err(error), out_len),
            Err(_) => super::hand_off(
                &super::Envelope::<SpikeResult>::err(EngineError::with_message(
                    EngineErrorCode::Unknown,
                    "the Rust spike panicked",
                )),
                out_len,
            ),
        }
    }

    fn open_session(slot: &mut Option<SpikeSession>) -> Result<&mut SpikeSession, EngineError> {
        slot.as_mut().ok_or_else(|| {
            EngineError::with_message(EngineErrorCode::DocNotOpen, "no spike session")
        })
    }

    /// Opens a document Rust owns, replacing any previous spike session.
    ///
    /// # Safety
    /// `bytes` must point to `len` readable bytes.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn epdf_spike_open(
        bytes: *const u8,
        len: u32,
        out_len: *mut u32,
    ) -> *mut u8 {
        respond(out_len, |slot| {
            if bytes.is_null() {
                return Err(EngineError::with_message(EngineErrorCode::InvalidArg, "null bytes"));
            }
            let input = unsafe { std::slice::from_raw_parts(bytes, len as usize) };
            let document = Document::from_bytes(Library::acquire(), input, None).map_err(|e| {
                EngineError::with_message(EngineErrorCode::DocOpenFailed, e.to_string())
            })?;
            let session = SpikeSession::new(document);
            let page_count = session.page_count()?;
            *slot = Some(session);
            Ok(SpikeResult { page_count, ..Default::default() })
        })
    }

    /// Leases the document pointer to the host.
    ///
    /// # Safety
    /// `out_len` must point to a writable `u32`.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn epdf_spike_checkout(out_len: *mut u32) -> *mut u8 {
        respond(out_len, |slot| {
            let (token, document) = open_session(slot)?.checkout()?;
            Ok(SpikeResult {
                document: document as usize as u32,
                token: token.get(),
                ..Default::default()
            })
        })
    }

    /// Takes the lease back, with what the host did.
    ///
    /// # Safety
    /// `out_len` must point to a writable `u32`.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn epdf_spike_return(
        token: u64,
        wrote: u32,
        failed: u32,
        out_len: *mut u32,
    ) -> *mut u8 {
        respond(out_len, |slot| {
            open_session(slot)?.return_lease(
                LeaseToken::from_raw(token),
                LeaseOutcome { wrote: wrote != 0, failed: failed != 0 },
            )?;
            Ok(SpikeResult { token, ..Default::default() })
        })
    }

    /// Settles the outcome and completes any pending close.
    ///
    /// # Safety
    /// `out_len` must point to a writable `u32`.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn epdf_spike_finalize(token: u64, out_len: *mut u32) -> *mut u8 {
        respond(out_len, |slot| {
            let settled = open_session(slot)?.finalize(LeaseToken::from_raw(token))?;
            Ok(SpikeResult {
                token,
                mutation_sequence: settled.mutation_sequence,
                form_model_invalidated: settled.form_model_invalidated,
                failed: settled.failed,
                ..Default::default()
            })
        })
    }

    /// Counts a page's annotations through the document Rust owns.
    ///
    /// # Safety
    /// `out_len` must point to a writable `u32`.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn epdf_spike_annotations(page_index: u32, out_len: *mut u32) -> *mut u8 {
        respond(out_len, |slot| {
            let annotations = open_session(slot)?.annotation_count(page_index)?;
            Ok(SpikeResult { annotations, ..Default::default() })
        })
    }

    /// Rejects new work; releases the document once the protocol settles.
    ///
    /// # Safety
    /// `out_len` must point to a writable `u32`.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn epdf_spike_close(out_len: *mut u32) -> *mut u8 {
        respond(out_len, |slot| {
            let released = open_session(slot)?.close();
            if released {
                *slot = None;
            }
            Ok(SpikeResult { released, ..Default::default() })
        })
    }
}
