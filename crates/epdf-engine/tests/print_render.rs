//! Print pictures (`usage: 'print'` with a `dpi` viewport), 7e0d96c1c.
//!
//! The cases of `runPrintRenderConformance` (`print-render.test.ts`) a host
//! decides. A print picture follows the print rules where a screen picture
//! follows the screen's: layers by their print state, annotations and form
//! fields by their `print` flag, `noView` ones included, `hidden` ones never.
//! It is the whole page at its DPI, never more pixels than the page fitted to
//! A4 or Letter needs. Options that don't fit a print picture are refused,
//! naming the option (`checkRenderUsage(options, null)`). Who may print at
//! which DPI (`doc.print`, `doc.print.high`) is the engine caller's check,
//! not this host's.

use epdf_engine::error::{EngineError, EngineErrorCode};
use epdf_engine::features::render::geometry::{RenderViewport, print_scale};
use epdf_engine::features::render::raster::{
    PageLayerRights, PageRaster, RenderOptions, RenderUsage, check_render_usage,
    resolve_page_layers,
};
use epdf_engine::features::server::{EncodedImage, ImageEncoder, RenderEncode};
use epdf_engine::host::Host;
use serde_json::{Value, json};

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

/// An annotation drawn as a black 40 by 40 square at `rect`, with the flags `f`.
fn square(rect: &str, f: u32) -> String {
    format!(
        "<< /Type /Annot /Subtype /Square /Rect [{rect}] /F {f} /C [0 0 0] /P 3 0 R /AP << /N 8 0 R >> >>"
    )
}

/// `PRINT_RULES_PDF`: a 200 by 200 page (object 3) with one black 40 by 40
/// square for each print rule, and an A0 page (object 4) with nothing on it:
/// a layer that only prints and one only shown on screen; four squares, one
/// only printed (`noView` + `print`), one only on screen, one on both, one
/// on neither (`hidden`); a push button only on screen and a text field that
/// only prints.
fn print_rules_pdf() -> Vec<u8> {
    const SQUARE: &str = "0 g 0 0 40 40 re f";
    const LAYERS: &str =
        "/OC /OCp BDC 0 g 10 150 40 40 re f EMC\n/OC /OCv BDC 0 g 60 150 40 40 re f EMC";
    let layers = format!("<< /Length {} >>\nstream\n{LAYERS}\nendstream", LAYERS.len());
    let form = format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 40 40] /Length {} >>\nstream\n{SQUARE}\nendstream",
        SQUARE.len()
    );
    let squares = [
        square("10 90 50 130", 36),
        square("60 90 100 130", 0),
        square("110 90 150 130", 4),
        square("150 90 190 130", 6),
    ];
    pdf_of(&[
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [13 0 R 14 0 R] >> /OCProperties << /OCGs [6 0 R 7 0 R] /D << /ON [7 0 R] /OFF [6 0 R] /AS [<< /Event /Print /Category [/Print] /OCGs [6 0 R 7 0 R] >> << /Event /View /Category [/View] /OCGs [6 0 R 7 0 R] >>] >> >> >>",
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 5 0 R /Resources << /Properties << /OCp 6 0 R /OCv 7 0 R >> >> /Annots [9 0 R 10 0 R 11 0 R 12 0 R 13 0 R 14 0 R] >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 2384 3370] /Resources << >> >>",
        &layers,
        "<< /Type /OCG /Name (Prints only) /Usage << /Print << /PrintState /ON >> /View << /ViewState /OFF >> >> >>",
        "<< /Type /OCG /Name (Screen only) /Usage << /Print << /PrintState /OFF >> /View << /ViewState /ON >> >> >>",
        &form,
        &squares[0],
        &squares[1],
        &squares[2],
        &squares[3],
        "<< /Type /Annot /Subtype /Widget /FT /Btn /Ff 65536 /T (print) /Rect [10 30 50 70] /F 0 /P 3 0 R /AP << /N 8 0 R >> >>",
        "<< /Type /Annot /Subtype /Widget /FT /Tx /T (stamp) /V (x) /Rect [60 30 100 70] /F 36 /P 3 0 R /AP << /N 8 0 R >> >>",
    ])
}

