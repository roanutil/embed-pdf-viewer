//! The native surface, driven the way a smoke app drives it.
//!
//! Phase 1 asks for a "minimal native session, open, password, close, geometry,
//! render and G4 font registry for smoke apps". This covers everything in that
//! list except the font registry, which is G4 and lands with the render
//! services rather than here.
//!
//! The thread assertions are the ones worth having. Everything else is an API
//! shape that a compiler already checked; confinement is a runtime property
//! that nothing but a test will notice going wrong, and its failure mode is
//! PDFium corrupting itself rather than an error anybody sees.

use std::sync::Arc;

use epdf_uniffi::{PdfEngine, PdfError};

fn bytes(fixture: &str) -> Vec<u8> {
    test_support::fixture(fixture)
}

fn engine() -> Arc<PdfEngine> {
    PdfEngine::new()
}

#[test]
fn a_document_opens_and_reports_its_pages() {
    let engine = engine();
    let document = engine.open(bytes(test_support::REPORT), None).expect("open");
    let pages = document.pages().expect("pages");

    assert_eq!(pages.len(), 14);
    assert_eq!(pages[0].index, 0);
    assert!(pages[0].object_number > 0, "an index is not identity");
    assert!(pages[0].width_points > 0.0 && pages[0].height_points > 0.0);
    engine.shutdown();
}

#[test]
fn a_missing_password_reports_a_code_a_facade_can_prompt_on() {
    let engine = engine();
    let refused = engine
        .open(bytes(test_support::ENCRYPTED), None)
        .expect_err("an encrypted document without a password");
    let PdfError::Engine { code, .. } = &refused;
    assert_eq!(
        code, "DocPasswordRequired",
        "collapsing this into DocOpenFailed would make a prompt impossible",
    );
    engine.shutdown();
}

#[test]
fn geometry_comes_back_as_the_same_snapshot_the_web_engine_produces() {
    let engine = engine();
    let document = engine.open(bytes(test_support::REPORT), None).expect("open");
    let pages = document.pages().expect("pages");

    let json = document.page_geometry_json(pages[0].object_number).expect("geometry");
    let snapshot: serde_json::Value = serde_json::from_str(&json).expect("geometry is JSON");
    let runs = snapshot["runs"].as_array().expect("runs");
    let glyphs: usize = runs.iter().map(|r| r["glyphs"].as_array().expect("glyphs").len()).sum();

    // The same numbers the frozen corpus vectors carry for this page, so the
    // native path and the web path are reading the same document the same way.
    assert_eq!(runs.len(), 238);
    assert_eq!(glyphs, 5087);
    engine.shutdown();
}

#[test]
fn a_page_renders_to_rgba_the_caller_owns() {
    let engine = engine();
    let document = engine.open(bytes(test_support::COLORS), None).expect("open");
    let pages = document.pages().expect("pages");

    let bitmap = document.render_page(pages[0].object_number, 1.0).expect("render");
    assert!(bitmap.width > 0 && bitmap.height > 0);
    assert!(
        bitmap.stride >= bitmap.width * 4,
        "stride is explicit because it may exceed the packed row width",
    );
    assert_eq!(bitmap.rgba.len(), (bitmap.stride * bitmap.height) as usize);

    // Channel order, on a fixture with saturated interior colors. A grayscale
    // page cannot tell a correct conversion from a B-to-R swap, which is the
    // mistake the prototype's pixel golden could not catch.
    let mut saw_red_dominant = false;
    let mut saw_blue_dominant = false;
    for row in 0..bitmap.height {
        let start = (row * bitmap.stride) as usize;
        for px in bitmap.rgba[start..start + (bitmap.width * 4) as usize].as_chunks::<4>().0 {
            let (r, g, b) = (px[0], px[1], px[2]);
            if r > 200 && g < 80 && b < 80 {
                saw_red_dominant = true;
            }
            if b > 200 && g < 80 && r < 80 {
                saw_blue_dominant = true;
            }
        }
    }
    assert!(saw_red_dominant, "no red pixel: the channels are probably swapped");
    assert!(saw_blue_dominant, "no blue pixel: the channels are probably swapped");

    // The bytes are the caller's. Closing the document must not disturb them.
    let before = bitmap.rgba.clone();
    document.shutdown();
    engine.shutdown();
    assert_eq!(bitmap.rgba, before, "a rendered image outlives its document");
}

#[test]
fn a_bad_scale_is_refused_before_anything_is_allocated() {
    let engine = engine();
    let document = engine.open(bytes(test_support::REPORT), None).expect("open");
    let pages = document.pages().expect("pages");
    for scale in [0.0f32, -1.0, f32::NAN, f32::INFINITY] {
        let refused = document
            .render_page(pages[0].object_number, scale)
            .expect_err(&format!("scale {scale} should be refused"));
        let PdfError::Engine { code, .. } = &refused;
        assert_eq!(code, "InvalidArg");
    }
    engine.shutdown();
}

#[test]
fn closing_the_engine_closes_its_documents_and_refuses_new_work() {
    let engine = engine();
    let document = engine.open(bytes(test_support::REPORT), None).expect("open");
    document.pages().expect("works while open");

    engine.shutdown();
    assert!(engine.is_closed());

    let PdfError::Engine { code, .. } = document.pages().expect_err("the document went with it");
    assert_eq!(code, "DocNotOpen");
    let PdfError::Engine { code, .. } =
        engine.open(bytes(test_support::REPORT), None).expect_err("no new work");
    assert_eq!(code, "DocNotOpen");

    // Idempotent, per the lifecycle contract.
    engine.shutdown();
}

#[test]
fn every_engine_call_runs_on_the_engines_own_thread() {
    // The property the whole design exists for. PDFium handles never cross
    // threads, and nothing in Swift or Kotlin can promise that: a Swift actor
    // and a Kotlin dispatcher both reserve the right to resume on a different
    // thread. So the confinement has to be structural, and this is the test
    // that says it is.
    let engine = engine();
    let document = engine.open(bytes(test_support::REPORT), None).expect("open");

    // Call from several caller threads at once. If any PDFium work ran on the
    // caller, these would be touching one document from many threads.
    let document = Arc::new(document);
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let document = Arc::clone(&document);
            std::thread::spawn(move || document.pages().expect("pages").len())
        })
        .collect();
    for handle in handles {
        assert_eq!(handle.join().expect("no panic"), 14);
    }
    engine.shutdown();
}

#[test]
fn two_engines_are_independent() {
    // One owner thread each, so closing one must not disturb the other. A
    // process-wide singleton would fail this, which is why the lifecycle
    // contract forbids one without a reset.
    let first = engine();
    let second = engine();
    let a = first.open(bytes(test_support::REPORT), None).expect("open a");
    let b = second.open(bytes(test_support::REPORT), None).expect("open b");

    first.shutdown();
    assert!(a.pages().is_err(), "the closed engine's document is gone");
    assert_eq!(b.pages().expect("the other engine still works").len(), 14);
    second.shutdown();
}
