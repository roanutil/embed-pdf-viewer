//! G7's appearance gate: the same appearances land in the same places.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run annotation-appearance
//!
//! # Placement, not pixels
//!
//! What is compared is which appearances are emitted, from which mode, at
//! which box and at what size. Not the pixels, and the reason is the one
//! `corpus.ts` gives for embedding fonts: the TypeScript reads through the
//! WASM build and this reads through the native dylib, so two identical
//! readers can still produce rasters that differ in the last bit of a
//! filtered edge. Comparing digests would compare two rasterizers.
//!
//! The placement is where the rule lives anyway. Whether an annotation is
//! emitted at all, and whether its box is `/Rect` or the logical unrotated
//! one, is the rotation-stripping decision, and getting that wrong puts a
//! picture somewhere the document did not.

use epdf_engine::access::{DocumentAccess, OwnedDocument};
use epdf_engine::cancel::CancelToken;
use epdf_engine::features::annotation::appearance::{
    APPEARANCE_PIXEL_CLAMP, AppearanceModeName, AppearanceRenderOptions, render_page_appearances,
};
use epdf_engine::features::annotation::subtype::AnnotationSubtype;
use epdf_engine::identity::PageObjectNumber;
use epdf_engine::revision::RevisionToken;
use pdfium::{Document, Library};
use serde::Deserialize;

const SCHEMA_VERSION: u32 = 1;
const SESSION_PLACEHOLDER: &str = "<session>";

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

#[derive(Debug, Deserialize, PartialEq)]
struct Placed {
    #[serde(rename = "ref")]
    annotation_ref: serde_json::Value,
    mode: String,
    rect: Rect,
    width: u32,
    height: u32,
}

#[derive(Debug, Deserialize, PartialEq)]
struct Rect {
    left: f64,
    bottom: f64,
    right: f64,
    top: f64,
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

fn mode_name(name: &str) -> AppearanceModeName {
    match name {
        "normal" => AppearanceModeName::Normal,
        "rollover" => AppearanceModeName::Rollover,
        "down" => AppearanceModeName::Down,
        other => panic!("unknown appearance mode {other}"),
    }
}

/// Read a case the way the generator did.
fn read(case: &Case) -> Vec<Placed> {
    let bytes = std::fs::read(test_support::workspace_root().join(&case.fixture))
        .unwrap_or_else(|e| panic!("{}: read {}: {e}", case.name, case.fixture));
    let document = OwnedDocument::new(
        Document::from_bytes(Library::acquire(), &bytes, None)
            .unwrap_or_else(|e| panic!("{}: open: {e}", case.name)),
    )
    .unwrap_or_else(|e| panic!("{}: index: {e}", case.name));

    let page_object_number =
        PageObjectNumber::new(case.page_object_number).expect("a page object number");
    let page = document.page(page_object_number).expect("page");
    let options = AppearanceRenderOptions {
        scale: case.scale.unwrap_or(1.0),
        modes: case
            .modes
            .as_ref()
            .map(|names| names.iter().map(|n| mode_name(n)).collect())
            .unwrap_or_default(),
        ..AppearanceRenderOptions::default()
    };
    let revision = RevisionToken::new(SESSION_PLACEHOLDER, page_object_number, 0);

    let result = render_page_appearances(
        &page,
        page_object_number,
        &revision,
        &options,
        &CancelToken::never(),
    )
    .unwrap_or_else(|e| panic!("{}: render: {e}", case.name));

    result
        .appearances
        .into_iter()
        .map(|a| Placed {
            annotation_ref: serde_json::to_value(&a.annotation_ref).expect("serialize"),
            mode: serde_json::to_value(a.mode)
                .expect("serialize")
                .as_str()
                .expect("a mode name")
                .to_string(),
            rect: Rect {
                left: a.rect.left,
                bottom: a.rect.bottom,
                right: a.rect.right,
                top: a.rect.top,
            },
            width: a.raster.width,
            height: a.raster.height,
        })
        .collect()
}

#[test]
fn the_appearances_land_where_the_typescript_engine_put_them() {
    let file = load();
    assert!(file.cases.len() >= 4, "the corpus shrank");
    let mut emitted = 0usize;

    for case in &file.cases {
        let actual = read(case);
        assert_eq!(
            actual.len(),
            case.appearances.len(),
            "{}: a different number of appearances: {}",
            case.name,
            case.covers,
        );
        for (index, (mine, theirs)) in actual.iter().zip(&case.appearances).enumerate() {
            assert_eq!(mine, theirs, "{}: appearance {index}: {}", case.name, case.covers);
        }
        emitted += actual.len();
    }

    assert!(emitted > 0, "the corpus emitted no appearances at all");
}

#[test]
fn a_rotated_box_kind_is_placed_by_its_logical_box_and_the_near_misses_are_not() {
    // The whole of the rotation-stripping rule, stated where it can be read.
    // The square's `/Rect` is the rotated visual box [220 580 350 680] and
    // its `/UnrotatedRect` is [240 600 340 660]; the callout and the polygon
    // carry rotation metadata too and must still be placed by `/Rect`.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "normal").expect("the normal case");
    let actual = read(case);

    let by_name = |nm: &str| {
        actual
            .iter()
            .find(|a| a.annotation_ref["nm"].as_str() == Some(nm))
            .unwrap_or_else(|| panic!("no appearance for {nm}"))
    };

    let stripped = by_name("a-rotated-square");
    assert_eq!(
        stripped.rect,
        Rect { left: 240.0, bottom: 600.0, right: 340.0, top: 660.0 },
        "a box kind with the rotation pair is placed by its logical box",
    );

    let callout = by_name("a-rotated-callout");
    assert_eq!(
        callout.rect,
        Rect { left: 72.0, bottom: 450.0, right: 240.0, top: 530.0 },
        "a callout tilts inline, so its form matrix is identity and nothing is stripped",
    );

    let polygon = by_name("a-rotated-polygon");
    assert_eq!(
        polygon.rect,
        Rect { left: 300.0, bottom: 420.0, right: 420.0, top: 520.0 },
        "a vertex kind has its rotation in its geometry, so there is no matrix to strip",
    );
}

#[test]
fn only_the_modes_an_annotation_has_are_emitted() {
    // Asking for three modes does not produce three appearances for an
    // annotation that has one. Only the square in the fixture carries /R and
    // /D, so this separates "the modes requested" from "the modes present".
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "every-mode").expect("the every-mode case");
    let actual = read(case);

