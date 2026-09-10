//! Every expected value comes from vectors/report-page4.json through
//! test_support, the one loader in this workspace.

use test_support::{fixture_bytes, max_abs_diff, mean_abs_diff, vectors};

#[test]
fn matches_the_frozen_vectors() {
    let v = vectors();
    let bytes = fixture_bytes();

    let lib = pdfium::Library::acquire().unwrap();
    let doc = pdfium::Document::from_bytes(lib, &bytes, None).unwrap();
    assert_eq!(doc.page_count(), v.page_count);

    let page = doc.page(v.page_index).unwrap();
    assert!((page.width() - v.width).abs() < 0.001, "{} vs {}", page.width(), v.width);
    assert!((page.height() - v.height).abs() < 0.001);

    let text_page = page.text().unwrap();
    assert_eq!(text_page.char_count(), v.char_count);
    assert_eq!(text_page.text().unwrap(), v.text);

    let hits = text_page.find("the", false).unwrap();
    assert!(!hits.is_empty(), "the fixture contains the query");
    assert!(text_page.find("", false).unwrap().is_empty());
    for query in ["\0", "the\0ignored"] {
        assert_eq!(text_page.find(query, false), Err(pdfium::PdfiumError::Search));
    }

    let bitmap = page.render_bgra(v.render_width, v.render_height).unwrap();
    assert_eq!(bitmap.width, v.render_width);
    assert_eq!(bitmap.height, v.render_height);
    assert_eq!(bitmap.stride, v.render_stride);
    let diff = mean_abs_diff(&bitmap.bgra, &v.golden);
    assert!(diff <= v.tolerance, "mean abs diff {diff} exceeds {}", v.tolerance);
    let max_diff = max_abs_diff(&bitmap.bgra, &v.golden);
    assert!(max_diff <= v.max_tolerance, "max abs diff {max_diff} exceeds {}", v.max_tolerance);
}

#[test]
fn rejects_a_page_past_the_end() {
    let bytes = fixture_bytes();
    let lib = pdfium::Library::acquire().unwrap();
    let doc = pdfium::Document::from_bytes(lib, &bytes, None).unwrap();
    let index = doc.page_count();
    assert!(matches!(
        doc.page(index),
        Err(pdfium::PdfiumError::PageOutOfRange(i)) if i == index
    ));
}

#[test]
fn rejects_bytes_that_are_not_a_pdf() {
    let lib = pdfium::Library::acquire().unwrap();
    let err = pdfium::Document::from_bytes(lib, b"not a pdf at all", None).unwrap_err();
    assert!(matches!(err, pdfium::PdfiumError::Load));
}
