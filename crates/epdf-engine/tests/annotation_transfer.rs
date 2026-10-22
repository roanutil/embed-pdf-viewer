//! Annotations out as a bundle and into another document
//! (`annotations.export`, `annotations.import`), through the host as
//! upstream's suites drive them: `runAnnotationExportConformance`,
//! `runAnnotationImportConformance` and `runAnnotationTransferConformance`
//! (`packages/engine/core/src/conformance/`), which
//! `packages/engine/main/test/annotation-export.test.ts`,
//! `annotation-import.test.ts`, `annotation-transfer.test.ts` and
//! `annotations-stamp-transfer.test.ts` run, each sent as the worker
//! requests `LocalDocumentAnnotationsService` sends.
//!
//! Not ported: `annotation-import.test.ts`'s failure injection (a native
//! call made to fail at each pass, through a `Proxy` over the runtime's
//! functions), which has no Rust counterpart: the Rust binding has no seam
//! to fail a PDFium call from a test. The rollback it pins is the change
//! transaction's, which `document_apply.rs` covers. Its "loads no page"
//! count needs the same seam.

use epdf_engine::features::transfer::Bundle;
use epdf_engine::features::transfer::bundle::{BundleKind, resource_id_of};
use epdf_engine::features::transfer::codec;
use epdf_engine::features::transfer::limits::DEFAULT_BUNDLE_LIMITS;
use epdf_engine::host::Host;
use serde_json::{Value, json};

const HELLO_WORLD: &str = "packages/engine/main/test/fixtures/hello_world.pdf";
const ACROBAT_STAMPS: &str = "packages/engine/main/test/fixtures/stamp-opacity-acrobat.pdf";
const ACROBAT_REWRAPPED: &str = "packages/engine/main/test/fixtures/stamp-rewrapped-acrobat.pdf";

/// `PNG_1X1`.
const PNG_1X1: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
/// `BANDS_PNG`: a 30 × 10 PNG of three bands.
const BANDS_PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAB4AAAAKCAIAAAAsFXl4AAAAGklEQVR42mP4z8CABzFQIj1q9KjRo0aTIg0AQXcq5OunV0AAAAAASUVORK5CYII=";

fn png(base64: &str) -> Vec<u8> {
    codec::from_base64(base64).expect("base64")
}

/// `pdfOf`: a PDF of `objects`, numbered from 1.
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

/// `BANDS_PDF`: a one-page 200 × 100 PDF, a red band and a blue band.
fn bands_pdf() -> Vec<u8> {
    let content = "1 0 0 rg 0 0 120 100 re f 0 0 1 rg 120 0 80 100 re f";
    pdf_of(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R /Resources << >> >>"
            .into(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
    ])
}

/// A page box (`PageBox`).
fn page_box(x: f64, y: f64, width: f64, height: f64) -> Value {
    json!({ "x": x, "y": y, "width": width, "height": height })
}

/// `box(x)` of the import suite.
fn at(x: f64) -> Value {
    page_box(x, 300.0, 40.0, 30.0)
}

/// `iconRect`: a note's or a file's icon at 20 × 20, by its top-left.
fn icon_rect(x: f64, y: f64) -> Value {
    page_box(x, y, 20.0, 20.0)
}

fn canonical(value: &Value) -> Value {
    test_support::canonical_numbers(value)
}

fn page_ref(pon: u64) -> Value {
    json!({ "kind": "objectNumber", "objectNumber": pon })
}

/// An annotation ref's key (`annotationKey`).
fn key(reference: &Value) -> String {
    match reference["kind"].as_str() {
        Some("objectNumber") => format!("obj:{}", reference["objectNumber"]),
        _ => format!("base:{}:{}", reference["page"]["objectNumber"], reference["baseIndex"]),
    }
}

/// A bundle as a read answers it: its JSON and its resources' bytes.
#[derive(Clone, Debug)]
struct Exported {
    bundle: Value,
    payloads: Vec<Vec<u8>>,
}

impl Exported {
    /// The bundle with its resources as bytes, for the file codec.
    fn bundle(&self) -> Bundle {
        let mut manifest = self.bundle.as_object().unwrap().clone();
        let resources = manifest.remove("resources").unwrap();
        Bundle {
            manifest,
            resources: resources
                .as_object()
                .unwrap()
                .iter()
                .map(|(id, slot)| {
                    (id.clone(), self.payloads[slot["payload"].as_u64().unwrap() as usize].clone())
                })
                .collect(),
        }
    }

    /// The bytes of resource `id`.
    fn resource(&self, id: &Value) -> &[u8] {
        let slot = &self.bundle["resources"][id.as_str().unwrap()]["payload"];
        &self.payloads[slot.as_u64().unwrap() as usize]
    }

    fn items(&self) -> Vec<Value> {
        self.bundle["items"].as_array().cloned().unwrap_or_default()
    }
}

/// The host with documents open, and the calls a test sends.
struct Engine {
    host: Host,
    next: u64,
}

impl Engine {
    fn new() -> Self {
        Self { host: Host::new(), next: 1 }
    }

    fn open(&mut self, doc: &str, bytes: Vec<u8>) {
        self.open_with(doc, bytes, json!({}));
    }

    fn open_with(&mut self, doc: &str, bytes: Vec<u8>, extra: Value) {
        let mut request = json!({ "kind": "open.fatMem", "effect": "open", "docId": doc });
        for (key, value) in extra.as_object().unwrap() {
            request[key] = value.clone();
        }
        self.send(request, vec![bytes]).expect("open");
    }

    fn send(
        &mut self,
        mut request: Value,
        payloads: Vec<Vec<u8>>,
    ) -> Result<(Value, Vec<Vec<u8>>), Value> {
        self.next += 1;
        request["jobId"] = json!(self.next);
        let response = self.host.call(&request.to_string(), payloads);
        let mut envelope: Value = serde_json::from_str(&response.json).unwrap();
        match envelope.get_mut("error") {
            Some(error) => Err(error.take()),
            None => Ok((envelope["result"].take(), response.payloads)),
        }
    }

    fn call(&mut self, request: Value) -> Value {
        let shown = request.to_string();
        self.send(request, Vec::new()).unwrap_or_else(|e| panic!("{shown}: {e}")).0
    }

    fn first_page(&mut self, doc: &str) -> u64 {
        self.pages(doc)[0]
    }

