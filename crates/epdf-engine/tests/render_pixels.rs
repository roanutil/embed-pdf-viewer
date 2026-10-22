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
use epdf_engine::features::render::raster::{
    PageRaster, RenderOptions, render_page, render_page_area_in_slices,
};
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
            viewport: Some(RenderViewport::Scale { scale: 1.0 }),
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
        RenderOptions { background: Some(RenderBackground::White), ..RenderOptions::default() },
    );
    assert_eq!(
        opaque.pixel(0, 0),
        Some((255, 255, 255, 255)),
        "a white background corner should be opaque white",
    );

    let transparent = render(
        &document,
        RenderOptions {
            background: Some(RenderBackground::Transparent),
            ..RenderOptions::default()
        },
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
        RenderOptions { rotation: Some(PdfRotation::Clockwise90), ..RenderOptions::default() },
    );

    assert_eq!((turned.width, turned.height), (upright.height, upright.width));
}

#[test]
fn a_half_turn_keeps_the_size_and_reverses_the_image() {
    let document = open(test_support::COLORS);
    let upright = render(&document, RenderOptions::default());
    let turned = render(
        &document,
        RenderOptions { rotation: Some(PdfRotation::Clockwise180), ..RenderOptions::default() },
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
            target: Some(RenderTarget::Rect { rect: tile_rect }),
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
            RenderOptions {
                viewport: Some(RenderViewport::Width { width }),
                ..RenderOptions::default()
            },
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
            viewport: Some(RenderViewport::Scale { scale: 4.0 }),
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
            viewport: Some(RenderViewport::Scale { scale: 0.1 }),
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
    let without = render(
        &document,
        RenderOptions { include_annotations: Some(false), ..RenderOptions::default() },
    );
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
    // `{page, area, raster}`, the raster as `PageRaster` with its pixels
    // payload 0 (`handlePagesRender`).
    let nested = &answer["raster"];
    assert_eq!(nested["color"], "rgba8");
    assert_eq!(nested["premultipliedAlpha"], false);
    assert_eq!(nested["data"], serde_json::json!({ "payload": 0 }));
    let raster = PageRaster {
        width: nested["width"].as_u64().unwrap() as u32,
        height: nested["height"].as_u64().unwrap() as u32,
        stride: nested["stride"].as_u64().unwrap() as u32,
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

#[test]
fn a_scale_viewport_that_names_no_scale_is_scale_one() {
    // `PageScreenViewport`'s `scale` is optional, and `viewport.scale ?? 1`
    // (`renderSize.ts`): `{kind: "scale"}` is the page at its own size.
    let mut host = epdf_engine::host::Host::new();
    let opened = host.call(
        &serde_json::json!({ "kind": "open.fatMem", "jobId": 1, "docId": "d" }).to_string(),
        vec![shifted_page_pdf()],
    );
    assert!(!opened.json.contains("\"error\""), "{}", opened.json);

    let (answer, _) =
        render_on_the_wire(&mut host, 3, serde_json::json!({ "viewport": { "kind": "scale" } }));
    assert_eq!(
        (answer["raster"]["width"].as_u64(), answer["raster"]["height"].as_u64()),
        (Some(612), Some(792))
    );
}

/// A PDF file of `objects`, numbered from 1, with its cross-reference table
/// (`pdfOf`).
fn pdf_of(objects: &[String]) -> Vec<u8> {
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

/// `heavyPdf(1, squares)`: one page (object 3) of small coloured squares, a
/// render of many steps.
fn heavy_pdf(squares: u32) -> Vec<u8> {
    let mut body = String::new();
    for i in 0..squares {
        let x = (i * 7) % 590;
        let y = (i * 13) % 830;
        body += &format!(
            "{} {} {} rg {x} {y} 4 4 re f\n",
            f64::from(i % 7) / 7.0,
            f64::from(i % 5) / 5.0,
            f64::from(i % 3) / 3.0
        );
    }
    pdf_of(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 840] /Contents 4 0 R >>".into(),
        format!("<< /Length {} >>\nstream\n{body}endstream", body.len()),
    ])
}

fn render_sliced(
    document: &OwnedDocument,
    options: &RenderOptions,
    cancel: &CancelToken,
    slice_ms: u32,
) -> Result<PageRaster, epdf_engine::error::EngineError> {
    let page = document.page(first_page(document)).expect("page");
    render_page_area_in_slices(&page, options, cancel, slice_ms).map(|r| r.raster)
}

#[test]
fn a_sliced_render_is_the_one_call_render_byte_for_byte() {
    // `EPDF_RenderPageBitmapWithMatrix_Start` and `_Continue` write the bytes
    // `FPDF_RenderPageBitmapWithMatrix` writes, however finely sliced.
    let document = open(test_support::COLORS);
    let page = document.page(first_page(&document)).expect("page");
    let view = page.view();
    let matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let (width, height) = (view.width().round() as u32, view.height().round() as u32);
    for flags in [
        pdfium::RenderFlags::default(),
        pdfium::RenderFlags { annotations: true, widgets: true, printing: false },
        pdfium::RenderFlags { annotations: true, widgets: true, printing: true },
    ] {
        for opaque in [true, false] {
            let once =
                view.render_rgba_with_matrix(width, height, matrix, opaque, flags).expect("once");
            for budget in [0, 60_000] {
                let sliced = view
                    .render_rgba_with_matrix_in_slices(
                        width,
                        height,
                        matrix,
                        opaque,
                        flags,
                        budget,
                        || Ok::<(), ()>(()),
                    )
                    .expect("sliced");
                assert!(sliced == once, "{flags:?} opaque {opaque} budget {budget}");
            }
        }
    }
    // And through the engine, at its own slices.
    let whole = render(&document, RenderOptions::default());
    let finest = render_sliced(&document, &RenderOptions::default(), &CancelToken::never(), 0)
        .expect("render");
    assert_eq!(whole, finest);
}

#[test]
fn a_render_in_slices_is_the_render_in_one_however_finely_sliced() {
    // `render-cancellation.test.ts`'s first case: the same bytes at a slice
    // of 0 ms as at 60 s, for the whole page, a tile, and a turned
    // transparent page. A token that cancels at its 14th check cancels the
    // finest render (more than 10 slices: three checks are the render's
    // own) and not the whole one (one slice).
    let document = OwnedDocument::new(
        Document::from_bytes(Library::acquire(), &heavy_pdf(30_000), None).expect("open"),
    )
    .expect("index");
    let cases = [
        RenderOptions::default(),
        RenderOptions {
            viewport: Some(RenderViewport::Scale { scale: 3.0 }),
            target: Some(RenderTarget::Rect {
                rect: PdfRect { left: 200.0, bottom: 410.0, right: 330.0, top: 540.0 },
            }),
            ..RenderOptions::default()
        },
        RenderOptions {
            rotation: Some(PdfRotation::Clockwise90),
            background: Some(RenderBackground::Transparent),
            ..RenderOptions::default()
        },
    ];
    for options in cases {
        let whole =
            render_sliced(&document, &options, &CancelToken::cancelled_at_check(14), 60_000)
                .expect("one slice");
        let finest = render_sliced(&document, &options, &CancelToken::never(), 0).expect("render");
        assert!(whole == finest, "{options:?}");
        let cut = render_sliced(&document, &options, &CancelToken::cancelled_at_check(14), 0)
            .expect_err("more than ten slices");
        assert_eq!(cut.code, EngineErrorCode::Aborted);
    }
}

#[test]
fn a_cancelled_render_stops_at_the_next_slice_and_leaves_the_page_ready() {
    // `render-cancellation.test.ts`'s second case. Once `between` refuses,
    // no slice runs after it, and the render is closed: the page renders
    // again, to the same bytes.
    let document = OwnedDocument::new(
        Document::from_bytes(Library::acquire(), &heavy_pdf(30_000), None).expect("open"),
    )
    .expect("index");
    let expected = render(&document, RenderOptions::default());
    let page = document.page(first_page(&document)).expect("page");
    let view = page.view();
    let matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let flags = pdfium::RenderFlags { annotations: true, widgets: true, printing: false };
    let mut between = 0;
    let stopped = view.render_rgba_with_matrix_in_slices(600, 840, matrix, true, flags, 0, || {
        between += 1;
        if between == 3 { Err("stop") } else { Ok(()) }
    });
    assert_eq!(stopped, Err(pdfium::SlicedRender::Stopped("stop")));
    assert_eq!(between, 3, "no slice ran after the stop");

    // Through the engine: the token, cancelled between two slices.
    let aborted = render_page_area_in_slices(
        &page,
        &RenderOptions::default(),
        &CancelToken::cancelled_at_check(5),
        0,
    )
    .expect_err("aborted");
    assert_eq!(aborted.code, EngineErrorCode::Aborted);
    let again = render_page(&page, &RenderOptions::default(), &CancelToken::never())
        .expect("the page renders again");
    assert_eq!(again, expected);
}

/// `APPEARANCE_STATES_FIXTURE_PDF`'s check box (green while off, 20..60)
/// and cyan square (100..140), both 140..180 from the top of a 200 by 200
/// page.
fn states_pdf() -> Vec<u8> {
    let filled = |color: &str| {
        let content = format!("{color} 0 0 40 40 re f");
        format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 40 40] /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        )
    };
    pdf_of(&[
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] >> >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [4 0 R 10 0 R] >>".into(),
        "<< /Type /Annot /Subtype /Widget /FT /Btn /T (states) /V /Off /AS /Off /F 4 \
         /Rect [20 20 60 60] /AP << /N << /Yes 5 0 R /Off 6 0 R >> \
         /D << /Yes 7 0 R /Off 8 0 R >> /R << /Yes 9 0 R >> >> >>"
            .into(),
        filled("1 0 0 rg"),
        filled("0 1 0 rg"),
        filled("0 0 1 rg"),
        filled("1 1 0 rg"),
        filled("0 g"),
        "<< /Type /Annot /Subtype /Square /Rect [100 20 140 60] /F 4 /AP << /N 11 0 R >> >>".into(),
        filled("0 1 1 rg"),
    ])
}

