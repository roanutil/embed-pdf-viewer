//! G7's appearance gate: the same appearances land in the same places.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run annotation-appearance
//!
//! # Placement, not pixels
//!
//! What is compared with the vector is which appearances are emitted, under
//! which name, from which mode, at which page-space box and at what size.
//! Not the pixels, and the reason is the one `corpus.ts` gives for embedding
//! fonts: the TypeScript reads through the WASM build and this reads through
//! the native dylib, so two identical readers can still produce rasters that
//! differ in the last bit of a filtered edge. Comparing digests would compare
//! two rasterizers.
//!
//! The placement is where the rule lives anyway. Whether an annotation is
//! emitted at all, and whether its box is `/Rect`, its turned box or the box
//! its in-memory drawing takes, is the reader's decision, and getting that
//! wrong puts a picture somewhere the document did not. The tests that look
//! at pixels look at solid fills, which do not depend on the build.

use epdf_engine::access::{DocumentAccess, OwnedDocument};
use epdf_engine::cancel::CancelToken;
use epdf_engine::features::annotation::appearance::{
    APPEARANCE_PIXEL_CLAMP, AnnotationAppearanceRaster, AnnotationAppearancesResult,
    AnnotationFamily, AppearanceModeName, AppearanceRenderOptions, render_page_appearances,
};
use epdf_engine::features::annotation::subtype::AnnotationSubtype;
use epdf_engine::features::render::geometry::RenderViewport;
use epdf_engine::geom::PdfRect;
use epdf_engine::host::Host;
use epdf_engine::identity::{AnnotationRef, PageObjectNumber};
use pdfium::{Document, Library};
use serde::Deserialize;
use serde_json::{Value, json};

const SCHEMA_VERSION: u32 = 1;
const APPEARANCES_FIXTURE: &str = "tooling/engine-conformance/fixtures/annotation_appearances.pdf";
const EBOOK: &str = "examples/engine-runtime-demo/public/ebook-annotated.pdf";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VectorFile {
    schema_version: u32,
    kind: String,
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    name: String,
    fixture: String,
    covers: String,
    #[serde(default)]
    scale: Option<f64>,
    #[serde(default)]
    modes: Option<Vec<String>>,
    page_object_number: u32,
    appearances: Vec<Placed>,
}

/// One appearance as the vector records it: in page space, as the wire
/// answers it.
#[derive(Debug, Deserialize, PartialEq)]
struct Placed {
    #[serde(rename = "ref")]
    annotation_ref: Value,
    mode: String,
    rect: Value,
    width: u32,
    height: u32,
}

fn load() -> VectorFile {
    let path = test_support::corpus::vectors_dir("annotation").join("appearances.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: VectorFile = serde_json::from_str(&text).expect("appearances.json");
    assert_eq!(file.schema_version, SCHEMA_VERSION, "vector envelope moved");
    assert_eq!(file.kind, "annotation.appearances");
    file
}

fn case<'a>(file: &'a VectorFile, name: &str) -> &'a Case {
    file.cases.iter().find(|c| c.name == name).unwrap_or_else(|| panic!("no case {name}"))
}

fn read_fixture(path: &str) -> Vec<u8> {
    std::fs::read(test_support::workspace_root().join(path))
        .unwrap_or_else(|e| panic!("read {path}: {e}"))
}

// ── through the host, as the generator read it ──────────────────────────────

/// Open `bytes` in a host as `docId` `doc`.
fn host_with(bytes: Vec<u8>) -> Host {
    let mut host = Host::new();
    let opened = host.call(
        &json!({ "kind": "open.fatMem", "effect": "open", "jobId": 1, "docId": "doc",
            "password": null })
        .to_string(),
        vec![bytes],
    );
    let opened: Value = serde_json::from_str(&opened.json).expect("JSON");
    assert!(opened.get("error").is_none(), "{opened}");
    host
}

/// One family's `annotations.renderAppearances` answer: the JSON result and
/// each appearance's pixels.
fn host_render(
    host: &mut Host,
    page: u32,
    family: &str,
    options: Option<Value>,
) -> (Value, Vec<Vec<u8>>) {
    let mut request = json!({ "kind": "annotations.renderAppearances", "effect": "read",
        "jobId": 2, "docId": "doc", "page": { "kind": "objectNumber", "objectNumber": page },
        "family": family });
    if let Some(options) = options {
        request["options"] = options;
    }
    let response = host.call(&request.to_string(), Vec::new());
    let answer: Value = serde_json::from_str(&response.json).expect("JSON");
    (answer, response.payloads)
}

