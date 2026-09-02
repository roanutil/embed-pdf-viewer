//! Native arms B and C behind one addon. Every export mirrors the wasm arm of
//! the same name, so bench/workloads.mjs needs no native-specific branch.
//!
//! Pointers cross as f64. JavaScript numbers hold every integer below 2^53
//! exactly and a macOS user-space pointer is 47 bits, so this is lossless, and
//! it avoids the BigInt conversion the shipping addon pays on every call.
//!
//! Arm C's four `rs_*` wrappers below hand a real 64-bit pointer to
//! `ops::rs_*`, whose parameters are the `pdfium-sys` handle aliases (64-bit
//! on this target). `p(v) as _` there infers that 64-bit type and is
//! lossless.
#![allow(non_snake_case)]

use napi::bindgen_prelude::*;
use napi::JsArrayBuffer;
use napi_derive::napi;
use pdfium_sys::*;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

#[inline]
fn p(v: f64) -> usize {
    v as usize
}

/// Byte length handed to `alloc` for every pointer it has returned, so
/// `view_buffer` (Fix 2) can refuse a `len` past what the caller is actually
/// entitled to instead of aliasing whatever foreign memory happens to sit
/// past the allocation. Never pruned: `free_ptr` below already deliberately
/// leaks for the same reason (a fixed, small set of scratch buffers,
/// allocated once), so an entry here outliving its allocation costs nothing
/// this benchmark's lifetime notices.
static ALLOCATIONS: LazyLock<Mutex<HashMap<usize, usize>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[napi]
pub fn init_library() {
    unsafe { FPDF_InitLibrary() }
}

#[napi]
pub fn destroy_library() {
    unsafe { FPDF_DestroyLibrary() }
}

/// 16-byte aligned: `peek_f64` reads `*(p as *const f64)` from this memory and
/// `FPDFBitmap_CreateEx` uses it as a scan buffer, so the byte-aligned `Vec<u8>`
/// this used to hand out (alignment 1) was UB by the letter, even though
/// macOS malloc happens to return 16-byte-aligned memory in practice.
#[napi]
pub fn alloc(bytes: u32) -> f64 {
    let size = bytes.max(1) as usize;
    let layout = std::alloc::Layout::from_size_align(size, 16).expect("invalid allocation layout");
    let ptr = unsafe { std::alloc::alloc(layout) };
    if ptr.is_null() {
        std::alloc::handle_alloc_error(layout);
    }
    ALLOCATIONS.lock().unwrap().insert(ptr as usize, size);
    ptr as usize as f64
}

#[napi]
pub fn free_ptr(_ptr: f64) {
    // Deliberately leaks. The benchmark allocates a fixed set of scratch
    // buffers once and lives for a few seconds; tracking capacities to
    // reconstruct the Layout would add bookkeeping to no measured benefit.
}

#[napi]
pub fn peek32(ptr: f64) -> i32 {
    unsafe { *(p(ptr) as *const i32) }
}

#[napi]
pub fn peek_f32(ptr: f64) -> f64 {
    unsafe { *(p(ptr) as *const f32) as f64 }
}

#[napi]
pub fn peek_f64(ptr: f64) -> f64 {
    unsafe { *(p(ptr) as *const f64) }
}

#[napi]
pub fn read_bytes(ptr: f64, len: u32) -> Buffer {
    let slice = unsafe { std::slice::from_raw_parts(p(ptr) as *const u8, len as usize) };
    Buffer::from(slice.to_vec())
}

#[napi]
pub fn write_bytes(ptr: f64, data: Buffer) {
    let dst = unsafe { std::slice::from_raw_parts_mut(p(ptr) as *mut u8, data.len()) };
    dst.copy_from_slice(&data);
}

#[napi]
pub fn write_u16(s: String) -> f64 {
    let mut units: Vec<u16> = s.encode_utf16().collect();
    units.push(0);
    let ptr = units.as_mut_ptr();
    std::mem::forget(units);
    ptr as usize as f64
}

