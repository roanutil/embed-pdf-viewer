//! The font surface the runtime at 8b1e69abd adds: a registered font's
//! program facts and the last resort (`FontRegistrar.ts`), a document's
//! faces, display programs and font resolution (`DocumentFontReader.ts`),
//! the missing-character list (`DocumentSession.ts`) and the `/DA` calls
//! that name a document's own program.
//!
//! PDFium's font registry is per thread, so every test that registers a font
//! runs on a thread of its own.

use pdfium::{
    Document, FontDescription, FontEmbedding, FontProgramFormat, FontResolveRun, FontSource,
    Library,
};

const EMBEDDED: &str = "tooling/engine-conformance/fixtures/embedded_orientations.pdf";
const NO_FONTS: &str = "tooling/engine-conformance/fixtures/annotation_kinds.pdf";
/// `FPDF_ANNOT_FREETEXT`.
const FREE_TEXT: i32 = 3;

fn on_a_fresh_thread<T: Send + 'static>(body: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::spawn(body).join().expect("the test thread panicked")
}

fn roboto() -> Vec<u8> {
    std::fs::read(
        test_support::workspace_root()
            .join("packages/engine/main/test/fixtures/Roboto-Regular.ttf"),
    )
    .expect("read Roboto-Regular.ttf")
}

fn open(fixture: &str) -> Document {
    Document::from_bytes(Library::acquire(), &test_support::fixture(fixture), None)
        .unwrap_or_else(|e| panic!("open {fixture}: {e}"))
}

fn run(text: &str, family: &str, weight: i32) -> FontResolveRun {
    FontResolveRun {
        text: text.to_string(),
        family: family.to_string(),
        weight,
        italic: false,
        program: 0,
    }
}

#[test]
fn a_registered_font_reports_its_program() {
    on_a_fresh_thread(|| {
        let _library = Library::acquire();
        let bytes = roboto();
        let id =
            pdfium::register_memory_font(&FontDescription::inferred(), &bytes).expect("register");

        // A static TrueType file is embedded as it is.
        let data = pdfium::font_program_data(id).expect("read").expect("a program");
        assert_eq!(data, bytes);
        assert_eq!(pdfium::font_program_format(id), Some(FontProgramFormat::TrueType));
        assert!(pdfium::font_program_hash(id).is_some());
        assert_eq!(
            pdfium::font_postscript_name(id).expect("read").as_deref(),
            Some("Roboto-Regular")
        );

        // hhea ascender 1900 and descender -500 over 2048 units per em.
        let metrics = pdfium::font_metrics(id).expect("metrics");
        assert_eq!(metrics.ascent, 1900.0 / 2048.0);
        assert_eq!(metrics.descent, 500.0 / 2048.0);

        let coverage = pdfium::font_coverage(id).expect("coverage");
        let covers =
            |c: char| coverage.iter().any(|r| (r.start..r.start + r.count).contains(&(c as u32)));
        assert!(covers('A') && covers('z'), "{coverage:?}");
        assert!(!covers('\u{4E2D}'), "Roboto has no CJK");
    });
}

#[test]
fn an_unknown_font_reports_nothing() {
    on_a_fresh_thread(|| {
        let _library = Library::acquire();
        let id = pdfium::register_memory_font(&FontDescription::inferred(), &roboto())
            .expect("register");
        pdfium::clear_registered_fonts();

        assert_eq!(pdfium::font_program_data(id).expect("read"), None);
        assert_eq!(pdfium::font_program_format(id), None);
        assert_eq!(pdfium::font_program_hash(id), None);
        assert_eq!(pdfium::font_postscript_name(id).expect("read"), None);
        assert_eq!(pdfium::font_metrics(id), None);
        assert!(pdfium::font_coverage(id).expect("coverage").is_empty());
        assert!(!pdfium::set_last_resort_font(Some(id)), "an unknown id became the last resort");
    });
}

#[test]
fn the_last_resort_can_be_set_and_cleared() {
    on_a_fresh_thread(|| {
        let _library = Library::acquire();
        let id = pdfium::register_memory_font(&FontDescription::inferred(), &roboto())
            .expect("register");
        assert!(pdfium::set_last_resort_font(Some(id)));
        assert!(pdfium::set_last_resort_font(None));
    });
}

#[test]
fn a_documents_faces_list_its_embedded_program() {
    let document = open(EMBEDDED);
    let faces = document.font_faces().expect("read").expect("a snapshot");

    assert_eq!(faces.len(), 1, "{faces:?}");
    let face = &faces[0];
    assert_eq!(face.family_name.as_deref(), Some("Roboto"));
    assert_eq!((face.weight, face.italic), (400, false));

    assert_eq!(face.programs.len(), 1, "{:?}", face.programs);
    let program = &face.programs[0];
    assert!(program.object_number > 0);
    assert_eq!(program.format, Some(FontProgramFormat::TrueType));
    assert_eq!(program.embedding, Some(FontEmbedding::Whole));
    assert_eq!(program.postscript_name.as_deref(), Some("Roboto-Regular"));
    // Installable: Roboto's fsType is 0.
    assert_eq!(program.embedding_permission, 0);
    assert!(program.maps_unicode);
    // A TrueType program with a Unicode cmap is what a browser loads as it is.
    assert!(program.sha256.is_some());
    assert_eq!(program.display_sha256, program.sha256);
    assert!(program.metrics.is_some());
}