/// Read a case the way the generator did: the annotations' appearances,
/// then the widgets', in page space.
fn read(case: &Case) -> Vec<Placed> {
    let mut host = host_with(read_fixture(&case.fixture));
    let mut options = json!({});
    if let Some(scale) = case.scale {
        options["viewport"] = json!({ "kind": "scale", "scale": scale });
    }
    if let Some(modes) = &case.modes {
        options["modes"] = json!(modes);
    }
    let mut placed = Vec::new();
    for family in ["annotations", "widgets"] {
        let (answer, payloads) =
            host_render(&mut host, case.page_object_number, family, Some(options.clone()));
        let result = &answer["result"];
        assert_eq!(
            result["page"],
            json!({ "kind": "objectNumber", "objectNumber": case.page_object_number }),
            "{}: {answer}",
            case.name,
        );
        let appearances = result["appearances"].as_array().unwrap_or_else(|| {
            panic!("{}: {family}: no appearances: {answer}", case.name);
        });
        assert_eq!(appearances.len(), payloads.len(), "{}: one payload per appearance", case.name);
        for (appearance, pixels) in appearances.iter().zip(&payloads) {
            let raster = &appearance["raster"];
            assert_eq!(raster["color"], "rgba8");
            assert_eq!(raster["premultipliedAlpha"], false);
            let stride = raster["stride"].as_u64().expect("stride") as usize;
            let height = raster["height"].as_u64().expect("height") as u32;
            assert_eq!(pixels.len(), stride * height as usize, "{}: payload size", case.name);
            placed.push(Placed {
                annotation_ref: appearance["ref"].clone(),
                mode: appearance["mode"].as_str().expect("mode").to_owned(),
                // `72.0` and the vector's `72` are one number.
                rect: test_support::canonical_numbers(&appearance["rect"]),
                width: raster["width"].as_u64().expect("width") as u32,
                height,
            });
        }
    }
    placed
}

// ── the engine directly, in the file's coordinates ──────────────────────────

/// `bytes`' first page, opened for a direct render.
fn open(bytes: &[u8]) -> (OwnedDocument, PageObjectNumber) {
    let document =
        OwnedDocument::new(Document::from_bytes(Library::acquire(), bytes, None).expect("open"))
            .expect("index");
    let page = PageObjectNumber::new(document.document().page_object_number_by_index(0))
        .expect("a page object number");
    (document, page)
}

fn render(
    document: &OwnedDocument,
    page: PageObjectNumber,
    family: AnnotationFamily,
    options: &AppearanceRenderOptions,
) -> AnnotationAppearancesResult {
    let opened = document.page(page).expect("page");
    render_page_appearances(
        document.document(),
        &opened,
        page,
        family,
        options,
        &CancelToken::never(),
    )
    .expect("render")
}

fn at_scale(scale: f64) -> AppearanceRenderOptions {
    AppearanceRenderOptions {
        viewport: Some(RenderViewport::Scale { scale }),
        ..AppearanceRenderOptions::default()
    }
}

/// The inline annotation born at `base_index` of the page.
fn born_at(appearance: &AnnotationAppearanceRaster, base_index: u32) -> bool {
    matches!(appearance.annotation_ref, AnnotationRef::BaseIndex { base_index: b, .. } if b == base_index)
}

fn by_base_index(
    result: &AnnotationAppearancesResult,
    base_index: u32,
) -> Vec<&AnnotationAppearanceRaster> {
    result.appearances.iter().filter(|a| born_at(a, base_index)).collect()
}

/// The annotation object `annot`.
fn by_object_number(
    result: &AnnotationAppearancesResult,
    annot: u32,
) -> Vec<&AnnotationAppearanceRaster> {
    result
        .appearances
        .iter()
        .filter(|a| matches!(a.annotation_ref, AnnotationRef::ObjectNumber { annot: n, .. } if n == annot))
        .collect()
}

/// `annotation_appearances.pdf`'s `/Annots`, in order: their birth names.
mod born {
    pub const THREE_MODE_SQUARE: u32 = 0;
    pub const ROTATED_SQUARE: u32 = 2;
    pub const ROTATED_CALLOUT: u32 = 3;
    pub const ROTATED_POLYGON: u32 = 5;
    pub const PAGE_SIZED_SQUARE: u32 = 6;
    pub const BARE_HIGHLIGHT: u32 = 7;
    pub const BARE_WIDGET: u32 = 8;
}
/// `a-real-field-widget` and `an-orphan-widget`, by object number.
const REAL_FIELD_WIDGET: u32 = 12;
const ORPHAN_WIDGET: u32 = 14;

fn rect(left: f64, bottom: f64, right: f64, top: f64) -> PdfRect {
    PdfRect { left, bottom, right, top }
}

