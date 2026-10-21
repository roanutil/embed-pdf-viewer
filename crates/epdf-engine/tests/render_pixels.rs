//! G4's pixel gate: exact interior colors, tiles, rotation, alpha, lifetime.
//!
//! The port order asks for "exact interior colors on the existing colored
//! fixture: BGRA internally where used, and RGBA with the existing alpha
//! contract at the public web boundary; loose whole-page gate; tile, rotation,
//! alpha and buffer-lifetime tests".
//!
//! Colors are the part a grayscale fixture cannot check: a page of black text
//! on white looks identical whether the channels are RGBA or BGRA, so
//! `colors.pdf` is what separates a correct conversion from a swapped one.

use epdf_engine::access::{DocumentAccess, OwnedDocument, PageAccess};
use epdf_engine::cancel::CancelToken;
use epdf_engine::error::EngineErrorCode;
use epdf_engine::features::render::geometry::{RenderBackground, RenderTarget, RenderViewport};
use epdf_engine::features::render::raster::{PageRaster, RenderOptions, render_page};
use epdf_engine::geom::{PdfRect, PdfRotation};
use epdf_engine::identity::PageObjectNumber;
use pdfium::{Document, Library};

fn open(fixture: &str) -> OwnedDocument {
    OwnedDocument::new(
        Document::from_bytes(Library::acquire(), &test_support::fixture(fixture), None)
            .expect("open"),
    )
    .expect("index")
}

fn first_page(document: &OwnedDocument) -> PageObjectNumber {
    PageObjectNumber::new(document.document().page(0).expect("page").object_number())
        .expect("object number")
}

fn render(document: &OwnedDocument, options: RenderOptions) -> PageRaster {
    let page = document.page(first_page(document)).expect("page");
    render_page(&page, &options, &CancelToken::never()).expect("render")
}

/// Every distinct pixel in the raster, so a color assertion can say what it
/// actually found rather than only that it did not find what it wanted.
fn colors(raster: &PageRaster) -> std::collections::BTreeSet<(u8, u8, u8, u8)> {
    let mut seen = std::collections::BTreeSet::new();
    for y in 0..raster.height {
        for x in 0..raster.width {
            seen.insert(raster.pixel(x, y).expect("in bounds"));
        }
    }
    seen
}

#[test]
fn a_whole_page_renders_at_the_size_the_viewport_asked_for() {
    let document = open(test_support::REPORT);
    let raster = render(
        &document,
        RenderOptions {
            viewport: RenderViewport::Scale { scale: 1.0 },
            ..RenderOptions::default()
        },
    );

    // report.pdf is US Letter, so scale 1 is 612 by 792 device pixels.
    assert_eq!((raster.width, raster.height), (612, 792));
    assert!(raster.stride >= raster.width * 4, "stride is at least a packed row");
    assert_eq!(raster.rgba.len(), (raster.stride * raster.height) as usize);
}

#[test]
fn the_channels_are_rgba_and_not_bgra() {
    // The assertion a grayscale page cannot make. colors.pdf has saturated
    // interior fills, so a swapped conversion turns its reds blue.
    let document = open(test_support::COLORS);
    let raster = render(&document, RenderOptions::default());

    let mut saw_red = false;
    let mut saw_blue = false;
    for (r, g, b, _) in colors(&raster) {
        if r > 200 && g < 80 && b < 80 {
            saw_red = true;
        }
        if b > 200 && g < 80 && r < 80 {
            saw_blue = true;
        }
    }
    assert!(saw_red, "no red pixel; the channels are probably swapped");
    assert!(saw_blue, "no blue pixel; the channels are probably swapped");
}

#[test]
fn a_white_background_is_opaque_and_a_transparent_one_is_not() {
    // The alpha contract. Straight alpha, not premultiplied, so an untouched
    // pixel under a transparent background is fully zero rather than white
    // with zero alpha.
    let document = open(test_support::REPORT);

    let opaque = render(
        &document,
        RenderOptions { background: RenderBackground::White, ..RenderOptions::default() },
    );
    assert_eq!(
        opaque.pixel(0, 0),
        Some((255, 255, 255, 255)),
        "a white background corner should be opaque white",
    );

    let transparent = render(
        &document,
        RenderOptions { background: RenderBackground::Transparent, ..RenderOptions::default() },
    );
    assert_eq!(
        transparent.pixel(0, 0),
        Some((0, 0, 0, 0)),
        "a transparent corner should be fully zero",
    );
}