#[napi]
pub fn load_mem_document(data: Buffer) -> f64 {
    let mut owned = data.to_vec();
    let ptr = owned.as_mut_ptr();
    let len = owned.len() as i32;
    std::mem::forget(owned);
    unsafe { FPDF_LoadMemDocument(ptr, len, std::ptr::null()) as f64 }
}

macro_rules! passthrough {
    ($name:ident, $target:path, ($($arg:ident : $ty:ty),*) -> $ret:ty) => {
        #[napi]
        pub fn $name($($arg: $ty),*) -> $ret {
            unsafe { $target($($arg as _),*) as $ret }
        }
    };
}

passthrough!(close_document, FPDF_CloseDocument, (doc: f64) -> ());
passthrough!(get_page_count, FPDF_GetPageCount, (doc: f64) -> i32);
passthrough!(load_page, FPDF_LoadPage, (doc: f64, index: i32) -> f64);
passthrough!(close_page, FPDF_ClosePage, (page: f64) -> ());
passthrough!(get_page_width, FPDF_GetPageWidthF, (page: f64) -> f64);
passthrough!(get_page_height, FPDF_GetPageHeightF, (page: f64) -> f64);
passthrough!(text_load_page, FPDFText_LoadPage, (page: f64) -> f64);
passthrough!(text_close_page, FPDFText_ClosePage, (tp: f64) -> ());
passthrough!(text_count_chars, FPDFText_CountChars, (tp: f64) -> i32);
passthrough!(text_get_text_object, FPDFText_GetTextObject, (tp: f64, i: i32) -> f64);
passthrough!(text_get_font_size, FPDFText_GetFontSize, (tp: f64, i: i32) -> f64);
passthrough!(text_find_next, FPDFText_FindNext, (h: f64) -> i32);
passthrough!(text_get_sch_result_index, FPDFText_GetSchResultIndex, (h: f64) -> i32);
passthrough!(text_get_sch_count, FPDFText_GetSchCount, (h: f64) -> i32);
passthrough!(text_find_close, FPDFText_FindClose, (h: f64) -> ());
passthrough!(text_count_rects, FPDFText_CountRects, (tp: f64, s: i32, c: i32) -> i32);
passthrough!(bitmap_destroy, FPDFBitmap_Destroy, (b: f64) -> ());

#[napi]
pub fn page_size_normalized(doc: f64, index: i32, out: f64) -> i32 {
    unsafe { EPDF_GetPageSizeByIndexNormalized(p(doc) as _, index, p(out) as *mut FS_SIZEF) }
}

#[napi]
pub fn char_geometry(tp: f64, index: i32, out: f64) -> i32 {
    unsafe {
        EPDFText_GetCharGeometry(p(tp) as _, index, p(out) as *mut EPDF_CHAR_GEOMETRY)
    }
}

#[napi]
pub fn text_full(tp: f64, buf: f64, len: i32) -> i32 {
    unsafe { EPDFText_GetTextFull(p(tp) as _, p(buf) as *mut u16, len) }
}

#[napi]
pub fn text_find_start(tp: f64, term: f64, flags: u32, start: i32) -> f64 {
    unsafe { FPDFText_FindStart(p(tp) as _, p(term) as *const u16, flags, start) as f64 }
}

#[napi]
pub fn text_get_rect(tp: f64, i: i32, l: f64, t: f64, r: f64, b: f64) -> i32 {
    unsafe {
        FPDFText_GetRect(
            p(tp) as _,
            i,
            p(l) as *mut f64,
            p(t) as *mut f64,
            p(r) as *mut f64,
            p(b) as *mut f64,
        )
    }
}

#[napi]
pub fn bitmap_create(w: i32, h: i32, buf: f64, stride: i32) -> f64 {
    unsafe { FPDFBitmap_CreateEx(w, h, FPDFBitmap_BGRA, p(buf) as *mut _, stride) as f64 }
}

#[napi]
pub fn bitmap_fill_rect(b: f64, l: i32, t: i32, w: i32, h: i32, color: u32) {
    unsafe { FPDFBitmap_FillRect(p(b) as _, l, t, w, h, color) };
}