/// A PDF file of `objects`, numbered from 1, with its cross-reference table
/// (`pdfOf`).
fn pdf_of(objects: &[&str]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for offset in offsets {
        out.extend(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// A form XObject that fills its `width` by `height` box with one colour,
/// under `matrix`.
fn filled(color: &str, width: u32, height: u32, matrix: &str) -> String {
    let content = format!("{color} 0 0 {width} {height} re f");
    format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 {width} {height}] {matrix} /Length {} >>\n\
         stream\n{content}\nendstream",
        content.len()
    )
}

/// A 612 by 792 page with a square (object 4) turned 30 degrees counterclockwise (330
/// clockwise) the way our writer turns one: the logical box
/// `[240 600 340 660]`, `/Rect` the upright box around it turned, and an
/// appearance whose matrix is the turn, so its drawing stays inside the
/// turned box. Beside it (object 6), the same drawing in a `/Rect` it doesn't fit
/// evenly, which is `annotation_appearances.pdf`'s `a-rotated-square`.
fn turned_square_pdf() -> Vec<u8> {
    let turn = "/Matrix [0.8660254 0.5 -0.5 0.8660254 0 0]";
    let fill = filled("0 0 1 rg", 100, 60, turn);
    pdf_of(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Annots [4 0 R 6 0 R] >>",
        "<< /Type /Annot /Subtype /Square /F 4 /NM (turned) \
         /Rect [231.69873 579.01924 348.30127 680.98076] /AP << /N 5 0 R >> \
         /EMBD_Metadata << /Rotation 30 /UnrotatedRect [240 600 340 660] >> >>",
        &fill,
        "<< /Type /Annot /Subtype /Square /F 4 /NM (stretched) /Rect [220 280 350 380] \
         /AP << /N 5 0 R >> /EMBD_Metadata << /Rotation 30 /UnrotatedRect [240 300 340 360] >> >>",
    ])
}

// ── the vector ──────────────────────────────────────────────────────────────

#[test]
fn the_appearances_land_where_the_typescript_engine_put_them() {
    let file = load();
    assert!(file.cases.len() >= 4, "the corpus shrank");
    let mut emitted = 0usize;

    for case in &file.cases {
        let actual = read(case);
        let expected: Vec<Placed> = case
            .appearances
            .iter()
            .map(|placed| Placed {
                annotation_ref: placed.annotation_ref.clone(),
                mode: placed.mode.clone(),
                rect: test_support::canonical_numbers(&placed.rect),
                width: placed.width,
                height: placed.height,
            })
            .collect();
        assert_eq!(
            actual.len(),
            case.appearances.len(),
            "{}: a different number of appearances: {}\n{actual:#?}",
            case.name,
            case.covers,
        );
        for (index, (mine, theirs)) in actual.iter().zip(&expected).enumerate() {
            assert_eq!(mine, theirs, "{}: appearance {index}: {}", case.name, case.covers);
        }
        emitted += actual.len();
    }

    assert!(emitted > 0, "the corpus emitted no appearances at all");
}

#[test]
fn a_scale_multiplies_the_raster_and_leaves_the_placement_alone() {
    // The placement box is page space and the raster is device pixels, so a
    // scale moves one and not the other. A reader that scaled the rect would
    // place every appearance in the wrong part of the page at any scale but
    // 1, which is the default and therefore the one a test forgets.
    let file = load();
    let at_one = read(case(&file, "normal"));
    let at_three = read(case(&file, "scaled"));

    assert_eq!(at_one.len(), at_three.len());
    for (small, large) in at_one.iter().zip(&at_three) {
        assert_eq!(small.rect, large.rect, "the placement moved with the scale");
        assert_eq!(large.width, small.width * 3);
        assert_eq!(large.height, small.height * 3);
    }
}

#[test]
fn only_the_modes_an_annotation_has_are_emitted() {
    // Every mode is asked for (and is the default now, 17350fb4c), and an
    // annotation that stores one gives one picture. Only the three-mode
    // square carries /R and /D, so this separates "the modes requested" from
    // "the modes present".
    let (document, page) = open(&read_fixture(APPEARANCES_FIXTURE));
    let result =
        render(&document, page, AnnotationFamily::Annotations, &AppearanceRenderOptions::default());

    let modes = |base_index| -> Vec<AppearanceModeName> {
        by_base_index(&result, base_index).iter().map(|a| a.mode).collect()
    };
    assert_eq!(
        modes(born::THREE_MODE_SQUARE),
        [AppearanceModeName::Normal, AppearanceModeName::Rollover, AppearanceModeName::Down]
    );
    for base_index in [born::ROTATED_SQUARE, born::ROTATED_CALLOUT, born::ROTATED_POLYGON] {
        assert_eq!(modes(base_index), [AppearanceModeName::Normal], "annotation {base_index}");
    }
}

