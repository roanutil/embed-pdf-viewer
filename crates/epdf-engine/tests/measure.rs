//! `measure.viewports` and `measure.setScale` over the wire, against the
//! TypeScript's `MeasureReader`/`MeasureMutator` behaviour, and the
//! measurement annotations' fields and labels: `main/test/measurements.test.ts`
//! and `core/src/conformance/runMeasurementConformance.ts`, through the host
//! in page space.

mod common;

use common::{call, call_err, open_fixture, page_ref};
use epdf_engine::features::measure::derive::{
    DimensionInput, Readout, fround, measurement_readout,
};
use epdf_engine::geom::PdfPoint;
use epdf_engine::host::Host;
use serde_json::{Value, json};

const FOREIGN: &str = test_support::MEASURE_FOREIGN_VIEWPORT;

/// What `measureFromKnownLength` writes for 1 in = 1 m, plus every
/// optional member a number format can carry, so the codec's round trip
/// covers them.
fn rl_measure() -> Value {
    json!({
        "subtype": "rectilinear",
        "ratio": "1 in = 1 m",
        "x": [{ "unit": "m", "conversion": 0.013888888888888888, "precision": 100.0 }],
        "distance": [{
            "unit": "m", "conversion": 1.0, "precision": 1000, "fraction": "decimal",
            "fixed": true, "labelPosition": "prefix", "thousands": "", "decimal": ",",
            "prefixSpacing": " ", "suffixSpacing": ""
        }],
        "area": [{ "unit": "m²", "conversion": 1, "precision": 100 }],
        "angle": [{ "unit": "°", "conversion": 1 }],
        "origin": { "x": 10.0, "y": 20.0 },
        "cyx": 2.0
    })
}

fn viewports(host: &mut Host, doc: &str, pon: u64) -> Vec<Value> {
    let result =
        call(host, json!({ "kind": "measure.viewports", "docId": doc, "page": page_ref(pon) }));
    assert_eq!(result["page"], page_ref(pon), "the answer names its page: {result}");
    result["viewports"].as_array().expect("viewports").clone()
}

fn set_scale(host: &mut Host, doc: &str, pon: u64, measure: Value) -> Value {
    call(
        host,
        json!({ "kind": "measure.setScale", "docId": doc, "page": page_ref(pon), "measure": measure }),
    )
}

#[test]
fn viewports_read_another_producers_scale_as_not_owned() {
    let mut host = Host::new();
    let (doc, pon) = open_fixture(&mut host, FOREIGN);
    let viewports = viewports(&mut host, &doc, pon);
    assert_eq!(viewports.len(), 1);
    assert_eq!(viewports[0]["owned"], json!(false));
    assert_eq!(viewports[0]["name"], json!("Acrobat"));
    // Page space: the whole page, from its top-left corner.
    assert_eq!(
        viewports[0]["bbox"],
        json!({ "x": 0.0, "y": 0.0, "width": 612.0, "height": 792.0 })
    );
    let measure = &viewports[0]["measure"];
    assert_eq!(measure["subtype"], json!("rectilinear"));
    assert_eq!(measure["ratio"], json!("1 in = 1 m"));
    assert_eq!(measure["distance"][0]["unit"], json!("m"));
    assert_eq!(measure["area"][0]["unit"], json!("sq m"));
    assert!(measure.get("y").is_none(), "an empty y list is left out: {measure}");
}

