//! Hand-written declarations for the slice of the PDFium C API this spike
//! touches. On wasm32 handles are `u32`, not pointers: the wasm artifact is
//! linked without WASM_BIGINT, so anything 64 bits wide gets legalized at the
//! JS boundary and the wasm arms stay clear of that entirely. On a real
//! 64-bit target (aarch64-apple-darwin, for the native arms) a PDFium handle
//! is an honest 64-bit pointer, so the aliases widen to match.
#![allow(non_camel_case_types, non_snake_case)]

#[cfg(target_pointer_width = "32")]
pub type FPDF_DOCUMENT = u32;
#[cfg(target_pointer_width = "64")]
pub type FPDF_DOCUMENT = u64;

#[cfg(target_pointer_width = "32")]
pub type FPDF_PAGE = u32;
#[cfg(target_pointer_width = "64")]
pub type FPDF_PAGE = u64;

#[cfg(target_pointer_width = "32")]
pub type FPDF_TEXTPAGE = u32;
#[cfg(target_pointer_width = "64")]
pub type FPDF_TEXTPAGE = u64;

#[cfg(target_pointer_width = "32")]
pub type FPDF_PAGEOBJECT = u32;
#[cfg(target_pointer_width = "64")]
pub type FPDF_PAGEOBJECT = u64;

#[cfg(target_pointer_width = "32")]
pub type FPDF_BITMAP = u32;
#[cfg(target_pointer_width = "64")]
pub type FPDF_BITMAP = u64;

#[cfg(target_pointer_width = "32")]
pub type FPDF_SCHHANDLE = u32;
#[cfg(target_pointer_width = "64")]
pub type FPDF_SCHHANDLE = u64;

pub type FPDF_BOOL = i32;

pub const FPDFBitmap_BGRA: i32 = 4;

#[repr(C)]
#[derive(Copy, Clone, Default, Debug)]
pub struct FS_SIZEF {
    pub width: f32,
    pub height: f32,
}

#[repr(C)]
#[derive(Copy, Clone, Default, Debug)]
pub struct FS_RECTF {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

#[repr(C)]
#[derive(Copy, Clone, Default, Debug)]
pub struct FS_QUADPOINTSF {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
    pub x3: f32,
    pub y3: f32,
    pub x4: f32,
    pub y4: f32,
}

#[repr(C)]
#[derive(Copy, Clone, Default, Debug)]
pub struct FS_MATRIX {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub e: f32,
    pub f: f32,
}

/// Mirrors EPDF_CHAR_GEOMETRY in
/// packages/engine/runtime/runtime-src/public/epdf_text.h:35.
#[repr(C)]
#[derive(Copy, Clone, Default, Debug)]
pub struct EPDF_CHAR_GEOMETRY {
    pub loose_box: FS_RECTF,
    pub tight_box: FS_RECTF,
    pub loose_quad: FS_QUADPOINTSF,
    pub tight_quad: FS_QUADPOINTSF,
    pub matrix: FS_MATRIX,
    pub flags: u32,
}

pub const EPDF_CHARGEO_HAS_TIGHT_BOX: u32 = 1 << 0;
pub const EPDF_CHARGEO_HAS_LOOSE_QUAD: u32 = 1 << 1;
pub const EPDF_CHARGEO_HAS_TIGHT_QUAD: u32 = 1 << 2;
pub const EPDF_CHARGEO_UPRIGHT: u32 = 1 << 3;
pub const EPDF_CHARGEO_SPACE: u32 = 1 << 4;
pub const EPDF_CHARGEO_EMPTY: u32 = 1 << 5;
pub const EPDF_CHARGEO_SYNTHESIZED: u32 = 1 << 6;

extern "C" {
    pub fn FPDF_InitLibrary();
    pub fn FPDF_DestroyLibrary();
    pub fn FPDF_LoadMemDocument(
        data_buf: *const u8,
        size: i32,
        password: *const i8,
    ) -> FPDF_DOCUMENT;
    pub fn FPDF_CloseDocument(document: FPDF_DOCUMENT);
    pub fn FPDF_GetPageCount(document: FPDF_DOCUMENT) -> i32;
    pub fn FPDF_LoadPage(document: FPDF_DOCUMENT, page_index: i32) -> FPDF_PAGE;
    pub fn FPDF_ClosePage(page: FPDF_PAGE);
    pub fn FPDF_GetPageWidthF(page: FPDF_PAGE) -> f32;
    pub fn FPDF_GetPageHeightF(page: FPDF_PAGE) -> f32;
    pub fn EPDF_GetPageSizeByIndexNormalized(
        document: FPDF_DOCUMENT,
        page_index: i32,
        size: *mut FS_SIZEF,
    ) -> FPDF_BOOL;

    pub fn FPDFText_LoadPage(page: FPDF_PAGE) -> FPDF_TEXTPAGE;
    pub fn FPDFText_ClosePage(text_page: FPDF_TEXTPAGE);
    pub fn FPDFText_CountChars(text_page: FPDF_TEXTPAGE) -> i32;
    pub fn FPDFText_GetTextObject(text_page: FPDF_TEXTPAGE, index: i32) -> FPDF_PAGEOBJECT;
    pub fn FPDFText_GetFontSize(text_page: FPDF_TEXTPAGE, index: i32) -> f64;
    pub fn EPDFText_GetCharGeometry(
        text_page: FPDF_TEXTPAGE,
        index: i32,
        geometry: *mut EPDF_CHAR_GEOMETRY,
    ) -> FPDF_BOOL;
    pub fn EPDFText_GetTextFull(
        text_page: FPDF_TEXTPAGE,
        buffer: *mut u16,
        buffer_len: i32,
    ) -> i32;

    pub fn FPDFText_FindStart(
        text_page: FPDF_TEXTPAGE,
        findwhat: *const u16,
        flags: u32,
        start_index: i32,
    ) -> FPDF_SCHHANDLE;
    pub fn FPDFText_FindNext(handle: FPDF_SCHHANDLE) -> FPDF_BOOL;
    pub fn FPDFText_GetSchResultIndex(handle: FPDF_SCHHANDLE) -> i32;
    pub fn FPDFText_GetSchCount(handle: FPDF_SCHHANDLE) -> i32;
    pub fn FPDFText_FindClose(handle: FPDF_SCHHANDLE);
    pub fn FPDFText_GetRect(
        text_page: FPDF_TEXTPAGE,
        rect_index: i32,
        left: *mut f64,
        top: *mut f64,
        right: *mut f64,
        bottom: *mut f64,
    ) -> FPDF_BOOL;
    pub fn FPDFText_CountRects(text_page: FPDF_TEXTPAGE, start: i32, count: i32) -> i32;

    pub fn FPDFBitmap_CreateEx(
        width: i32,
        height: i32,
        format: i32,
        first_scan: *mut core::ffi::c_void,
        stride: i32,
    ) -> FPDF_BITMAP;
    pub fn FPDFBitmap_FillRect(
        bitmap: FPDF_BITMAP,
        left: i32,
        top: i32,
        width: i32,
        height: i32,
        color: u32,
    ) -> FPDF_BOOL;
    pub fn FPDFBitmap_Destroy(bitmap: FPDF_BITMAP);
    pub fn FPDF_RenderPageBitmap(
        bitmap: FPDF_BITMAP,
        page: FPDF_PAGE,
        start_x: i32,
        start_y: i32,
        size_x: i32,
        size_y: i32,
        rotate: i32,
        flags: i32,
    );
}
