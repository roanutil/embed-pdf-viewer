//! The browser adapter: one extern "C" entry point per operation, over the
//! `inline` dispatch epdf-ops compiles for emscripten.
//!
//! Everything except the render buffer crosses as UTF-8 JSON. A hand-packed
//! struct would have to agree in two languages, and
//! pdfium-binding-benchmarks/crates/ops/src/record.rs:1-3 says out loud that its
//! 112-byte record is duplicated in three places with only a fairness gate
//! proving they match. One crossing per operation is what recommendation 3 of
//! docs/research/pdfium-rust-wrapper-decision.md asks for; what rides on that
//! crossing is free to be convenient. The render path stays raw, because a
//! megabyte of base64 is not convenient.
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::CString;
use std::sync::Arc;

use serde::Serialize;

const OP_PAGE_SIZE: u32 = 0;
const OP_PAGE_TEXT: u32 = 1;
const OP_SEARCH: u32 = 2;

thread_local! {
    static ENGINE: RefCell<Option<Arc<epdf_ops::Engine>>> = const { RefCell::new(None) };
    static DOCS: RefCell<HashMap<u32, Arc<epdf_ops::Document>>> = RefCell::new(HashMap::new());
    static NEXT_ID: RefCell<u32> = const { RefCell::new(1) };
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::new("").unwrap());
    static LAST_ERROR_CODE: RefCell<CString> = RefCell::new(CString::new("").unwrap());
}

/// Records both halves of the error contract packages/node/README.md documents:
/// the stable `OpsError::code()` and the human-readable message. The JS glue
/// reads them back through `epdf_last_error_code` and `epdf_last_error`.
fn set_error(err: epdf_ops::OpsError) {
    let text = err.to_string().replace('\0', " ");
    LAST_ERROR.with(|cell| *cell.borrow_mut() = CString::new(text).unwrap());
    LAST_ERROR_CODE.with(|cell| *cell.borrow_mut() = CString::new(err.code()).unwrap());
}

fn engine() -> Result<Arc<epdf_ops::Engine>, epdf_ops::OpsError> {
    ENGINE.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            *slot = Some(epdf_ops::Engine::new()?);
        }
        Ok(Arc::clone(slot.as_ref().unwrap()))
    })
}

fn document(id: u32) -> Result<Arc<epdf_ops::Document>, epdf_ops::OpsError> {
    DOCS.with(|cell| cell.borrow().get(&id).cloned().ok_or(epdf_ops::OpsError::Closed))
}