#[test]
fn set_scale_keeps_a_foreign_viewport() {
    let mut host = Host::new();
    let (doc, pon) = open_fixture(&mut host, FOREIGN);
    let request = json!({ "kind": "measure.setScale", "jobId": 5, "docId": doc,
        "page": page_ref(pon), "opId": "scale-1", "measure": rl_measure() });
    let response = host.call(&request.to_string(), Vec::new());
    let written: Value = serde_json::from_str(&response.json).unwrap();
    assert_eq!(
        written["result"]["result"],
        json!({
            "page": page_ref(pon),
            "meta": { "affectedPages": [], "cacheDelta": null, "opId": "scale-1", "undoable": false }
        }),
        "{written}"
    );
    assert_eq!(written["result"]["event"]["type"], json!("page.viewportsChanged"), "{written}");
    assert!(written["result"]["mutationSequence"].is_u64(), "{written}");

    let after = viewports(&mut host, &doc, pon);
    assert_eq!(after.len(), 2);
    assert_eq!(after[0]["name"], json!("Acrobat"));
    assert_eq!(after[1]["name"], json!("EmbedPDF"));
    assert_eq!(after[1]["owned"], json!(true));
    assert_eq!(after[1]["bbox"], json!({ "x": 0.0, "y": 0.0, "width": 612.0, "height": 792.0 }));
    let m = &after[1]["measure"];
    assert_eq!(m["ratio"], json!("1 in = 1 m"));
    // The origin is a place on the page, and comes back where it was put.
    assert_eq!(m["origin"], json!({ "x": 10.0, "y": 20.0 }));
    assert_eq!(m["cyx"], json!(2.0));
    assert_eq!(m["x"][0]["precision"], json!(100));
    assert_eq!(
        m["distance"][0],
        json!({
            "unit": "m", "conversion": 1.0, "precision": 1000, "fraction": "decimal",
            "fixed": true, "labelPosition": "prefix", "thousands": "", "decimal": ",",
            "prefixSpacing": " ", "suffixSpacing": ""
        })
    );
    assert_eq!(m["angle"][0]["unit"], json!("°"));

    // A second write updates the owned viewport in place, never appends.
    set_scale(&mut host, &doc, pon, rl_measure());
    let again = viewports(&mut host, &doc, pon);
    assert_eq!(again.len(), 2);
    assert_eq!(
        again[1]["measure"]["x"].as_array().unwrap().len(),
        1,
        "the reset measure holds one x format"
    );

    // An absent `measure` is a malformed request, as in the TypeScript
    // (`assertWritableMeasure(undefined)` throws): it must not clear.
    let error = call_err(
        &mut host,
        json!({ "kind": "measure.setScale", "docId": doc, "page": page_ref(pon) }),
    );
    assert_eq!(error["code"], json!("InvalidArg"), "{error}");
    assert_eq!(viewports(&mut host, &doc, pon).len(), 2);

    // An explicit `null` removes the owned viewport only.
    set_scale(&mut host, &doc, pon, Value::Null);
    let cleared = viewports(&mut host, &doc, pon);
    assert_eq!(cleared.len(), 1);
    assert_eq!(cleared[0]["name"], json!("Acrobat"));
}

#[test]
fn set_scale_refuses_an_unwritable_measure() {
    let mut host = Host::new();
    let (doc, pon) = open_fixture(&mut host, FOREIGN);
    for (measure, message) in [
        (json!({ "subtype": "geospatial" }), "Only rectilinear measures can be authored"),
        (
            json!({ "subtype": "RL", "x": [], "distance": [], "area": [] }),
            "Only rectilinear measures can be authored",
        ),
        (
            json!({ "subtype": "rectilinear", "ratio": "1 in = 1 m", "x": [] }),
            "Missing distance formats",
        ),
        (
            json!({ "subtype": "rectilinear", "x": [{ "unit": "m", "conversion": 0 }],
                "distance": [], "area": [] }),
            "Invalid number format",
        ),
    ] {
        let error = call_err(
            &mut host,
            json!({
                "kind": "measure.setScale", "docId": doc, "page": page_ref(pon), "measure": measure
            }),
        );
        assert_eq!(error["code"], json!("InvalidArg"), "{error}");
        assert_eq!(error["message"], json!(message), "{error}");
    }
    assert_eq!(viewports(&mut host, &doc, pon).len(), 1, "a refused write leaves /VP alone");
}

