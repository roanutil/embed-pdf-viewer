//! Plain and rich annotation line breaks agree with the public TS boundary.

use epdf_engine::features::annotation::write::primitives::set_string_or_clear;
use pdfium::{Document, Library};

fn document() -> Document {
    Document::from_bytes(
        Library::acquire(),
        &test_support::fixture("tooling/engine-conformance/fixtures/rich_metadata.pdf"),
        None,
    )
    .expect("open fixture")
}

#[test]
fn contents_write_pdf_line_breaks_without_normalizing_other_strings() {
    let document = document();
    let page = document.page(0).expect("page");
    let annotation = page.create_annotation(1).expect("text annotation");
    let mixed = "a\r\nb\rc\nd";
    set_string_or_clear(&annotation, "Contents", Some(mixed)).expect("contents");
    assert_eq!(annotation.string_value("Contents").expect("read"), Some("a\rb\rc\rd".into()));
    set_string_or_clear(&annotation, "Subj", Some(mixed)).expect("subject");
    assert_eq!(annotation.string_value("Subj").expect("read"), Some(mixed.into()));
}

#[test]
fn null_contents_removes_the_key_but_empty_contents_stays_present() {
    let document = document();
    let page = document.page(0).expect("page");
    let annotation = page.create_annotation(1).expect("text annotation");
    assert_eq!(annotation.string_value("Contents").expect("read"), None);
    set_string_or_clear(&annotation, "Contents", Some("")).expect("empty contents");
    assert_eq!(annotation.string_value("Contents").expect("read"), Some(String::new()));
    set_string_or_clear(&annotation, "Contents", None).expect("clear contents");
    assert_eq!(annotation.string_value("Contents").expect("read"), None);
}