// ── turned box kinds ────────────────────────────────────────────────────────

#[test]
fn a_rotated_box_kind_is_placed_by_its_logical_box_and_the_near_misses_are_not() {
    // `appearanceTurnOf`: a box kind drawn turned whose drawing stays inside
    // the turned box is placed by that box. The square here is turned the
    // way our writer turns one, so `/Rect` is the upright box around its
    // box turned.
    let (document, page) = open(&turned_square_pdf());
    let result =
        render(&document, page, AnnotationFamily::Annotations, &AppearanceRenderOptions::default());
    let turned = by_object_number(&result, 4);
    assert_eq!(turned.len(), 1);
    let placed = turned[0].rect;
    for (got, want) in
        [(placed.left, 240.0), (placed.bottom, 600.0), (placed.right, 340.0), (placed.top, 660.0)]
    {
        assert!((got - want).abs() < 0.01, "placed by {placed:?}, not its box");
    }

    // The same drawing stretched unevenly onto a `/Rect` isn't a turn, so
    // its recorded keys don't describe it and it is placed by `/Rect`.
    let stretched = by_object_number(&result, 6);
    assert_eq!(stretched[0].rect, rect(220.0, 280.0, 350.0, 380.0));

    // The near misses in `annotation_appearances.pdf` keep `/Rect`: its
    // rotated square is that stretched drawing, the callout's line isn't
    // turned, and a polygon is drawn from turned points.
    let (document, page) = open(&read_fixture(APPEARANCES_FIXTURE));
    let result =
        render(&document, page, AnnotationFamily::Annotations, &AppearanceRenderOptions::default());
    for (base_index, expected) in [
        (born::ROTATED_SQUARE, rect(220.0, 580.0, 350.0, 680.0)),
        (born::ROTATED_CALLOUT, rect(72.0, 450.0, 240.0, 530.0)),
        (born::ROTATED_POLYGON, rect(300.0, 420.0, 420.0, 520.0)),
    ] {
        assert_eq!(by_base_index(&result, base_index)[0].rect, expected, "annotation {base_index}");
    }
}

#[test]
fn a_stripped_appearance_is_drawn_flat_and_fills_its_logical_box() {
    // The other half of rendering a turned box kind upright. Placing the
    // raster by the box is one decision; drawing the appearance with its
    // turn taken out (`EPDF_RenderAnnotBitmapUnrotated`, the turn passed in
    // by the caller since runtime 5e9241945) is the other, and the placement
    // metadata cannot tell them apart.
    //
    // The turned square is a solid blue fill under a 30-degree matrix. Drawn
    // upright it covers its whole raster. Drawn turned into the same box it
    // covers a tilted rectangle inside it, and the corners go transparent.
    let (document, page) = open(&turned_square_pdf());
    let result =
        render(&document, page, AnnotationFamily::Annotations, &AppearanceRenderOptions::default());
    let stripped = by_object_number(&result, 4)[0];
    assert_eq!((stripped.raster.width, stripped.raster.height), (100, 60));

    for (x, y) in [
        (1, 1),
        (stripped.raster.width - 2, 1),
        (1, stripped.raster.height - 2),
        (stripped.raster.width - 2, stripped.raster.height - 2),
    ] {
        let pixel = stripped.raster.pixel(x, y).expect("in bounds");
        assert_eq!(
            (pixel.0, pixel.1, pixel.2, pixel.3),
            (0, 0, 255, 255),
            "corner ({x}, {y}) is not the fill; the turn was not taken out",
        );
    }
}

// ── drawing ─────────────────────────────────────────────────────────────────

#[test]
fn each_mode_draws_its_own_appearance_stream() {
    // The fixture paints /N red, /R green and /D blue, so a raster that came
    // from the wrong sub-dictionary is visible rather than merely different.
    let (document, page) = open(&read_fixture(APPEARANCES_FIXTURE));
    let result =
        render(&document, page, AnnotationFamily::Annotations, &AppearanceRenderOptions::default());

    for (mode, expected) in [
        (AppearanceModeName::Normal, (255u8, 0u8, 0u8)),
        (AppearanceModeName::Rollover, (0, 255, 0)),
        (AppearanceModeName::Down, (0, 0, 255)),
    ] {
        let drawn = by_base_index(&result, born::THREE_MODE_SQUARE)
            .into_iter()
            .find(|a| a.mode == mode)
            .unwrap_or_else(|| panic!("no {mode:?} appearance"));
        assert_eq!(drawn.state, None, "a single appearance has no state");
        let centre = drawn
            .raster
            .pixel(drawn.raster.width / 2, drawn.raster.height / 2)
            .expect("a centre pixel");
        assert_eq!(
            (centre.0, centre.1, centre.2),
            expected,
            "{mode:?} drew the wrong appearance stream"
        );
        assert_eq!(centre.3, 255, "{mode:?} drew a transparent fill");
    }
}