fn save(host: &mut Host, doc: &str) -> Vec<u8> {
    let response = host.call(
        &json!({ "kind": "document.saveBuffer", "jobId": 9, "docId": doc, "mode": "rewrite" })
            .to_string(),
        Vec::new(),
    );
    response.payloads.into_iter().next().expect("the saved bytes")
}

fn open_bytes(host: &mut Host, doc: &str, bytes: Vec<u8>) {
    let open = host
        .call(&json!({ "kind": "open.fatMem", "jobId": 1, "docId": doc }).to_string(), vec![bytes]);
    assert!(!open.json.contains("\"error\""), "{}", open.json);
}

#[test]
fn a_saved_scale_survives_a_reopen() {
    let mut host = Host::new();
    let (doc, pon) = open_fixture(&mut host, FOREIGN);
    set_scale(&mut host, &doc, pon, rl_measure());
    let saved = save(&mut host, &doc);
    let mut reopened = Host::new();
    open_bytes(&mut reopened, "r", saved);
    let pages = call(&mut reopened, json!({ "kind": "pages.list", "docId": "r" }));
    let pon = pages["pages"][0]["ref"]["objectNumber"].as_u64().unwrap();
    let after = viewports(&mut reopened, "r", pon);
    assert_eq!(after.len(), 2);
    assert_eq!(after[1]["owned"], json!(true));
}

/// A PDF of these objects after the catalog, the page tree and a page
/// with these entries. Cross-reference offsets are computed, so PDFium
/// reads it as written.
fn pdf_with(page_entries: &str, more: &[&str]) -> Vec<u8> {
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        format!("<< /Type /Page /Parent 2 0 R {page_entries} >>"),
    ];
    objects.extend(more.iter().map(|object| (*object).to_owned()));
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
fn a_scale_covers_the_visible_page_as_page_boxes_of_reads_it() {
    // `MeasureMutator.setScale` calibrates over `readBoxes(...).crop`: US
    // Letter for a page with no media box, and a crop box cut to the media
    // box. In page space the viewport is the visible box, from its corner.
    for (entries, expected) in [
        ("", json!({ "x": 0.0, "y": 0.0, "width": 612.0, "height": 792.0 })),
        (
            "/MediaBox [0 0 612 792] /CropBox [-10 -10 700 800]",
            json!({ "x": 0.0, "y": 0.0, "width": 612.0, "height": 792.0 }),
        ),
        (
            "/MediaBox [0 0 612 792] /CropBox [50 60 562 732]",
            json!({ "x": 0.0, "y": 0.0, "width": 512.0, "height": 672.0 }),
        ),
    ] {
        let mut host = Host::new();
        open_bytes(&mut host, "d", pdf_with(entries, &[]));
        set_scale(&mut host, "d", 3, rl_measure());
        let after = viewports(&mut host, "d", 3);
        assert_eq!(after.len(), 1, "{entries}");
        assert_eq!(after[0]["bbox"], expected, "{entries}");
    }
}

// ── measurements.test.ts ────────────────────────────────────────────────────

/// `measurements.test.ts`'s page: its media box off the origin, so page
/// space and the file's coordinates differ.
const MEASURED_PAGE: &str = "/MediaBox [-20 -40 592 752] /Resources << >>";
const DOC: &str = "measured";
const PAGE: u64 = 3;

/// `measureFromKnownLength(100, { value: 3, unit: 'm' })`.
fn scale() -> Value {
    json!({
        "subtype": "rectilinear",
        "ratio": "1 in = 2.16 m",
        "x": [{ "unit": "m", "conversion": fround(0.03), "precision": 100 }],
        "distance": [{ "unit": "m", "conversion": 1, "precision": 100 }],
        "area": [{ "unit": "m²", "conversion": 1, "precision": 100 }],
    })
}

/// `measurements.test.ts`'s `line()`.
fn line() -> Value {
    json!({
        "subtype": "line",
        "intent": "line-dimension",
        "measure": scale(),
        "contents": "wrong",
        "linePoints": { "start": { "x": 100, "y": 100 }, "end": { "x": 200, "y": 100 } },
        "captionEnabled": true,
        "captionPosition": "inline",
        "leader": { "length": 12, "extension": 5 },
    })
}