#[test]
fn a_document_without_fonts_lists_no_faces() {
    let document = open(NO_FONTS);
    assert_eq!(document.font_faces().expect("read"), Some(Vec::new()));
}

#[test]
fn the_fingerprint_is_stable_and_tells_documents_apart() {
    let embedded = open(EMBEDDED);
    let first = embedded.fonts_fingerprint().expect("a fingerprint");
    assert_eq!(embedded.fonts_fingerprint(), Some(first));
    assert_ne!(open(NO_FONTS).fonts_fingerprint(), Some(first));
}

#[test]
fn a_display_program_is_read_by_its_hash() {
    let document = open(EMBEDDED);
    let faces = document.font_faces().expect("read").expect("a snapshot");
    let display = faces[0].programs[0].display_sha256.expect("a display hash");

    let bytes = document.font_display_program_data(&display).expect("read").expect("bytes");
    assert_eq!(&bytes[..4], &[0, 1, 0, 0], "a TrueType sfnt");
    assert!(!document.font_display_program_coverage(&display).expect("coverage").is_empty());

    let unknown = [0u8; 32];
    assert_eq!(document.font_display_program_data(&unknown).expect("read"), None);
    assert!(document.font_display_program_coverage(&unknown).expect("coverage").is_empty());
}

#[test]
fn resolving_reports_the_font_each_run_would_use() {
    on_a_fresh_thread(|| {
        let document = open(NO_FONTS);
        let id = pdfium::register_memory_font(&FontDescription::inferred(), &roboto())
            .expect("register");

        let resolution = document
            .resolve_fonts(
                None,
                &[run("Hi", "Helvetica", 700), run("Hi \u{10FFFD}", "Roboto", 400)],
            )
            .expect("read")
            .expect("a snapshot");

        assert_eq!(resolution.runs.len(), 2);
        let standard = &resolution.runs[0];
        assert_eq!(standard.source, Some(FontSource::Standard));
        assert_eq!(standard.base_font_name.as_deref(), Some("Helvetica-Bold"));
        assert_eq!(standard.registered_font, None);
        assert_eq!(standard.program_object_number, 0);

        let registered = &resolution.runs[1];
        assert_eq!(registered.source, Some(FontSource::Registered));
        assert_eq!(registered.registered_font, Some(id));
        assert_eq!(registered.family_name.as_deref(), Some("Roboto"));
        assert!(!registered.degraded);

        // A private-use plane-16 character no font has.
        assert_eq!(resolution.missing_characters, ["\u{10FFFD}"]);
    });
}

#[test]
fn a_fallback_is_reported_for_the_runs_style() {
    on_a_fresh_thread(|| {
        let document = open(NO_FONTS);
        let id = pdfium::register_memory_font(&FontDescription::inferred(), &roboto())
            .expect("register");
        pdfium::add_fallback_font(id, 0).expect("fallback");

        let resolution = document
            .resolve_fonts(None, &[run("Hi", "Helvetica", 400)])
            .expect("read")
            .expect("a snapshot");
        assert_eq!(resolution.runs[0].fallbacks, [id]);
    });
}

#[test]
fn a_document_program_can_be_named_by_a_boxs_default_appearance() {
    let document = open(EMBEDDED);
    let faces = document.font_faces().expect("read").expect("a snapshot");
    let program = faces[0].programs[0].object_number;

    let page = document.page(0).expect("page 0");
    let annotation = page.create_annotation(FREE_TEXT).expect("a free text box");
    assert_eq!(annotation.default_appearance_font_postscript_name().expect("read"), None);

    assert!(annotation.set_default_appearance_document_font(program, 12.0, (0, 0, 0)));
    assert_eq!(
        annotation.default_appearance_font_postscript_name().expect("read").as_deref(),
        Some("Roboto-Regular")
    );
    // Not a font program: the page's own object.
    assert!(!annotation.set_default_appearance_document_font(
        page.object_number(),
        12.0,
        (0, 0, 0)
    ));
}

#[test]
fn an_appearance_reports_the_characters_it_drew_with_no_font() {
    let document = open(NO_FONTS);
    assert!(document.missing_characters().expect("read").is_empty());

    let page = document.page(0).expect("page 0");
    let annotation = page.create_annotation(FREE_TEXT).expect("a free text box");
    assert!(annotation.set_rect(pdfium::RectF {
        left: 10.0,
        bottom: 10.0,
        right: 200.0,
        top: 60.0
    }));
    assert!(annotation.set_default_appearance(0, 12.0, (0, 0, 0)), "Helvetica /DA");
    assert!(annotation.set_string_value("Contents", "Hi \u{10FFFD}").expect("contents"));
    assert!(annotation.generate_appearance(None));
    assert_eq!(document.missing_characters().expect("read"), ["\u{10FFFD}"]);

    document.clear_missing_characters();
    assert!(document.missing_characters().expect("read").is_empty());
}
