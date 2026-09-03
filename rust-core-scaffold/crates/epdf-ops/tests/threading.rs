//! The reason epdf_ops exists. pdfium::Document is !Send because the fork's
//! native builds set embedpdf_thread_local_globals=true, so a handle opened on
//! one thread is unusable on another. epdf_ops::Document must be usable from
//! any thread, and from several at once, by routing to the one thread that owns
//! the handle.

use std::sync::Arc;
use std::thread;

#[test]
fn a_document_opened_here_is_usable_from_other_threads() {
    let bytes = test_support::fixture_bytes();
    let engine = epdf_ops::Engine::new().unwrap();
    let doc: Arc<epdf_ops::Document> = engine.open(bytes, None).unwrap();

    let expected = doc.page_count().unwrap();

    let handles: Vec<_> = (0..8)
        .map(|_| {
            let doc = Arc::clone(&doc);
            thread::spawn(move || doc.page_count().unwrap())
        })
        .collect();

    for handle in handles {
        assert_eq!(handle.join().unwrap(), expected);
    }
}

#[test]
fn concurrent_renders_from_many_threads_all_succeed() {
    let bytes = test_support::fixture_bytes();
    let engine = epdf_ops::Engine::new().unwrap();
    let doc = engine.open(bytes, None).unwrap();

    let handles: Vec<_> = (0..4)
        .map(|i| {
            let doc = Arc::clone(&doc);
            thread::spawn(move || doc.render_page(i, 0.25).map(|b| b.bgra.len()))
        })
        .collect();

    for handle in handles {
        assert!(handle.join().unwrap().unwrap() > 0);
    }
}

#[test]
fn engine_and_document_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<epdf_ops::Engine>();
    assert_send_sync::<epdf_ops::Document>();
}