    let square: Vec<&str> = actual
        .iter()
        .filter(|a| a.annotation_ref["nm"].as_str() == Some("a-three-mode-square"))
        .map(|a| a.mode.as_str())
        .collect();
    assert_eq!(square, ["normal", "rollover", "down"]);

    for nm in ["a-rotated-square", "a-rotated-callout", "a-rotated-polygon"] {
        let modes: Vec<&str> = actual
            .iter()
            .filter(|a| a.annotation_ref["nm"].as_str() == Some(nm))
            .map(|a| a.mode.as_str())
            .collect();
        assert_eq!(modes, ["normal"], "{nm} has only a normal appearance");
    }
}

#[test]
fn an_annotation_with_no_appearance_stream_is_absent_rather_than_empty() {
    // Two of them in the fixture and neither appears anywhere: a highlight,
    // which a viewer draws itself, and a widget with no `/AP` key at all.
    //
    // The widget is the interesting one. The reader has a fallback that
    // generates a form field's appearance, and it cannot fire: the caller
    // skips an annotation whose available-modes word is zero, which is
    // exactly the case the fallback is for. That is true of the TypeScript
    // too, and this is what pins the consequence rather than the intent.
    let file = load();
    for case in &file.cases {
        let actual = read(case);
        for nm in ["a-bare-highlight", "a-bare-widget"] {
            assert!(
                !actual.iter().any(|a| a.annotation_ref["nm"].as_str() == Some(nm)),
                "{}: {nm} has no /AP and should not have been rendered",
                case.name,
            );
        }
    }
}

#[test]
fn a_scale_multiplies_the_raster_and_leaves_the_placement_alone() {
    // The placement box is PDF user space and the raster is device pixels, so
    // a scale moves one and not the other. A reader that scaled the rect
    // would place every appearance in the wrong part of the page at any scale
    // but 1, which is the default and therefore the one a test forgets.
    let file = load();
    let one = file.cases.iter().find(|c| c.name == "normal").expect("normal");
    let three = file.cases.iter().find(|c| c.name == "scaled").expect("scaled");
    let at_one = read(one);
    let at_three = read(three);

    assert_eq!(at_one.len(), at_three.len());
    for (small, large) in at_one.iter().zip(&at_three) {
        assert_eq!(small.rect, large.rect, "the placement moved with the scale");
        assert_eq!(large.width, small.width * 3);
        assert_eq!(large.height, small.height * 3);
    }
}

