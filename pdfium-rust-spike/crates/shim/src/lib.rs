//! Arm B. Every export is `rs_`-prefixed, takes and returns only i32/f32/f64,
//! and does exactly what the corresponding PDFium function does. The point is
//! that the call count is identical to arm A and the only difference is one
//! extra wasm frame.
#![allow(non_snake_case)]

use pdfium_sys::*;

/// Writes width into out[0] and height into out[1]. Returns 1 on success.
#[no_mangle]
pub extern "C" fn rs_probe_page_size(doc: u32, index: i32, out: *mut f32) -> i32 {
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
