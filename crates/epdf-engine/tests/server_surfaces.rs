//! G15's gate: the file and encoder surfaces.
//!
//! No vectors, and the port order says why: "no new Rust behavior beyond G4
//! raster and G10 save". What is here is COMPOSITION, and what it composes
//! is already pinned. So these tests check the joins rather than the
//! pixels: that a path opens and closes, that an index resolves to the same
//! page a session would, and that an engine with no encoder refuses instead
//! of inventing one.

use epdf_engine::cancel::CancelToken;
use epdf_engine::error::{EngineError, EngineErrorCode};
use epdf_engine::features::render::raster::{PageRaster, RenderOptions, render_page};
use epdf_engine::features::server::{
    EncodedImage, ImageEncoder, RenderEncode, encode_raster, render_page_file,
};
use epdf_engine::session::authority::SessionAuthority;
use epdf_engine::session::document::DocumentSession;

/// Four pages, object numbers 3 through 6.
const FIXTURE: &str = "tooling/engine-conformance/fixtures/rotated_pages.pdf";

fn library() -> std::rc::Rc<pdfium::Library> {
    pdfium::Library::acquire()
}

fn path(relative: &str) -> std::path::PathBuf {
    test_support::workspace_root().join(relative)
}

#[test]
fn a_page_rendered_from_a_file_is_the_page_a_session_would_render() {
    // The whole claim of `render_page_file`: opening a path and rendering
    // one page out of it produces what a caller who opened a session would
    // have got. If the two ever differed, the cheap path would be a second
    // renderer with its own bugs.
    let library = library();
    let options: RenderOptions = RenderOptions::default();

    for index in 0..4u32 {
        let from_file = render_page_file(
            &library,
            &path(FIXTURE),
            None,
            index,
            // The same options in page space, as the wire sends them.
            &RenderOptions::default(),
            &CancelToken::never(),
        )
        .unwrap_or_else(|e| panic!("page {index}: {e}"));

        let bytes = std::fs::read(path(FIXTURE)).expect("read");
        let mut session = DocumentSession::new();
        session.open_with(bytes, None, SessionAuthority::unrestricted()).expect("open");
        let owned = session.document().expect("open");
        let page = epdf_engine::access::DocumentAccess::page(owned, from_file.page_object_number)
            .expect("the page resolves");
        let from_session =
            render_page(&page, &options, &CancelToken::never()).expect("the page renders");

        // Pinned to the fixture's own numbering, because comparing the two
        // renders alone would pass a build that answered index 3 with page
        // one: both sides would move together.
        assert_eq!(
            from_file.page_object_number.get(),
            3 + index,
            "page {index}: the index resolved to the wrong page",
        );
        assert_eq!(from_file.page_count, 4, "page {index}: the page count");
        assert_eq!(
            (from_file.raster.width, from_file.raster.height),
            (from_session.width, from_session.height),
            "page {index}: the rasters are different sizes",
        );
        assert_eq!(from_file.raster.rgba, from_session.rgba, "page {index}: the pixels differ");
    }
}

#[test]
fn a_page_index_past_the_end_is_not_found() {
    let error = render_page_file(
        &library(),
        &path(FIXTURE),
        None,
        4,
        &RenderOptions::default(),
        &CancelToken::never(),
    )
    .expect_err("the fixture has four pages");
    assert!(error.is(EngineErrorCode::NotFound), "{error}");
    assert!(error.to_string().contains("the file has 4"), "{error}");
}

#[test]
fn a_path_that_is_not_there_is_not_found() {
    let error = render_page_file(
        &library(),
        &path("tooling/engine-conformance/fixtures/no-such-file.pdf"),
        None,
        0,
        &RenderOptions::default(),
        &CancelToken::never(),
    )
    .expect_err("no such file");
    assert!(error.is(EngineErrorCode::NotFound), "{error}");
}

#[test]
fn an_encrypted_file_without_its_password_says_so() {
    // The distinction the error codes carry: a file this build cannot
    // decrypt is not a file it cannot parse, and a caller can act on the
    // first by asking for a password.
    let error = render_page_file(
        &library(),
        &path("tooling/engine-conformance/fixtures/encrypted-r6-aes-256.pdf"),
        None,
        0,
        &RenderOptions::default(),
        &CancelToken::never(),
    )
    .expect_err("no password");
    assert!(error.is(EngineErrorCode::DocPasswordRequired), "{error}");
}

/// An encoder that records what it was asked for.
struct RecordingEncoder {
    calls: std::cell::RefCell<Vec<RenderEncode>>,
}

impl ImageEncoder for RecordingEncoder {
    fn encode(
        &self,
        raster: &PageRaster,
        encode: &RenderEncode,
    ) -> Result<EncodedImage, EngineError> {
        self.calls.borrow_mut().push(encode.clone());
        Ok(EncodedImage {
            // Deliberately not derived from the request: an encoder that
            // fell back to another format says so here, and the engine
            // passes its answer through rather than restating the ask.
            content_type: String::from("image/png"),
            width: raster.width,
            height: raster.height,
            bytes: vec![1, 2, 3],
        })
    }
}

#[test]
fn an_engine_with_no_encoder_refuses_rather_than_inventing_one() {
    // The shipping behaviour on two of the three targets, and the reason
    // the encoder is injected: a PNG library in the engine would ship to
    // every host to satisfy the one that needs it.
    let raster = PageRaster { width: 1, height: 1, stride: 4, rgba: vec![0, 0, 0, 255] };
    let error = encode_raster(None, &raster, &RenderEncode::Png, &CancelToken::never())
        .expect_err("no encoder");
    assert!(error.is(EngineErrorCode::NotImplemented), "{error}");
    assert!(error.to_string().contains("server surface"), "{error}");
}

#[test]
fn the_encoders_answer_is_passed_through_rather_than_the_request() {
    let encoder = RecordingEncoder { calls: std::cell::RefCell::new(Vec::new()) };
    let raster = PageRaster { width: 7, height: 5, stride: 28, rgba: vec![0; 140] };
    let asked = RenderEncode::Webp { quality: Some(80) };

    let encoded = encode_raster(Some(&encoder), &raster, &asked, &CancelToken::never())
        .expect("the encoder answers");

    assert_eq!(encoder.calls.borrow().as_slice(), &[asked], "the request was not passed on");
    assert_eq!(encoded.content_type, "image/png", "the request was restated as the answer");
    assert_eq!((encoded.width, encoded.height), (7, 5), "the raster's size was not carried");
}

#[test]
fn a_cancellation_during_an_encode_refuses_the_result() {
    // An encoder is not interruptible, so the token is checked on the far
    // side: a job that was cancelled while it ran gets an error rather than
    // a result nobody is waiting for.
    let encoder = RecordingEncoder { calls: std::cell::RefCell::new(Vec::new()) };
    let raster = PageRaster { width: 1, height: 1, stride: 4, rgba: vec![0, 0, 0, 255] };
    let cancel = CancelToken::never();
    cancel.cancel();

    let error =
        encode_raster(Some(&encoder), &raster, &RenderEncode::Png, &cancel).expect_err("cancelled");
    assert!(error.is(EngineErrorCode::Aborted), "{error}");
    assert_eq!(encoder.calls.borrow().len(), 1, "the encoder was not given the chance to run");
}