#[test]
fn the_per_appearance_pixel_clamp_reduces_the_scale_rather_than_refusing() {
    // Rust-side rather than in the vector, and deliberately so: a clamped
    // appearance is 16 million pixels by construction, which is 64 MB of
    // transient RGBA on EACH engine. Asking the TypeScript oracle for one per
    // annotation would cost a quarter of a gigabyte to learn something the
    // arithmetic already says.
    //
    // `a-page-sized-square` is 600 by 750, so its ceiling is crossed around
    // scale 6 where a 100-point annotation needs scale 50. Everything else in
    // the fixture stays small at scale 8, which is what makes this affordable.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "normal").expect("normal");
    let bytes = std::fs::read(test_support::workspace_root().join(&case.fixture)).expect("read");
    let document =
        OwnedDocument::new(Document::from_bytes(Library::acquire(), &bytes, None).expect("open"))
            .expect("index");

    let page_object_number = PageObjectNumber::new(case.page_object_number).expect("page number");
    let page = document.page(page_object_number).expect("page");
    let result = render_page_appearances(
        &page,
        page_object_number,
        &RevisionToken::new(SESSION_PLACEHOLDER, page_object_number, 0),
        &AppearanceRenderOptions { scale: 8.0, ..AppearanceRenderOptions::default() },
        &CancelToken::never(),
    )
    .expect("render");

    let big = result
        .appearances
        .iter()
        .find(|a| {
            serde_json::to_value(&a.annotation_ref).expect("serialize")["nm"].as_str()
                == Some("a-page-sized-square")
        })
        .expect("the page-sized square");

    // The ceiling bounds the SCALE, and each dimension is rounded up from it
    // independently, so the product can clear it by one row plus one column.
    // Measured here: 3578 by 4472 is 16,000,816 against a ceiling of
    // 16,000,000. Asserting the exact ceiling would be asserting that the
    // rounding does not exist.
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
    assert_eq!(big.rect.left, 6.0);
    assert_eq!(big.rect.right, 606.0);

    // A small annotation at the same scale is not clamped at all.
    let small = result
        .appearances
        .iter()
        .find(|a| {
            serde_json::to_value(&a.annotation_ref).expect("serialize")["nm"].as_str()
                == Some("a-three-mode-square")
        })
        .expect("the three-mode square");
    assert_eq!(small.raster.width, 800);
    assert_eq!(small.raster.height, 480);
}

#[test]
fn each_mode_draws_its_own_appearance_stream() {
    // The fixture paints /N red, /R green and /D blue, so a raster that came
    // from the wrong sub-dictionary is visible rather than merely different.
    // This is also the only thing here that looks at pixels at all, and it
    // can, because a solid fill does not depend on which build rasterized it
    // the way a glyph edge does.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "every-mode").expect("every-mode");
    let bytes = std::fs::read(test_support::workspace_root().join(&case.fixture)).expect("read");
    let document =
        OwnedDocument::new(Document::from_bytes(Library::acquire(), &bytes, None).expect("open"))
            .expect("index");

    let page_object_number = PageObjectNumber::new(case.page_object_number).expect("page number");
    let page = document.page(page_object_number).expect("page");
    let result = render_page_appearances(
        &page,
        page_object_number,
        &RevisionToken::new(SESSION_PLACEHOLDER, page_object_number, 0),
        &AppearanceRenderOptions {
            modes: vec![
                AppearanceModeName::Normal,
                AppearanceModeName::Rollover,
                AppearanceModeName::Down,
            ],
            ..AppearanceRenderOptions::default()
        },
        &CancelToken::never(),
    )
    .expect("render");

    for (mode, expected) in [
        (AppearanceModeName::Normal, (255u8, 0u8, 0u8)),
        (AppearanceModeName::Rollover, (0, 255, 0)),
        (AppearanceModeName::Down, (0, 0, 255)),
    ] {
        let drawn = result
            .appearances
            .iter()
            .find(|a| {
                a.mode == mode
                    && serde_json::to_value(&a.annotation_ref).expect("serialize")["nm"].as_str()
                        == Some("a-three-mode-square")
            })
            .unwrap_or_else(|| panic!("no {mode:?} appearance"));

        let centre = drawn
            .raster
            .pixel(drawn.raster.width / 2, drawn.raster.height / 2)
            .expect("a centre pixel");
        assert_eq!(
            (centre.0, centre.1, centre.2),
            expected,
            "{mode:?} drew the wrong appearance stream",
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
    //
    // The page-sized square is filled solid grey, so every pixel of its
    // appearance is grey when it is drawn from the right place and the corner
    // pixels go transparent when it is not.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "normal").expect("normal");
    let bytes = std::fs::read(test_support::workspace_root().join(&case.fixture)).expect("read");
    let document =
        OwnedDocument::new(Document::from_bytes(Library::acquire(), &bytes, None).expect("open"))
            .expect("index");

    let page_object_number = PageObjectNumber::new(case.page_object_number).expect("page number");
    let page = document.page(page_object_number).expect("page");
    let result = render_page_appearances(
        &page,
        page_object_number,
        &RevisionToken::new(SESSION_PLACEHOLDER, page_object_number, 0),
        &AppearanceRenderOptions::default(),
        &CancelToken::never(),
    )
    .expect("render");

    for (nm, expected) in [
        ("a-page-sized-square", (128u8, 128u8, 128u8)),
        ("a-three-mode-square", (255, 0, 0)),
        ("a-rotated-polygon", (0, 255, 255)),
    ] {
        let drawn = result
            .appearances
            .iter()
            .find(|a| {
                serde_json::to_value(&a.annotation_ref).expect("serialize")["nm"].as_str()
                    == Some(nm)
            })
            .unwrap_or_else(|| panic!("no appearance for {nm}"));

        // Two pixels well inside opposite corners. A mirrored placement moves
        // the fill off one of them even when the sizes agree.
        for (x, y) in [(2, 2), (drawn.raster.width - 3, drawn.raster.height - 3)] {
            let pixel = drawn.raster.pixel(x, y).expect("in bounds");
            assert_eq!(
                (pixel.0, pixel.1, pixel.2, pixel.3),
                (expected.0, expected.1, expected.2, 255),
                "{nm}: pixel ({x}, {y}) is not the fill; the appearance was placed wrong",
            );
        }
    }
}

#[test]
fn a_stripped_appearance_is_drawn_flat_and_fills_its_logical_box() {
    // The other half of rotation stripping. Placing the raster by the logical
    // box is one decision; drawing the form content WITHOUT its `/Matrix` is
    // the other, and the placement metadata cannot tell them apart.
    //
    // `a-rotated-square` is a solid blue fill under a 30-degree matrix. Drawn
    // flat it covers its whole raster. Drawn rotated into the same box it
    // covers a tilted square inside it, and the corners go transparent.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "normal").expect("normal");
    let bytes = std::fs::read(test_support::workspace_root().join(&case.fixture)).expect("read");
    let document =
        OwnedDocument::new(Document::from_bytes(Library::acquire(), &bytes, None).expect("open"))
            .expect("index");

    let page_object_number = PageObjectNumber::new(case.page_object_number).expect("page number");
    let page = document.page(page_object_number).expect("page");
    let result = render_page_appearances(
        &page,
        page_object_number,
        &RevisionToken::new(SESSION_PLACEHOLDER, page_object_number, 0),
        &AppearanceRenderOptions::default(),
        &CancelToken::never(),
    )
    .expect("render");

    let stripped = result
        .appearances
        .iter()
        .find(|a| {
            serde_json::to_value(&a.annotation_ref).expect("serialize")["nm"].as_str()
                == Some("a-rotated-square")
        })
        .expect("the rotated square");

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
            "corner ({x}, {y}) is not the fill; the rotation was not stripped",
        );
    }
}