fn open_measured(more_page_entries: &str, objects: &[&str]) -> Host {
    let mut host = Host::new();
    open_bytes(&mut host, DOC, pdf_with(&format!("{MEASURED_PAGE} {more_page_entries}"), objects));
    host
}

fn create(host: &mut Host, draft: Value) -> Value {
    call(
        host,
        json!({ "kind": "annotations.create", "docId": DOC, "page": page_ref(PAGE),
            "draft": draft }),
    )["result"]
        .take()
}

fn update(host: &mut Host, reference: &Value, patch: Value) -> Value {
    call(
        host,
        json!({ "kind": "annotations.update", "docId": DOC, "ref": reference, "patch": patch }),
    )["result"]
        .take()
}

fn update_err(host: &mut Host, reference: &Value, patch: Value) -> Value {
    call_err(
        host,
        json!({ "kind": "annotations.update", "docId": DOC, "ref": reference, "patch": patch }),
    )
}

fn list(host: &mut Host) -> Vec<Value> {
    call(host, json!({ "kind": "annotations.list", "docId": DOC }))["annotations"]
        .as_array()
        .expect("annotations")
        .clone()
}

fn point(value: &Value) -> Option<PdfPoint> {
    serde_json::from_value(value.clone()).ok()
}

/// `measurementReadout(annotation)` over a read.
fn readout(annotation: &Value) -> Readout {
    let line = &annotation["linePoints"];
    measurement_readout(&DimensionInput {
        subtype: annotation["subtype"].as_str().unwrap_or_default(),
        intent: annotation["intent"].as_str(),
        measure: annotation.get("measure"),
        line_points: point(&line["start"]).zip(point(&line["end"])),
        vertices: serde_json::from_value(annotation["vertices"].clone()).ok(),
    })
}

fn close(a: &Value, b: f64) -> bool {
    (a.as_f64().unwrap_or(f64::NAN) - b).abs() < 1e-3
}

#[test]
fn derived_contents_overrides_clients_and_caption_patches_preserve_placement() {
    let mut host = open_measured("", &[]);
    let created = create(&mut host, line())["annotation"].take();
    assert_eq!(created["contents"], "3.00 m", "{created}");
    assert_eq!(created["intent"], "line-dimension");
    assert_eq!(created["captionEnabled"], true);
    assert_eq!(created["leader"]["length"], json!(12.0), "{created}");
    let reference = created["ref"].clone();

    let wrong = update(&mut host, &reference, json!({ "subtype": "line", "contents": "999 m" }));
    assert_eq!(wrong["annotation"]["contents"], "3.00 m");
    assert_eq!(wrong["appearance"], json!({ "action": "preserved", "changed": false }));

    let moved = update(
        &mut host,
        &reference,
        json!({ "subtype": "line",
            "linePoints": { "start": { "x": 100, "y": 100 }, "end": { "x": 300, "y": 100 } } }),
    );
    assert_eq!(moved["annotation"]["contents"], "6.00 m");
    assert_eq!(moved["appearance"]["changed"], true, "{moved}");

    update(
        &mut host,
        &reference,
        json!({ "subtype": "line", "captionOffset": { "along": 12, "perpendicular": 25 },
            "captionPosition": "top" }),
    );
    let hidden =
        update(&mut host, &reference, json!({ "subtype": "line", "captionEnabled": false }));
    let a = &hidden["annotation"];
    assert_eq!(a["captionEnabled"], false, "{a}");
    assert_eq!(a["captionPosition"], "top", "{a}");
    assert!(close(&a["captionOffset"]["along"], 12.0), "{a}");
    assert!(close(&a["captionOffset"]["perpendicular"], 25.0), "{a}");
    assert_eq!(a["contents"], "6.00 m");

    let reset = update(
        &mut host,
        &reference,
        json!({ "subtype": "line", "captionEnabled": true, "captionOffset": null }),
    );
    let a = &reset["annotation"];
    assert_eq!((&a["captionEnabled"], &a["captionPosition"]), (&json!(true), &json!("top")));
    assert_eq!(a["captionOffset"], Value::Null, "{a}");

    // `runMeasurementConformance`: removing the scale keeps the label it
    // last worked out, and the line reads as unscaled.
    let unscaled = update(&mut host, &reference, json!({ "subtype": "line", "measure": null }));
    let a = &unscaled["annotation"];
    assert_eq!(a["contents"], "6.00 m");
    assert_eq!(readout(a), Err("no-measure"));
}