/// Transfers an exactly sized allocation to JavaScript; epdf_free reclaims it.
fn leak(bytes: Vec<u8>, out_len: *mut u32) -> *mut u8 {
    let bytes = bytes.into_boxed_slice();
    unsafe { *out_len = bytes.len() as u32 };
    Box::into_raw(bytes) as *mut u8
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonSize {
    width: f32,
    height: f32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonRect {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonSearchHit {
    char_index: u32,
    char_count: u32,
    rects: Vec<JsonRect>,
}

#[unsafe(no_mangle)]
pub extern "C" fn epdf_open(bytes: *const u8, len: u32, password: *const u8, password_len: u32) -> u32 {
    let slice = unsafe { std::slice::from_raw_parts(bytes, len as usize) };
    let password = if password.is_null() {
        None
    } else {
        Some(String::from_utf8_lossy(unsafe {
            std::slice::from_raw_parts(password, password_len as usize)
        }).into_owned())
    };
    let result = engine().and_then(|engine| engine.open(slice.to_vec(), password));
    match result {
        Ok(doc) => {
            let id = NEXT_ID.with(|cell| {
                let mut next = cell.borrow_mut();
                let id = *next;
                *next += 1;
                id
            });
            DOCS.with(|cell| cell.borrow_mut().insert(id, doc));
            id
        }
        Err(err) => {
            set_error(err);
            0
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn epdf_page_count(doc: u32) -> i32 {
    match document(doc).and_then(|d| d.page_count()) {
        Ok(count) => count as i32,
        Err(err) => {
            set_error(err);
            -1
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn epdf_json(
    doc: u32,
    op: u32,
    index: u32,
    arg: *const u8,
    arg_len: u32,
    flag: u32,
    out_len: *mut u32,
) -> *mut u8 {
    let json = document(doc).and_then(|d| match op {
        OP_PAGE_SIZE => {
            let size = d.page_size(index)?;
            Ok(serde_json::to_vec(&JsonSize { width: size.width, height: size.height }).unwrap())
        }
        OP_PAGE_TEXT => Ok(serde_json::to_vec(&d.page_text(index)?).unwrap()),
        OP_SEARCH => {
            let query = unsafe { std::slice::from_raw_parts(arg, arg_len as usize) };
            let query = String::from_utf8_lossy(query).into_owned();
            let hits: Vec<JsonSearchHit> = d
                .search(index, query, flag != 0)?
                .into_iter()
                .map(|hit| JsonSearchHit {
                    char_index: hit.char_index,
                    char_count: hit.char_count,
                    rects: hit
                        .rects
                        .into_iter()
                        .map(|r| JsonRect { left: r.left, top: r.top, right: r.right, bottom: r.bottom })
                        .collect(),
                })
                .collect();
            Ok(serde_json::to_vec(&hits).unwrap())
        }
        _ => Err(epdf_ops::OpsError::Internal { message: format!("unknown op {op}") }),
    });

    match json {
        Ok(bytes) => leak(bytes, out_len),
        Err(err) => {
            set_error(err);
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn epdf_render(
    doc: u32,
    index: u32,
    scale: f32,
    out_w: *mut u32,
    out_h: *mut u32,
    out_stride: *mut u32,
    out_len: *mut u32,
) -> *mut u8 {
    match document(doc).and_then(|d| d.render_page(index, scale)) {
        Ok(bitmap) => {
            unsafe {
                *out_w = bitmap.width;
                *out_h = bitmap.height;
                *out_stride = bitmap.stride;
            }
            leak(bitmap.bgra, out_len)
        }
        Err(err) => {
            set_error(err);
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn epdf_free(ptr: *mut u8, len: u32) {
    if ptr.is_null() {
        return;
    }
    unsafe { drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len as usize))) };
}

#[unsafe(no_mangle)]
pub extern "C" fn epdf_close(doc: u32) {
    DOCS.with(|cell| {
        if let Some(d) = cell.borrow_mut().remove(&doc) {
            d.close();
        }
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn epdf_last_error() -> *const std::os::raw::c_char {
    LAST_ERROR.with(|cell| cell.borrow().as_ptr())
}

/// The `OpsError::code()` of the last failure, e.g. "PAGE_OUT_OF_RANGE". Read it
/// together with `epdf_last_error`; the next failing call overwrites both.
#[unsafe(no_mangle)]
pub extern "C" fn epdf_last_error_code() -> *const std::os::raw::c_char {
    LAST_ERROR_CODE.with(|cell| cell.borrow().as_ptr())
}

/// Exercises catch_unwind and borrow-guard release inside the real dispatch.
/// Returns the same error sentinel as other exports; absent from normal builds.
#[cfg(feature = "panic-probe")]
#[unsafe(no_mangle)]
pub extern "C" fn epdf_test_panic() -> i32 {
    match engine().and_then(|engine| engine.test_panic()) {
        Ok(()) => 0,
        Err(err) => { set_error(err); -1 }
    }
}

#[cfg(test)]
mod error_code_tests {
    use super::*;
    use std::ffi::CStr;

    #[test]
    fn last_error_reports_the_ops_code_beside_the_message() {
        set_error(epdf_ops::OpsError::PageOutOfRange { index: 9 });
        let code = unsafe { CStr::from_ptr(epdf_last_error_code()) }.to_str().unwrap();
        let message = unsafe { CStr::from_ptr(epdf_last_error()) }.to_str().unwrap();
        assert_eq!(code, "PAGE_OUT_OF_RANGE");
        assert_eq!(message, "page index 9 is past the end of the document");
    }
}