const SMALL: u32 = 3;
const LARGE: u32 = 4;

/// Each spot's middle in page space, and whether it shows on screen and
/// on paper (`SPOTS`).
const SPOTS: [(&str, (f64, f64), bool, bool); 8] = [
    ("layerPrintOnly", (30.0, 30.0), false, true),
    ("layerScreenOnly", (80.0, 30.0), true, false),
    ("annotationPrintOnly", (30.0, 90.0), false, true),
    ("annotationScreenOnly", (80.0, 90.0), true, false),
    ("annotationBoth", (130.0, 90.0), true, true),
    ("annotationHidden", (170.0, 90.0), false, false),
    ("fieldScreenOnly", (30.0, 150.0), true, false),
    ("fieldPrintOnly", (80.0, 150.0), false, true),
];
const ANNOTATION_SPOTS: [&str; 4] =
    ["annotationPrintOnly", "annotationScreenOnly", "annotationBoth", "annotationHidden"];
const FIELD_SPOTS: [&str; 2] = ["fieldScreenOnly", "fieldPrintOnly"];

fn open() -> Host {
    let mut host = Host::new();
    let opened = host.call_filled(
        &json!({ "kind": "open.fatMem", "jobId": 1, "docId": "d" }).to_string(),
        vec![print_rules_pdf()],
    );
    assert!(!opened.json.contains("\"error\""), "{}", opened.json);
    host
}

/// `pages.render` of `page` with `options`: the answer's envelope and its
/// payloads.
fn render(host: &mut Host, page: u32, options: Value) -> (Value, Vec<Vec<u8>>) {
    let response = host.call_filled(
        &json!({
            "kind": "pages.render", "jobId": 2, "docId": "d",
            "page": { "kind": "objectNumber", "objectNumber": page },
            "options": options,
        })
        .to_string(),
        Vec::new(),
    );
    (serde_json::from_str(&response.json).expect("JSON"), response.payloads)
}

/// The raster a render answered, its pixels payload 0.
fn raster(host: &mut Host, page: u32, options: Value) -> PageRaster {
    let (envelope, payloads) = render(host, page, options);
    assert!(envelope.get("error").is_none(), "{envelope}");
    let raster = &envelope["result"]["raster"];
    assert_eq!(raster["data"], json!({ "payload": 0 }), "{envelope}");
    let number = |key: &str| raster[key].as_u64().expect(key) as u32;
    PageRaster {
        width: number("width"),
        height: number("height"),
        stride: number("stride"),
        rgba: payloads.into_iter().next().expect("the pixels"),
    }
}

fn print(dpi: u32) -> Value {
    json!({ "usage": "print", "viewport": { "kind": "dpi", "dpi": dpi } })
}

/// `options` with `more`'s members added.
fn with(mut options: Value, more: Value) -> Value {
    for (key, value) in more.as_object().expect("an object") {
        options[key] = value.clone();
    }
    options
}

/// The spots dark in a picture of the small page at `scale` pixels a point.
fn dark_spots(host: &mut Host, options: Value, scale: f64) -> Vec<(&'static str, bool)> {
    let raster = raster(host, SMALL, options);
    SPOTS
        .iter()
        .map(|&(name, (x, y), _, _)| {
            let (r, g, b, _) =
                raster.pixel((x * scale) as u32, (y * scale) as u32).expect("in the picture");
            (name, r < 60 && g < 60 && b < 60)
        })
        .collect()
}

/// The spots `print` (or the screen) shows when a picture draws the layers
/// and `spots` (`shown`).
fn shown(print: bool, spots: &[&str]) -> Vec<(&'static str, bool)> {
    SPOTS
        .iter()
        .map(|&(name, _, display, printed)| {
            let layer = name.starts_with("layer");
            (name, (layer || spots.contains(&name)) && if print { printed } else { display })
        })
        .collect()
}