#[test]
fn a_manual_area_centre_moves_with_rigid_geometry_and_stays_fixed_on_vertex_edits() {
    let mut host = open_measured("", &[]);
    let vertices = json!([{ "x": 0, "y": 0 }, { "x": 100, "y": 0 }, { "x": 100, "y": 100 },
        { "x": 0, "y": 100 }]);
    let a = create(
        &mut host,
        json!({ "subtype": "polygon", "intent": "polygon-dimension", "measure": scale(),
            "vertices": vertices, "captionEnabled": true, "captionCenter": { "x": 0, "y": 0 } }),
    )["annotation"]
        .take();
    assert_eq!(a["contents"], "9.00 m²", "{a}");
    assert_eq!(a["captionCenter"], json!({ "x": 0.0, "y": 0.0 }), "(0,0) is a centre: {a}");

    let shifted: Vec<Value> = vertices
        .as_array()
        .unwrap()
        .iter()
        .map(|p| json!({ "x": p["x"].as_f64().unwrap() + 20.0, "y": p["y"].as_f64().unwrap() + 30.0 }))
        .collect();
    let moved = update(&mut host, &a["ref"], json!({ "subtype": "polygon", "vertices": shifted }));
    let m = &moved["annotation"];
    assert!(close(&m["captionCenter"]["x"], 20.0) && close(&m["captionCenter"]["y"], 30.0), "{m}");
    assert_eq!(m["contents"], "9.00 m²");
    assert_eq!(moved["appearance"]["action"], "preserved", "{moved}");
    // The rect moves with the points; the appearance isn't drawn again.
    for (key, by) in [("x", 20.0), ("y", 30.0), ("width", 0.0), ("height", 0.0)] {
        assert!(close(&m["rect"][key], a["rect"][key].as_f64().unwrap() + by), "{key}: {m}");
    }

    let edit = update(
        &mut host,
        &a["ref"],
        json!({ "subtype": "polygon", "vertices": [{ "x": 20, "y": 30 }, { "x": 130, "y": 30 },
            { "x": 120, "y": 130 }, { "x": 20, "y": 130 }] }),
    );
    let e = &edit["annotation"];
    assert!(close(&e["captionCenter"]["x"], 20.0) && close(&e["captionCenter"]["y"], 30.0), "{e}");

    let reset =
        update(&mut host, &a["ref"], json!({ "subtype": "polygon", "captionCenter": null }));
    assert_eq!(reset["annotation"]["captionCenter"], Value::Null, "{reset}");
}