#[test]
fn an_appearance_is_drawn_from_the_annotations_own_place_on_the_page() {
    // The y-up flip. An appearance is placed by a matrix built from the rect
    // AFTER it has been converted to PDFium's y-down display space, and a
    // reader that skipped the conversion still produces a raster of exactly
    // the right size, drawn from the mirrored part of the page. Nothing about
    // the placement metadata would show it.
    let (document, page) = open(&read_fixture(APPEARANCES_FIXTURE));
    let result =
        render(&document, page, AnnotationFamily::Annotations, &AppearanceRenderOptions::default());

    for (base_index, expected) in [
        (born::PAGE_SIZED_SQUARE, (128u8, 128u8, 128u8)),
        (born::THREE_MODE_SQUARE, (255, 0, 0)),
        (born::ROTATED_POLYGON, (0, 255, 255)),
    ] {
        let drawn = by_base_index(&result, base_index)[0];
        // Two pixels well inside opposite corners. A mirrored placement moves
        // the fill off one of them even when the sizes agree.
        for (x, y) in [(2, 2), (drawn.raster.width - 3, drawn.raster.height - 3)] {
            let pixel = drawn.raster.pixel(x, y).expect("in bounds");
            assert_eq!(
                (pixel.0, pixel.1, pixel.2, pixel.3),
                (expected.0, expected.1, expected.2, 255),
                "annotation {base_index}: pixel ({x}, {y}) is not the fill; it was placed wrong",
            );
        }
    }
}

#[test]
fn an_annotation_with_no_appearance_stream_is_drawn_in_memory_and_nothing_is_written() {
    // Was `an_annotation_with_no_appearance_stream_is_absent_rather_than_empty`.
    // Upstream reversed it (25dd21f38): an annotation with no normal
    // appearance renders as PDFium draws it in memory, into the box that
    // drawing takes (`EPDFAnnot_GetDrawingRect`), and nothing is written.
    let (document, page) = open(&read_fixture(APPEARANCES_FIXTURE));
    let result =
        render(&document, page, AnnotationFamily::Annotations, &AppearanceRenderOptions::default());

    let highlight = by_base_index(&result, born::BARE_HIGHLIGHT);
    assert_eq!(highlight.len(), 1, "drawn in memory, normal only");
    assert_eq!(highlight[0].mode, AppearanceModeName::Normal);
    assert_eq!(highlight[0].state, None);
    assert_eq!(highlight[0].rect, rect(72.0, 380.0, 240.0, 400.0));
    let painted = (0..highlight[0].raster.height).any(|y| {
        (0..highlight[0].raster.width)
            .any(|x| highlight[0].raster.pixel(x, y).is_some_and(|p| p.3 > 0))
    });
    assert!(painted, "the in-memory highlight paints nothing");

    // Drawn in memory, never written: the file still has no /AP.
    let view = document.page(page).expect("page");
    let annotation = epdf_engine::access::PageAccess::view(&view)
        .annotation(born::BARE_HIGHLIGHT)
        .expect("the highlight");
    assert_eq!(annotation.available_appearance_modes(), 0, "the render wrote an appearance");

    // A kind PDFium can't draw without writing an appearance (the kinds
    // file gives none here) has no raster, and the empty answer is still an
    // answer.
    let (document, page) =
        open(&read_fixture("tooling/engine-conformance/fixtures/annotation_kinds.pdf"));
    let drawn =
        render(&document, page, AnnotationFamily::Annotations, &AppearanceRenderOptions::default());
    assert!(!drawn.appearances.is_empty(), "annotation_kinds.pdf is drawn in memory");
}

