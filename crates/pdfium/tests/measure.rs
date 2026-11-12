//! Measurement viewports (`/VP`) through the fork's `epdf_measure.h`.

use pdfium::measure::{MeasureAxis, MeasureSubtype, MeasureText};
use pdfium::{Document, Library, RectF};

fn foreign() -> Document {
    let lib = Library::acquire();
    Document::from_bytes(lib, &test_support::fixture(test_support::MEASURE_FOREIGN_VIEWPORT), None)
        .expect("open measure-foreign-viewport.pdf")
}

#[test]
fn a_foreign_viewport_reads_with_its_measure() {
    let doc = foreign();
    let page = doc.page(0).unwrap();
    assert_eq!(page.viewport_count(), 1);
    let vp = page.viewport(0).expect("a viewport");
    assert!(vp.bbox().is_some());
    assert_eq!(vp.name().unwrap().as_deref(), Some("Acrobat"));
    let measure = vp.measure().expect("a measure");
    assert_eq!(measure.subtype(), MeasureSubtype::Rl);
    assert!(measure.format_count(MeasureAxis::Distance) > 0);
    let format = measure.format(MeasureAxis::Distance, 0).expect("a distance format");
    assert!(!format.unit().unwrap().unwrap_or_default().is_empty());
    assert!(format.conversion().is_some_and(|c| c > 0.0));
}

#[test]
fn a_written_viewport_reads_back_and_removes() {
    let doc = foreign();
    let page = doc.page(0).unwrap();
    let bbox = RectF { left: 0.0, top: 792.0, right: 612.0, bottom: 0.0 };
    let vp = page.add_viewport(bbox).expect("added");
    assert!(vp.set_name("EmbedPDF"));
    let measure = vp.add_measure().expect("a fresh measure");
    assert!(measure.set_ratio("1 in = 1 m"));
    let x = measure.add_format(MeasureAxis::X, "m", 1.0 / 72.0).expect("x format");
    assert!(x.set_precision(100));
    assert!(x.set_text(MeasureText::DecimalSeparator, ","));
    assert!(measure.add_format(MeasureAxis::Distance, "m", 1.0).is_some());
    assert!(measure.add_format(MeasureAxis::Area, "m²", 1.0).is_some());

    assert_eq!(page.viewport_count(), 2);
    let read = page.viewport(1).unwrap();
    assert_eq!(read.name().unwrap().as_deref(), Some("EmbedPDF"));
    let m = read.measure().unwrap();
    assert_eq!(m.ratio().unwrap().as_deref(), Some("1 in = 1 m"));
    let x = m.format(MeasureAxis::X, 0).unwrap();
    assert_eq!(x.precision(), Some(100));
    assert_eq!(x.text(MeasureText::DecimalSeparator).unwrap().as_deref(), Some(","));
    assert_eq!(x.text(MeasureText::ThousandsSeparator).unwrap(), None);
    assert_eq!(m.format(MeasureAxis::Area, 0).unwrap().unit().unwrap().as_deref(), Some("m²"));

    assert!(page.remove_viewport(1));
    assert_eq!(page.viewport_count(), 1);
}

/// The raw viewport functions (runtime-src 55cc10b20) read and write `/VP`
/// through the page dictionary, with no page loaded, and see what a loaded
/// page sees.
#[test]
fn raw_viewports_read_and_write_without_loading_the_page() {
    let lib = Library::acquire();
    let doc = Document::from_bytes(
        lib,
        &test_support::fixture("tooling/engine-conformance/fixtures/viewports.pdf"),
        None,
    )
    .expect("open viewports.pdf");
    assert_eq!(doc.viewport_count_raw(0), 2);
    assert_eq!(doc.viewport_count_raw(1), 0);
    assert_eq!(doc.viewport_count_raw(99), 0, "no page at that index");
    {
        let first = doc.viewport_raw(0, 0).expect("a viewport");
        assert_eq!(first.get().name().unwrap().as_deref(), Some("Plan"));
        assert_eq!(first.get().measure().expect("a measure").subtype(), MeasureSubtype::Rl);
        assert!(doc.viewport_raw(0, 2).is_none(), "past the end");
    }

    let bbox = RectF { left: 10.0, top: 280.0, right: 390.0, bottom: 20.0 };
    {
        let added = doc.add_viewport_raw(1, bbox).expect("added");
        assert!(added.get().set_name("EmbedPDF"));
    }
    assert_eq!(doc.viewport_count_raw(1), 1);
    assert_eq!(doc.page(1).unwrap().viewport_count(), 1, "a loaded page sees it");
    assert!(doc.remove_viewport_raw(1, 0));
    assert_eq!(doc.viewport_count_raw(1), 0);
}