#[test]
fn a_save_and_reopen_keeps_all_three_kinds_and_the_full_page_calibration() {
    let mut host = open_measured(
        "/VP [<< /Type /Viewport /Name (Foreign) /BBox [0 0 10 10] /Measure << /Subtype /GEO \
         /Vendor (keep) >> /PtData << /Keep 1 >> >>]",
        &[],
    );
    let written = set_scale(&mut host, DOC, PAGE, scale());
    assert_eq!(written["result"]["page"], page_ref(PAGE), "{written}");
    create(&mut host, line());
    create(
        &mut host,
        json!({ "subtype": "polyline", "intent": "polyline-dimension", "measure": scale(),
            "vertices": [{ "x": 0, "y": 0 }, { "x": 100, "y": 0 }, { "x": 100, "y": 100 }],
            "captionEnabled": true, "captionCenter": { "x": 0, "y": 0 } }),
    );
    create(
        &mut host,
        json!({ "subtype": "polygon", "intent": "polygon-dimension", "measure": scale(),
            "vertices": [{ "x": 0, "y": 0 }, { "x": 100, "y": 0 }, { "x": 100, "y": 100 }],
            "captionEnabled": true }),
    );
    let saved = save(&mut host, DOC);
    let mut host = Host::new();
    open_bytes(&mut host, DOC, saved);
    let annotations = list(&mut host);
    let contents: Vec<&Value> = annotations.iter().map(|a| &a["contents"]).collect();
    assert_eq!(contents, [&json!("3.00 m"), &json!("6.00 m"), &json!("4.50 m²")]);
    assert_eq!(annotations[1]["captionCenter"], json!({ "x": 0.0, "y": 0.0 }));
    let listed = viewports(&mut host, DOC, PAGE);
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0]["measure"], json!({ "subtype": "geospatial" }));
    assert_eq!(listed[0]["owned"], false);
    assert_eq!(listed[1]["owned"], true);
    assert_eq!(listed[1]["bbox"], json!({ "x": 0.0, "y": 0.0, "width": 612.0, "height": 792.0 }));
    set_scale(&mut host, DOC, PAGE, Value::Null);
    assert_eq!(viewports(&mut host, DOC, PAGE), [listed[0].clone()]);
}

#[test]
fn the_float32_preview_matches_the_commit_the_reopen_and_a_style_only_update() {
    let mut host = open_measured("", &[]);
    let mut draft = line();
    // `measureFromKnownLength(1, { value: 1.00000001, unit: 'm' })`: both
    // the coordinates and /C cross float32 rounding boundaries.
    draft["measure"]["x"][0]["conversion"] = json!(fround(1.00000001));
    draft["measure"]["ratio"] = json!("1 in = 72 m");
    draft["linePoints"] =
        json!({ "start": { "x": 0, "y": 0 }, "end": { "x": 3.4450000001, "y": 0 } });
    let expected = readout(&draft).expect("a readout").label;
    assert_eq!(expected, "3.44 m");
    let created = create(&mut host, draft)["annotation"].take();
    assert_eq!(created["contents"], json!(expected));
    let saved = save(&mut host, DOC);
    let mut host = Host::new();
    open_bytes(&mut host, DOC, saved);
    let a = list(&mut host).remove(0);
    assert_eq!(a["contents"], created["contents"]);
    let changed = update(&mut host, &a["ref"], json!({ "subtype": "line", "color": "#0000ff" }));
    assert_eq!(changed["annotation"]["contents"], created["contents"]);
}

#[test]
fn an_invalid_measure_fails_before_any_write_and_an_unavailable_scale_keeps_contents() {
    let mut host = open_measured("", &[]);
    let a = create(&mut host, line())["annotation"].take();
    let mut zero = scale();
    zero["x"] = json!([{ "unit": "m", "conversion": 0 }]);
    let refused = update_err(
        &mut host,
        &a["ref"],
        json!({ "subtype": "line", "contents": "corrupt", "measure": zero }),
    );
    assert_eq!(refused["code"], "InvalidArg", "{refused}");
    let refused = update_err(
        &mut host,
        &a["ref"],
        json!({ "subtype": "line", "contents": "corrupt", "captionEnabled": "invalid" }),
    );
    assert_eq!(refused["code"], "InvalidArg", "{refused}");
    let refused = call_err(
        &mut host,
        json!({ "kind": "measure.setScale", "docId": DOC, "page": page_ref(PAGE) }),
    );
    assert_eq!(refused["code"], "InvalidArg", "{refused}");
    assert_eq!(viewports(&mut host, DOC, PAGE), Vec::<Value>::new());
    assert_eq!(list(&mut host)[0]["contents"], "3.00 m");

    let mut anisotropic = scale();
    anisotropic["y"] = json!([{ "unit": "m", "conversion": 2 }]);
    let unavailable =
        update(&mut host, &a["ref"], json!({ "subtype": "line", "measure": anisotropic }));
    assert_eq!(unavailable["annotation"]["contents"], "3.00 m");
    assert_eq!(readout(&unavailable["annotation"]), Err("no-scale"));
}

