//! The catalog's named-page trees against a real file.

use pdfium::named_pages::{NamedPage, NamedPageTarget, NamedPageTree};
use pdfium::{Document, Library};

fn report() -> Document {
    Document::from_bytes(Library::acquire(), &test_support::fixture(test_support::REPORT), None)
        .expect("open report.pdf")
}

#[test]
fn a_name_set_reads_back_and_removes() {
    let doc = report();
    let pon = doc.page_object_number_by_index(0);
    assert!(doc.named_pages(NamedPageTree::Pages).unwrap().is_empty());
    assert!(doc.set_named_page("approved=Approved", pon).unwrap());
    assert_eq!(
        doc.named_pages(NamedPageTree::Pages).unwrap(),
        vec![NamedPage { name: "approved=Approved".into(), target: NamedPageTarget::Page(pon) }]
    );
    assert!(doc.named_pages(NamedPageTree::Templates).unwrap().is_empty());
    assert!(doc.remove_named_page("approved=Approved").unwrap());
    assert!(!doc.remove_named_page("approved=Approved").unwrap());
    assert!(doc.named_pages(NamedPageTree::Pages).unwrap().is_empty());
}

#[test]
fn an_empty_name_is_refused() {
    let doc = report();
    let pon = doc.page_object_number_by_index(0);
    assert!(doc.set_named_page("", pon).is_err());
    assert!(doc.remove_named_page("").is_err());
}

#[test]
fn a_page_outside_the_tree_is_refused() {
    let doc = report();
    assert!(!doc.set_named_page("a=A", 999_999).unwrap());
}
