//! The browser adapter: one extern "C" entry point per operation, over the
//! `inline` dispatch epdf-ops compiles for emscripten.
//!
//! Everything except the render buffer crosses as UTF-8 JSON. A hand-packed
//! struct would have to agree in two languages, and
//! pdfium-rust-spike/crates/ops/src/record.rs:1-3 says out loud that its
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
}

fn set_error(message: impl std::fmt::Display) {
    let text = message.to_string().replace('\0', " ");
    LAST_ERROR.with(|cell| *cell.borrow_mut() = CString::new(text).unwrap());
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

/// Hands a buffer to JavaScript. `epdf_free` rebuilds the Vec with
/// `from_raw_parts(ptr, len, len)`, which is sound only when capacity equals
/// length, so shrink before leaking.
fn leak(mut bytes: Vec<u8>, out_len: *mut u32) -> *mut u8 {
    bytes.shrink_to_fit();
    // A real assert, not debug_assert: this compiles out under `cargo build
    // --release`, which is what link-wasm.sh uses to produce the shipped
    // build, and `epdf_free` rebuilds the Vec with
    // `from_raw_parts(ptr, len, len)` -- undefined behaviour the moment
    // capacity differs from length. Both callers land on an exact size today
    // (the render buffer is allocated exactly; `serde_json::to_vec` plus this
    // `shrink_to_fit` reaches one through `realloc`), but `Vec::shrink_to_fit`
    // only documents "the resulting vector might still have some excess
    // capacity", so that exactness is an allocator implementation detail, not
    // a promise. One integer comparison per crossing is free.
    assert_eq!(bytes.len(), bytes.capacity());
    let len = bytes.len();
    let ptr = bytes.as_mut_ptr();
    std::mem::forget(bytes);
    unsafe { *out_len = len as u32 };
    ptr
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
pub extern "C" fn epdf_open(bytes: *const u8, len: u32) -> u32 {
    let slice = unsafe { std::slice::from_raw_parts(bytes, len as usize) };
    let result = engine().and_then(|engine| engine.open(slice.to_vec(), None));
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
    unsafe { drop(Vec::from_raw_parts(ptr, len as usize, len as usize)) };
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

/// Panics on purpose, so a test can prove a panic crossing the wasm boundary
/// surfaces as an error rather than aborting the instance. Behind a feature so
/// it is absent from a normal build.
#[cfg(feature = "panic-probe")]
#[unsafe(no_mangle)]
pub extern "C" fn epdf_test_panic() -> i32 {
    panic!("epdf_test_panic: deliberate panic for the wasm EH test");
}