#[test]
fn a_quarter_turn_swaps_the_output_dimensions() {
    let document = open(test_support::REPORT);
    let upright = render(&document, RenderOptions::default());
    let turned = render(
        &document,
        RenderOptions { rotation: PdfRotation::Clockwise90, ..RenderOptions::default() },
    );

    assert_eq!((turned.width, turned.height), (upright.height, upright.width));
}

#[test]
fn a_half_turn_keeps_the_size_and_reverses_the_image() {
    let document = open(test_support::COLORS);
    let upright = render(&document, RenderOptions::default());
    let turned = render(
        &document,
        RenderOptions { rotation: PdfRotation::Clockwise180, ..RenderOptions::default() },
    );

    assert_eq!((turned.width, turned.height), (upright.width, upright.height));
    // The same picture upside down: the corner that was top-left is now
    // bottom-right. Checked on the page corners, which are background on both.
    let last = (upright.width - 1, upright.height - 1);
    assert_eq!(upright.pixel(0, 0), turned.pixel(last.0, last.1));
    assert_ne!(upright.rgba, turned.rgba, "a half turn that changed nothing did not happen");
}

#[test]
fn a_tile_is_the_same_pixels_as_that_region_of_the_whole_page() {
    // The tile gate. A sub-rect rendered at scale 1 must match the
    // corresponding window of the full-page render, or tiled viewers show
    // seams.
    let document = open(test_support::COLORS);
    let page = document.page(first_page(&document)).expect("page");
    let (page_width, page_height) =
        (f64::from(page.view().width()), f64::from(page.view().height()));

    let whole = render(&document, RenderOptions::default());

    // The top-left quarter, in PDF user space: y is up, so the top half is the
    // upper half of the page box.
    let tile_rect =
        PdfRect { left: 0.0, bottom: page_height / 2.0, right: page_width / 2.0, top: page_height };
    let tile = render(
        &document,
        RenderOptions {
            target: RenderTarget::Rect { rect: tile_rect },
            ..RenderOptions::default()
        },
    );

    assert_eq!(tile.width, whole.width / 2);
    assert_eq!(tile.height, whole.height / 2);
    for y in 0..tile.height {
        for x in 0..tile.width {
            assert_eq!(
                tile.pixel(x, y),
                whole.pixel(x, y),
                "tile pixel ({x}, {y}) differs from the whole-page render",
            );
        }
    }
}

#[test]
fn a_width_viewport_lands_on_exactly_that_width() {
    let document = open(test_support::REPORT);
    for width in [100.0, 333.0, 1000.0] {
        let raster = render(
            &document,
            RenderOptions { viewport: RenderViewport::Width { width }, ..RenderOptions::default() },
        );
        assert_eq!(raster.width, width as u32, "a width viewport missed its width");
    }
}

#[test]
fn the_pixel_budget_refuses_before_allocating() {
    // The decode-bomb guard. A budget that only checked the width would let a
    // tall page through, so the check is on the product.
    let document = open(test_support::REPORT);
    let page = document.page(first_page(&document)).expect("page");
    let refused = render_page(
        &page,
        &RenderOptions {
            viewport: RenderViewport::Scale { scale: 4.0 },
            max_output_pixels: Some(1_000),
            ..RenderOptions::default()
        },
        &CancelToken::never(),
    )
    .expect_err("over budget");
    assert_eq!(refused.code, EngineErrorCode::InvalidArg);
    assert!(refused.message.contains("budget"), "{}", refused.message);

    // And a budget the request fits inside does not refuse it.
    render_page(
        &page,
        &RenderOptions {
            viewport: RenderViewport::Scale { scale: 0.1 },
            max_output_pixels: Some(1_000_000),
            ..RenderOptions::default()
        },
        &CancelToken::never(),
    )
    .expect("within budget");
}

#[test]
fn the_default_is_unbudgeted() {
    // Exactness is the local product's promise, so a local render is not
    // silently capped.
    assert_eq!(RenderOptions::<PdfRect>::default().max_output_pixels, None);
}

#[test]
fn the_pixels_outlive_the_document_they_came_from() {
    // The buffer-lifetime gate. The raster owns its bytes, so closing the
    // document cannot disturb an image a caller is still holding.
    let raster = {
        let document = open(test_support::COLORS);
        render(&document, RenderOptions::default())
    };
    assert!(!raster.rgba.is_empty());
    assert!(colors(&raster).len() > 1, "the rendered image survived intact");
}

#[test]
fn a_cancelled_render_reports_aborted() {
    let document = open(test_support::REPORT);
    let page = document.page(first_page(&document)).expect("page");
    let cancel = CancelToken::never();
    cancel.cancel();

    let refused = render_page(&page, &RenderOptions::default(), &cancel).expect_err("cancelled");
    assert_eq!(refused.code, EngineErrorCode::Aborted);
}

