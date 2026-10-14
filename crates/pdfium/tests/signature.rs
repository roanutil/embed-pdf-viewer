//! The signature model and revision calls against the signing fixtures.

use pdfium::{DigestAlgorithm, Document, Library, string_key};

const FIX: &str = "packages/engine/main/test/fixtures";

fn open(name: &str) -> Document {
    Document::from_bytes(Library::acquire(), &test_support::fixture(&format!("{FIX}/{name}")), None)
        .unwrap_or_else(|e| panic!("open {name}: {e}"))
}

#[test]
fn two_approvals_have_two_signed_fields_and_three_revisions() {
    let doc = open("signed_two_approvals.pdf");
    let model = doc.signature_model().unwrap();
    assert_eq!(model.count(), 2);
    assert!(model.is_revision_chain_valid());
    assert_eq!(doc.revision_count(), 3);
    for i in 0..2 {
        assert!(model.is_signed(i));
        assert_eq!(model.coverage(i), 0, "whole-revision");
        let contents = model.contents(i).unwrap();
        assert_eq!(contents[0], 0x30);
        assert_eq!(contents.len() as u64, model.contents_len(i));
        assert!(model.field_name(i).unwrap().is_some());
        let field = model.field_object_number(i);
        assert_eq!(model.index_by_field_object_number(field), i);
    }
    assert_eq!(model.string(0, string_key::FILTER).unwrap().as_deref(), Some("Adobe.PPKLite"));
    let (end, _) = doc.revision(0).unwrap();
    let first = doc.read_loaded_bytes(0, end).unwrap();
    assert!(first.starts_with(b"%PDF-"));
    let size = doc.loaded_bytes_size();
    let digest = doc.digest_byte_range([0, size, size, 0], DigestAlgorithm::Sha256).unwrap();
    assert_eq!(digest.len(), 32);
}

#[test]
fn an_unsigned_field_is_listed_unsigned() {
    let doc = open("unsigned_sigfield.pdf");
    let model = doc.signature_model().unwrap();
    assert_eq!(model.count(), 1);
    assert!(!model.is_signed(0));
    assert_eq!(model.contents_len(0), 0);
    assert!(model.contents(0).unwrap().is_empty());
}