    fn pages(&mut self, doc: &str) -> Vec<u64> {
        let pages = self.call(json!({ "kind": "pages.list", "effect": "read", "docId": doc }));
        pages["pages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["ref"]["objectNumber"].as_u64().unwrap())
            .collect()
    }

    /// `page.annotations.create(draft, { resources })`.
    fn create(
        &mut self,
        doc: &str,
        page: u64,
        draft: Value,
        resources: &[(&str, Vec<u8>)],
    ) -> Value {
        let mut slots = serde_json::Map::new();
        let mut payloads = Vec::new();
        for (role, bytes) in resources {
            slots.insert((*role).into(), json!({ "payload": payloads.len() }));
            payloads.push(bytes.clone());
        }
        let request = json!({ "kind": "annotations.create", "effect": "write", "docId": doc,
            "page": page_ref(page), "draft": draft, "resources": slots });
        let shown = request.to_string();
        let (result, _) = self.send(request, payloads).unwrap_or_else(|e| panic!("{shown}: {e}"));
        result["result"]["annotation"].clone()
    }

    fn export(&mut self, doc: &str, selection: Value) -> Result<Exported, Value> {
        let (result, payloads) = self.send(
            json!({ "kind": "annotations.export", "effect": "snapshot", "docId": doc,
                "selection": selection }),
            Vec::new(),
        )?;
        Ok(Exported { bundle: result["bundle"].clone(), payloads })
    }

    /// `doc.annotations.import(bundle, options)`: the whole answer
    /// (`{result, mutationSequence, events}`).
    fn import(&mut self, doc: &str, bundle: &Exported, options: Value) -> Result<Value, Value> {
        let mut request = json!({ "kind": "annotations.import", "effect": "write", "docId": doc,
            "bundle": bundle.bundle });
        for (key, value) in options.as_object().unwrap() {
            request[key] = value.clone();
        }
        self.send(request, bundle.payloads.clone()).map(|(result, _)| result)
    }

    fn list(&mut self, doc: &str, page: u64) -> Vec<Value> {
        let listed = self.call(json!({ "kind": "annotations.list", "effect": "read", "docId": doc,
            "pages": [page_ref(page)] }));
        listed["annotations"].as_array().cloned().unwrap_or_default()
    }

    fn keys(&mut self, doc: &str, page: u64) -> Vec<String> {
        let mut keys: Vec<String> = self.list(doc, page).iter().map(|a| key(&a["ref"])).collect();
        keys.sort();
        keys
    }

    fn apply(&mut self, doc: &str, change: Value) -> Result<Value, Value> {
        self.send(
            json!({ "kind": "document.apply", "effect": "write", "docId": doc, "change": change }),
            Vec::new(),
        )
        .map(|(result, _)| result)
    }

    fn read_appearance(&mut self, doc: &str, page: u64, reference: &Value) -> Vec<u8> {
        let (_, payloads) = self
            .send(
                json!({ "kind": "annotations.readAppearance", "effect": "read", "docId": doc,
                    "page": page_ref(page), "ref": reference }),
                Vec::new(),
            )
            .expect("readAppearance");
        payloads[0].clone()
    }

    fn read_file(&mut self, doc: &str, page: u64, reference: &Value) -> Vec<u8> {
        let (_, payloads) = self
            .send(
                json!({ "kind": "annotations.readFile", "effect": "read", "docId": doc,
                    "page": page_ref(page), "ref": reference }),
                Vec::new(),
            )
            .expect("readFile");
        payloads[0].clone()
    }
}

/// Two fresh copies of a fixture: where a bundle comes from, and where it
/// goes. Answers the engine and the first page.
fn two_copies(fixture: &str) -> (Engine, u64) {
    let mut engine = Engine::new();
    engine.open("source", test_support::fixture(fixture));
    engine.open("target", test_support::fixture(fixture));
    let page = engine.first_page("source");
    (engine, page)
}

fn one(fixture: &str) -> (Engine, u64) {
    let mut engine = Engine::new();
    engine.open("source", test_support::fixture(fixture));
    let page = engine.first_page("source");
    (engine, page)
}

// ── Export (`runAnnotationExportConformance`) ──────────────────────────────

#[test]
fn exports_the_annotations_as_a_read_returns_them_and_each_resource_once() {
    let (mut engine, page) = one(HELLO_WORLD);
    let first = engine.create(
        "source",
        page,
        json!({ "subtype": "stamp", "box": page_box(20.0, 20.0, 60.0, 30.0) }),
        &[("appearance", bands_pdf())],
    );
    let second = engine.create(
        "source",
        page,
        json!({ "subtype": "stamp", "box": page_box(100.0, 20.0, 60.0, 30.0), "opacity": 0.5 }),
        &[("appearance", bands_pdf())],
    );
    let image = engine.create(
        "source",
        page,
        json!({ "subtype": "stamp", "box": page_box(180.0, 20.0, 60.0, 30.0) }),
        &[("appearance", png(BANDS_PNG))],
    );
    let file = engine.create("source", page,
        json!({ "subtype": "file-attachment", "rect": icon_rect(260.0, 20.0), "file": { "name": "note.txt" } }),
        &[("file", b"attached".to_vec())]);

    let exported = engine.export("source", json!({})).unwrap();
    let bundle = exported.bundle();
    epdf_engine::features::transfer::annotation_bundle::assert_annotation_bundle(
        &bundle,
        &DEFAULT_BUNDLE_LIMITS,
    )
    .expect("a bundle the checks accept");
    let read = engine.list("source", page);
    // A bundle spells its numbers as JavaScript does; a read keeps `f64`.
    let data: Vec<Value> = exported.items().iter().map(|item| item["data"].clone()).collect();
    assert_eq!(canonical(&json!(data)), canonical(&json!(read)));

    let listed = engine.call(json!({ "kind": "pages.list", "effect": "read", "docId": "source" }));
    assert_eq!(
        exported.bundle["pages"],
        canonical(
            &json!([{ "page": page_ref(page), "position": 0, "size": listed["pages"][0]["size"] }])
        )
    );

    let id_of = |reference: &Value, role: &str| -> Value {
        exported.items().iter().find(|item| key(&item["data"]["ref"]) == key(reference)).unwrap()
            ["resources"][role]
            .clone()
    };
    // One drawing, one resource, however many stamps place it.
    assert_eq!(id_of(&first["ref"], "appearance"), id_of(&second["ref"], "appearance"));
    assert_eq!(exported.bundle["resources"].as_object().unwrap().len(), 3);
    for (annotation, role) in [(&first, "appearance"), (&image, "appearance"), (&file, "file")] {
        let bytes = if role == "file" {
            engine.read_file("source", page, &annotation["ref"])
        } else {
            engine.read_appearance("source", page, &annotation["ref"])
        };
        assert_eq!(exported.resource(&id_of(&annotation["ref"], role)), bytes.as_slice(), "{role}");
    }

    let text = codec::stringify(BundleKind::Annotation, &bundle, &DEFAULT_BUNDLE_LIMITS).unwrap();
    assert_eq!(
        codec::parse(BundleKind::Annotation, &text, &DEFAULT_BUNDLE_LIMITS).unwrap(),
        bundle
    );
}

#[test]
fn exports_a_stamp_another_tool_made_as_read_appearance_returns_it() {
    let (mut engine, page) = one(ACROBAT_STAMPS);
    let exported = engine.export("source", json!({})).unwrap();
    let stamps: Vec<Value> =
        exported.items().into_iter().filter(|item| item["data"]["subtype"] == "stamp").collect();
    assert_eq!(stamps.len(), 2);
    for stamp in stamps {
        let bytes = engine.read_appearance("source", page, &stamp["data"]["ref"]);
        assert_eq!(exported.resource(&stamp["resources"]["appearance"]), bytes.as_slice());
    }
}

/// Every engine exports these fixtures to exactly this file
/// (`exports a document to the same file on every engine`): the hashes are
/// upstream's, so a bundle is the same whichever engine made it.
#[test]
fn exports_a_document_to_the_same_file_as_the_typescript_engine() {
    for (fixture, hash) in [
        (ACROBAT_STAMPS, "8e7b1ade219fe9c78e355e8c282a46e9b8131f76a2c98b70331d909830fff9b6"),
        (ACROBAT_REWRAPPED, "4cead3c1918229234fad1651b082aa0b09b49325d6e462177eae30f049939b11"),
    ] {
        let (mut engine, _) = one(fixture);
        let exported = engine.export("source", json!({})).unwrap();
        let text =
            codec::stringify(BundleKind::Annotation, &exported.bundle(), &DEFAULT_BUNDLE_LIMITS)
                .unwrap();
        assert_eq!(
            resource_id_of(text.as_bytes()).unwrap(),
            format!("sha256-{hash}"),
            "{fixture}: {text}"
        );
    }
}

#[test]
fn takes_a_comment_with_its_thread_or_only_what_it_points_at() {
    let (mut engine, page) = one(HELLO_WORLD);
    let rect = page_box(300.0, 300.0, 20.0, 20.0);
    let note = engine.create(
        "source",
        page,
        json!({ "subtype": "text", "rect": icon_rect(300.0, 300.0), "contents": "Check this" }),
        &[],
    );
    let popup = engine.create(
        "source",
        page,
        json!({ "subtype": "popup", "rect": rect, "parent": note["ref"] }),
        &[],
    );
    let reply = engine.create("source", page,
        json!({ "subtype": "text", "rect": icon_rect(300.0, 300.0), "reply": { "to": note["ref"] } }), &[]);
    let answer = engine.create("source", page,
        json!({ "subtype": "text", "rect": icon_rect(300.0, 300.0), "reply": { "to": reply["ref"] } }), &[]);
    engine.create("source", page, json!({ "subtype": "square", "box": rect }), &[]);

    let mut keys_of = |selection: Value| -> Vec<String> {
        engine
            .export("source", selection)
            .unwrap()
            .items()
            .iter()
            .map(|i| key(&i["data"]["ref"]))
            .collect()
    };
    let keys = |annotations: &[&Value]| -> Vec<String> {
        annotations.iter().map(|a| key(&a["ref"])).collect()
    };
    assert_eq!(keys_of(json!({ "refs": [note["ref"]] })), keys(&[&note, &popup, &reply, &answer]));
    assert_eq!(
        keys_of(json!({ "refs": [note["ref"]], "include": "references" })),
        keys(&[&note, &popup])
    );
    assert_eq!(
        keys_of(json!({ "refs": [answer["ref"]], "include": "references" })),
        keys(&[&note, &popup, &reply, &answer])
    );
}

#[test]
fn exports_the_document_as_it_is_now_after_an_edit_and_after_a_page_move() {
    let (mut engine, first) = one(HELLO_WORLD);
    let rect = page_box(20.0, 20.0, 60.0, 30.0);
    engine.create("source", first, json!({ "subtype": "square", "box": rect }), &[]);
    let before = engine.export("source", json!({})).unwrap();
    assert_eq!(before.items().len(), 1);
    engine.create("source", first, json!({ "subtype": "circle", "box": rect }), &[]);
    assert_eq!(engine.export("source", json!({})).unwrap().items().len(), 2);

    engine
        .apply(
            "source",
            json!({ "ops": [{ "type": "pages.insertBlank",
            "spec": { "size": { "width": 200, "height": 200 } }, "position": "start" }] }),
        )
        .unwrap();
    let positions: Vec<Value> = engine.export("source", json!({})).unwrap().bundle["pages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["position"].clone())
        .collect();
    assert_eq!(positions, [json!(1)]);
    engine
        .apply(
            "source",
            json!({ "ops": [{ "type": "pages.reorder", "pages": [page_ref(first)],
            "position": "start" }] }),
        )
        .unwrap();
    let moved = engine.export("source", json!({})).unwrap();
    let mut expected = before.bundle["pages"][0].clone();
    expected["position"] = json!(0);
    assert_eq!(moved.bundle["pages"], json!([expected]));
}

#[test]
fn an_export_refuses_a_page_or_an_annotation_the_document_does_not_have() {
    let (mut engine, page) = one(HELLO_WORLD);
    let missing = engine.export("source", json!({ "pages": [page_ref(999_999)] })).unwrap_err();
    assert_eq!(missing["code"], "NotFound", "{missing}");
    let missing = engine
        .export(
            "source",
            json!({ "refs": [{ "kind": "objectNumber", "page": page_ref(page),
            "objectNumber": 999_999 }] }),
        )
        .unwrap_err();
    assert_eq!(missing["code"], "NotFound", "{missing}");
}

// ── Import (`runAnnotationImportConformance`) ──────────────────────────────

/// `creatables()`: one create for every kind the engine can create.
/// A create's draft and its resources by role.
type Creatable = (Value, Vec<(&'static str, Vec<u8>)>);

fn creatables() -> Vec<Creatable> {
    let rect = page_box(40.0, 40.0, 100.0, 60.0);
    let quad = json!({ "upperLeft": { "x": 40, "y": 40 }, "upperRight": { "x": 140, "y": 40 },
        "lowerLeft": { "x": 40, "y": 60 }, "lowerRight": { "x": 140, "y": 60 } });
    let vertices = json!([{ "x": 50, "y": 90 }, { "x": 130, "y": 90 }, { "x": 90, "y": 50 }]);
    let drafts = vec![
        json!({ "subtype": "highlight", "quadPoints": [quad] }),
        json!({ "subtype": "underline", "quadPoints": [quad] }),
        json!({ "subtype": "squiggly", "quadPoints": [quad] }),
        json!({ "subtype": "strikeout", "quadPoints": [quad] }),
        json!({ "subtype": "square", "box": rect }),
        json!({ "subtype": "square", "box": page_box(60.0, 50.0, 60.0, 40.0), "rotation": 30 }),
        json!({ "subtype": "square", "box": page_box(60.0, 50.0, 60.0, 40.0), "cloudyIntensity": 1 }),
        json!({ "subtype": "circle", "box": rect }),
        json!({ "subtype": "polygon", "rect": rect, "vertices": vertices }),
        json!({ "subtype": "polygon", "rect": rect, "vertices": vertices, "captionEnabled": true,
            "captionCenter": { "x": 90, "y": 80 } }),
        json!({ "subtype": "polyline", "rect": rect, "vertices": vertices }),
        json!({ "subtype": "line", "rect": rect,
            "linePoints": { "start": { "x": 50, "y": 90 }, "end": { "x": 130, "y": 50 } } }),
        json!({ "subtype": "ink", "rect": rect, "inkList": [vertices] }),
        json!({ "subtype": "free-text", "box": rect, "intent": "free-text", "fontFamily": "helvetica",
            "fontSize": 12, "textAlign": "left", "contents": "Declaration conformance" }),
        json!({ "subtype": "free-text", "box": page_box(80.0, 40.0, 60.0, 40.0),
            "intent": "free-text-callout", "fontFamily": "helvetica", "fontSize": 12,
            "textAlign": "left", "contents": "A callout",
            "calloutLine": [{ "x": 45, "y": 95 }, { "x": 60, "y": 70 }, { "x": 80, "y": 60 }],
            "lineEnding": "open-arrow" }),
        json!({ "subtype": "caret", "box": rect }),
        json!({ "subtype": "text", "rect": icon_rect(40.0, 40.0) }),
        json!({ "subtype": "link", "rect": rect, "target": { "kind": "uri", "uri": "https://example.com" } }),
        json!({ "subtype": "redact", "rect": rect, "quadPoints": [quad] }),
    ];
    let mut all: Vec<Creatable> = drafts.into_iter().map(|draft| (draft, Vec::new())).collect();
    all.push((json!({ "subtype": "stamp", "box": rect }), vec![("appearance", png(PNG_1X1))]));
    all.push((
        json!({ "subtype": "file-attachment", "rect": icon_rect(40.0, 40.0),
            "file": { "name": "note.txt", "mimeType": "text/plain", "description": "A note" } }),
        vec![("file", b"attached".to_vec())],
    ));
    all
}

/// A bundle as two documents can agree on it (`comparable`): each ref as
/// the source had it, and without the attribution an import stamps, the
/// attached file's dates included.
fn comparable(bundle: &Exported, ref_map: Option<&Value>) -> Vec<Value> {
    let back: std::collections::HashMap<String, Value> = ref_map
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|pair| (key(&pair["to"]), pair["from"].clone()))
        .collect();
    let source = |reference: &Value| -> Value {
        if reference.is_null() {
            Value::Null
        } else {
            back.get(&key(reference)).cloned().unwrap_or(reference.clone())
        }
    };
    bundle
        .items()
        .into_iter()
        .map(|item| {
            let mut data = item["data"].as_object().unwrap().clone();
            for drop in
                ["author", "createdAt", "modifiedAt", "userId", "createdBy", "modifiedBy", "nm"]
            {
                data.remove(drop);
            }
            if data["subtype"] == "file-attachment" && data["file"].is_object() {
                let file = data["file"].as_object_mut().unwrap();
                file.remove("createdAt");
                file.remove("modifiedAt");
            }
            data.insert("ref".into(), source(&item["data"]["ref"]));
            data.insert("popup".into(), source(&item["data"]["popup"]));
            if !item["data"]["reply"].is_null() {
                data["reply"]["to"] = source(&item["data"]["reply"]["to"]);
            }
            if item["data"]["subtype"] == "popup" {
                data.insert("parent".into(), source(&item["data"]["parent"]));
            }
            json!({ "data": data, "resources": item["resources"] })
        })
        .collect()
}

/// Every `/P` bit allowed, as an authority's grants carry them.
fn all_bits() -> Value {
    json!({ "bit3": true, "bit4": true, "bit5": true, "bit6": true, "bit9": true,
        "bit10": true, "bit11": true, "bit12": true })
}

fn is_uuid_v7(text: &str) -> bool {
    text.len() == 36
        && text.as_bytes()[14] == b'7'
        && text.chars().filter(|c| *c == '-').count() == 4
}

#[test]
fn imports_every_kind_a_create_makes_as_the_export_has_it() {
    let (mut engine, page) = two_copies(HELLO_WORLD);
    for (draft, resources) in creatables() {
        engine.create("source", page, draft, &resources);
    }
    engine.create(
        "source",
        page,
        json!({ "subtype": "stamp", "box": at(20.0) }),
        &[("appearance", bands_pdf())],
    );
    engine.create(
        "source",
        page,
        json!({ "subtype": "stamp", "box": at(80.0), "opacity": 0.5 }),
        &[("appearance", bands_pdf())],
    );
    let note = engine.create(
        "source",
        page,
        json!({ "subtype": "text", "rect": icon_rect(140.0, 300.0), "contents": "Check" }),
        &[],
    );
    engine.create(
        "source",
        page,
        json!({ "subtype": "popup", "rect": at(200.0), "parent": note["ref"], "open": true }),
        &[],
    );
    engine.create("source", page, json!({ "subtype": "text", "rect": icon_rect(140.0, 300.0), "reply": { "to": note["ref"] } }), &[]);
    engine.create(
        "source",
        page,
        json!({ "subtype": "link", "rect": at(260.0),
        "target": { "kind": "goto", "destination": { "kind": "fit", "page": page_ref(page) } } }),
        &[],
    );

    let bundle = engine.export("source", json!({})).unwrap();
    let answer = engine.import("target", &bundle, json!({ "attribution": "stamp" })).unwrap();
    let result = &answer["result"];
    assert_eq!(result["dropped"], json!([]));
    assert_eq!(result["annotations"].as_array().unwrap().len(), bundle.items().len());
    let from: Vec<String> =
        result["refMap"].as_array().unwrap().iter().map(|p| key(&p["from"])).collect();
    let items: Vec<String> = bundle.items().iter().map(|i| key(&i["data"]["ref"])).collect();
    assert_eq!(from, items);

    // Copies, so each has a fresh name.
    let names: Vec<Value> = bundle.items().iter().map(|i| i["data"]["nm"].clone()).collect();
    for annotation in result["annotations"].as_array().unwrap() {
        let nm = annotation["nm"].as_str().unwrap();
        assert!(is_uuid_v7(nm), "{nm}");
        assert!(!names.contains(&annotation["nm"]));
    }
    // Exported again, the target gives the same file, apart from the new
    // refs and names and the attribution the import stamped.
    let again = engine.export("target", json!({})).unwrap();
    assert_eq!(
        canonical(&json!(comparable(&again, Some(&result["refMap"])))),
        canonical(&json!(comparable(&bundle, None)))
    );
    let ids = |exported: &Exported| -> Vec<String> {
        let mut ids: Vec<String> =
            exported.bundle["resources"].as_object().unwrap().keys().cloned().collect();
        ids.sort();
        ids
    };
    assert_eq!(ids(&again), ids(&bundle));
    let again_data: Vec<Value> = again.items().iter().map(|i| i["data"].clone()).collect();
    assert_eq!(canonical(&result["annotations"]), canonical(&json!(again_data)));
}

#[test]
fn emits_one_annotations_created_per_annotation_as_one_transaction() {
    let (mut engine, page) = two_copies(HELLO_WORLD);
    engine.create("source", page, json!({ "subtype": "square", "box": at(20.0) }), &[]);
    engine.create("source", page, json!({ "subtype": "circle", "box": at(80.0) }), &[]);
    let bundle = engine.export("source", json!({})).unwrap();
    let answer = engine
        .import("target", &bundle, json!({ "attribution": "stamp", "opId": "import-1" }))
        .unwrap();
    let events = answer["events"].as_array().unwrap();
    assert_eq!(events.len(), 2);
    for (index, event) in events.iter().enumerate() {
        assert_eq!(event["type"], "annotations.created");
        assert_eq!(event["annotation"], answer["result"]["annotations"][index]);
        assert_eq!(event["page"], page_ref(page));
        assert_eq!(event["origin"]["tx"], json!({ "id": "import-1", "index": index, "count": 2 }));
    }
}

#[test]
fn a_restore_leaves_out_a_name_the_page_has_and_what_points_at_it_then_names_on_a_repeat() {
    let (mut engine, page) = two_copies(HELLO_WORLD);
    let taken = engine.create(
        "source",
        page,
        json!({ "subtype": "text", "rect": icon_rect(20.0, 300.0), "nm": "taken" }),
        &[],
    );
    let reply = engine.create("source", page,
        json!({ "subtype": "text", "rect": icon_rect(20.0, 300.0), "reply": { "to": taken["ref"] } }), &[]);
    let free = engine.create(
        "source",
        page,
        json!({ "subtype": "square", "box": at(80.0), "nm": "free" }),
        &[],
    );
    let circle =
        engine.create("source", page, json!({ "subtype": "circle", "box": at(140.0) }), &[]);
    engine.create(
        "target",
        page,
        json!({ "subtype": "square", "box": at(200.0), "nm": "taken" }),
        &[],
    );
    let bundle = engine.export("source", json!({})).unwrap();

    let first =
        engine.import("target", &bundle, json!({ "attribution": "restore" })).unwrap()["result"]
            .clone();
    assert_eq!(
        first["dropped"],
        json!([{ "ref": taken["ref"], "reason": "name-conflict" },
            { "ref": reply["ref"], "reason": "parent-dropped" }])
    );
    let from: Vec<String> =
        first["refMap"].as_array().unwrap().iter().map(|p| key(&p["from"])).collect();
    assert_eq!(from, [key(&free["ref"]), key(&circle["ref"])]);
    let names: Vec<Value> =
        first["annotations"].as_array().unwrap().iter().map(|a| a["nm"].clone()).collect();
    assert_eq!(names, [json!("free"), circle["nm"].clone()]);

    // Every name is recognized again.
    let second = engine.import("target", &bundle, json!({ "attribution": "restore" })).unwrap();
    let reasons: Vec<Value> = second["result"]["dropped"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["reason"].clone())
        .collect();
    assert_eq!(reasons, ["name-conflict", "parent-dropped", "name-conflict", "name-conflict"]);
    assert_eq!(second["result"]["annotations"], json!([]));
    assert_eq!(second["events"], json!([]), "nothing was written");
}

#[test]
fn a_stamp_makes_copies_each_with_a_fresh_name() {
    let (mut engine, page) = two_copies(HELLO_WORLD);
    let taken = engine.create(
        "source",
        page,
        json!({ "subtype": "text", "rect": icon_rect(20.0, 300.0), "nm": "taken" }),
        &[],
    );
    engine.create("source", page,
        json!({ "subtype": "text", "rect": icon_rect(20.0, 300.0), "reply": { "to": taken["ref"] } }), &[]);
    engine.create(
        "target",
        page,
        json!({ "subtype": "square", "box": at(200.0), "nm": "taken" }),
        &[],
    );
    let bundle = engine.export("source", json!({})).unwrap();
    for _ in 0..2 {
        let result =
            engine.import("target", &bundle, json!({ "attribution": "stamp" })).unwrap()["result"]
                .clone();
        assert_eq!(result["dropped"], json!([]));
        let (note, reply) = (&result["annotations"][0], &result["annotations"][1]);
        assert!(is_uuid_v7(note["nm"].as_str().unwrap()));
        assert!(is_uuid_v7(reply["nm"].as_str().unwrap()));
        assert_ne!(note["nm"], "taken");
        assert_eq!(reply["reply"]["to"], note["ref"]);
    }
}

#[test]
fn puts_each_page_where_the_options_say() {
    let (mut engine, page) = two_copies(HELLO_WORLD);
    engine.create("source", page, json!({ "subtype": "square", "box": at(20.0) }), &[]);
    let bundle = engine.export("source", json!({})).unwrap();
    engine
        .apply(
            "target",
            json!({ "ops": [{ "type": "pages.insertBlank",
            "spec": { "size": { "width": 300, "height": 300 } }, "position": "start" }] }),
        )
        .unwrap();
    let blank = engine.first_page("target");
    let mut page_of = |options: Value| -> Value {
        engine.import("target", &bundle, options).unwrap()["result"]["annotations"][0]["ref"]["page"].clone()
    };
    assert_eq!(page_of(json!({ "attribution": "stamp" })), page_ref(page));
    assert_eq!(page_of(json!({ "attribution": "stamp", "pages": "by-position" })), page_ref(blank));
    assert_eq!(
        page_of(
            json!({ "attribution": "stamp", "pages": [{ "from": page_ref(page), "to": page_ref(blank) }] })
        ),
        page_ref(blank)
    );
    assert_eq!(engine.list("target", blank).len(), 2);
}

#[test]
fn refuses_before_writing_anything_an_unmapped_page_a_tampered_or_oversized_resource() {
    let (mut engine, page) = two_copies(HELLO_WORLD);
    let stamp = engine.create(
        "source",
        page,
        json!({ "subtype": "stamp", "box": at(20.0) }),
        &[("appearance", png(PNG_1X1))],
    );
    engine
        .apply("source", json!({ "ops": [{ "type": "pages.insertBlank",
            "spec": { "size": { "width": 300, "height": 300 } }, "position": { "after": page_ref(page) } }] }))
        .unwrap();
    let second = engine.pages("source")[1];
    engine.create("source", second, json!({ "subtype": "square", "box": at(20.0) }), &[]);
    let bundle = engine.export("source", json!({})).unwrap();

    // The second page is not in the target.
    let refused = engine.import("target", &bundle, json!({ "attribution": "stamp" })).unwrap_err();
    assert_eq!(refused["code"], "InvalidArg", "{refused}");
    assert_eq!(refused["details"], json!({ "pages": [format!("obj:{second}")] }));

    let stamp_only = |bytes: Vec<u8>, id: String| -> Exported {
        let mut json = bundle.bundle.clone();
        json["pages"] = json!([bundle.bundle["pages"][0]]);
        json["items"] = json!([{ "data": stamp, "resources": { "appearance": id } }]);
        json["resources"] = json!({ id.clone(): { "payload": 0 } });
        Exported { bundle: json, payloads: vec![bytes] }
    };
    let tampered = stamp_only(png(PNG_1X1), resource_id_of(&bands_pdf()).unwrap());
    let refused =
        engine.import("target", &tampered, json!({ "attribution": "stamp" })).unwrap_err();
    assert_eq!(refused["code"], "InvalidArg", "{refused}");

    // A PNG whose header says 10 000 × 10 000: refused from the header.
    let mut huge = png(PNG_1X1);
    huge[16..20].copy_from_slice(&10_000u32.to_be_bytes());
    huge[20..24].copy_from_slice(&10_000u32.to_be_bytes());
    let id = resource_id_of(&huge).unwrap();
    let refused = engine
        .import("target", &stamp_only(huge, id), json!({ "attribution": "stamp" }))
        .unwrap_err();
    // `PayloadTooLarge` upstream, which this engine gains with 8.27.
    assert_eq!(refused["code"], "InvalidArg", "{refused}");
    assert_eq!(refused["details"]["limit"], "imagePixels");

    assert!(engine.list("target", page).is_empty());
}

/// A thread (a note, its popup, a reply), a square and a stamp, exported.
fn thread_bundle(engine: &mut Engine, page: u64) -> Exported {
    let note = engine.create(
        "source",
        page,
        json!({ "subtype": "text", "rect": icon_rect(20.0, 300.0), "contents": "Check" }),
        &[],
    );
    engine.create(
        "source",
        page,
        json!({ "subtype": "popup", "rect": at(80.0), "parent": note["ref"] }),
        &[],
    );
    engine.create("source", page,
        json!({ "subtype": "text", "rect": icon_rect(20.0, 300.0), "reply": { "to": note["ref"] } }), &[]);
    engine.create("source", page, json!({ "subtype": "square", "box": at(140.0) }), &[]);
    engine.create(
        "source",
        page,
        json!({ "subtype": "stamp", "box": at(200.0) }),
        &[("appearance", png(PNG_1X1))],
    );
    engine.export("source", json!({})).unwrap()
}

#[test]
fn an_import_is_undone_and_redone_with_the_same_refs_and_attribution() {
    for attribution in ["stamp", "restore"] {
        let (mut engine, page) = two_copies(HELLO_WORLD);
        let bundle = thread_bundle(&mut engine, page);
        let before = engine.keys("target", page);
        let answer =
            engine.import("target", &bundle, json!({ "attribution": attribution })).unwrap();
        let imported = &answer["result"];
        assert_eq!(imported["meta"]["undoable"], true, "{attribution}");
        assert_eq!(imported["annotations"].as_array().unwrap().len(), 5);

        // The thread goes as one, the square and the stamp each alone.
        let undo = engine.apply("target", json!({ "undoOf": imported["meta"]["opId"] })).unwrap();
        let types: Vec<Value> =
            undo["result"]["items"].as_array().unwrap().iter().map(|i| i["type"].clone()).collect();
        assert_eq!(
            types,
            ["annotations.delete", "annotations.delete", "annotations.delete"],
            "{attribution}"
        );
        assert_eq!(engine.keys("target", page), before);

        // The undo's undo brings back the same annotations, as the import wrote them.
        engine.apply("target", json!({ "undoOf": undo["result"]["meta"]["opId"] })).unwrap();
        let back = engine.list("target", page);
        for annotation in imported["annotations"].as_array().unwrap() {
            let again = back.iter().find(|a| key(&a["ref"]) == key(&annotation["ref"])).unwrap();
            for field in ["nm", "userId", "createdBy", "author", "rect"] {
                assert_eq!(again[field], annotation[field], "{attribution}: {field}");
            }
        }
    }
}

#[test]
fn an_imported_annotation_changed_since_stays_and_the_rest_of_the_import_goes() {
    let (mut engine, page) = two_copies(HELLO_WORLD);
    engine.create("source", page, json!({ "subtype": "square", "box": at(20.0) }), &[]);
    engine.create("source", page, json!({ "subtype": "circle", "box": at(80.0) }), &[]);
    let bundle = engine.export("source", json!({})).unwrap();
    let imported =
        engine.import("target", &bundle, json!({ "attribution": "stamp" })).unwrap()["result"]
            .clone();
    let (square, circle) = (&imported["annotations"][0], &imported["annotations"][1]);
    engine.call(json!({ "kind": "annotations.update", "effect": "write", "docId": "target",
        "ref": square["ref"], "patch": { "color": "#00aa00" } }));

    let undo = engine.apply("target", json!({ "undoOf": imported["meta"]["opId"] })).unwrap();
    let mut types: Vec<String> = undo["result"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["type"].as_str().unwrap().to_owned())
        .collect();
    types.sort();
    assert_eq!(types, ["annotations.delete", "skipped"]);
    let left = engine.keys("target", page);
    assert!(left.contains(&key(&square["ref"])));
    assert!(!left.contains(&key(&circle["ref"])));
}

#[test]
fn a_retry_under_the_same_op_id_answers_again_and_writes_nothing_more() {
    let (mut engine, page) = two_copies(HELLO_WORLD);
    engine.create("source", page, json!({ "subtype": "square", "box": at(20.0) }), &[]);
    let bundle = engine.export("source", json!({})).unwrap();
    let options = json!({ "attribution": "stamp", "opId": "import-retry" });
    let first = engine.import("target", &bundle, options.clone()).unwrap();
    let again = engine.import("target", &bundle, options).unwrap();
    assert_eq!(again["result"]["refMap"], first["result"]["refMap"]);
    let squares =
        engine.list("target", page).into_iter().filter(|a| a["subtype"] == "square").count();
    assert_eq!(squares, 1);
}

#[test]
fn document_apply_takes_no_import() {
    let (mut engine, _) = two_copies(HELLO_WORLD);
    let bundle = engine.export("source", json!({})).unwrap();
    let refused = engine
        .apply(
            "target",
            json!({ "ops": [{ "type": "annotations.import", "bundle": bundle.bundle,
            "attribution": "restore" }] }),
        )
        .unwrap_err();
    assert_eq!(refused["code"], "InvalidArg", "{refused}");
    assert!(refused["message"].as_str().unwrap().contains("doc.annotations.import"), "{refused}");
}

#[test]
fn a_final_write_ends_undo_for_an_import_before_it() {
    let (mut engine, page) = two_copies(HELLO_WORLD);
    engine.create("source", page, json!({ "subtype": "square", "box": at(20.0) }), &[]);
    let bundle = engine.export("source", json!({})).unwrap();
    let imported =
        engine.import("target", &bundle, json!({ "attribution": "stamp" })).unwrap()["result"]
            .clone();
    engine.call(json!({ "kind": "annotations.flatten", "effect": "write", "docId": "target",
        "page": page_ref(page), "refs": [imported["annotations"][0]["ref"]], "usage": "display" }));
    let refused =
        engine.apply("target", json!({ "undoOf": imported["meta"]["opId"] })).unwrap_err();
    assert_eq!(refused["code"], "UndoUnavailable", "{refused}");
}

#[test]
fn undoing_a_restoring_import_takes_the_imports_rights_not_the_per_annotation_ones() {
    let mut engine = Engine::new();
    engine.open("source", test_support::fixture(HELLO_WORLD));
    // May delete only its own annotations, and may import.
    let scope = [
        "doc.open",
        "doc.render",
        "doc.download",
        "doc.annotate.modify",
        "doc.annotate.import",
        "annotations:delete:self",
    ];
    engine.open_with("target", test_support::fixture(HELLO_WORLD),
        json!({ "scope": scope, "identity": { "user_id": "importer", "display_name": "Importer" } }));
    let page = engine.first_page("source");
    let authority = json!({ "identity": { "userId": "importer", "displayName": "Importer" },
        "grants": { "scope": scope, "pdfBits": all_bits() } });
    engine.create("source", page, json!({ "subtype": "square", "box": at(20.0) }), &[]);
    let bundle = engine.export("source", json!({})).unwrap();
    let imported = engine
        .import(
            "target",
            &bundle,
            json!({ "attribution": "restore",
            "actor": { "userId": "importer", "displayName": "Importer" } }),
        )
        .unwrap()["result"]
        .clone();
    let square = imported["annotations"][0].clone();

    // The square isn't the session's own: a delete is refused.
    let refused = engine
        .send(
            json!({ "kind": "annotations.delete", "effect": "write", "docId": "target",
            "ref": square["ref"], "authority": authority }),
            Vec::new(),
        )
        .unwrap_err();
    assert_eq!(refused["code"], "Forbidden", "{refused}");

    // The import's undo and its redo take what the import took.
    let undo = engine
        .send(
            json!({ "kind": "document.apply", "effect": "write", "docId": "target",
            "change": { "undoOf": imported["meta"]["opId"] }, "authority": authority }),
            Vec::new(),
        )
        .unwrap_or_else(|e| panic!("{e}"))
        .0;
    assert!(!engine.keys("target", page).contains(&key(&square["ref"])));
    engine
        .send(
            json!({ "kind": "document.apply", "effect": "write", "docId": "target",
            "change": { "undoOf": undo["result"]["meta"]["opId"] }, "authority": authority }),
            Vec::new(),
        )
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(engine.keys("target", page).contains(&key(&square["ref"])));
}

/// A server's change may hold an import (`document.applyChanges`), which
/// runs as the verb's op does.
#[test]
fn a_server_change_runs_an_import() {
    let (mut engine, page) = two_copies(HELLO_WORLD);
    engine.create("source", page, json!({ "subtype": "square", "box": at(20.0) }), &[]);
    let bundle = engine.export("source", json!({})).unwrap();
    let (answer, _) = engine
        .send(
            json!({ "kind": "document.applyChanges", "effect": "write", "docId": "target",
                "changes": [{ "opId": "server-import-1",
                    "change": { "ops": [{ "type": "annotations.import", "bundle": bundle.bundle,
                        "attribution": "stamp" }] } }] }),
            bundle.payloads.clone(),
        )
        .unwrap_or_else(|e| panic!("{e}"));
    let outcome = &answer["outcomes"][0];
    assert_eq!(outcome["status"], "applied", "{answer}");
    assert_eq!(outcome["result"]["items"][0]["type"], "annotations.import");
    assert_eq!(engine.list("target", page).len(), 1);
}

// ── Round trips (`runAnnotationTransferConformance`) ───────────────────────
//
// Each fixture's bundle is imported into a copy without annotations, then
// exported again.

/// The fixtures `annotation-transfer.test.ts` round-trips, `authoring`
/// first.
const ROUND_TRIPS: &[&str] = &[
    "authoring",
    "stamp-opacity-acrobat.pdf",
    "stamp-rewrapped-acrobat.pdf",
    "stamp-roundtrip-acrobat-60.pdf",
    "stamp-roundtrip-acrobat-100.pdf",
    "measure-acrobat-imperial.pdf",
    "measure-acrobat-metric.pdf",
    "action_payloads.pdf",
    "annots_action_handling.pdf",
    "annot_javascript.pdf",
    "link_javascript.pdf",
    "dates.pdf",
    "freetext_document_subset_helvetica.pdf",
    "flatten_selective.pdf",
    "embedded_attachments.pdf",
];

/// What `stamp` writes afresh (`STAMPED`).
const STAMPED: &[&str] =
    &["author", "createdAt", "modifiedAt", "userId", "createdBy", "modifiedBy", "nm"];

/// Kinds whose data is points: the box is the frame of what is drawn.
const FRAMED: &[&str] =
    &["line", "polyline", "polygon", "ink", "highlight", "underline", "squiggly", "strikeout"];

fn fixture_path(fixture: &str) -> String {
    let file = if fixture == "authoring" { "hello_world.pdf" } else { fixture };
    format!("packages/engine/main/test/fixtures/{file}")
}

/// Fill our own file (`fill`): every kind a create makes, stamps sharing a
/// drawing, a thread, a link.
fn fill(engine: &mut Engine, doc: &str) {
    let page = engine.first_page(doc);
    for (draft, resources) in creatables() {
        engine.create(doc, page, draft, &resources);
    }
    engine.create(
        doc,
        page,
        json!({ "subtype": "stamp", "box": at(20.0), "nm": "approved" }),
        &[("appearance", bands_pdf())],
    );
    engine.create(
        doc,
        page,
        json!({ "subtype": "stamp", "box": at(80.0), "opacity": 0.5 }),
        &[("appearance", bands_pdf())],
    );
    let note = engine.create(doc, page,
        json!({ "subtype": "text", "rect": icon_rect(140.0, 300.0), "nm": "note", "contents": "Check" }), &[]);
    engine.create(
        doc,
        page,
        json!({ "subtype": "popup", "rect": at(200.0), "parent": note["ref"], "open": true }),
        &[],
    );
    engine.create(doc, page, json!({ "subtype": "text", "rect": icon_rect(140.0, 300.0), "reply": { "to": note["ref"] } }), &[]);
    engine.create(
        doc,
        page,
        json!({ "subtype": "link", "rect": at(260.0),
        "target": { "kind": "goto", "destination": { "kind": "fit", "page": page_ref(page) } } }),
        &[],
    );
}

/// A fresh copy of the fixture, its annotations deleted apart from form
/// fields' widgets, which belong to their fields (`bare`). Answers the keys
/// of those it kept.
fn bare(engine: &mut Engine, doc: &str, fixture: &str) -> Vec<String> {
    engine.open(doc, test_support::fixture(&fixture_path(fixture)));
    let mut kept = Vec::new();
    for page in engine.pages(doc) {
        loop {
            let annotations = engine.list(doc, page);
            let field_widget = |a: &Value| a["subtype"] == "widget" && !a["field"].is_null();
            let Some(last) = annotations.iter().rev().find(|a| !field_widget(a)) else { break };
            engine.call(json!({ "kind": "annotations.delete", "effect": "write", "docId": doc,
                "ref": last["ref"] }));
        }
        kept.extend(engine.list(doc, page).iter().map(|a| key(&a["ref"])));
    }
    kept
}

/// A bundle's JSON without the resources and pages no item names
/// (`pruned`).
fn pruned(mut bundle: Value) -> Value {
    let items = bundle["items"].as_array().cloned().unwrap_or_default();
    let named: Vec<Value> = items
        .iter()
        .flat_map(|item| item["resources"].as_object().cloned().unwrap_or_default().into_values())
        .collect();
    let on_pages: Vec<u64> = items
        .iter()
        .flat_map(|item| {
            let mut pages = vec![item["data"]["ref"]["page"]["objectNumber"].as_u64().unwrap_or(0)];
            pages.extend(
                epdf_engine::features::transfer::page_refs::page_refs_in(&item["data"])
                    .into_iter()
                    .map(|p| u64::from(p.object_number().get())),
            );
            pages
        })
        .collect();
    let pages: Vec<Value> = bundle["pages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| on_pages.contains(&entry["page"]["objectNumber"].as_u64().unwrap()))
        .cloned()
        .collect();
    bundle["pages"] = json!(pages);
    let resources: serde_json::Map<String, Value> = bundle["resources"]
        .as_object()
        .unwrap()
        .iter()
        .filter(|(id, _)| named.contains(&json!(id)))
        .map(|(id, slot)| (id.clone(), slot.clone()))
        .collect();
    bundle["resources"] = Value::Object(resources);
    bundle
}

/// The bundle without the items `keys` name (`without`).
fn without(exported: &Exported, keys: &[String]) -> Exported {
    let mut bundle = exported.bundle.clone();
    let items: Vec<Value> = exported
        .items()
        .into_iter()
        .filter(|item| !keys.contains(&key(&item["data"]["ref"])))
        .collect();
    bundle["items"] = json!(items);
    Exported { bundle: pruned(bundle), payloads: exported.payloads.clone() }
}

/// The bundle as the import took it (`asTaken`): the items left out go, a
/// field left out reads `null`.
fn as_taken(exported: &Exported, dropped: &Value) -> Exported {
    let dropped = dropped.as_array().cloned().unwrap_or_default();
    let left: Vec<String> =
        dropped.iter().filter(|d| d.get("field").is_none()).map(|d| key(&d["ref"])).collect();
    let mut bundle = exported.bundle.clone();
    let items: Vec<Value> = exported
        .items()
        .into_iter()
        .filter(|item| !left.contains(&key(&item["data"]["ref"])))
        .map(|mut item| {
            for drop in &dropped {
                if let Some(field) = drop.get("field").and_then(Value::as_str)
                    && key(&drop["ref"]) == key(&item["data"]["ref"])
                {
                    item["data"][field] = Value::Null;
                }
            }
            item
        })
        .collect();
    bundle["items"] = json!(items);
    Exported { bundle: pruned(bundle), payloads: exported.payloads.clone() }
}

/// What two documents can agree on after an import (`normalized`): refs to
/// annotations as positions in the bundle, pages as their position, without
/// `importedBy`, `hasAppearance`, a framed or turned kind's `rect`, a
/// link's `activate` action, a file's size and checksum, and in `stamp`
/// mode the attribution it stamps.
fn normalized(exported: &Exported, attribution: &str) -> Value {
    let items = exported.items();
    let mut position: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for (at, item) in items.iter().enumerate() {
        position.entry(key(&item["data"]["ref"])).or_insert(at);
    }
    let at = |reference: &Value| -> Value {
        if reference.is_null() {
            Value::Null
        } else {
            position.get(&key(reference)).map_or(Value::Null, |n| json!(n))
        }
    };
    let page_position: std::collections::HashMap<u64, Value> = exported.bundle["pages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| (entry["page"]["objectNumber"].as_u64().unwrap(), entry["position"].clone()))
        .collect();
    fn pages(
        value: Value,
        key: Option<&str>,
        positions: &std::collections::HashMap<u64, Value>,
    ) -> Value {
        match value {
            Value::Array(items) => {
                Value::Array(items.into_iter().map(|v| pages(v, None, positions)).collect())
            }
            Value::Object(fields) => {
                if key == Some("page") && fields.get("kind") == Some(&json!("objectNumber")) {
                    let number = fields["objectNumber"].as_u64().unwrap_or(0);
                    return json!({ "position": positions.get(&number) });
                }
                Value::Object(
                    fields
                        .into_iter()
                        .map(|(k, v)| {
                            let v = pages(v, Some(&k), positions);
                            (k, v)
                        })
                        .collect(),
                )
            }
            other => other,
        }
    }
    let normal_items: Vec<Value> = items
        .iter()
        .map(|item| {
            let data = &item["data"];
            let mut fields = data.as_object().unwrap().clone();
            for drop in ["importedBy", "hasAppearance"] {
                fields.remove(drop);
            }
            fields.insert("ref".into(), at(&data["ref"]));
            fields.insert("popup".into(), at(&data["popup"]));
            if !data["reply"].is_null() {
                fields.insert("reply".into(), json!({ "to": at(&data["reply"]["to"]), "type": data["reply"]["type"] }));
            }
            if data["subtype"] == "popup" {
                fields.insert("parent".into(), at(&data["parent"]));
            }
            let subtype = data["subtype"].as_str().unwrap_or_default();
            let turned = data["rotation"].as_f64().is_some_and(|r| r != 0.0) && data["box"].is_object();
            if FRAMED.contains(&subtype) || turned {
                fields.remove("rect");
            }
            if subtype == "link" && data["actions"].is_object() {
                let mut triggers = data["actions"].as_object().unwrap().clone();
                triggers.remove("activate");
                fields.insert("actions".into(), if triggers.is_empty() { Value::Null } else { Value::Object(triggers) });
            }
            if attribution == "stamp" {
                for drop in STAMPED {
                    fields.remove(*drop);
                }
            }
            if subtype == "file-attachment" && data["file"].is_object() {
                let mut file = data["file"].as_object().unwrap().clone();
                file.remove("size");
                file.remove("checksum");
                fields.insert("file".into(), Value::Object(file));
            }
            json!({ "data": pages(Value::Object(fields), None, &page_position), "resources": item["resources"] })
        })
        .collect();
    let page_entries: Vec<Value> = exported.bundle["pages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| json!({ "position": entry["position"], "size": entry["size"] }))
        .collect();
    canonical(&json!({ "pages": page_entries, "items": normal_items }))
}

/// The points a framed item's data places on the page (`geometryOf`): a
/// line's, polyline's, polygon's or ink's as drawn, the upright points a read
/// gives turned by `rotation` about the middle of their box (`drawnPointsOf`);
/// a text markup's quads as they are.
fn geometry_of(data: &Value) -> Vec<epdf_engine::geom::PagePoint> {
    use epdf_engine::geom::PagePoint;
    use epdf_engine::page_space::helpers::{page_point_turned, page_turn_of_upright};
    let point = |p: &Value| PagePoint { x: p["x"].as_f64().unwrap(), y: p["y"].as_f64().unwrap() };
    let list = |v: &Value| v.as_array().into_iter().flatten().map(point).collect::<Vec<_>>();
    let drawn: Option<Vec<PagePoint>> = match data["subtype"].as_str().unwrap_or_default() {
        "line" => {
            Some(vec![point(&data["linePoints"]["start"]), point(&data["linePoints"]["end"])])
        }
        "polyline" | "polygon" => Some(list(&data["vertices"])),
        "ink" => Some(data["inkList"].as_array().into_iter().flatten().flat_map(list).collect()),
        _ => None,
    };
    if let Some(points) = drawn {
        let Some(rotation) = data["rotation"].as_f64().filter(|r| *r != 0.0) else { return points };
        let turn = page_turn_of_upright(&points, rotation);
        return points.into_iter().map(|p| page_point_turned(p, turn)).collect();
    }
    let mut points = Vec::new();
    fn visit(value: &Value, points: &mut Vec<PagePoint>) {
        match value {
            Value::Array(items) => items.iter().for_each(|v| visit(v, points)),
            Value::Object(fields) => {
                match (
                    fields.get("x").and_then(Value::as_f64),
                    fields.get("y").and_then(Value::as_f64),
                ) {
                    (Some(x), Some(y)) => points.push(PagePoint { x, y }),
                    _ => fields.values().for_each(|v| visit(v, points)),
                }
            }
            _ => {}
        }
    }
    visit(&data["quadPoints"], &mut points);
    points
}

/// The frame of each copy whose data is points holds those points
/// (`expectFramesHoldGeometry`). Answers how many turned shapes it checked.
fn expect_frames_hold_geometry(exported: &Exported) -> usize {
    let mut turned = 0;
    for item in exported.items() {
        let data = &item["data"];
        if !FRAMED.contains(&data["subtype"].as_str().unwrap_or_default()) {
            continue;
        }
        if data["rotation"].as_f64().is_some_and(|r| r != 0.0) {
            turned += 1;
        }
        let rect = &data["rect"];
        let (x, y, w, h) = (
            rect["x"].as_f64().unwrap(),
            rect["y"].as_f64().unwrap(),
            rect["width"].as_f64().unwrap(),
            rect["height"].as_f64().unwrap(),
        );
        let epsilon = 0.01;
        let outside: Vec<_> = geometry_of(data)
            .into_iter()
            .filter(|p| {
                p.x < x - epsilon
                    || p.x > x + w + epsilon
                    || p.y < y - epsilon
                    || p.y > y + h + epsilon
            })
            .collect();
        assert!(outside.is_empty(), "{}: {outside:?}", key(&data["ref"]));
    }
    turned
}

/// An appearance as pixels: RGBA, no row padding (`Raster`).
struct Raster {
    width: usize,
    height: usize,
    rgba: Vec<u8>,
}

/// Every normal annotation appearance on a page, by annotation key, at
/// scale 1, in one call (`appearanceRasters`).
fn appearance_rasters(
    engine: &mut Engine,
    doc: &str,
    page: u64,
) -> std::collections::HashMap<String, Raster> {
    let (answer, payloads) = engine
        .send(
            json!({ "kind": "annotations.renderAppearances", "effect": "read", "docId": doc,
                "page": page_ref(page), "family": "annotations",
                "options": { "viewport": { "kind": "scale", "scale": 1 } } }),
            Vec::new(),
        )
        .unwrap_or_else(|e| panic!("renderAppearances: {e}"));
    let mut rasters = std::collections::HashMap::new();
    for (appearance, pixels) in answer["appearances"].as_array().unwrap().iter().zip(payloads) {
        if appearance["mode"] != "normal" {
            continue;
        }
        let raster = &appearance["raster"];
        let (width, height, stride) = (
            raster["width"].as_u64().unwrap() as usize,
            raster["height"].as_u64().unwrap() as usize,
            raster["stride"].as_u64().unwrap() as usize,
        );
        let mut rgba = Vec::with_capacity(width * height * 4);
        for row in 0..height {
            rgba.extend_from_slice(&pixels[row * stride..row * stride + width * 4]);
        }
        rasters.insert(key(&appearance["ref"]), Raster { width, height, rgba });
    }
    rasters
}

/// Channel `channel` of the pixel at `at` as drawn: color times alpha, or
/// the alpha (`drawn`).
fn drawn(rgba: &[u8], at: usize, channel: usize) -> i32 {
    let alpha = i32::from(rgba[at + 3]);
    if channel == 3 {
        alpha
    } else {
        (f64::from(i32::from(rgba[at + channel]) * alpha) / 255.0).round() as i32
    }
}

/// The largest difference between a pixel of one raster and the closest
/// pixel of the other within `radius` pixels, both ways
/// (`maxShiftedDifference`).
fn max_shifted_difference(a: &Raster, b: &Raster, radius: i64) -> i32 {
    let one_way = |from: &Raster, to: &Raster| {
        let mut max = 0;
        for y in 0..from.height as i64 {
            for x in 0..from.width as i64 {
                let mut best = 255;
                'search: for dy in -radius..=radius {
                    for dx in -radius..=radius {
                        if best == 0 {
                            break 'search;
                        }
                        let (ny, nx) = (y + dy, x + dx);
                        if ny < 0 || nx < 0 || ny >= to.height as i64 || nx >= to.width as i64 {
                            continue;
                        }
                        let a = ((y * from.width as i64 + x) * 4) as usize;
                        let b = ((ny * to.width as i64 + nx) * 4) as usize;
                        let difference = (0..4)
                            .map(|c| (drawn(&from.rgba, a, c) - drawn(&to.rgba, b, c)).abs())
                            .max()
                            .unwrap();
                        best = best.min(difference);
                    }
                }
                max = max.max(best);
            }
        }
        max
    };
    one_way(a, b).max(one_way(b, a))
}

/// Each turned box's `rect` is the upright box around its own box, turned
/// (`expectTurnedRects`).
fn expect_turned_rects(exported: &Exported) {
    for item in exported.items() {
        let data = &item["data"];
        let Some(rotation) = data["rotation"].as_f64().filter(|r| *r != 0.0) else { continue };
        if !data["box"].is_object() {
            continue;
        }
        let edges = |b: &Value| epdf_engine::geom::PdfRect {
            left: b["x"].as_f64().unwrap(),
            bottom: b["y"].as_f64().unwrap(),
            right: b["x"].as_f64().unwrap() + b["width"].as_f64().unwrap(),
            top: b["y"].as_f64().unwrap() + b["height"].as_f64().unwrap(),
        };
        let expected = epdf_engine::geom::pdf_rect_turned_bounds(edges(&data["box"]), rotation);
        let actual = edges(&data["rect"]);
        for (a, e) in [
            (actual.left, expected.left),
            (actual.bottom, expected.bottom),
            (actual.right, expected.right),
            (actual.top, expected.top),
        ] {
            assert!((a - e).abs() <= 0.01, "{}: {actual:?} vs {expected:?}", key(&data["ref"]));
        }
    }
}

/// Every resource has the same bytes under the same id.
fn expect_same_resources(actual: &Exported, expected: &Exported) {
    let ids = |e: &Exported| -> Vec<String> {
        let mut ids: Vec<String> =
            e.bundle["resources"].as_object().unwrap().keys().cloned().collect();
        ids.sort();
        ids
    };
    assert_eq!(ids(actual), ids(expected));
    for id in ids(expected) {
        assert_eq!(actual.resource(&json!(id)), expected.resource(&json!(id)), "{id}");
    }
}

/// `roundTrip`: the fixture's bundle into a bare copy, and the copy's
/// export without the widgets it kept.
fn round_trip(
    fixture: &str,
    attribution: &str,
) -> (Engine, Exported, Value, Exported, Vec<String>) {
    let mut engine = Engine::new();
    engine.open("source", test_support::fixture(&fixture_path(fixture)));
    if fixture == "authoring" {
        fill(&mut engine, "source");
    }
    let bundle = engine.export("source", json!({})).unwrap();
    let kept = bare(&mut engine, "copy", fixture);
    let answer = engine
        .import("copy", &bundle, json!({ "attribution": attribution }))
        .unwrap_or_else(|e| panic!("{fixture}: {e}"));
    let again = without(&engine.export("copy", json!({})).unwrap(), &kept);
    (engine, bundle, answer["result"].clone(), again, kept)
}

#[test]
fn r1_to_r4_a_restoring_import_exports_as_its_source_as_far_as_the_import_took_it() {
    for fixture in ROUND_TRIPS {
        let (_, bundle, result, again, _) = round_trip(fixture, "restore");
        let taken = as_taken(&bundle, &result["dropped"]);
        assert_eq!(normalized(&again, "restore"), normalized(&taken, "restore"), "{fixture}");
        expect_frames_hold_geometry(&again);
        expect_turned_rects(&again);
        expect_same_resources(&again, &taken);
    }
}

#[test]
fn r1_r2_a_stamping_import_does_too_apart_from_the_attribution_and_names_it_stamps() {
    for fixture in ROUND_TRIPS {
        let (_, bundle, result, again, _) = round_trip(fixture, "stamp");
        let taken = as_taken(&bundle, &result["dropped"]);
        for annotation in result["annotations"].as_array().unwrap() {
            assert!(
                is_uuid_v7(annotation["nm"].as_str().unwrap_or_default()),
                "{fixture}: {annotation}"
            );
        }
        assert_eq!(normalized(&again, "stamp"), normalized(&taken, "stamp"), "{fixture}");
        expect_frames_hold_geometry(&again);
        expect_turned_rects(&again);
        expect_same_resources(&again, &taken);
    }
}

#[test]
fn r5_a_stamp_draws_as_its_source_and_in_our_own_file_every_kind_does() {
    for fixture in ROUND_TRIPS {
        let (mut engine, bundle, result, _, _) = round_trip(fixture, "restore");
        let copy_of: std::collections::HashMap<String, Value> = result["refMap"]
            .as_array()
            .unwrap()
            .iter()
            .map(|pair| (key(&pair["from"]), pair["to"].clone()))
            .collect();
        let mut compared = 0;
        for entry in bundle.bundle["pages"].as_array().unwrap() {
            let page = entry["page"]["objectNumber"].as_u64().unwrap();
            let drawn = appearance_rasters(&mut engine, "source", page);
            let copies = appearance_rasters(&mut engine, "copy", page);
            for item in bundle.items() {
                let data = &item["data"];
                if data["ref"]["page"]["objectNumber"] != page {
                    continue;
                }
                if *fixture != "authoring" && data["subtype"] != "stamp" {
                    continue;
                }
                let from = key(&data["ref"]);
                let (Some(to), Some(original)) = (copy_of.get(&from), drawn.get(&from)) else {
                    continue;
                };
                let copied = copies.get(&key(to));
                let size = copied.map(|c| (c.width, c.height));
                let same = copied.is_some_and(|c| max_shifted_difference(c, original, 1) <= 3);
                assert_eq!(
                    (from.as_str(), size, same),
                    (from.as_str(), Some((original.width, original.height)), true),
                    "{fixture}"
                );
                compared += 1;
            }
        }
        if fixture.starts_with("stamp-") || *fixture == "authoring" {
            assert!(compared > 0, "{fixture}: nothing drawn was compared");
        }
    }
}

/// Turned point shapes through a round trip: the frame of each copy holds
/// its points as drawn, turned about the middle of their box. Not one of
/// upstream's inputs (its `fill` makes no turned point shape), so it runs
/// apart from R1–R4, whose exact comparison a turned shape's points would
/// fail by a few millionths of a point: the file keeps them turned, as
/// 32-bit floats, and a read turns them back.
#[test]
fn a_turned_point_shapes_frame_holds_its_drawn_points() {
    let (mut engine, page) = two_copies(HELLO_WORLD);
    let rect = page_box(40.0, 40.0, 100.0, 60.0);
    let vertices = json!([{ "x": 50, "y": 90 }, { "x": 130, "y": 90 }, { "x": 90, "y": 50 }]);
    for draft in [
        json!({ "subtype": "ink", "rect": rect, "inkList": [vertices], "rotation": 30 }),
        json!({ "subtype": "polygon", "rect": rect, "vertices": vertices, "rotation": 45 }),
        json!({ "subtype": "polyline", "rect": rect, "vertices": vertices, "rotation": 90 }),
        json!({ "subtype": "line", "rect": rect,
            "linePoints": { "start": { "x": 50, "y": 90 }, "end": { "x": 130, "y": 50 } }, "rotation": 60 }),
    ] {
        engine.create("source", page, draft, &[]);
    }
    let bundle = engine.export("source", json!({})).unwrap();
    assert_eq!(expect_frames_hold_geometry(&bundle), 4);
    let imported = engine.import("target", &bundle, json!({ "attribution": "stamp" })).unwrap();
    assert_eq!(imported["result"]["dropped"], json!([]));
    let again = engine.export("target", json!({})).unwrap();
    assert_eq!(expect_frames_hold_geometry(&again), 4);
}

#[test]
fn r7_r8_a_second_import_names_nothing_twice_and_adds_no_drawing() {
    for fixture in ROUND_TRIPS {
        let (mut engine, bundle, result, again, kept) = round_trip(fixture, "restore");
        let taken = as_taken(&bundle, &result["dropped"]);
        let second = engine.import("copy", &bundle, json!({ "attribution": "restore" })).unwrap();
        let conflicts: Vec<String> = second["result"]["dropped"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|drop| drop["reason"] == "name-conflict")
            .map(|drop| key(&drop["ref"]))
            .collect();
        for item in taken.items() {
            if item["data"]["nm"].as_str().is_some_and(|nm| !nm.is_empty()) {
                assert!(
                    conflicts.contains(&key(&item["data"]["ref"])),
                    "{fixture}: {}",
                    item["data"]["ref"]
                );
            }
        }
        assert!(
            second["result"]["annotations"].as_array().unwrap().iter().all(|a| a["nm"].is_null()),
            "{fixture}"
        );
        let third = without(&engine.export("copy", json!({})).unwrap(), &kept);
        let ids = |e: &Exported| -> Vec<String> {
            let mut ids: Vec<String> =
                e.bundle["resources"].as_object().unwrap().keys().cloned().collect();
            ids.sort();
            ids
        };
        assert_eq!(ids(&third), ids(&again), "{fixture}");
    }
}
