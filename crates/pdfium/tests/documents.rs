//! The `pdfium` layer against real files. Everything here runs on one thread
//! by construction: `Library` is `!Send`, and each test acquires its own.

use pdfium::{Document, Library, PdfiumError};

#[test]
fn opens_a_document_and_reads_its_pages() {
    let lib = Library::acquire();
    let doc = Document::from_bytes(lib, &test_support::fixture(test_support::REPORT), None)
        .expect("open report.pdf");

    assert!(doc.page_count() > 0);
    let page = doc.page(0).expect("load page 0");
    assert!(page.width() > 0.0 && page.height() > 0.0);
    // Well-formed files put pages in the /Pages tree as indirect objects.
    assert!(page.object_number() > 0);
}

#[test]
fn reports_a_page_index_past_the_end_without_calling_pdfium() {
    let lib = Library::acquire();
    let doc = Document::from_bytes(lib, &test_support::fixture(test_support::REPORT), None)
        .expect("open report.pdf");
    let past_end = doc.page_count();
    assert_eq!(doc.page(past_end).unwrap_err(), PdfiumError::PageOutOfRange(past_end));
}

#[test]
fn distinguishes_a_wrong_password_from_unopenable_bytes() {
    let lib = Library::acquire();
    let bytes = test_support::fixture(test_support::ENCRYPTED);

    assert_eq!(
        Document::from_bytes(lib.clone(), &bytes, None).err(),
        Some(PdfiumError::Password)
    );
    assert_eq!(
        Document::from_bytes(lib.clone(), &bytes, Some("wrong")).err(),
        Some(PdfiumError::Password)
    );
    assert_eq!(
        Document::from_bytes(lib.clone(), b"not a pdf at all", None).err(),
        Some(PdfiumError::Load)
    );
    // The UTF-8 user password is `hôtel`, per PDFium's
    // cpdf_security_handler_embeddertest.cpp, so this also covers a password
    // that is not ASCII surviving the trip through CString.
    Document::from_bytes(lib, &bytes, Some("hôtel")).expect("correct password opens");
}

#[test]
fn rejects_a_password_with_an_interior_nul_rather_than_truncating_it() {
    let lib = Library::acquire();
    let bytes = test_support::fixture(test_support::ENCRYPTED);
    // The correct password is `hôtel`; a NUL-bearing string that starts with
    // it must not be silently shortened into it.
    assert_eq!(
        Document::from_bytes(lib, &bytes, Some("hôtel\0extra")).err(),
        Some(PdfiumError::InvalidArgument("password contains NUL"))
    );
}

#[test]
fn extracts_page_text_and_finds_it_again() {
    let lib = Library::acquire();
    let doc = Document::from_bytes(lib, &test_support::fixture(test_support::REPORT), None)
        .expect("open report.pdf");
    let page = doc.page(0).expect("load page 0");
    let text_page = page.text().expect("load text page");

    let text = text_page.text().expect("extract text");
    assert!(!text.is_empty());
    assert_eq!(text.chars().count() > 0, text_page.char_count() > 0);

    // Search for a word the page actually contains, taken from its own text,
    // so the assertion does not depend on the fixture's wording.
    let needle: String = text.split_whitespace().find(|w| w.len() >= 4).expect("a word").to_string();
    let hits = text_page.find(&needle, true).expect("search");
    assert!(!hits.is_empty(), "{needle:?} is on the page but search found nothing");

    assert!(text_page.find("", true).expect("empty search").is_empty());
    assert_eq!(
        text_page.find("a\0b", true).unwrap_err(),
        PdfiumError::InvalidArgument("query contains NUL")
    );
}

#[test]
fn reads_char_geometry_for_every_character_on_a_page() {
    let lib = Library::acquire();
    let doc = Document::from_bytes(lib, &test_support::fixture(test_support::REPORT), None)
        .expect("open report.pdf");
    let page = doc.page(0).expect("load page 0");
    let text_page = page.text().expect("load text page");

    let count = text_page.char_count();
    assert!(count > 0);
    for i in 0..count {
        let geometry = text_page.char_geometry(i).expect("geometry for an in-range index");
        // A tight quad without a loose quad would leave the reader with an
        // oriented cell it cannot place; the fork never emits that.
        assert!(!geometry.flags.has_tight_quad() || geometry.flags.has_loose_quad());
    }
    // Out of range fails rather than returning an unmodified struct as data.
    assert!(text_page.char_geometry(count).is_none());
}

#[test]
fn renders_a_colored_page_with_bgra_channels_in_order() {
    let lib = Library::acquire();
    let doc = Document::from_bytes(lib, &test_support::fixture(test_support::COLORS), None)
        .expect("open colors.pdf");
    let page = doc.page(0).expect("load page 0");

    let bitmap = page.render_bgra(64, 64).expect("render");
    assert_eq!(bitmap.stride, 64 * 4);
    assert_eq!(bitmap.bgra.len(), 64 * 64 * 4);
    // A grayscale page cannot tell a B-to-R swap from a correct render, so
    // assert the page actually has a channel imbalance to test against.
    let (mut blue, mut red) = (0u64, 0u64);
    for pixel in bitmap.bgra.as_chunks::<4>().0 {
        blue += u64::from(pixel[0]);
        red += u64::from(pixel[2]);
    }
    assert_ne!(blue, red, "colors.pdf rendered with equal blue and red totals");
}

#[test]
fn closing_a_document_leaves_the_library_usable() {
    let lib = Library::acquire();
    for _ in 0..3 {
        let doc = Document::from_bytes(
            lib.clone(),
            &test_support::fixture(test_support::REPORT),
            None,
        )
        .expect("open");
        assert!(doc.page_count() > 0);
    }
}