#[test]
fn generating_a_form_fields_appearance_never_reaches_the_reader() {
    // The reader never generates a widget's `/AP` into the file. A widget
    // with no appearance is drawn the way PDFium draws it in memory, and one
    // it can draw only by generating an appearance has no raster
    // (`AnnotationAppearanceReader`, 25dd21f38). Widgets are the form's
    // batch (1233d1349): the annotations' batch never holds one.
    //
    // That PDFium can generate one is asserted too, so the absences below
    // are the reader's rule and not a missing capability.
    let bytes = read_fixture(APPEARANCES_FIXTURE);
    let mut generated = 0usize;
    {
        let document = Document::from_bytes(Library::acquire(), &bytes, None).expect("open");
        let page = document.page(0).expect("page");
        for index in 0..page.annotation_count() {
            let annotation = page.annotation(index).expect("annotation");
            if annotation.subtype_code() != AnnotationSubtype::Widget.code() {
                continue;
            }
            assert_eq!(annotation.available_appearance_modes(), 0, "a widget already has an /AP");
            annotation.generate_form_field_appearance();
            if annotation.available_appearance_modes() != 0 {
                generated += 1;
            }
        }
    }
    assert!(generated > 0, "EPDFAnnot_GenerateFormFieldAP produced nothing for any widget");

    // A fresh document, because the calls above wrote into the other one.
    let (document, page) = open(&bytes);
    let annotations =
        render(&document, page, AnnotationFamily::Annotations, &AppearanceRenderOptions::default());
    assert!(
        annotations.appearances.iter().all(|a| !born_at(a, born::BARE_WIDGET)
            && !matches!(a.annotation_ref, AnnotationRef::ObjectNumber { .. })),
        "the annotations' batch holds a widget",
    );

    let widgets =
        render(&document, page, AnnotationFamily::Widgets, &AppearanceRenderOptions::default());
    let names: Vec<&AnnotationRef> =
        widgets.appearances.iter().map(|a| &a.annotation_ref).collect();
    assert_eq!(
        names,
        [&AnnotationRef::ObjectNumber { page, annot: REAL_FIELD_WIDGET }],
        "only the field's widget is drawn in memory; the bare and orphan widgets ({}, {ORPHAN_WIDGET}) need a generated /AP",
        born::BARE_WIDGET,
    );
    assert_eq!(widgets.appearances[0].rect, rect(72.0, 240.0, 240.0, 270.0));

    let view = document.page(page).expect("page");
    let view = epdf_engine::access::PageAccess::view(&view);
    for index in 0..view.annotation_count() {
        let annotation = view.annotation(index).expect("annotation");
        if annotation.subtype_code() == AnnotationSubtype::Widget.code() {
            assert_eq!(annotation.available_appearance_modes(), 0, "the reader wrote an /AP");
        }
    }
}

#[test]
fn the_per_appearance_pixel_clamp_reduces_the_scale_rather_than_refusing() {
    // Rust-side rather than in the vector, and deliberately so: a clamped
    // appearance is 16 million pixels by construction, which is 64 MB of
    // transient RGBA on EACH engine.
    //
    // `a-page-sized-square` is 600 by 750, so its ceiling is crossed around
    // scale 6 where a 100-point annotation needs scale 50. Everything else in
    // the fixture stays small at scale 8, which is what makes this affordable.
    let (document, page) = open(&read_fixture(APPEARANCES_FIXTURE));
    let result = render(&document, page, AnnotationFamily::Annotations, &at_scale(8.0));
    let big = by_base_index(&result, born::PAGE_SIZED_SQUARE)[0];

    // The ceiling bounds the SCALE, and each dimension is rounded up from it
    // independently, so the product can clear it by one row plus one column.
    let pixels = u64::from(big.raster.width) * u64::from(big.raster.height);
    let slack = u64::from(big.raster.width) + u64::from(big.raster.height) + 1;
    assert!(
        pixels <= APPEARANCE_PIXEL_CLAMP as u64 + slack,
        "a clamped appearance is {}x{} = {pixels} pixels",
        big.raster.width,
        big.raster.height,
    );
    // And it really was clamped rather than merely small: scale 8 on a
    // 600-point rect would have asked for 4800 pixels across.
    assert!(big.raster.width < 4800, "the clamp did not reduce the scale");
    // The placement is untouched. The clamp is a memory ceiling, not a crop.
    assert_eq!(big.rect, rect(6.0, 20.0, 606.0, 770.0));

    // A small annotation at the same scale is not clamped at all.
    let small = by_base_index(&result, born::THREE_MODE_SQUARE)[0];
    assert_eq!((small.raster.width, small.raster.height), (800, 480));
}

#[test]
fn a_deep_zoom_batch_stays_bounded_and_complete() {
    // `appearance-clamp.test.ts`: at scale 50, the territory that ran the
    // wasm heap out before the clamp, every raster stays within the ceiling
    // and the batch resolves, while a small annotation still renders sharper
    // than the clamped page-sized stamp's scale would allow.
    let (document, page) = open(&read_fixture(EBOOK));
    let result = render(&document, page, AnnotationFamily::Annotations, &at_scale(50.0));
    assert!(!result.appearances.is_empty());
    for appearance in &result.appearances {
        let (width, height) = (appearance.raster.width, appearance.raster.height);
        assert!(width > 0 && height > 0);
        assert!(f64::from(width) * f64::from(height) <= APPEARANCE_PIXEL_CLAMP * 1.02);
    }
    let sharp = result.appearances.iter().any(|a| {
        let width = a.rect.right - a.rect.left;
        width > 0.0 && f64::from(a.raster.width) / width > 20.0
    });
    assert!(sharp, "a small annotation was clamped too");
}

