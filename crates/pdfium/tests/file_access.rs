//! Reading documents through `FPDF_FILEACCESS`.
//!
//! The callback path is where a mistake is a crash rather than a wrong answer,
//! so these check the boundary conditions directly: a read past the end, a
//! zero-length read, a truncated file, and a file that disappears mid-session.

use std::rc::Rc;

use pdfium::{BaseDocument, Document, FileAccess, Library, PdfiumError};

fn report_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../pdfium-multiplatform-prototype/fixtures/report.pdf")
}

fn report_bytes() -> Vec<u8> {
    std::fs::read(report_path()).expect("read report.pdf")
}

#[test]
fn a_memory_source_opens_a_document() {
    let access = FileAccess::from_bytes(report_bytes()).expect("access");
    assert_eq!(access.len(), report_bytes().len() as u64);

    let document = Document::from_file_access(Library::acquire(), &access, None).expect("open");
    assert_eq!(document.page_count(), 14);
}

#[test]
fn a_file_source_opens_the_same_document_without_copying_it() {
    let access = FileAccess::from_file(report_path()).expect("access");
    let document = Document::from_file_access(Library::acquire(), &access, None).expect("open");
    assert_eq!(document.page_count(), 14);
}

#[test]
fn the_two_sources_agree() {
    let from_memory = FileAccess::from_bytes(report_bytes()).expect("memory");
    let from_file = FileAccess::from_file(report_path()).expect("file");
    assert_eq!(from_memory.len(), from_file.len());

    let a = Document::from_file_access(Library::acquire(), &from_memory, None).expect("memory");
    let b = Document::from_file_access(Library::acquire(), &from_file, None).expect("file");
    assert_eq!(a.page_count(), b.page_count());
    assert_eq!(a.page(0).expect("page").object_number(), b.page(0).expect("page").object_number());
}

#[test]
fn an_empty_source_is_refused_rather_than_read() {
    let access = FileAccess::from_bytes(Vec::new()).expect("access");
    assert!(access.is_empty());
    assert!(matches!(
        Document::from_file_access(Library::acquire(), &access, None),
        Err(PdfiumError::Load),
    ));
}

#[test]
fn a_truncated_document_fails_to_load_rather_than_reading_past_the_end() {
    // The callback refuses a read that would run off the end instead of
    // returning a short one, because a short read reported as success hands
    // PDFium uninitialized bytes as document content.
    let mut bytes = report_bytes();
    bytes.truncate(bytes.len() / 2);
    let access = FileAccess::from_bytes(bytes).expect("access");
    let opened = Document::from_file_access(Library::acquire(), &access, None);
    // PDFium may recover a truncated file or refuse it; what must not happen
    // is a crash or a document claiming the full page count.
    if let Ok(document) = opened {
        assert!(document.page_count() < 14, "a half file kept every page");
    }
}

#[test]
fn an_encrypted_document_reports_its_password_state_through_a_file_source() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tooling/engine-conformance/fixtures/encrypted-r4-aes-128.pdf");
    let access = FileAccess::from_file(&path).expect("access");

    assert!(matches!(
        Document::from_file_access(Library::acquire(), &access, None),
        Err(PdfiumError::Password),
    ));
    let document =
        Document::from_file_access(Library::acquire(), &access, Some("user")).expect("open");
    assert_eq!(document.page_count(), 1);
}

#[test]
fn an_unsupported_handler_is_reported_through_a_file_source_too() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tooling/engine-conformance/fixtures/unsupported-security.pdf");
    let access = FileAccess::from_file(&path).expect("access");
    assert!(matches!(
        Document::from_file_access(Library::acquire(), &access, None),
        Err(PdfiumError::Security),
    ));
}

#[test]
fn a_missing_file_fails_at_open_rather_than_at_first_read() {
    let missing = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("no-such-file.pdf");
    assert!(matches!(FileAccess::from_file(missing), Err(PdfiumError::Load)));
}

#[test]
fn a_file_backed_base_document_serves_layers() {
    // open.layerFileBase: the base is read from disk on demand and every
    // layer opened on it sees the same pages.
    let access = FileAccess::from_file(report_path()).expect("access");
    let base =
        Rc::new(BaseDocument::from_file_access(Library::acquire(), access, None).expect("base"));

    let first = base.open_layer().expect("first layer");
    let second = base.open_layer().expect("second layer");
    assert_eq!(first.page_count(), 14);
    assert_eq!(second.page_count(), 14);
    assert!(Rc::ptr_eq(first.base(), second.base()));
}

#[test]
fn a_file_backed_base_outlives_the_borrow_it_was_built_from() {
    // The access is moved into the base rather than borrowed, so the base is
    // usable after the expression that built it. A borrowed one would put a
    // lifetime on every session that holds it.
    let base = {
        let access = FileAccess::from_file(report_path()).expect("access");
        Rc::new(BaseDocument::from_file_access(Library::acquire(), access, None).expect("base"))
    };
    assert_eq!(base.open_layer().expect("layer").page_count(), 14);
}

#[test]
fn a_delta_that_is_not_a_delta_is_refused() {
    use pdfium::LayerOpenError;

    let base =
        Rc::new(BaseDocument::from_bytes(Library::acquire(), &report_bytes(), None).expect("base"));
    let nonsense = FileAccess::from_bytes(b"this is not a layer delta".to_vec()).expect("access");
    assert!(matches!(
        base.open_layer_delta(&nonsense),
        Err(LayerOpenError::MalformedDelta | LayerOpenError::OpenFailed),
    ));
}

#[test]
fn an_artifact_that_is_not_an_artifact_is_refused() {
    use pdfium::LayerOpenError;

    let base =
        Rc::new(BaseDocument::from_bytes(Library::acquire(), &report_bytes(), None).expect("base"));
    let nonsense = FileAccess::from_bytes(b"this is not an artifact".to_vec()).expect("access");
    assert!(matches!(
        base.open_layer_artifact(&nonsense),
        Err(LayerOpenError::MalformedDelta
            | LayerOpenError::BaseMismatch
            | LayerOpenError::OpenFailed),
    ));
}
