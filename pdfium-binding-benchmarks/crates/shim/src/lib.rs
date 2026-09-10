//! Arm B. Every export is `rs_`-prefixed, takes and returns only i32/f32/f64,
//! and does exactly what the corresponding PDFium function does. The point is
//! that the call count is identical to arm A and the only difference is one
//! extra wasm frame.
#![allow(non_snake_case)]

use pdfium_sys::*;

/// Writes width into out[0] and height into out[1]. Returns 1 on success.
#[no_mangle]
pub extern "C" fn rs_probe_page_size(doc: FPDF_DOCUMENT, index: i32, out: *mut f32) -> i32 {
    let mut size = FS_SIZEF::default();
    let ok = unsafe { EPDF_GetPageSizeByIndexNormalized(doc, index, &mut size) };
    if ok == 0 {
        return 0;
    }
    unsafe {
        *out = size.width;
        *out.add(1) = size.height;
    }
    1
}

#[no_mangle]
pub extern "C" fn rs_EPDF_GetPageSizeByIndexNormalized(
    doc: FPDF_DOCUMENT,
    index: i32,
    size: *mut FS_SIZEF,
) -> i32 {
    unsafe { EPDF_GetPageSizeByIndexNormalized(doc, index, size) }
}

#[no_mangle]
pub extern "C" fn rs_FPDFText_CountChars(text_page: FPDF_TEXTPAGE) -> i32 {
    unsafe { FPDFText_CountChars(text_page) }
}

#[no_mangle]
pub extern "C" fn rs_FPDFText_GetTextObject(text_page: FPDF_TEXTPAGE, index: i32) -> FPDF_PAGEOBJECT {
    unsafe { FPDFText_GetTextObject(text_page, index) }
}

#[no_mangle]
pub extern "C" fn rs_FPDFText_GetFontSize(text_page: FPDF_TEXTPAGE, index: i32) -> f64 {
    unsafe { FPDFText_GetFontSize(text_page, index) }
}

#[no_mangle]
pub extern "C" fn rs_EPDFText_GetCharGeometry(
    text_page: FPDF_TEXTPAGE,
    index: i32,
    geometry: *mut EPDF_CHAR_GEOMETRY,
) -> i32 {
    unsafe { EPDFText_GetCharGeometry(text_page, index, geometry) }
}

#[no_mangle]
pub extern "C" fn rs_EPDFText_GetTextFull(text_page: FPDF_TEXTPAGE, buffer: *mut u16, len: i32) -> i32 {
    unsafe { EPDFText_GetTextFull(text_page, buffer, len) }
}

#[no_mangle]
pub extern "C" fn rs_FPDFText_FindStart(
    text_page: FPDF_TEXTPAGE,
    findwhat: *const u16,
    flags: u32,
    start_index: i32,
) -> FPDF_SCHHANDLE {
    unsafe { FPDFText_FindStart(text_page, findwhat, flags, start_index) }
}

#[no_mangle]
pub extern "C" fn rs_FPDFText_FindNext(handle: FPDF_SCHHANDLE) -> i32 {
    unsafe { FPDFText_FindNext(handle) }
}

#[no_mangle]
pub extern "C" fn rs_FPDFText_GetSchResultIndex(handle: FPDF_SCHHANDLE) -> i32 {
    unsafe { FPDFText_GetSchResultIndex(handle) }
}

#[no_mangle]
pub extern "C" fn rs_FPDFText_GetSchCount(handle: FPDF_SCHHANDLE) -> i32 {
    unsafe { FPDFText_GetSchCount(handle) }
}

#[no_mangle]
pub extern "C" fn rs_FPDFText_FindClose(handle: FPDF_SCHHANDLE) {
    unsafe { FPDFText_FindClose(handle) }
}

#[no_mangle]
pub extern "C" fn rs_FPDFText_CountRects(text_page: FPDF_TEXTPAGE, start: i32, count: i32) -> i32 {
    unsafe { FPDFText_CountRects(text_page, start, count) }
}

#[no_mangle]
pub extern "C" fn rs_FPDFText_GetRect(
    text_page: FPDF_TEXTPAGE,
    rect_index: i32,
    left: *mut f64,
    top: *mut f64,
    right: *mut f64,
    bottom: *mut f64,
) -> i32 {
    unsafe { FPDFText_GetRect(text_page, rect_index, left, top, right, bottom) }
}

/// The render workload's second PDFium call. renderThin used to reach for the
/// unprefixed `FPDFBitmap_FillRect` while going through `rs_` for the render
/// itself, so `render-1x`/`render-4x` priced roughly half the extra wasm frame
/// arm B exists to measure.
#[no_mangle]
pub extern "C" fn rs_FPDFBitmap_FillRect(
    bitmap: FPDF_BITMAP,
    left: i32,
    top: i32,
    width: i32,
    height: i32,
    color: u32,
) -> FPDF_BOOL {
    unsafe { FPDFBitmap_FillRect(bitmap, left, top, width, height, color) }
}

#[no_mangle]
pub extern "C" fn rs_FPDF_RenderPageBitmap(
    bitmap: FPDF_BITMAP,
    page: FPDF_PAGE,
    start_x: i32,
    start_y: i32,
    size_x: i32,
    size_y: i32,
    rotate: i32,
    flags: i32,
) {
    unsafe { FPDF_RenderPageBitmap(bitmap, page, start_x, start_y, size_x, size_y, rotate, flags) }
}