#[test]
fn an_acrobat_import_keeps_its_stored_labels_and_opts_no_shape_into_a_caption() {
    for (unit, count) in [("metric", 11), ("imperial", 4)] {
        let mut host = Host::new();
        let (doc, _) = open_fixture(
            &mut host,
            &format!("packages/engine/main/test/fixtures/measure-acrobat-{unit}.pdf"),
        );
        let listed = call(&mut host, json!({ "kind": "annotations.list", "docId": doc }));
        let dimensions: Vec<Value> = listed["annotations"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|a| matches!(a["subtype"].as_str(), Some("line" | "polyline" | "polygon")))
            .cloned()
            .collect();
        assert_eq!(dimensions.len(), count, "{unit}");
        for a in dimensions {
            let label = readout(&a).unwrap_or_else(|why| panic!("{unit}: {why}: {a}")).label;
            // Acrobat can keep trailing zeros without /FD: the numbers
            // agree, and its saved text is kept.
            let number = |text: &str| -> f64 {
                let digits: String = text
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
                    .collect();
                digits.parse().unwrap_or(f64::NAN)
            };
            assert_eq!(number(&label), number(a["contents"].as_str().unwrap()), "{unit}: {a}");
            if a["subtype"] != "line" {
                assert_eq!(a["captionEnabled"], Value::Null, "{unit}: {a}");
            }
            let updated = call(
                &mut host,
                json!({ "kind": "annotations.update", "docId": doc, "ref": a["ref"],
                    "patch": { "subtype": a["subtype"], "color": "#0000ff" } }),
            );
            assert_eq!(updated["result"]["annotation"]["contents"], a["contents"], "{unit}");
        }
    }
}

#[test]
fn shared_and_foreign_measures_survive_edits_and_a_malformed_scale_stays_unavailable() {
    let annotation = |measure: &str| {
        format!(
            "<< /Type /Annot /Subtype /Line /IT /LineDimension /Rect [0 0 100 100] /L [0 0 100 0] \
             /Contents (imported) /Measure {measure} >>"
        )
    };
    let (shared_a, shared_b) = (annotation("6 0 R"), annotation("6 0 R"));
    let (foreign, malformed) = (
        annotation("<< /Subtype /GEO /Vendor (preserve) >>"),
        annotation("<< /Subtype /RL /X [<< /U (m) >>] /D [<< /U (m) /C 1 >>] /A [] >>"),
    );
    let mut host = open_measured(
        "/Annots [4 0 R 5 0 R 7 0 R 8 0 R]",
        &[
            &shared_a,
            &shared_b,
            "<< /Type /Measure /Subtype /RL /X [<< /U (m) /C 1 >>] /D [<< /U (m) /C 1 >>] /A [] >>",
            &foreign,
            &malformed,
        ],
    );
    let before = list(&mut host);
    assert_eq!(readout(&before[3]), Err("no-scale"));
    update(&mut host, &before[0]["ref"], json!({ "subtype": "line", "measure": scale() }));
    // Sending the marker a read returned keeps the foreign measure in place.
    assert_eq!(before[2]["measure"], json!({ "subtype": "geospatial" }));
    let kept = update(
        &mut host,
        &before[2]["ref"],
        json!({ "subtype": "line", "measure": before[2]["measure"], "color": "#0000ff" }),
    );
    assert_eq!(kept["annotation"]["measure"], json!({ "subtype": "geospatial" }));
    // Writing a geospatial measure is refused; an explicit scale replaces one.
    let refused = update_err(
        &mut host,
        &before[0]["ref"],
        json!({ "subtype": "line", "measure": { "subtype": "geospatial" } }),
    );
    assert_eq!(refused["code"], "InvalidArg", "{refused}");
    let replaced = update(
        &mut host,
        &before[2]["ref"],
        json!({ "subtype": "line", "contents": "corrupt", "measure": scale() }),
    );
    assert_eq!(replaced["annotation"]["measure"]["subtype"], "rectilinear");
    assert_eq!(replaced["annotation"]["contents"], "3.00 m");
    let after = list(&mut host);
    assert_eq!(after[0]["contents"], "3.00 m");
    assert_eq!(after[1], before[1], "the annotation sharing the scale is untouched");
    update(
        &mut host,
        &before[3]["ref"],
        json!({ "subtype": "line",
            "linePoints": { "start": { "x": 0, "y": 0 }, "end": { "x": 200, "y": 0 } } }),
    );
    assert_eq!(list(&mut host)[3]["contents"], "imported");
}