#[test]
fn a_real_document_renders_visible_appearances_under_their_names() {
    // `runAnnotationAppearanceConformance` on `ebook-annotated.pdf`, page
    // object 2: at least four appearances, each named by its object number
    // or its birth position, each with pixels, and some of them visible.
    let mut host = host_with(read_fixture(EBOOK));
    let (answer, payloads) = host_render(
        &mut host,
        2,
        "annotations",
        Some(json!({ "viewport": { "kind": "scale", "scale": 1 } })),
    );
    let result = &answer["result"];
    assert_eq!(result["page"], json!({ "kind": "objectNumber", "objectNumber": 2 }), "{answer}");
    let appearances = result["appearances"].as_array().expect("appearances");
    assert!(appearances.len() >= 4, "{answer}");
    for appearance in appearances {
        assert!(
            ["objectNumber", "baseIndex"].contains(&appearance["ref"]["kind"].as_str().unwrap())
        );
        assert!(["normal", "rollover", "down"].contains(&appearance["mode"].as_str().unwrap()));
        assert!(appearance["raster"]["width"].as_u64().unwrap() > 0);
        assert!(appearance["raster"]["height"].as_u64().unwrap() > 0);
    }
    let visible =
        payloads.iter().any(|rgba| rgba.iter().skip(3).step_by(4).any(|alpha| *alpha != 0));
    assert!(visible, "every appearance is blank");
}

// ── states (`runAppearanceStatesConformance`) ───────────────────────────────

/// A form XObject that fills its 40 × 40 box with one colour.
fn filled_box(color: &str) -> String {
    filled(color, 40, 40, "")
}

/// `APPEARANCE_STATES_FIXTURE_PDF`: one 200 × 200 page with a check box that
/// is off and stores every mode (normal Yes red and Off green, down Yes blue
/// and Off yellow, rollover Yes black), and beside it a square whose normal
/// appearance is one stream (cyan).
fn states_pdf() -> Vec<u8> {
    let fills = ["1 0 0 rg", "0 1 0 rg", "0 0 1 rg", "1 1 0 rg", "0 g"].map(filled_box);
    let cyan = filled_box("0 1 1 rg");
    pdf_of(&[
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] >> >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [4 0 R 10 0 R] >>",
        "<< /Type /Annot /Subtype /Widget /FT /Btn /T (states) /V /Off /AS /Off /F 4 \
         /Rect [20 20 60 60] /AP << /N << /Yes 5 0 R /Off 6 0 R >> \
         /D << /Yes 7 0 R /Off 8 0 R >> /R << /Yes 9 0 R >> >> >>",
        &fills[0],
        &fills[1],
        &fills[2],
        &fills[3],
        &fills[4],
        "<< /Type /Annot /Subtype /Square /Rect [100 20 140 60] /F 4 /AP << /N 11 0 R >> >>",
        &cyan,
    ])
}

/// `mode:state` of each picture, sorted.
fn labels(result: &AnnotationAppearancesResult) -> Vec<String> {
    let mut labels: Vec<String> = result
        .appearances
        .iter()
        .map(|a| {
            let mode = serde_json::to_value(a.mode).unwrap();
            format!("{}:{}", mode.as_str().unwrap(), a.state.as_deref().unwrap_or("-"))
        })
        .collect();
    labels.sort();
    labels
}

#[test]
fn every_mode_and_state_an_annotation_stores_comes_back_labelled() {
    // Each family has its own batch: the square's is the annotations', the
    // check box's the form's, and neither holds the other's.
    let (document, page) = open(&states_pdf());
    let annotations =
        render(&document, page, AnnotationFamily::Annotations, &AppearanceRenderOptions::default());
    let widgets =
        render(&document, page, AnnotationFamily::Widgets, &AppearanceRenderOptions::default());

    assert_eq!(labels(&annotations), ["normal:-"]);
    assert!(
        annotations
            .appearances
            .iter()
            .all(|a| a.annotation_ref == AnnotationRef::ObjectNumber { page, annot: 10 })
    );
    assert_eq!(
        labels(&widgets),
        ["down:Off", "down:Yes", "normal:Off", "normal:Yes", "rollover:Yes"]
    );
    assert!(
        widgets
            .appearances
            .iter()
            .all(|a| a.annotation_ref == AnnotationRef::ObjectNumber { page, annot: 4 })
    );

    // `modes` asks for some only, in the engine's order; every mode is the
    // same as none.
    let normal = AppearanceRenderOptions {
        modes: Some(vec![AppearanceModeName::Normal]),
        ..AppearanceRenderOptions::default()
    };
    assert_eq!(
        labels(&render(&document, page, AnnotationFamily::Widgets, &normal)),
        ["normal:Off", "normal:Yes"]
    );
    let every = AppearanceRenderOptions {
        modes: Some(vec![
            AppearanceModeName::Down,
            AppearanceModeName::Normal,
            AppearanceModeName::Rollover,
        ]),
        ..AppearanceRenderOptions::default()
    };
    assert_eq!(
        labels(&render(&document, page, AnnotationFamily::Widgets, &every)),
        labels(&widgets)
    );
}

