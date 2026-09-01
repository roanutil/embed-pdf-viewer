//! Hand-written declarations for the slice of the PDFium C API this spike
//! touches. Handles are `u32`, not pointers: the wasm artifact is linked
//! without WASM_BIGINT, so anything 64 bits wide gets legalized at the JS
//! boundary and the spike stays clear of that entirely.
#![allow(non_camel_case_types, non_snake_case)]

pub type FPDF_DOCUMENT = u32;
pub type FPDF_BOOL = i32;

#[repr(C)]
#[derive(Copy, Clone, Default, Debug)]
pub struct FS_SIZEF {
    pub width: f32,
    pub height: f32,
}

extern "C" {
    pub fn FPDF_InitLibrary();
    pub fn FPDF_DestroyLibrary();
    pub fn FPDF_LoadMemDocument(
        data_buf: *const u8,
        size: i32,
        password: *const i8,
    ) -> FPDF_DOCUMENT;
    pub fn FPDF_CloseDocument(document: FPDF_DOCUMENT);
    pub fn EPDF_GetPageSizeByIndexNormalized(
        document: FPDF_DOCUMENT,
        page_index: i32,
        size: *mut FS_SIZEF,
    ) -> FPDF_BOOL;
}
