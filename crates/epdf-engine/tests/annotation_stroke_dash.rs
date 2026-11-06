mod common;

use common::replay::Replay;
use epdf_engine::session::registry::SessionKey;
use serde_json::{Value, json};

const DOC: &str = "stroke-dash";
const FIXTURE: &str = "tooling/engine-conformance/fixtures/rich_metadata.pdf";

fn square(extra: Value) -> Value {
    let mut draft =
        json!({ "subtype": "square", "box": { "x": 40, "y": 50, "width": 60, "height": 40 } });
    draft.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    draft
}

fn create(replay: &mut Replay, draft: Value) -> Result<Value, Value> {
    replay
        .send(
            &json!({ "kind": "annotations.create", "jobId": "<job>", "docId": DOC,
        "opId": "<op>", "page": { "kind": "objectNumber", "objectNumber": 3 }, "draft": draft }),
            Vec::new(),
        )
        .outcome
        .map(|written| written["result"]["annotation"].clone())
}

fn update(replay: &mut Replay, annotation: &Value, patch: Value) -> Result<Value, Value> {
    replay
        .send(
            &json!({ "kind": "annotations.update", "jobId": "<job>", "docId": DOC,
        "opId": "<op>", "ref": annotation["ref"], "patch": patch }),
            Vec::new(),
        )
        .outcome
        .map(|written| written["result"]["annotation"].clone())
}

