//! Proves the generated bindings link and that one FPDF and one EPDF symbol are
//! both reachable. The EPDF one matters: it is a fork extension, so its presence
//! is what distinguishes our libembedpdf from vanilla PDFium.

use std::ffi::c_void;
use std::ptr;

fn fixture() -> Vec<u8> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/report.pdf");
    std::fs::read(path).expect("fixtures/report.pdf; run Task 2 first")
}

#[test]
fn loads_the_fixture_and_reads_a_page_size_through_an_epdf_extension() {
    let bytes = fixture();
    unsafe {
        pdfium_sys::FPDF_InitLibrary();

        let doc = pdfium_sys::FPDF_LoadMemDocument(
            bytes.as_ptr() as *const c_void,
            bytes.len() as i32,
            ptr::null(),
        );
        assert!(!doc.is_null(), "FPDF_LoadMemDocument returned null");

        let page_count = pdfium_sys::FPDF_GetPageCount(doc);
        assert!(page_count > 4, "fixture must have a page 4, got {page_count}");

        let mut size = pdfium_sys::FS_SIZEF { width: 0.0, height: 0.0 };
        let ok = pdfium_sys::EPDF_GetPageSizeByIndexNormalized(doc, 4, &mut size);
        assert_ne!(ok, 0, "EPDF_GetPageSizeByIndexNormalized failed");
        assert!(size.width > 0.0 && size.height > 0.0, "got {size:?}");

        pdfium_sys::FPDF_CloseDocument(doc);
        pdfium_sys::FPDF_DestroyLibrary();
    }
}
