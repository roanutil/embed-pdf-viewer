//! A form's fields out as a bundle and into another document
//! (`forms.export`, `forms.import`, `forms.importValues`), through the host
//! as `runFormTransferConformance`
//! (`packages/engine/core/src/conformance/runFormTransferConformance.ts`,
//! which `packages/engine/main/test/form-transfer.test.ts` runs) drives
//! them, each sent as the worker requests `LocalDocumentFormsService`
//! sends: the session's own authority, and `mayScript` as the session's
//! scope has `doc.forms.script`. And the rotation suite's form bundle case
//! (`runWidgetRotationConformance`, `a form bundle carries the turn`),
//! carried here from 8.21.

use epdf_engine::host::Host;
use serde_json::{Value, json};

/// `pdfOf`: a PDF of `objects`, numbered from 1.
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

/// `TWO_PAGES_PDF`: two empty 300 × 300 pages (3 and 4), and no form.
fn two_pages() -> Vec<u8> {
    pdf_of(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Contents 5 0 R /Resources << >> >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Contents 5 0 R /Resources << >> >>",
        "<< /Length 0 >>\nstream\n\nendstream",
    ])
}

/// `ONE_PAGE_PDF`: one empty 300 × 300 page (3), and no form.
fn one_page() -> Vec<u8> {
    pdf_of(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Contents 4 0 R /Resources << >> >>",
        "<< /Length 0 >>\nstream\n\nendstream",
    ])
}

const BASE: &[&str] = &["doc.open", "doc.render", "doc.download", "doc.forms.read"];

fn scope(extra: &[&str]) -> Vec<&'static str> {
    let mut all: Vec<&'static str> = BASE.to_vec();
    for name in extra {
        all.push(Box::leak(name.to_string().into_boxed_str()));
    }
    all
}

fn everything() -> Vec<&'static str> {
    scope(&["doc.forms.fill", "doc.forms.modify", "doc.forms.script", "doc.forms.import"])
}

/// A design import without `doc.forms.script`, stamped.
fn design() -> Vec<&'static str> {
    scope(&["doc.forms.fill", "doc.forms.modify"])
}

/// A values import, stamped.
fn fill_only() -> Vec<&'static str> {
    scope(&["doc.forms.fill"])
}

/// An open's identity, spelled as `IdentityClaims` is (`user_id`).
fn alice() -> Value {
    json!({ "user_id": "alice", "display_name": "Alice Author" })
}

fn bob() -> Value {
    json!({ "user_id": "bob", "display_name": "Bob Builder" })
}

const FIRST: u64 = 3;
const SECOND: u64 = 4;

fn page(pon: u64) -> Value {
    json!({ "kind": "objectNumber", "objectNumber": pon })
}

fn fqn(name: &str) -> Value {
    json!({ "kind": "fqn", "name": name })
}

fn script(source: &str) -> Value {
    json!({ "type": "javascript", "script": source })
}

fn wide(y: f64) -> Value {
    json!({ "x": 20, "y": y, "width": 160, "height": 24 })
}

fn small(x: f64, y: f64) -> Value {
    json!({ "x": x, "y": y, "width": 16, "height": 16 })
}

/// The field names, in the source's form order.
const SOURCE_FIELDS: &[&str] =
    &["customer.name", "agree", "plan", "country", "tags", "go", "sign", "subtotal", "total"];

/// One host, the documents it opened by name, and each one's scope.
struct Engine {
    host: Host,
    next: u64,
    scopes: std::collections::HashMap<String, Vec<&'static str>>,
}

impl Engine {
    fn new() -> Self {
        Self { host: Host::new(), next: 1, scopes: Default::default() }
    }