fn inspect<R>(
    replay: &Replay,
    annotation: &Value,
    inspect: impl FnOnce(&pdfium::Annotation<'_>) -> R,
) -> R {
    let object_number = annotation["ref"]["objectNumber"].as_u64().unwrap() as u32;
    let session = replay.host.sessions().session(&SessionKey::document(DOC)).expect("open session");
    let document = session.document().expect("document").document();
    let count = document.raw_annotation_count(0).expect("annotation count");
    let written = (0..count)
        .filter_map(|i| document.raw_annotation(0, i))
        .find(|a| a.object_number() == object_number)
        .expect("written annotation");
    inspect(&written)
}

#[test]
fn null_stroke_retains_width_and_survives_save_reopen() {
    let mut replay = Replay::open(FIXTURE, DOC);
    let created = create(
        &mut replay,
        square(json!({ "color": null, "strokeWidth": 6,
        "interiorColor": "#00ff00" })),
    )
    .expect("create without stroke");
    assert_eq!(created["color"], Value::Null);
    assert_eq!(created["strokeWidth"], json!(6.0));
    inspect(&replay, &created, |written| {
        assert!(written.has_key("C").unwrap());
        assert_eq!(written.value_type("C").unwrap(), 5, "no color is an array");
        assert!(written.color(0).is_none());
        assert_eq!(written.border_style().width, 6.0);
    });
    let bytes = {
        let session = replay.host.sessions().session(&SessionKey::document(DOC)).unwrap();
        session
            .document()
            .unwrap()
            .document()
            .save_to_buffer(pdfium::PdfSaveMode::Full)
            .expect("save no stroke")
    };
    let reopened = pdfium::Document::from_bytes(pdfium::Library::acquire(), &bytes, None)
        .expect("reopen no stroke");
    let written = reopened.raw_annotation(0, 0).expect("saved annotation");
    assert!(written.has_key("C").unwrap());
    assert!(written.color(0).is_none());
    assert_eq!(written.border_style().width, 6.0);
}

#[test]
fn omitted_stroke_keeps_default_and_null_patch_can_be_restored() {
    let mut replay = Replay::open(FIXTURE, DOC);
    let created = create(&mut replay, square(json!({}))).expect("create default stroke");
    assert_eq!(created["color"], "#ff0000");
    let cleared = update(&mut replay, &created, json!({ "color": null })).expect("clear stroke");
    assert_eq!(cleared["color"], Value::Null);
    assert_eq!(cleared["ref"], created["ref"]);
    let resized = update(&mut replay, &cleared, json!({ "strokeWidth": 4 })).expect("width only");
    assert_eq!(resized["color"], Value::Null);
    let restored =
        update(&mut replay, &resized, json!({ "color": "#123456" })).expect("restore stroke");
    assert_eq!(restored["color"], "#123456");
}

#[test]
fn dashed_borders_state_default_but_null_clears_and_zero_is_valid() {
    let mut replay = Replay::open(FIXTURE, DOC);
    let created =
        create(&mut replay, square(json!({ "borderStyle": "dashed" }))).expect("dashed create");
    assert_eq!(created["dashArray"], json!([3.0, 3.0]));
    let zero =
        update(&mut replay, &created, json!({ "dashArray": [0] })).expect("zero dash length");
    assert_eq!(zero["dashArray"], json!([0.0]));
    let cleared = update(&mut replay, &zero, json!({ "dashArray": null })).expect("clear dash");
    assert_eq!(cleared["dashArray"], Value::Null);
    let solid =
        update(&mut replay, &cleared, json!({ "borderStyle": "solid" })).expect("solid border");
    let defaulted =
        update(&mut replay, &solid, json!({ "borderStyle": "dashed" })).expect("default dash");
    assert_eq!(defaulted["dashArray"], json!([3.0, 3.0]));
}

#[test]
fn empty_dash_refuses_before_mutating_other_fields() {
    let mut replay = Replay::open(FIXTURE, DOC);
    let refused = create(&mut replay, square(json!({ "dashArray": [] })));
    let refused = refused.expect_err("an empty dash pattern must refuse");
    assert_eq!(refused["code"], "InvalidArg");
    assert_eq!(refused["details"]["field"], "dashArray");
    let listed = replay
        .replay(&json!({ "kind": "annotations.list", "jobId": "<job>" }))
        .expect("list after refused create");
    assert!(
        listed["annotations"].as_array().unwrap().is_empty(),
        "refused create left an annotation"
    );
    let created = create(&mut replay, square(json!({ "contents": "before", "color": "#123456" })))
        .expect("valid create");
    let refused = update(
        &mut replay,
        &created,
        json!({ "dashArray": [], "contents": "after", "color": "#654321" }),
    );
    let refused = refused.expect_err("an empty dash patch must refuse");
    assert_eq!(refused["code"], "InvalidArg");
    assert_eq!(refused["details"]["field"], "dashArray");
    inspect(&replay, &created, |written| {
        assert_eq!(written.string_value("Contents").unwrap().as_deref(), Some("before"));
        assert_eq!(written.color(0), Some((0x12, 0x34, 0x56)));
    });
}

fn compare_stroke_geometry(name: &str, actual: &Value, expected: &Value) {
    for key in [
        "subtype",
        "color",
        "strokeWidth",
        "borderStyle",
        "dashArray",
        "rect",
        "box",
        "linePoints",
        "vertices",
        "inkList",
        "lineEndings",
        "rotation",
        "interiorColor",
    ] {
        if let Some(expected) = expected.get(key) {
            let actual = actual.get(key).unwrap_or_else(|| panic!("{name}: missing {key}"));
            assert_eq!(
                test_support::corpus::quantized(actual),
                test_support::corpus::quantized(expected),
                "{name}: {key}"
            );
        }
    }
}

fn load_vectors(filename: &str) -> Value {
    let path = test_support::corpus::vectors_dir("annotation").join(filename);
    serde_json::from_str(&std::fs::read_to_string(path).expect("stroke/dash vectors"))
        .expect("vector JSON")
}

#[test]
fn stroke_and_dash_creates_agree_with_typescript() {
    let file = load_vectors("stroke-dash-create.json");
    assert_eq!(file["schemaVersion"], 1);
    assert_eq!(file["kind"], "annotation.create");
    let cases = file["cases"].as_array().unwrap();
    assert!(cases.len() >= 12);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let mut replay = Replay::open(FIXTURE, DOC);
        let actual = create(&mut replay, case["draft"].clone());
        if let Some(code) = case["refusedWith"].as_str() {
            assert_eq!(actual.expect_err(name)["code"], code, "{name}");
        } else {
            compare_stroke_geometry(
                name,
                &actual.unwrap_or_else(|e| panic!("{name}: {e}")),
                &case["created"],
            );
        }
    }
}