#[napi]
pub fn render_page_bitmap(b: f64, page: f64, w: i32, h: i32, rotate: i32, flags: i32) {
    unsafe { FPDF_RenderPageBitmap(p(b) as _, p(page) as _, 0, 0, w, h, rotate, flags) }
}

// Arm C: the four coarse operations, calling straight into the ops crate. See
// the module doc comment: ops's parameters are the pdfium-sys handle aliases
// (64-bit on this target), so `as _` below infers that type and is lossless.

#[napi]
pub fn rs_read_page_geometry(tp: f64, out: f64, cap: i32) -> i32 {
    ops::rs_read_page_geometry(p(tp) as _, p(out) as *mut u8, cap)
}

#[napi]
pub fn rs_read_page_text(tp: f64, buf: f64, len: i32) -> i32 {
    ops::rs_read_page_text(p(tp) as _, p(buf) as *mut u16, len)
}

#[napi]
pub fn rs_render_page(page: f64, bitmap: f64, w: i32, h: i32, rotate: i32) -> i32 {
    ops::rs_render_page(p(page) as _, p(bitmap) as _, w, h, rotate)
}

#[napi]
pub fn rs_search_page(tp: f64, term: f64, out: f64, cap: i32) -> i32 {
    ops::rs_search_page(p(tp) as _, p(term) as *const u16, p(out) as *mut f32, cap)
}

/// A zero-copy view onto native memory, for the coarse arms' read-back path
/// (bench/workloads.mjs's `geometryCoarse`/`searchCoarse`). The wasm arms read
/// their packed buffer straight out of the live `HEAPU8.buffer`, paying no
/// copy; a native arm has no such heap to view into, so this hands back a
/// real JS `ArrayBuffer` that ALIASES `[ptr, ptr+len)` rather than copying it,
/// via napi-rs's external-arraybuffer constructor, so a native coarse arm
/// pays the same "one crossing, then a typed read" cost the wasm arms do
/// instead of an extra `read_bytes`-shaped memcpy the wasm side never pays.
///
/// # Safety contract with the caller
/// The returned ArrayBuffer aliases scratch memory this addon owns (`alloc`
/// above); the caller must not read it after that memory is freed or reused,
/// and must rebuild the view on every call rather than caching it, exactly as
/// it must for the wasm heap (which can move on growth).
#[napi]
pub fn view_buffer(env: Env, ptr: f64, len: u32) -> Result<JsArrayBuffer> {
    // A zero-length external arraybuffer would carry a null backing pointer
    // (napi-rs's own `create_arraybuffer_with_borrowed_data`, matching the
    // rule node-api and V8 apply to a real 0x0 buffer), and node then hands
    // back a buffer `new DataView` refuses as detached. `n == 0` happens for
    // real: a search fixture with no hits writes zero records. A plain
    // VM-owned zero-length buffer sidesteps the case entirely, since its
    // content can never be read.
    if len == 0 {
        return Ok(env.create_arraybuffer(0)?.value);
    }
    let addr = p(ptr);
    // Fix 2: `len` used to be trusted outright. A negative record count from
    // a coarse function (there is currently no code path that produces one,
    // but nothing enforced it either) would wrap to roughly 4 GiB as a u32
    // and hand back an external ArrayBuffer spanning 4 GiB of memory this
    // addon does not own, silently readable from JS, where wasm's DataView
    // would throw RangeError instead. Bound `len` against the allocation the
    // caller actually owns rather than trusting it, so the failure mode here
    // is a thrown JS error, not silent corruption.
    match ALLOCATIONS.lock().unwrap().get(&addr).copied() {
        Some(cap) if (len as usize) <= cap => {}
        Some(cap) => {
            return Err(Error::from_reason(format!(
                "view_buffer: len {len} exceeds the {cap}-byte allocation at this pointer"
            )));
        }
        None => {
            return Err(Error::from_reason(
                "view_buffer: pointer was not returned by alloc(); refusing to hand out a \
                 view whose size this addon cannot verify",
            ));
        }
    }
    let data = addr as *mut u8;
    let view =
        unsafe { env.create_arraybuffer_with_borrowed_data(data, len as usize, (), |_, _| {}) }?;
    Ok(view.value)
}