#[test]
fn a_print_picture_is_the_whole_page_at_its_dpi() {
    let mut host = open();
    for (dpi, size) in [(150, 417), (300, 833)] {
        assert_eq!(size, (200.0 * f64::from(dpi) / 72.0_f64).round() as u32);
        let (envelope, _) = render(&mut host, SMALL, print(dpi));
        let answer = &envelope["result"];
        assert_eq!(
            (answer["raster"]["width"].as_u64(), answer["raster"]["height"].as_u64()),
            (Some(u64::from(size)), Some(u64::from(size))),
            "{envelope}"
        );
        assert_eq!(
            test_support::corpus::quantized(&answer["area"]),
            test_support::corpus::quantized(
                &json!({ "x": 0, "y": 0, "width": 200, "height": 200 })
            ),
        );
    }
}

#[test]
fn a_page_bigger_than_the_paper_is_printed_at_the_size_it_has_fitted_to_a4_or_letter() {
    // A0 fitted to 612 by 842 points is A4's long side, 1754 pixels at 150 DPI.
    let mut host = open();
    let options =
        with(print(150), json!({ "includeAnnotations": false, "includeFormFields": false }));
    let raster = raster(&mut host, LARGE, options);
    assert_eq!((raster.width, raster.height), (1241, 1754));
    assert!((print_scale(2384.0, 3370.0, 150.0) - 150.0 / 72.0 * 842.0 / 3370.0).abs() < 1e-12);
    assert_eq!(print_scale(200.0, 200.0, 300.0), 300.0 / 72.0, "a page that fits keeps its size");
}

#[test]
fn a_screen_picture_follows_the_screen_rules_a_print_picture_the_print_rules() {
    let mut host = open();
    let layers = json!({ "includeAnnotations": true, "includeFormFields": true });
    let all: Vec<&str> = ANNOTATION_SPOTS.iter().chain(&FIELD_SPOTS).copied().collect();
    assert_eq!(dark_spots(&mut host, layers.clone(), 1.0), shown(false, &all));
    assert_eq!(dark_spots(&mut host, with(layers, print(150)), 150.0 / 72.0), shown(true, &all));
    // Left out, the layers draw everything: the same pictures.
    assert_eq!(dark_spots(&mut host, json!({}), 1.0), shown(false, &all));
    assert_eq!(dark_spots(&mut host, print(150), 150.0 / 72.0), shown(true, &all));
}

#[test]
fn each_print_picture_draws_what_its_family_draws() {
    let mut host = open();
    let cases: [(Value, &[&str]); 3] = [
        (json!({ "includeAnnotations": false, "includeFormFields": false }), &[]),
        (json!({ "includeAnnotations": true, "includeFormFields": false }), &ANNOTATION_SPOTS),
        (json!({ "includeAnnotations": false, "includeFormFields": true }), &FIELD_SPOTS),
    ];
    for (layers, spots) in cases {
        assert_eq!(
            dark_spots(&mut host, with(layers.clone(), print(300)), 300.0 / 72.0),
            shown(true, spots),
            "{layers}"
        );
    }
    // `includeAnnotations: false` with no `includeFormFields` draws neither.
    assert_eq!(
        dark_spots(
            &mut host,
            with(json!({ "includeAnnotations": false }), print(150)),
            150.0 / 72.0
        ),
        shown(true, &[])
    );
}