#[test]
fn generating_a_form_fields_appearance_never_reaches_the_reader() {
    // The reader has a fallback that asks PDFium to synthesize a widget's
    // `/AP`, and it never runs. Not because PDFium refuses: this calls the
    // same function directly and it works. The fallback is unreachable
    // because the reader samples the available-modes word ONCE, before any
    // generation could happen, and then skips every mode whose bit is clear
    // in that sample.
    //
    // Both halves are asserted, because each one alone would be misleading.
    // That generation works says the fallback is not dead for want of
    // support; that the reader emits nothing says it is dead anyway.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "normal").expect("normal");
    let bytes = std::fs::read(test_support::workspace_root().join(&case.fixture)).expect("read");

    let mut generated = 0usize;
    {
        let document = Document::from_bytes(Library::acquire(), &bytes, None).expect("open");
        let page = document.page(0).expect("page");
        for index in 0..page.annotation_count() {
            let annotation = page.annotation(index).expect("annotation");
            if annotation.subtype_code() != AnnotationSubtype::Widget.code() {
                continue;
            }
            assert_eq!(
                annotation.available_appearance_modes(),
                0,
                "a widget in this fixture already has an /AP, so it proves nothing",
            );
            annotation.generate_form_field_appearance();
            if annotation.available_appearance_modes() != 0 {
                generated += 1;
            }
        }
    }
    assert!(
        generated > 0,
        "EPDFAnnot_GenerateFormFieldAP produced nothing for any widget; the fallback is now \
         dead for a different reason and the comment in appearance.rs is wrong",
    );

    // A fresh document, because the call above wrote into the other one.
    let document =
        OwnedDocument::new(Document::from_bytes(Library::acquire(), &bytes, None).expect("open"))
            .expect("index");
    let page_object_number = PageObjectNumber::new(case.page_object_number).expect("page number");
    let page = document.page(page_object_number).expect("page");
    let result = render_page_appearances(
        &page,
        page_object_number,
        &RevisionToken::new(SESSION_PLACEHOLDER, page_object_number, 0),
        &AppearanceRenderOptions::default(),
        &CancelToken::never(),
    )
    .expect("render");

    for nm in ["a-bare-widget", "a-real-field-widget"] {
        assert!(
            !result.appearances.iter().any(|a| {
                serde_json::to_value(&a.annotation_ref).expect("serialize")["nm"].as_str()
                    == Some(nm)
            }),
            "{nm} was rendered; the generation fallback is reachable after all",
        );
    }
}