#[test]
fn each_state_draws_its_own_picture_whatever_the_check_box_shows() {
    let (document, page) = open(&states_pdf());
    let widgets =
        render(&document, page, AnnotationFamily::Widgets, &AppearanceRenderOptions::default());
    for picture in &widgets.appearances {
        let mode = serde_json::to_value(picture.mode).unwrap();
        let label = format!("{}:{}", mode.as_str().unwrap(), picture.state.as_deref().unwrap());
        let expected = match label.as_str() {
            "normal:Yes" => (255, 0, 0),
            "normal:Off" => (0, 255, 0),
            "down:Yes" => (0, 0, 255),
            "down:Off" => (255, 255, 0),
            "rollover:Yes" => (0, 0, 0),
            other => panic!("unexpected picture {other}"),
        };
        let middle =
            picture.raster.pixel(picture.raster.width / 2, picture.raster.height / 2).unwrap();
        assert_eq!((middle.0, middle.1, middle.2), expected, "{label}");
    }
}

#[test]
fn an_empty_modes_list_is_refused() {
    let mut host = host_with(states_pdf());
    for family in ["annotations", "widgets"] {
        let (answer, _) = host_render(&mut host, 3, family, Some(json!({ "modes": [] })));
        assert_eq!(answer["error"]["code"], "InvalidArg", "{family}: {answer}");
    }
}

// ── annotations.readAppearance ──────────────────────────────────────────────

#[test]
fn a_stamps_appearance_reads_as_a_one_page_drawing_and_other_kinds_have_none() {
    // `AnnotationFlattener.readAppearance`: only a kind that takes an
    // `appearance` resource has one, and of those only the stamp does.
    let stamp = filled("0 0 1 rg", 100, 60, "");
    let bytes = pdf_of(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Annots [4 0 R 6 0 R 7 0 R] >>",
        "<< /Type /Annot /Subtype /Stamp /F 4 /Rect [100 100 200 160] /AP << /N 5 0 R >> >>",
        &stamp,
        "<< /Type /Annot /Subtype /Square /F 4 /Rect [300 100 400 160] /AP << /N 5 0 R >> >>",
        "<< /Type /Annot /Subtype /Stamp /F 4 /Rect [100 300 200 360] >>",
    ]);
    let mut host = host_with(bytes);
    let read_appearance = |host: &mut Host, annot: u32| {
        let response = host.call(
            &json!({ "kind": "annotations.readAppearance", "effect": "read", "jobId": 3,
                "docId": "doc", "page": { "kind": "objectNumber", "objectNumber": 3 },
                "ref": { "kind": "objectNumber",
                    "page": { "kind": "objectNumber", "objectNumber": 3 },
                    "objectNumber": annot } })
            .to_string(),
            Vec::new(),
        );
        let answer: Value = serde_json::from_str(&response.json).expect("JSON");
        (answer, response.payloads)
    };

    let (answer, payloads) = read_appearance(&mut host, 4);
    assert!(answer.get("error").is_none(), "{answer}");
    assert_eq!(payloads.len(), 1);
    assert_eq!(answer["result"]["size"], payloads[0].len());
    let drawing =
        Document::from_bytes(Library::acquire(), &payloads[0], None).expect("a PDF of the drawing");
    assert_eq!(drawing.page_count(), 1);

    let (answer, _) = read_appearance(&mut host, 6);
    assert_eq!(answer["error"]["code"], "InvalidArg", "{answer}");
    assert_eq!(answer["error"]["message"], "a square annotation has no 'appearance' resource");

    let (answer, _) = read_appearance(&mut host, 7);
    assert_eq!(answer["error"]["code"], "InvalidArg", "{answer}");
    assert_eq!(answer["error"]["message"], "the annotation has no normal appearance to read");
}