#[test]
fn stroke_and_dash_updates_agree_with_typescript() {
    let file = load_vectors("stroke-dash-update.json");
    assert_eq!(file["schemaVersion"], 1);
    assert_eq!(file["kind"], "annotation.update");
    let cases = file["cases"].as_array().unwrap();
    assert!(cases.len() >= 9);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let mut replay = Replay::open(FIXTURE, DOC);
        let created = create(&mut replay, case["draft"].clone()).expect("vector subject");
        let actual = update(&mut replay, &created, case["patch"].clone());
        if let Some(code) = case["refusedWith"].as_str() {
            assert_eq!(actual.expect_err(name)["code"], code, "{name}");
        } else {
            let actual = actual.unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(actual["ref"], created["ref"], "{name}: identity");
            compare_stroke_geometry(name, &actual, &case["updated"]);
        }
    }
}

#[test]
fn imported_missing_stroke_and_empty_dash_read_as_none_and_solid() {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [4 0 R] >>",
        "<< /Type /Annot /Subtype /Square /Rect [20 20 120 120] /BS << /S /D /W 4 /D [] >> >>",
    ];
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
    }
    let xref = bytes.len();
    bytes.extend(b"xref\n0 5\n0000000000 65535 f \n");
    for offset in offsets {
        bytes.extend(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend(
        format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    let document = pdfium::Document::from_bytes(pdfium::Library::acquire(), &bytes, None)
        .expect("import square");
    let annotation = document.raw_annotation(0, 0).expect("square");
    let typed = epdf_engine::features::annotation::read::style::read_geometry_style(&annotation)
        .expect("geometry style");
    let typed = serde_json::to_value(typed).unwrap();
    assert_eq!(typed["color"], Value::Null);
    assert_eq!(typed.get("dashArray"), Some(&Value::Null));
    let mut host = epdf_engine::host::Host::new();
    let opened = host.call_filled(
        &json!({ "kind": "open.fatMem", "jobId": 1, "docId": DOC }).to_string(),
        vec![bytes],
    );
    assert!(!opened.json.contains("\"error\""), "{}", opened.json);
    let mut replay = Replay::adopt(host, DOC);
    let listed = replay
        .replay(&json!({ "kind": "annotations.list", "jobId": "<job>" }))
        .expect("list imported square");
    let square = &listed["annotations"][0];
    assert_eq!(square["color"], Value::Null);
    assert_eq!(square["dashArray"], Value::Null);
    assert_eq!(square["borderStyle"], "solid");
    assert_eq!(square["strokeWidth"], json!(4.0));
}

#[test]
fn a_null_stroke_paints_fill_without_a_border_and_can_be_restored() {
    fn pixels(replay: &Replay) -> Vec<u8> {
        let session = replay.host.sessions().session(&SessionKey::document(DOC)).unwrap();
        let document = session.document().unwrap().document();
        document
            .page(0)
            .unwrap()
            .render_rgba_with_matrix(
                612,
                792,
                [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                true,
                pdfium::RenderFlags { annotations: true, ..Default::default() },
            )
            .expect("render square")
            .bgra
    }
    fn pixel(bytes: &[u8], x: usize, y: usize) -> &[u8] {
        let offset = (y * 612 + x) * 4;
        &bytes[offset..offset + 4]
    }
    let mut replay = Replay::open(FIXTURE, DOC);
    let created = create(
        &mut replay,
        square(json!({ "color": null, "strokeWidth": 6,
        "interiorColor": "#00ff00" })),
    )
    .expect("no stroke");
    let without = pixels(&replay);
    assert_eq!(pixel(&without, 70, 70), &[0, 255, 0, 255], "the fill is present");
    assert_eq!(
        pixel(&without, 42, 70),
        &[255, 255, 255, 255],
        "the retained-width border band has no stroke"
    );
    let restored =
        update(&mut replay, &created, json!({ "color": "#ff0000" })).expect("red stroke");
    let with = pixels(&replay);
    assert_eq!(
        pixel(&with, 42, 70),
        &[255, 0, 0, 255],
        "the stroke control paints the border band"
    );
    update(&mut replay, &restored, json!({ "color": null })).expect("remove stroke");
    let without = pixels(&replay);
    assert_eq!(pixel(&without, 42, 70), &[255, 255, 255, 255], "the restored border is gone");
}