/// The options `print-render.test.ts` refuses, each with the field
/// `checkRenderUsage` names.
fn refused_options() -> Vec<(Value, &'static str)> {
    vec![
        (json!({ "usage": "print" }), "viewport"),
        (json!({ "usage": "print", "viewport": { "kind": "scale", "scale": 2 } }), "viewport"),
        (json!({ "usage": "print", "viewport": { "kind": "width", "width": 400 } }), "viewport"),
        (json!({ "usage": "print", "viewport": { "kind": "dpi", "dpi": 200 } }), "viewport"),
        (json!({ "viewport": { "kind": "dpi", "dpi": 150 } }), "viewport"),
        (json!({ "usage": "display", "viewport": { "kind": "dpi", "dpi": 300 } }), "viewport"),
        (
            with(
                print(150),
                json!({ "target": { "kind": "rect",
                "rect": { "x": 0, "y": 0, "width": 50, "height": 50 } } }),
            ),
            "target",
        ),
        (with(print(150), json!({ "rotation": 90 })), "rotation"),
        (with(print(150), json!({ "background": "transparent" })), "background"),
        (json!({ "usage": "paper", "viewport": { "kind": "dpi", "dpi": 150 } }), "usage"),
        // As `!== undefined` reads them: a `null` is given, a usage that
        // isn't a string is not one of the two, and a dpi that is missing or
        // not a number is neither 150 nor 300.
        (with(print(150), json!({ "target": null })), "target"),
        (with(print(150), json!({ "rotation": null })), "rotation"),
        (with(print(150), json!({ "background": null })), "background"),
        (json!({ "usage": null, "viewport": { "kind": "dpi", "dpi": 150 } }), "usage"),
        (json!({ "usage": 1, "viewport": { "kind": "dpi", "dpi": 150 } }), "usage"),
        (json!({ "usage": "print", "viewport": { "kind": "dpi" } }), "viewport"),
        (json!({ "usage": "print", "viewport": { "kind": "dpi", "dpi": "150" } }), "viewport"),
        (json!({ "viewport": { "kind": "dpi", "dpi": null } }), "viewport"),
    ]
}

fn assert_refused(envelope: &Value, field: &str, options: &Value) {
    assert_eq!(envelope["error"]["code"], "InvalidArg", "{options}: {envelope}");
    assert_eq!(envelope["error"]["details"], json!({ "field": field }), "{options}: {envelope}");
}

/// An encoder that answers three bytes for any raster.
struct Stub;

impl ImageEncoder for Stub {
    fn encode(&self, raster: &PageRaster, _: &RenderEncode) -> Result<EncodedImage, EngineError> {
        Ok(EncodedImage {
            content_type: "image/png".into(),
            width: raster.width,
            height: raster.height,
            bytes: vec![1, 2, 3],
        })
    }
}