#[test]
fn form_fields_are_drawn_unless_left_out() {
    // `page-render-rights.test.ts` with every right, as the worker draws
    // (`resolvePageLayers(options, {annotations: true, formFields: true})`),
    // and `includeFormFields` (17350fb4c, `EPDF_RENDER_WIDGETS`).
    const GREEN: (u8, u8, u8) = (0, 255, 0);
    const CYAN: (u8, u8, u8) = (0, 255, 255);
    const WHITE: (u8, u8, u8) = (255, 255, 255);
    let document = OwnedDocument::new(
        Document::from_bytes(Library::acquire(), &states_pdf(), None).expect("open"),
    )
    .expect("index");
    let middles = |include_annotations, include_form_fields| {
        let raster = render(
            &document,
            RenderOptions { include_annotations, include_form_fields, ..RenderOptions::default() },
        );
        let at = |x, y| {
            let (r, g, b, _) = raster.pixel(x, y).expect("in bounds");
            (r, g, b)
        };
        (at(40, 160), at(120, 160))
    };
    assert_eq!(middles(None, None), (GREEN, CYAN));
    assert_eq!(
        middles(Some(false), None),
        (WHITE, WHITE),
        "includeAnnotations: false draws neither"
    );
    assert_eq!(middles(None, Some(false)), (WHITE, CYAN));
    assert_eq!(middles(Some(false), Some(true)), (GREEN, WHITE));
    assert_eq!(middles(Some(true), Some(true)), (GREEN, CYAN));
}