#[test]
fn a_manual_shape_centre_follows_a_turn_and_an_explicit_centre_wins() {
    let mut host = open_measured("", &[]);
    let vertices = [(10.0, 10.0), (110.0, 10.0), (110.0, 110.0)];
    let at = |points: &[(f64, f64)]| -> Value {
        Value::Array(points.iter().map(|(x, y)| json!({ "x": x, "y": y })).collect())
    };
    let a = create(
        &mut host,
        json!({ "subtype": "polygon", "intent": "polygon-dimension", "measure": scale(),
            "vertices": at(&vertices), "captionEnabled": true,
            "captionCenter": { "x": 90, "y": 20 } }),
    )["annotation"]
        .take();
    let turned: Vec<(f64, f64)> = vertices.iter().map(|(x, y)| (200.0 - y, *x)).collect();
    let result = update(
        &mut host,
        &a["ref"],
        json!({ "subtype": "polygon", "vertices": at(&turned), "captionEnabled": false }),
    );
    let r = &result["annotation"];
    assert_eq!(r["contents"], "4.50 m²", "{r}");
    assert_eq!(r["captionEnabled"], false);
    assert!(close(&r["captionCenter"]["x"], 180.0) && close(&r["captionCenter"]["y"], 90.0), "{r}");
    let explicit = update(
        &mut host,
        &a["ref"],
        json!({ "subtype": "polygon", "vertices": at(&vertices),
            "captionCenter": { "x": 0, "y": 0 } }),
    );
    assert_eq!(explicit["annotation"]["captionCenter"], json!({ "x": 0.0, "y": 0.0 }));
}

/// `runMeasurementConformance`: "calibration updates only viewports and
/// emits one event per write".
#[test]
fn a_calibration_changes_only_the_viewports_and_publishes_once_per_write() {
    let mut host = open_measured("", &[]);
    create(&mut host, line());
    let annotations = list(&mut host);
    let foreign: Vec<Value> =
        viewports(&mut host, DOC, PAGE).into_iter().filter(|v| v["owned"] == false).collect();
    let mut events = Vec::new();
    for measure in [scale(), Value::Null] {
        let request = json!({ "kind": "measure.setScale", "jobId": 7, "docId": DOC,
            "page": page_ref(PAGE), "measure": measure });
        let answer: Value =
            serde_json::from_str(&host.call(&request.to_string(), Vec::new()).json).unwrap();
        assert_eq!(answer["result"]["result"]["page"], page_ref(PAGE), "{answer}");
        events.push(answer["result"]["event"].clone());
        if !measure.is_null() {
            let owned: Vec<Value> = viewports(&mut host, DOC, PAGE)
                .into_iter()
                .filter(|v| v["owned"] == true)
                .collect();
            assert_eq!(owned.len(), 1);
            assert_eq!(list(&mut host), annotations, "a calibration leaves annotations alone");
        }
    }
    assert_eq!(viewports(&mut host, DOC, PAGE), foreign);
    for event in events {
        assert_eq!(event["page"], page_ref(PAGE), "{event}");
        assert_eq!(event["meta"]["affectedPages"], json!([]), "{event}");
    }
}