#[test]
fn options_that_do_not_fit_a_print_picture_are_refused_on_every_render_kind() {
    let mut host = Host::new().with_encoder(Box::new(Stub));
    let opened = host.call_filled(
        &json!({ "kind": "open.fatMem", "jobId": 1, "docId": "d" }).to_string(),
        vec![print_rules_pdf()],
    );
    assert!(!opened.json.contains("\"error\""), "{}", opened.json);
    let dir = std::env::temp_dir().join(format!("epdf-print-render-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let path = dir.join("print-rules.pdf");
    std::fs::write(&path, print_rules_pdf()).expect("the file");

    let page = json!({ "kind": "objectNumber", "objectNumber": SMALL });
    let encode = json!({ "format": "png" });
    for (options, field) in refused_options() {
        let parsed: RenderOptions<epdf_engine::geom::PageRect> =
            serde_json::from_value(options.clone()).expect("the options parse");
        let error = check_render_usage(&parsed).expect_err("refused");
        assert_eq!(
            (error.code, error.details.clone()),
            (EngineErrorCode::InvalidArg, Some(json!({ "field": field })))
        );

        for request in [
            json!({ "kind": "pages.render", "page": page }),
            json!({ "kind": "pages.renderEncoded", "page": page, "encode": encode }),
            json!({ "kind": "document.renderPageFile", "path": path, "password": null, "pageIndex": 0 }),
            json!({ "kind": "document.renderPageFileEncoded", "path": path, "password": null,
                "pageIndex": 0, "encode": encode }),
        ] {
            let mut request =
                with(request, json!({ "jobId": 2, "docId": "d", "options": options }));
            if request["kind"].as_str().unwrap().starts_with("document.") {
                request.as_object_mut().unwrap().remove("docId");
            }
            let response = host.call_filled(&request.to_string(), Vec::new());
            let envelope: Value = serde_json::from_str(&response.json).expect("JSON");
            assert_refused(&envelope, field, &request);
        }
    }

    // And a print picture fits every kind: the image of a 150 DPI picture
    // is the raster's size.
    for request in [
        json!({ "kind": "pages.renderEncoded", "page": page, "encode": encode, "docId": "d" }),
        json!({ "kind": "document.renderPageFileEncoded", "path": path, "password": null,
            "pageIndex": 0, "encode": encode }),
    ] {
        let request = with(request, json!({ "jobId": 3, "options": print(150) }));
        let response = host.call_filled(&request.to_string(), Vec::new());
        let envelope: Value = serde_json::from_str(&response.json).expect("JSON");
        let image = &envelope["result"]["image"];
        assert_eq!(
            *image,
            json!({ "contentType": "image/png", "width": 417, "height": 417, "bytes": { "payload": 0 } }),
            "{envelope}"
        );
        assert_eq!(response.payloads, vec![vec![1, 2, 3]]);
    }
    let response = host.call_filled(
        &json!({ "kind": "document.renderPageFile", "jobId": 4, "path": path, "password": null,
            "pageIndex": 1, "options": print(150) })
        .to_string(),
        Vec::new(),
    );
    let envelope: Value = serde_json::from_str(&response.json).expect("JSON");
    assert_eq!(envelope["result"]["pageCount"], 2, "{envelope}");
    assert_eq!(envelope["result"]["page"]["objectNumber"], LARGE, "{envelope}");
    assert_eq!(
        (
            envelope["result"]["raster"]["width"].as_u64(),
            envelope["result"]["raster"]["height"].as_u64()
        ),
        (Some(1241), Some(1754))
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_screen_picture_takes_any_option_and_no_dpi() {
    // `checkRenderUsage` on a display picture reads only the viewport.
    for options in [
        json!({}),
        json!({ "usage": "display", "rotation": 90, "background": "transparent",
            "target": { "kind": "page" }, "viewport": { "kind": "width", "width": 10 } }),
        json!({ "target": null, "rotation": null, "background": null }),
    ] {
        let parsed: RenderOptions<epdf_engine::geom::PageRect> =
            serde_json::from_value(options.clone()).expect("the options parse");
        assert_eq!(check_render_usage(&parsed), Ok(()), "{options}");
    }
    let print: RenderOptions = RenderOptions {
        usage: Some(RenderUsage::Print),
        viewport: Some(RenderViewport::Dpi { dpi: 300.0 }),
        ..RenderOptions::default()
    };
    assert_eq!(check_render_usage(&print), Ok(()));
}

#[test]
fn the_layers_default_to_what_the_caller_may_read() {
    // `resolvePageLayers`: a left-out option draws what the caller may see,
    // and a left-out `includeFormFields` follows `includeAnnotations: false`.
    let all = PageLayerRights::ALL;
    let layers = |a, f, may| {
        resolve_page_layers(a, f, may).map(|l| (l.include_annotations, l.include_form_fields))
    };
    assert_eq!(layers(None, None, all), Ok((true, true)));
    assert_eq!(layers(Some(false), None, all), Ok((false, false)));
    assert_eq!(layers(Some(false), Some(true), all), Ok((false, true)));
    assert_eq!(layers(None, Some(false), all), Ok((true, false)));

    let filler = PageLayerRights { annotations: false, form_fields: true };
    assert_eq!(layers(None, None, filler), Ok((false, true)));
    let refused = resolve_page_layers(Some(true), None, filler).expect_err("refused");
    assert_eq!(refused.code, EngineErrorCode::Forbidden);
    assert_eq!(refused.details.as_ref().unwrap()["context"], "includeAnnotations");

    let commenter = PageLayerRights { annotations: true, form_fields: false };
    assert_eq!(layers(None, None, commenter), Ok((true, false)));
    let refused = resolve_page_layers(None, Some(true), commenter).expect_err("refused");
    assert_eq!(refused.details.as_ref().unwrap()["context"], "includeFormFields");
}
