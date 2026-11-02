//! Form fields through `pages.insert` and `pages.extract`, against
//! upstream's `runPageFormFieldsConformance` (`core/src/conformance/`,
//! through `main/test/page-form-fields.test.ts`) over the same two files:
//! the pages bring the fields of their widgets, written into the document's
//! `/AcroForm`, and a top-level field whose name the document already uses
//! comes in as `name_2`.
//!
//! The form is `form_page_import.pdf`: on page 1, `person.name` (Ann) and
//! the monthly button of the radio group `plan`; on page 2, `person.email`,
//! the yearly button (on) and the check box `agree` (checked). Its field
//! `hidden` has no widget, so no page brings it.

use std::collections::BTreeMap;

use epdf_engine::host::Host;
use serde_json::{Value, json};

const FORM: &str = "packages/engine/main/test/fixtures/form_page_import.pdf";
const TARGET: &str = "packages/engine/main/test/fixtures/hello_world.pdf";

fn open(bytes: Vec<u8>) -> Host {
    let mut host = Host::new();
    let response = host.call_filled(
        &json!({ "kind": "open.fatMem", "jobId": 1, "docId": "d" }).to_string(),
        vec![bytes],
    );
    assert!(!response.json.contains("\"error\""), "{}", response.json);
    host
}

/// `request` to the document: its result and payloads, or its error.
fn send(
    host: &mut Host,
    mut request: Value,
    payloads: Vec<Vec<u8>>,
) -> Result<(Value, Vec<Vec<u8>>), Value> {
    request["jobId"] = json!(7);
    request["docId"] = json!("d");
    let response = host.call_filled(&request.to_string(), payloads);
    let mut envelope: Value = serde_json::from_str(&response.json).expect("JSON");
    match envelope.get_mut("error") {
        Some(error) => Err(error.take()),
        None => Ok((envelope["result"].take(), response.payloads)),
    }
}

fn insert(host: &mut Host, op_id: &str, bytes: Vec<u8>) -> Value {
    let (answer, _) = send(
        host,
        json!({ "kind": "document.apply", "opId": op_id,
            "change": { "ops": [{ "type": "pages.insert", "source": { "payload": 0 } }] } }),
        vec![bytes],
    )
    .unwrap_or_else(|e| panic!("{op_id}: {e}"));
    answer
}

fn forms(host: &mut Host) -> Vec<Value> {
    let (answer, _) = send(host, json!({ "kind": "forms.list" }), Vec::new()).expect("forms.list");
    answer["fields"].as_array().expect("fields").clone()
}

/// Each field by its full name: its value and how many widgets it has.
fn fields_of(host: &mut Host) -> BTreeMap<String, (Value, usize)> {
    forms(host)
        .iter()
        .map(|field| {
            let value = match field["family"].as_str() {
                Some("text" | "radio") => field["value"].clone(),
                Some("checkbox") => field["checked"].clone(),
                _ => json!(""),
            };
            let widgets = field["widgets"].as_array().map_or(0, Vec::len);
            (field["name"].as_str().unwrap().to_owned(), (value, widgets))
        })
        .collect()
}

fn form_fields() -> BTreeMap<String, (Value, usize)> {
    BTreeMap::from([
        ("person.name".to_owned(), (json!("Ann"), 1)),
        ("person.email".to_owned(), (json!("ann@example.com"), 1)),
        ("plan".to_owned(), (json!("yearly"), 2)),
        ("agree".to_owned(), (json!(true), 1)),
    ])
}

fn save(host: &mut Host, mode: &str) -> Vec<u8> {
    let (_, mut payloads) =
        send(host, json!({ "kind": "document.saveBuffer", "mode": mode }), Vec::new())
            .expect("save");
    payloads.remove(0)
}

#[test]
fn an_insert_brings_the_form_fields_of_the_inserted_pages() {
    let mut host = open(test_support::fixture(TARGET));
    let before = send(&mut host, json!({ "kind": "pages.list" }), Vec::new()).unwrap().0;
    let before = before["pageCount"].as_u64().unwrap() as usize;
    insert(&mut host, "insert", test_support::fixture(FORM));
    assert_eq!(fields_of(&mut host), form_fields());

    // Every widget is on an inserted page.
    let listing = send(&mut host, json!({ "kind": "pages.list" }), Vec::new()).unwrap().0;
    let inserted: Vec<Value> = listing["pages"].as_array().unwrap()[before..]
        .iter()
        .map(|page| page["ref"]["objectNumber"].clone())
        .collect();
    for field in forms(&mut host) {
        for widget in field["widgets"].as_array().unwrap() {
            assert!(inserted.contains(&widget["page"]["objectNumber"]), "{widget}");
        }
    }
}