#[test]
fn annotations_can_be_left_out() {
    // Both paths render; what this pins is that the flag reaches PDFium at
    // all, since report.pdf carries no annotations to make the two differ.
    let document = open(test_support::REPORT);
    let with = render(&document, RenderOptions::default());
    let without =
        render(&document, RenderOptions { include_annotations: false, ..RenderOptions::default() });
    assert_eq!((with.width, with.height), (without.width, without.height));
}

/// Render through the wire, answering the result and the pixels.
fn render_on_the_wire(
    host: &mut epdf_engine::host::Host,
    page: u32,
    options: serde_json::Value,
) -> (serde_json::Value, Vec<u8>) {
    let response = host.call(
        &serde_json::json!({
            "kind": "pages.render", "jobId": 2, "docId": "d",
            "page": { "kind": "objectNumber", "objectNumber": page },
            "options": options,
        })
        .to_string(),
        Vec::new(),
    );
    let mut envelope: serde_json::Value = serde_json::from_str(&response.json).expect("JSON");
    assert!(envelope.get("error").is_none(), "{envelope}");
    let pixels = response.payloads.into_iter().next().expect("the pixels");
    (envelope["result"].take(), pixels)
}

/// A one-page PDF whose crop box does not start at the origin, its top-left
/// quarter painted red (in the file's coordinates `[0 396 306 396]` from the
/// crop box's corner). Cross-reference offsets are computed.
fn shifted_page_pdf() -> Vec<u8> {
    let content = "1 0 0 rg -306 0 306 396 re f";
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [-306 -396 306 396] /Contents 4 0 R >>".to_owned(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
    ];
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

#[test]
fn a_page_space_target_is_measured_from_the_visible_boxs_top_left() {
    // `pages.render` takes its target in page space and converts it with
    // `pdfRectOf` on the page's crop box (`pdfRenderTargetOf`), and answers
    // the area it drew in page space. On a page whose box is centred on the
    // origin, page space's top-left quarter is the red one.
    let mut host = epdf_engine::host::Host::new();
    let opened = host.call(
        &serde_json::json!({ "kind": "open.fatMem", "jobId": 1, "docId": "d" }).to_string(),
        vec![shifted_page_pdf()],
    );
    assert!(!opened.json.contains("\"error\""), "{}", opened.json);

    let quarter = serde_json::json!({ "x": 0, "y": 0, "width": 306, "height": 396 });
    let (answer, pixels) = render_on_the_wire(
        &mut host,
        3,
        serde_json::json!({ "target": { "kind": "rect", "rect": quarter }, "viewport": { "kind": "scale", "scale": 0.25 } }),
    );
    assert_eq!(answer["page"], serde_json::json!({ "kind": "objectNumber", "objectNumber": 3 }));
    assert_eq!(
        test_support::corpus::quantized(&answer["area"]),
        test_support::corpus::quantized(&quarter),
        "the area comes back in page space",
    );
    let raster = PageRaster {
        width: answer["width"].as_u64().unwrap() as u32,
        height: answer["height"].as_u64().unwrap() as u32,
        stride: answer["stride"].as_u64().unwrap() as u32,
        rgba: pixels,
    };
    assert_eq!((raster.width, raster.height), (77, 99));
    let middle = raster.pixel(raster.width / 2, raster.height / 2).expect("in bounds");
    assert_eq!(middle, (255, 0, 0, 255), "the top-left quarter is the red one");

    // The bottom-right quarter is white.
    let (_, pixels) = render_on_the_wire(
        &mut host,
        3,
        serde_json::json!({
            "target": { "kind": "rect", "rect": { "x": 306, "y": 396, "width": 306, "height": 396 } },
            "viewport": { "kind": "scale", "scale": 0.25 },
        }),
    );
    let white = PageRaster { width: 77, height: 99, stride: 77 * 4, rgba: pixels };
    assert_eq!(white.pixel(38, 49), Some((255, 255, 255, 255)));

    // A whole-page render's area is the visible box at the origin.
    let (answer, _) = render_on_the_wire(
        &mut host,
        3,
        serde_json::json!({ "viewport": { "kind": "scale", "scale": 0.1 } }),
    );
    assert_eq!(
        test_support::corpus::quantized(&answer["area"]),
        test_support::corpus::quantized(
            &serde_json::json!({ "x": 0, "y": 0, "width": 612, "height": 792 })
        ),
    );
}
