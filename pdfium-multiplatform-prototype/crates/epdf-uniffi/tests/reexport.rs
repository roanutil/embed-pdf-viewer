//! The wrapper crate adds no behavior, so this test only proves the wrapped
//! surface still answers the same numbers. The real parity assertions live in
//! the Swift and Kotlin suites, which is where a binding can actually drift.

#[test]
fn the_wrapped_engine_opens_the_fixture() {
    let bytes = test_support::fixture_bytes();
    let engine = epdf_uniffi::EpdfEngine::new().unwrap();
    let doc = engine.open(bytes, None).unwrap();
    assert!(doc.page_count().unwrap() > 4);
    let size = doc.page_size(4).unwrap();
    assert!(size.width > 0.0 && size.height > 0.0);
    doc.close();
}

#[test]
fn errors_cross_the_wrapper_unchanged() {
    let engine = epdf_uniffi::EpdfEngine::new().unwrap();
    let err = engine.open(b"not a pdf".to_vec(), None).unwrap_err();
    assert!(matches!(err, epdf_uniffi::OpsError::LoadFailed));
}