#[test]
fn the_inserted_fields_read_the_same_after_a_save_in_either_mode() {
    for mode in ["rewrite", "incremental"] {
        let mut host = open(test_support::fixture(TARGET));
        insert(&mut host, "insert", test_support::fixture(FORM));
        let bytes = save(&mut host, mode);
        // Other PDF apps find the fields through /AcroForm.
        assert!(bytes.windows(9).any(|w| w == b"/AcroForm"), "{mode}");
        assert_eq!(fields_of(&mut open(bytes)), form_fields(), "{mode}");
    }
}

#[test]
fn fields_named_as_the_document_names_its_own_come_in_as_name_2_kept_apart() {
    let mut host = open(test_support::fixture(TARGET));
    insert(&mut host, "first", test_support::fixture(FORM));
    insert(&mut host, "second", test_support::fixture(FORM));
    let mut expected = form_fields();
    expected.insert("person_2.name".to_owned(), expected["person.name"].clone());
    expected.insert("person_2.email".to_owned(), expected["person.email"].clone());
    expected.insert("plan_2".to_owned(), expected["plan"].clone());
    expected.insert("agree_2".to_owned(), expected["agree"].clone());
    assert_eq!(fields_of(&mut host), expected);

    // Filling one copy leaves the other as it was.
    let name = forms(&mut host).into_iter().find(|field| field["name"] == "person.name").unwrap();
    send(
        &mut host,
        json!({ "kind": "forms.setValue", "opId": "fill", "ref": name["ref"],
            "value": { "value": "Bea" } }),
        Vec::new(),
    )
    .expect("setValue");
    let after = fields_of(&mut host);
    assert_eq!(after["person.name"].0, json!("Bea"));
    assert_eq!(after["person_2.name"].0, json!("Ann"));

    let bytes = save(&mut host, "rewrite");
    assert_eq!(fields_of(&mut open(bytes)), after);
}

#[test]
fn an_extract_brings_only_the_fields_of_the_extracted_pages() {
    let mut form = open(test_support::fixture(FORM));
    let listing = send(&mut form, json!({ "kind": "pages.list" }), Vec::new()).unwrap().0;
    let second = listing["pages"][1]["ref"].clone();
    let (_, mut payloads) =
        send(&mut form, json!({ "kind": "pages.extract", "pages": [second] }), Vec::new())
            .expect("extract");
    let mut host = open(test_support::fixture(TARGET));
    insert(&mut host, "insert", payloads.remove(0));
    assert_eq!(
        fields_of(&mut host),
        BTreeMap::from([
            ("person.email".to_owned(), (json!("ann@example.com"), 1)),
            ("plan".to_owned(), (json!("yearly"), 1)),
            ("agree".to_owned(), (json!(true), 1)),
        ])
    );
}

#[test]
fn an_insert_at_chosen_numbers_puts_each_page_at_its_number() {
    let mut host = Host::new();
    let response = host.call_filled(
        &json!({ "kind": "open.fatMem", "jobId": 1, "docId": "d",
            "reserveObjectNumbers": 16 })
        .to_string(),
        vec![test_support::fixture(TARGET)],
    );
    assert!(!response.json.contains("\"error\""), "{}", response.json);
    let opened: Value = serde_json::from_str(&response.json).unwrap();
    let first = opened["result"]["objectNumbers"]["first"].as_u64().expect("a reserved range");
    let numbers = [first, first + 1];
    let (answer, _) = send(
        &mut host,
        json!({ "kind": "document.apply", "opId": "numbered",
            "change": { "ops": [{ "type": "pages.insert", "source": { "payload": 0 },
                "objectNumbers": numbers }] } }),
        vec![test_support::fixture(FORM)],
    )
    .expect("a numbered insert");
    let inserted: Vec<u64> = answer["result"]["items"][0]["insertedPages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|page| page["objectNumber"].as_u64().unwrap())
        .collect();
    assert_eq!(inserted, numbers);

    let refused = send(
        &mut host,
        json!({ "kind": "document.apply", "opId": "short",
            "change": { "ops": [{ "type": "pages.insert", "source": { "payload": 0 },
                "objectNumbers": [first + 2] }] } }),
        vec![test_support::fixture(FORM)],
    )
    .expect_err("one number for two pages");
    assert_eq!(refused["code"], "InvalidArg", "{refused}");
    assert_eq!(refused["details"]["field"], "objectNumbers", "{refused}");
}
