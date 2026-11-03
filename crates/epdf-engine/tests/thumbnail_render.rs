//! Thumbnails (`{kind: 'thumbnail'}` viewports), 8638dc749.
//!
//! A thumbnail is the whole page, `PAGE_THUMBNAIL_SIZE` (512) pixels on its
//! long side, for the screen. It takes no `target`, and no print picture is
//! one. Each kind of picture needs its own right and no other
//! (`checkRenderUsage(options, may)`): a thumbnail `doc.render.thumbnail`, a
//! screen picture `doc.render`, a print the print right of its DPI.

use epdf_engine::error::EngineErrorCode;
use epdf_engine::features::render::geometry::{
    PAGE_THUMBNAIL_SIZE, RenderTarget, RenderViewport, device_size,
};
use epdf_engine::features::render::raster::{
    PagePictureRights, RenderOptions, RenderUsage, check_render_usage, check_render_usage_for,
};
use epdf_engine::geom::{PdfRect, PdfRotation};
use epdf_engine::host::Host;
use serde_json::{Value, json};

/// A one-page file with a US Letter page (object 3).
fn letter_pdf() -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>",
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

fn render(options: Value) -> Value {
    let mut host = Host::new();
    let opened = host.call_filled(
        &json!({ "kind": "open.fatMem", "jobId": 1, "docId": "d" }).to_string(),
        vec![letter_pdf()],
    );
    assert!(!opened.json.contains("\"error\""), "{}", opened.json);
    let response = host.call_filled(
        &json!({
            "kind": "pages.render", "jobId": 2, "docId": "d",
            "page": { "kind": "objectNumber", "objectNumber": 3 },
            "options": options,
        })
        .to_string(),
        Vec::new(),
    );
    serde_json::from_str(&response.json).expect("JSON")
}

const THUMBNAIL: fn() -> Value = || json!({ "viewport": { "kind": "thumbnail" } });

#[test]
fn a_thumbnail_is_the_whole_page_at_512_on_its_long_side() {
    let envelope = render(THUMBNAIL());
    let raster = &envelope["result"]["raster"];
    // A US Letter page is 396 by 512.
    assert_eq!((raster["width"].clone(), raster["height"].clone()), (json!(396), json!(512)));

    let turned = render(json!({ "viewport": { "kind": "thumbnail" }, "rotation": 90 }));
    let raster = &turned["result"]["raster"];
    assert_eq!((raster["width"].clone(), raster["height"].clone()), (json!(512), json!(396)));
}

#[test]
fn the_size_is_the_long_side_of_the_area_before_the_turn() {
    let letter = PdfRect { left: 0.0, bottom: 0.0, right: 612.0, top: 792.0 };
    assert_eq!(
        device_size(letter, PdfRotation::None, RenderViewport::Thumbnail).unwrap(),
        (396, 512)
    );
    let wide = PdfRect { left: 0.0, bottom: 0.0, right: 1024.0, top: 256.0 };
    assert_eq!(
        device_size(wide, PdfRotation::Clockwise270, RenderViewport::Thumbnail).unwrap(),
        (128, PAGE_THUMBNAIL_SIZE as u32),
    );
}

#[test]
fn a_thumbnail_takes_no_target_and_is_no_print_picture() {
    let targeted = render(json!({
        "viewport": { "kind": "thumbnail" },
        "target": { "kind": "rect", "rect": { "x": 0, "y": 0, "width": 10, "height": 10 } },
    }));
    assert_eq!(targeted["error"]["code"], "InvalidArg", "{targeted}");
    assert_eq!(targeted["error"]["details"], json!({ "field": "target" }));
    assert_eq!(targeted["error"]["message"], "a thumbnail is the whole page: it takes no target");

    // `target: {kind: 'page'}` is the whole page, so a thumbnail takes it.
    let whole =
        render(json!({ "viewport": { "kind": "thumbnail" }, "target": { "kind": "page" } }));
    assert!(whole.get("error").is_none(), "{whole}");

    let printed = render(json!({ "usage": "print", "viewport": { "kind": "thumbnail" } }));
    assert_eq!(printed["error"]["code"], "InvalidArg", "{printed}");
    assert_eq!(printed["error"]["details"], json!({ "field": "viewport" }));
}

fn rights(render: bool, thumbnail: bool, print: bool, print_high: bool) -> PagePictureRights {
    PagePictureRights { render, thumbnail, print, print_high }
}

fn options(viewport: Option<RenderViewport>, usage: Option<RenderUsage>) -> RenderOptions {
    RenderOptions { viewport, usage, ..RenderOptions::default() }
}

/// The right a refusal names, or `None` when the picture was allowed.
fn refused(options: &RenderOptions, may: PagePictureRights) -> Option<(String, String)> {
    let error = check_render_usage_for(options, Some(may)).err()?;
    assert_eq!(error.code, EngineErrorCode::Forbidden, "{error}");
    let details = error.details.expect("details");
    Some((
        details["required"].as_str().unwrap().to_string(),
        details["context"].as_str().unwrap().to_string(),
    ))
}

#[test]
fn each_kind_of_picture_needs_its_own_right_and_no_other() {
    let thumbnail = options(Some(RenderViewport::Thumbnail), None);
    let screen = options(Some(RenderViewport::Width { width: 100.0 }), None);
    let whole_page = options(None, None);
    let print150 = options(Some(RenderViewport::Dpi { dpi: 150.0 }), Some(RenderUsage::Print));
    let print300 = options(Some(RenderViewport::Dpi { dpi: 300.0 }), Some(RenderUsage::Print));

    // Only the thumbnail right: thumbnails, and nothing else.
    let thumbnails_only = rights(false, true, false, false);
    assert_eq!(refused(&thumbnail, thumbnails_only), None);
    let no_render = Some(("doc.render".to_string(), "usage".to_string()));
    assert_eq!(refused(&screen, thumbnails_only), no_render);
    assert_eq!(refused(&whole_page, thumbnails_only), no_render);
    assert_eq!(refused(&print150, thumbnails_only), Some(("doc.print".into(), "usage".into())));

    // A screen right without the thumbnail one makes no thumbnail.
    assert_eq!(
        refused(&thumbnail, rights(true, false, true, true)),
        Some(("doc.render.thumbnail".into(), "viewport".into())),
    );

    // Printing needs no doc.render; 300 DPI needs doc.print.high too.
    let print_only = rights(false, false, true, false);
    assert_eq!(refused(&print150, print_only), None);
    assert_eq!(refused(&print300, print_only), Some(("doc.print.high".into(), "viewport".into())));
    assert_eq!(refused(&print300, rights(false, false, true, true)), None);

    // The worker's check leaves rights to its caller.
    for picture in [&thumbnail, &screen, &whole_page, &print150, &print300] {
        assert!(check_render_usage(picture).is_ok());
    }
}

#[test]
fn a_thumbnail_target_of_the_whole_page_is_no_target() {
    let page: RenderOptions = RenderOptions {
        viewport: Some(RenderViewport::Thumbnail),
        target: Some(RenderTarget::Page),
        ..RenderOptions::default()
    };
    assert!(check_render_usage(&page).is_ok());
}