    fn open(&mut self, doc: &str, identity: Value, scope: Vec<&'static str>, bytes: Vec<u8>) {
        self.send(
            json!({ "kind": "open.fatMem", "effect": "open", "docId": doc, "scope": scope,
                "identity": identity }),
            vec![bytes],
        )
        .unwrap_or_else(|e| panic!("open {doc}: {e}"));
        self.scopes.insert(doc.to_owned(), scope);
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

    fn ok(&mut self, request: Value) -> Value {
        let shown = request.to_string();
        self.send(request, Vec::new()).unwrap_or_else(|e| panic!("{shown}: {e}")).0
    }

    fn create(&mut self, doc: &str, draft: Value) -> Value {
        self.ok(
            json!({ "kind": "forms.createField", "effect": "write", "docId": doc, "draft": draft }),
        )
    }

    fn set_value(&mut self, doc: &str, name: &str, value: Value) {
        self.ok(
            json!({ "kind": "forms.setValue", "effect": "write", "docId": doc, "ref": fqn(name),
            "value": value }),
        );
    }

    fn list(&mut self, doc: &str) -> Value {
        self.ok(json!({ "kind": "forms.list", "effect": "read", "docId": doc }))
    }

    fn field(&mut self, doc: &str, name: &str) -> Value {
        self.list(doc)["fields"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["name"] == name)
            .cloned()
            .unwrap_or_else(|| panic!("no field '{name}'"))
    }

    fn names(&mut self, doc: &str) -> Vec<String> {
        self.list(doc)["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["name"].as_str().unwrap().to_owned())
            .collect()
    }

    /// The calculation order, by field name.
    fn order(&mut self, doc: &str) -> Vec<String> {
        let form = self.list(doc);
        form["calculationOrder"]
            .as_array()
            .unwrap()
            .iter()
            .map(|reference| {
                form["fields"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|f| f["ref"] == *reference)
                    .map_or("?".to_owned(), |f| f["name"].as_str().unwrap().to_owned())
            })
            .collect()
    }

    fn export(&mut self, doc: &str, selection: Value) -> Result<(Value, Vec<Vec<u8>>), Value> {
        let (result, payloads) = self.send(
            json!({ "kind": "forms.export", "effect": "snapshot", "docId": doc, "selection": selection }),
            Vec::new(),
        )?;
        Ok((result["bundle"].clone(), payloads))
    }

    fn may_script(&self, doc: &str) -> bool {
        self.scopes[doc].contains(&"doc.forms.script")
    }

    /// `doc.forms.import(bundle, options)`: the whole answer.
    fn import(&mut self, doc: &str, bundle: &Value, options: Value) -> Result<Value, Value> {
        let mut request = json!({ "kind": "forms.import", "effect": "write", "docId": doc,
            "bundle": bundle, "attribution": "restore", "values": true,
            "mayScript": self.may_script(doc) });
        for (key, value) in options.as_object().unwrap() {
            request[key] = value.clone();
        }
        self.send(request, Vec::new()).map(|(result, _)| result)
    }

    /// `doc.forms.importValues(bundle, options)`: the whole answer.
    fn import_values(&mut self, doc: &str, bundle: &Value, options: Value) -> Result<Value, Value> {
        let mut request = json!({ "kind": "forms.importValues", "effect": "write", "docId": doc,
            "bundle": bundle, "attribution": "restore" });
        for (key, value) in options.as_object().unwrap() {
            request[key] = value.clone();
        }
        self.send(request, Vec::new()).map(|(result, _)| result)
    }

    fn apply(&mut self, doc: &str, change: Value) -> Result<Value, Value> {
        self.send(
            json!({ "kind": "document.apply", "effect": "write", "docId": doc, "change": change }),
            Vec::new(),
        )
        .map(|(result, _)| result)
    }
}

/// Alice's form, a field of every family on both pages: values, a script, a
/// link, a radio group across the pages, and two calculated fields, the
/// second calculated first (`build`).
fn build(engine: &mut Engine, doc: &str) {
    engine.create(
        doc,
        json!({ "family": "text", "name": "customer.name", "maxLength": 40,
        "actions": { "validate": script("validated();") },
        "widgets": [{ "page": page(FIRST), "rect": wide(240.0) }] }),
    );
    engine.set_value(doc, "customer.name", json!({ "value": "Ada" }));
    engine.create(
        doc,
        json!({ "family": "checkbox", "name": "agree",
        "widgets": [{ "page": page(FIRST), "rect": small(20.0, 200.0), "exportValue": "Yes" }] }),
    );
    engine.set_value(doc, "agree", json!({ "checked": true }));
    engine.create(
        doc,
        json!({ "family": "radio", "name": "plan", "widgets": [
        { "page": page(FIRST), "rect": small(20.0, 160.0), "exportValue": "basic" },
        { "page": page(SECOND), "rect": small(20.0, 160.0), "exportValue": "pro" }] }),
    );
    engine.set_value(doc, "plan", json!({ "value": "pro" }));
    engine.create(doc, json!({ "family": "combobox", "name": "country",
        "options": [{ "label": "Netherlands", "value": "NL" }, { "label": "Belgium", "value": "BE" }],
        "widgets": [{ "page": page(SECOND), "rect": wide(240.0) }] }));
    engine.set_value(doc, "country", json!({ "value": "BE" }));
    engine.create(doc, json!({ "family": "listbox", "name": "tags", "multiSelect": true,
        "options": [{ "label": "A", "value": "a" }, { "label": "B", "value": "b" },
            { "label": "C", "value": "c" }],
        "widgets": [{ "page": page(SECOND), "rect": { "x": 20, "y": 120, "width": 80, "height": 60 } }] }));
    engine.set_value(doc, "tags", json!({ "selectedValues": ["a", "c"] }));
    engine.create(doc, json!({ "family": "pushbutton", "name": "go", "widgets": [{
        "page": page(SECOND), "rect": { "x": 200, "y": 240, "width": 60, "height": 24 },
        "caption": "Go", "actions": { "activate": { "type": "uri", "uri": "https://example.com" } } }] }));
    engine.create(doc, json!({ "family": "signature", "name": "sign",
        "widgets": [{ "page": page(SECOND), "rect": { "x": 200, "y": 20, "width": 80, "height": 40 } }] }));
    engine.create(
        doc,
        json!({ "family": "text", "name": "subtotal",
        "actions": { "calculate": script("event.value = 1;") },
        "widgets": [{ "page": page(FIRST), "rect": wide(100.0) }] }),
    );
    engine.create(
        doc,
        json!({ "family": "text", "name": "total",
        "actions": { "calculate": script("event.value = 2;") },
        "widgets": [{ "page": page(FIRST), "rect": wide(60.0) }] }),
    );
    engine.ok(json!({ "kind": "forms.reorderCalculations", "effect": "write", "docId": doc,
        "fields": [fqn("total")], "position": "start" }));
}

/// Alice's form, exported (`sourceBundle`).
fn source_bundle(engine: &mut Engine) -> Value {
    engine.open("source", alice(), everything(), two_pages());
    build(engine, "source");
    engine.export("source", json!({})).unwrap().0
}

/// A field ref's key (`encodeFieldRefKey`).
fn field_key(reference: &Value) -> String {
    match reference["kind"].as_str() {
        Some("objectNumber") => format!("obj:{}", reference["objectNumber"]),
        _ => format!("fqn:{}", reference["name"].as_str().unwrap_or_default()),
    }
}

/// A bundle as two documents can agree on it (`comparable`): without refs,
/// object numbers, names and the attribution an import's mode writes, and
/// the calculation order by field name.
fn comparable(bundle: &Value) -> Value {
    let name_of: std::collections::HashMap<String, Value> = bundle["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| (field_key(&row["data"]["ref"]), row["data"]["name"].clone()))
        .collect();
    fn strip(value: &Value, key: Option<&str>) -> Value {
        const DROP: &[&str] = &[
            "ref",
            "objectNumber",
            "nm",
            "field",
            "createdBy",
            "createdAt",
            "filledBy",
            "filledByName",
            "filledAt",
            "importedBy",
        ];
        if key == Some("page") {
            return value.clone();
        }
        match value {
            Value::Array(items) => Value::Array(items.iter().map(|v| strip(v, None)).collect()),
            Value::Object(fields) => Value::Object(
                fields
                    .iter()
                    .filter(|(k, _)| !DROP.contains(&k.as_str()))
                    .map(|(k, v)| (k.clone(), strip(v, Some(k))))
                    .collect(),
            ),
            other => other.clone(),
        }
    }
    test_support::canonical_numbers(&json!({
        "pages": bundle["pages"],
        "fields": bundle["fields"].as_array().unwrap().iter().map(|row| strip(&row["data"], None)).collect::<Vec<_>>(),
        "widgets": bundle["widgets"].as_array().unwrap().iter().map(|row| strip(&row["data"], None)).collect::<Vec<_>>(),
        "calculationOrder": bundle["calculationOrder"].as_array().unwrap().iter()
            .map(|reference| name_of.get(&field_key(reference)).cloned().unwrap_or(Value::Null))
            .collect::<Vec<_>>(),
    }))
}

fn names_of(result: &Value) -> Vec<String> {
    result["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap().to_owned())
        .collect()
}

fn is_permission_refusal(error: &Value) -> bool {
    matches!(error["code"].as_str(), Some("Forbidden" | "ProtectedDocument"))
}

#[test]
fn a_design_import_copies_every_field_and_the_copy_exports_as_its_source() {
    let mut engine = Engine::new();
    let bundle = source_bundle(&mut engine);
    let names: Vec<&Value> =
        bundle["fields"].as_array().unwrap().iter().map(|row| &row["data"]["name"]).collect();
    assert_eq!(
        names,
        SOURCE_FIELDS.iter().map(|n| json!(n)).collect::<Vec<_>>().iter().collect::<Vec<_>>()
    );

    engine.open("target", alice(), everything(), two_pages());
    let answer = engine.import("target", &bundle, json!({})).unwrap();
    let result = &answer["result"];
    assert_eq!(result["dropped"], json!([]));
    assert_eq!(result["meta"]["undoable"], true);
    assert_eq!(names_of(result), SOURCE_FIELDS);
    let from: Vec<&Value> =
        result["refMap"].as_array().unwrap().iter().map(|p| &p["from"]).collect();
    let sources: Vec<&Value> =
        bundle["fields"].as_array().unwrap().iter().map(|row| &row["data"]["ref"]).collect();
    assert_eq!(from, sources);
    let to: Vec<&Value> = result["refMap"].as_array().unwrap().iter().map(|p| &p["to"]).collect();
    let made: Vec<&Value> =
        result["fields"].as_array().unwrap().iter().map(|f| &f["ref"]).collect();
    assert_eq!(to, made);
    let again = engine.export("target", json!({})).unwrap().0;
    assert_eq!(comparable(&again), comparable(&bundle));
    assert_eq!(engine.order("target"), ["total", "subtotal"]);
}

#[test]
fn an_export_takes_whole_fields_by_name_or_every_field_with_a_widget_on_a_page() {
    let mut engine = Engine::new();
    engine.open("source", alice(), everything(), two_pages());
    build(&mut engine, "source");
    let named =
        engine.export("source", json!({ "fields": [fqn("plan"), fqn("agree")] })).unwrap().0;
    // In the form's order, each with every widget it has, wherever it is.
    let names: Vec<&Value> =
        named["fields"].as_array().unwrap().iter().map(|r| &r["data"]["name"]).collect();
    assert_eq!(names, [&json!("agree"), &json!("plan")]);
    assert_eq!(named["widgets"].as_array().unwrap().len(), 3);
    let pages: Vec<&Value> =
        named["pages"].as_array().unwrap().iter().map(|e| &e["page"]).collect();
    assert_eq!(pages, [&page(FIRST), &page(SECOND)]);

    let on_second = engine.export("source", json!({ "pages": [page(SECOND)] })).unwrap().0;
    let names: Vec<&str> = on_second["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["data"]["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["plan", "country", "tags", "go", "sign"]);
    let missing = engine.export("source", json!({ "fields": [fqn("nothing")] })).unwrap_err();
    assert_eq!(missing["code"], "NotFound", "{missing}");
}

#[test]
fn restore_writes_the_sources_attribution_and_the_importer_stamp_makes_the_fields_the_sessions() {
    let mut engine = Engine::new();
    let bundle = source_bundle(&mut engine);

    engine.open("restored", bob(), everything(), two_pages());
    engine.import("restored", &bundle, json!({})).unwrap();
    let copied = engine.field("restored", "customer.name");
    assert_eq!(copied["createdBy"], "alice");
    assert_eq!(copied["filledBy"], "alice");
    assert_eq!(copied["filledByName"], "Alice Author");
    assert_eq!(copied["importedBy"], "bob");
    let source = bundle["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["data"]["name"] == "customer.name")
        .unwrap();
    assert_eq!(copied["createdAt"], source["data"]["createdAt"]);
    assert_eq!(copied["filledAt"], source["data"]["filledAt"]);
    let go = engine.field("restored", "go");
    assert_eq!((go["createdBy"].clone(), go["filledBy"].clone()), (json!("alice"), Value::Null));

    engine.open("stamped", bob(), everything(), two_pages());
    engine.import("stamped", &bundle, json!({ "attribution": "stamp" })).unwrap();
    let stamped = engine.field("stamped", "customer.name");
    assert_eq!(stamped["createdBy"], "bob");
    assert_eq!(stamped["filledBy"], "bob");
    assert_eq!(stamped["filledByName"], "Bob Builder");
    assert_eq!(stamped["importedBy"], Value::Null);
}

#[test]
fn restoring_needs_doc_forms_import_and_stamping_only_what_a_create_needs() {
    let mut engine = Engine::new();
    let bundle = source_bundle(&mut engine);
    let mut scope = design();
    scope.push("doc.forms.script");
    engine.open("target", bob(), scope, two_pages());
    let refused = engine.import("target", &bundle, json!({})).unwrap_err();
    assert!(is_permission_refusal(&refused), "{refused}");
    let refused = engine.import_values("target", &bundle, json!({})).unwrap_err();
    assert!(is_permission_refusal(&refused), "{refused}");
    assert!(engine.names("target").is_empty());

    let stamped = engine.import("target", &bundle, json!({ "attribution": "stamp" })).unwrap();
    assert_eq!(stamped["result"]["fields"].as_array().unwrap().len(), SOURCE_FIELDS.len());

    engine.open("filler", bob(), fill_only(), two_pages());
    let refused = engine.import("filler", &bundle, json!({ "attribution": "stamp" })).unwrap_err();
    assert!(is_permission_refusal(&refused), "{refused}");
}

#[test]
fn without_doc_forms_script_scripts_submits_and_links_are_left_out_and_the_rest_is_copied() {
    let mut engine = Engine::new();
    let bundle = source_bundle(&mut engine);
    engine.open("target", bob(), design(), two_pages());
    let result =
        engine.import("target", &bundle, json!({ "attribution": "stamp" })).unwrap()["result"]
            .clone();
    assert_eq!(names_of(&result), SOURCE_FIELDS);
    let name_of = |reference: &Value| -> Value {
        bundle["fields"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| field_key(&row["data"]["ref"]) == field_key(reference))
            .map_or(Value::Null, |row| row["data"]["name"].clone())
    };
    let left: Vec<Value> = result["dropped"]
        .as_array()
        .unwrap()
        .iter()
        .map(|drop| {
            json!({ "name": name_of(&drop["ref"]), "widget": drop.get("widget").is_some(),
            "field": drop["field"], "reason": drop["reason"] })
        })
        .collect();
    assert_eq!(
        left,
        [
            json!({ "name": "customer.name", "widget": false, "field": "actions.validate",
                "reason": "script-not-allowed" }),
            json!({ "name": "go", "widget": true, "field": "actions.activate", "reason": "script-not-allowed" }),
            json!({ "name": "subtotal", "widget": false, "field": "actions.calculate",
                "reason": "script-not-allowed" }),
            json!({ "name": "total", "widget": false, "field": "actions.calculate",
                "reason": "script-not-allowed" }),
        ]
    );
    let copied = engine.field("target", "customer.name");
    assert!(copied["actions"]["validate"].is_null(), "{copied}");
    // Without their calculate scripts, the copies have no place in the order.
    assert!(engine.order("target").is_empty());
}

#[test]
fn pages_map_as_they_are_by_default_or_as_a_list_says_and_a_page_that_maps_nowhere_refuses() {
    let mut engine = Engine::new();
    let bundle = source_bundle(&mut engine);
    engine.open("swapped", alice(), everything(), two_pages());
    let result = engine
        .import("swapped", &bundle, json!({ "pages": [
            { "from": page(FIRST), "to": page(SECOND) }, { "from": page(SECOND), "to": page(FIRST) }] }))
        .unwrap()["result"]
        .clone();
    let plan = result["fields"].as_array().unwrap().iter().find(|f| f["name"] == "plan").unwrap();
    let pages: Vec<&Value> =
        plan["widgets"].as_array().unwrap().iter().map(|w| &w["page"]).collect();
    assert_eq!(pages, [&page(SECOND), &page(FIRST)]);

    engine.open("short", alice(), everything(), one_page());
    let refused = engine.import("short", &bundle, json!({})).unwrap_err();
    assert_eq!(refused["code"], "InvalidArg", "{refused}");
    assert!(engine.names("short").is_empty());
}

#[test]
fn a_field_whose_name_is_taken_is_left_out_and_so_is_one_that_would_be_its_parent_or_child() {
    let mut engine = Engine::new();
    let bundle = source_bundle(&mut engine);
    engine.open("target", alice(), everything(), two_pages());
    for name in ["agree", "customer", "go.now"] {
        engine.create("target", json!({ "family": "text", "name": name }));
    }
    let result = engine.import("target", &bundle, json!({})).unwrap()["result"].clone();
    let names: std::collections::HashMap<String, Value> = bundle["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| (field_key(&row["data"]["ref"]), row["data"]["name"].clone()))
        .collect();
    let conflicts: Vec<Value> = result["dropped"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|drop| drop["reason"] == "name-conflict")
        .map(|drop| names[&field_key(&drop["ref"])].clone())
        .collect();
    assert_eq!(conflicts, ["customer.name", "agree", "go"]);
    let kept: Vec<&str> = SOURCE_FIELDS
        .iter()
        .copied()
        .filter(|n| !["customer.name", "agree", "go"].contains(n))
        .collect();
    assert_eq!(names_of(&result), kept);
}

#[test]
fn the_calculation_order_of_the_copies_follows_the_forms_own_as_the_bundle_has_it() {
    let mut engine = Engine::new();
    let bundle = source_bundle(&mut engine);
    engine.open("target", alice(), everything(), two_pages());
    engine.create(
        "target",
        json!({ "family": "text", "name": "own",
        "actions": { "calculate": script("event.value = 0;") } }),
    );
    let result = engine.import("target", &bundle, json!({})).unwrap()["result"].clone();
    assert_eq!(engine.order("target"), ["own", "total", "subtotal"]);
    assert_eq!(result["calculationOrder"].as_array().unwrap().len(), 3);
}

#[test]
fn a_signature_never_travels_and_a_field_the_form_does_not_export_goes_without_its_value() {
    let mut engine = Engine::new();
    let bundle = source_bundle(&mut engine);
    let sign =
        bundle["fields"].as_array().unwrap().iter().find(|r| r["data"]["name"] == "sign").unwrap();
    assert_eq!(sign["data"]["valueEntry"], json!({ "kind": "none" }));
    engine.ok(json!({ "kind": "forms.updateField", "effect": "write", "docId": "source",
        "ref": fqn("customer.name"), "patch": { "noExport": true } }));
    let exported = engine.export("source", json!({ "fields": [fqn("customer.name")] })).unwrap().0;
    let name = &exported["fields"][0]["data"];
    assert_eq!(name["value"], "");
    assert_eq!(name["valueEntry"], json!({ "kind": "none" }));
    assert_eq!((name["filledBy"].clone(), name["filledAt"].clone()), (Value::Null, Value::Null));
}

#[test]
fn a_values_import_fills_the_fields_of_the_same_name_and_reports_what_it_leaves_out() {
    let mut engine = Engine::new();
    let bundle = source_bundle(&mut engine);
    engine.open("target", bob(), everything(), two_pages());
    engine.import("target", &bundle, json!({ "attribution": "stamp", "values": false })).unwrap();
    assert_eq!(engine.field("target", "customer.name")["valueEntry"], json!({ "kind": "none" }));

    let row = |name: &str| -> Value {
        bundle["fields"].as_array().unwrap().iter().find(|r| r["data"]["name"] == name).unwrap()["data"].clone()
    };
    let renamed = |mut data: Value, name: &str| {
        data["name"] = json!(name);
        data
    };
    let mut country = row("country");
    country["value"] = json!("FR");
    country["valueEntry"] = json!({ "kind": "scalar", "value": "FR" });
    let mut values = bundle.clone();
    values["fields"] = json!([
        { "data": row("customer.name") },
        // No field of that name.
        { "data": renamed(row("agree"), "missing") },
        // A text value for the checkbox.
        { "data": renamed(row("customer.name"), "agree") },
        // An option the dropdown doesn't have.
        { "data": country },
        { "data": row("tags") },
    ]);
    let result = engine
        .import_values("target", &values, json!({ "attribution": "restore" }))
        .unwrap()["result"]
        .clone();
    assert_eq!(names_of(&result), ["customer.name", "tags"]);
    let reasons: Vec<&Value> =
        result["dropped"].as_array().unwrap().iter().map(|d| &d["reason"]).collect();
    assert_eq!(reasons, [&json!("no-field"), &json!("wrong-family"), &json!("value-not-allowed")]);
    let name = engine.field("target", "customer.name");
    assert_eq!(
        (name["value"].clone(), name["filledBy"].clone(), name["importedBy"].clone()),
        (json!("Ada"), json!("alice"), json!("bob"))
    );
    assert_eq!(engine.field("target", "tags")["selectedValues"], json!(["a", "c"]));
    assert_eq!(engine.field("target", "country")["value"], "");
}

#[test]
fn a_design_import_is_undone_and_redone_with_fields_widgets_and_calculation_order_back_exactly() {
    let mut engine = Engine::new();
    let bundle = source_bundle(&mut engine);
    engine.open("target", alice(), everything(), two_pages());
    engine.create(
        "target",
        json!({ "family": "text", "name": "own",
        "actions": { "calculate": script("event.value = 0;") } }),
    );
    let before = engine.list("target");
    let imported = engine.import("target", &bundle, json!({})).unwrap()["result"].clone();
    let after = engine.list("target");

    let undo = engine.apply("target", json!({ "undoOf": imported["meta"]["opId"] })).unwrap();
    let types: Vec<&Value> =
        undo["result"]["items"].as_array().unwrap().iter().map(|i| &i["type"]).collect();
    assert_eq!(types, vec![&json!("forms.delete"); SOURCE_FIELDS.len()]);
    assert_eq!(engine.list("target"), before);

    engine.apply("target", json!({ "undoOf": undo["result"]["meta"]["opId"] })).unwrap();
    let redone = engine.list("target");
    assert_eq!(redone["fields"], after["fields"]);
    assert_eq!(redone["calculationOrder"], after["calculationOrder"]);
    assert_eq!(engine.order("target"), ["own", "total", "subtotal"]);
}

#[test]
fn a_values_import_is_undone_and_redone_with_values_and_who_filled_them_back() {
    let mut engine = Engine::new();
    let bundle = source_bundle(&mut engine);
    engine.open("target", bob(), everything(), two_pages());
    engine.import("target", &bundle, json!({ "attribution": "stamp", "values": false })).unwrap();
    engine.set_value("target", "customer.name", json!({ "value": "Bob" }));

    let filled = engine.import_values("target", &bundle, json!({})).unwrap()["result"].clone();
    assert_eq!(filled["meta"]["undoable"], true);
    let name = engine.field("target", "customer.name");
    assert_eq!((name["value"].clone(), name["filledBy"].clone()), (json!("Ada"), json!("alice")));

    let undo = engine.apply("target", json!({ "undoOf": filled["meta"]["opId"] })).unwrap();
    let name = engine.field("target", "customer.name");
    assert_eq!((name["value"].clone(), name["filledBy"].clone()), (json!("Bob"), json!("bob")));
    assert_eq!(engine.field("target", "agree")["valueEntry"], json!({ "kind": "none" }));

    engine.apply("target", json!({ "undoOf": undo["result"]["meta"]["opId"] })).unwrap();
    let name = engine.field("target", "customer.name");
    assert_eq!(
        (name["value"].clone(), name["filledBy"].clone(), name["importedBy"].clone()),
        (json!("Ada"), json!("alice"), json!("bob"))
    );
    assert_eq!(engine.field("target", "agree")["checked"], true);
}

#[test]
fn an_imported_field_changed_since_is_left_alone_by_the_undo_and_the_rest_goes() {
    let mut engine = Engine::new();
    let bundle = source_bundle(&mut engine);
    engine.open("target", alice(), everything(), two_pages());
    let imported = engine.import("target", &bundle, json!({})).unwrap()["result"].clone();
    engine.ok(json!({ "kind": "forms.updateField", "effect": "write", "docId": "target",
        "ref": fqn("country"), "patch": { "required": true } }));
    let undo = engine.apply("target", json!({ "undoOf": imported["meta"]["opId"] })).unwrap();
    let skipped = undo["result"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["type"] == "skipped")
        .count();
    assert_eq!(skipped, 1);
    assert_eq!(engine.names("target"), ["country"]);
}

#[test]
fn a_retry_under_the_same_op_id_answers_again_and_writes_nothing_more() {
    let mut engine = Engine::new();
    let bundle = source_bundle(&mut engine);
    engine.open("target", alice(), everything(), two_pages());
    let first = engine.import("target", &bundle, json!({ "opId": "form-import-retry" })).unwrap();
    let again = engine.import("target", &bundle, json!({ "opId": "form-import-retry" })).unwrap();
    assert_eq!(again["result"]["refMap"], first["result"]["refMap"]);
    assert_eq!(engine.names("target"), SOURCE_FIELDS);
    let values =
        engine.import_values("target", &bundle, json!({ "opId": "form-values-retry" })).unwrap();
    let values_again =
        engine.import_values("target", &bundle, json!({ "opId": "form-values-retry" })).unwrap();
    assert_eq!(values_again["result"]["fields"], values["result"]["fields"]);
}

#[test]
fn each_import_is_one_transaction_one_event_per_field_sharing_its_op_id() {
    let mut engine = Engine::new();
    let bundle = source_bundle(&mut engine);
    engine.open("target", alice(), everything(), two_pages());
    let imported = engine.import("target", &bundle, json!({ "opId": "form-import-1" })).unwrap();
    let created: Vec<&Value> = imported["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["type"] == "forms.created")
        .collect();
    assert_eq!(created.len(), SOURCE_FIELDS.len());
    for (index, event) in created.iter().enumerate() {
        assert_eq!(event["field"], imported["result"]["fields"][index]);
        assert_eq!(
            event["origin"]["tx"],
            json!({ "id": "form-import-1", "index": index, "count": SOURCE_FIELDS.len() })
        );
    }

    engine.set_value("target", "customer.name", json!({ "value": "Changed" }));
    engine.set_value("target", "agree", json!({ "checked": false }));
    let filled =
        engine.import_values("target", &bundle, json!({ "opId": "form-values-1" })).unwrap();
    let set: Vec<&Value> = filled["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["type"] == "forms.valueSet")
        .map(|e| &e["field"]["name"])
        .collect();
    assert_eq!(names_of(&filled["result"]), ["customer.name", "agree"]);
    assert_eq!(set, [&json!("customer.name"), &json!("agree")]);
}

#[test]
fn refuses_before_writing_a_malformed_bundle_and_document_apply_takes_no_import() {
    let mut engine = Engine::new();
    let bundle = source_bundle(&mut engine);
    engine.open("target", alice(), everything(), two_pages());
    let mut broken = bundle.clone();
    broken["format"] = json!("embedpdf/annotations");
    let refused = engine.import("target", &broken, json!({})).unwrap_err();
    assert_eq!(refused["code"], "InvalidArg", "{refused}");
    let mut stray = bundle.clone();
    stray["fields"] = json!(bundle["fields"].as_array().unwrap()[1..]);
    let result =
        engine.import("target", &stray, json!({ "attribution": "stamp" })).unwrap()["result"]
            .clone();
    let no_field =
        result["dropped"].as_array().unwrap().iter().filter(|d| d["reason"] == "no-field").count();
    assert_eq!(no_field, 1);

    let refused = engine
        .apply("target", json!({ "ops": [{ "type": "forms.import", "bundle": bundle, "attribution": "restore" }] }))
        .unwrap_err();
    assert_eq!(refused["code"], "InvalidArg", "{refused}");
}

// ── `runWidgetRotationConformance`: a form bundle carries the turn ─────────

fn text_field(name: &str, rect: &str, mk: &str, page: u32) -> String {
    format!(
        "<< /Type /Annot /Subtype /Widget /FT /Tx /T ({name}) /V ({name}) /Rect [{rect}] {mk} /P {page} 0 R >>"
    )
}

/// `TURNED_FIELDS_PDF`: two 300 × 300 pages, the second shown turned 90°,
/// and text fields with `/MK /R` as Acrobat writes it, counterclockwise.
fn turned_fields() -> Vec<u8> {
    let fields = [
        text_field("upright", "20 260 180 284", "", 3),
        text_field("r90", "20 20 44 180", "/MK << /R 90 >>", 3),
        text_field("r180", "60 220 220 244", "/MK << /R 180 >>", 3),
        text_field("r270", "250 20 274 180", "/MK << /R 270 >>", 3),
        text_field("odd", "60 180 220 204", "/MK << /R 45 >>", 3),
        text_field("negative", "200 20 224 180", "/MK << /R -90 >>", 3),
        text_field("on-turned", "20 20 44 180", "/MK << /R 90 >>", 4),
    ];
    let mut objects: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [6 0 R 7 0 R 8 0 R 9 0 R 10 0 R 11 0 R 12 0 R] /DA (/Helv 10 Tf 0 g) /DR << /Font << /Helv << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >> >> >>".into(),
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Contents 5 0 R /Resources << >> /Annots [6 0 R 7 0 R 8 0 R 9 0 R 10 0 R 11 0 R] >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Rotate 90 /Contents 5 0 R /Resources << >> /Annots [12 0 R] >>".into(),
        "<< /Length 0 >>\nstream\n\nendstream".into(),
    ];
    objects.extend(fields);
    let refs: Vec<&str> = objects.iter().map(String::as_str).collect();
    pdf_of(&refs)
}

#[test]
fn a_form_bundle_carries_the_turn() {
    let mut engine = Engine::new();
    engine.open("source", json!({}), vec!["*"], turned_fields());
    let bundle = engine.export("source", json!({ "pages": [page(FIRST)] })).unwrap().0;
    engine.open("copy", json!({}), vec!["*"], two_pages());
    engine.import("copy", &bundle, json!({ "attribution": "stamp", "mayScript": true })).unwrap();
    let form = engine.list("copy");
    for name in ["upright", "r90", "r180", "r270"] {
        let first = bundle["fields"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["data"]["name"] == name)
            .unwrap()["data"]["widgets"][0]["ref"]
            .clone();
        let source = bundle["widgets"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["data"]["ref"] == first)
            .unwrap()["data"]
            .clone();
        let field = form["fields"].as_array().unwrap().iter().find(|f| f["name"] == name).unwrap();
        let reference = &field["widgets"][0]["ref"];
        let row =
            form["widgets"].as_array().unwrap().iter().find(|w| w["ref"] == *reference).unwrap();
        assert_eq!(row["rotation"], source["rotation"], "{name}");
        for edge in ["x", "y", "width", "height"] {
            let (a, b) =
                (row["rect"][edge].as_f64().unwrap(), source["rect"][edge].as_f64().unwrap());
            assert!((a - b).abs() < 0.01, "{name} {edge}: {a} vs {b}");
        }
    }
}
