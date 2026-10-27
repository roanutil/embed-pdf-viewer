//! `pages.setName` and `pages.removeName`, and the registrations
//! `pages.list` reports (`PagesMutator.ts`, `PagesReader.ts`).

use epdf_engine::host::Host;
use serde_json::{Value, json};

mod common;
use common::{call, open_fixture, page_ref};

const REPORT: &str = "pdfium-multiplatform-prototype/fixtures/report.pdf";

fn error_code(host: &mut Host, request: Value) -> Value {
    let response = host.call_filled(&request.to_string(), Vec::new());
    let envelope: Value = serde_json::from_str(&response.json).unwrap();
    envelope["error"]["code"].clone()
}

#[test]
fn a_plain_pdf_lists_an_empty_registry() {
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, REPORT);
    let list = call(&mut host, json!({ "kind": "pages.list", "docId": doc }));
    assert_eq!(list["namedPages"], json!([]), "{list}");
}

#[test]
fn a_set_name_is_in_the_next_page_list() {
    let mut host = Host::new();
    let (doc, pon) = open_fixture(&mut host, REPORT);
    let set = call(
        &mut host,
        json!({ "kind": "pages.setName", "docId": doc, "name": "approved=Approved", "page": page_ref(pon) }),
    );
    let named = json!([{ "name": "approved=Approved", "target": { "kind": "page", "page": page_ref(pon) } }]);
    assert_eq!(set["result"]["layout"]["namedPages"], named);
    let list = call(&mut host, json!({ "kind": "pages.list", "docId": doc }));
    assert_eq!(list["namedPages"], named);
}

#[test]
fn a_name_change_publishes_pages_named() {
    let mut host = Host::new();
    let (doc, pon) = open_fixture(&mut host, REPORT);
    let response = host.call_filled(
        &json!({ "kind": "pages.setName", "jobId": 1, "docId": doc, "opId": "name-1", "name": "a=A",
            "page": page_ref(pon) })
            .to_string(),
        Vec::new(),
    );
    let envelope: Value = serde_json::from_str(&response.json).unwrap();
    assert_eq!(envelope["result"]["event"]["type"], json!("pages.named"), "{}", envelope["error"]);
    assert_eq!(envelope["result"]["event"]["name"], json!("a=A"));
    assert_eq!(envelope["result"]["event"]["page"], page_ref(pon));
    // The one fact of its write (`publishWrite`).
    assert_eq!(
        envelope["result"]["event"]["origin"]["tx"],
        json!({ "id": "name-1", "index": 0, "count": 1 })
    );
    let response = host.call_filled(
        &json!({ "kind": "pages.removeName", "jobId": 2, "docId": doc, "opId": "name-2",
            "name": "a=A" })
        .to_string(),
        Vec::new(),
    );
    let envelope: Value = serde_json::from_str(&response.json).unwrap();
    assert_eq!(envelope["result"]["event"]["type"], json!("pages.named"), "{}", envelope["error"]);
    assert_eq!(envelope["result"]["event"]["page"], Value::Null);
    assert_eq!(envelope["result"]["event"]["origin"]["tx"]["id"], json!("name-2"));
    assert_eq!(envelope["result"]["result"]["layout"]["namedPages"], json!([]));
}

#[test]
fn removing_a_missing_name_is_not_found() {
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, REPORT);
    let code = error_code(
        &mut host,
        json!({ "kind": "pages.removeName", "jobId": 1, "docId": doc, "name": "nope" }),
    );
    assert_eq!(code, json!("NotFound"));
}

#[test]
fn an_empty_name_is_invalid() {
    let mut host = Host::new();
    let (doc, pon) = open_fixture(&mut host, REPORT);
    let set = json!({ "kind": "pages.setName", "jobId": 1, "docId": doc, "name": "", "page": page_ref(pon) });
    assert_eq!(error_code(&mut host, set), json!("InvalidArg"));
    let remove = json!({ "kind": "pages.removeName", "jobId": 2, "docId": doc, "name": "" });
    assert_eq!(error_code(&mut host, remove), json!("InvalidArg"));
}

#[test]
fn an_unknown_page_is_not_found() {
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, REPORT);
    let set = json!({ "kind": "pages.setName", "jobId": 1, "docId": doc, "name": "a=A", "page": page_ref(999_999u64) });
    assert_eq!(error_code(&mut host, set), json!("NotFound"));
}

#[test]
fn replace_moves_a_name() {
    let mut host = Host::new();
    let (doc, pon) = open_fixture(&mut host, REPORT);
    call(
        &mut host,
        json!({ "kind": "pages.setName", "docId": doc, "name": "a=A", "page": page_ref(pon) }),
    );
    call(
        &mut host,
        json!({ "kind": "pages.setName", "docId": doc, "name": "a=B", "page": page_ref(pon), "replace": "a=A" }),
    );
    let list = call(&mut host, json!({ "kind": "pages.list", "docId": doc }));
    assert_eq!(list["namedPages"].as_array().unwrap().len(), 1);
    assert_eq!(list["namedPages"][0]["name"], json!("a=B"));
}

// `runNamedPagesConformance` (`core/src/conformance/runNamedPagesConformance.ts`),
// over the same multi-page `sample.pdf`.

const SAMPLE: &str = "examples/engine-runtime-demo/public/sample.pdf";

/// What `name` resolves to in a result's layout.
fn target(result: &Value, name: &str) -> Value {
    result["layout"]["namedPages"]
        .as_array()
        .expect("namedPages")
        .iter()
        .find(|entry| entry["name"] == name)
        .map_or(Value::Null, |entry| entry["target"].clone())
}

fn first_pages(host: &mut Host, doc: &str) -> Vec<Value> {
    let list = call(host, json!({ "kind": "pages.list", "docId": doc }));
    list["pages"].as_array().expect("pages").iter().map(|page| page["ref"].clone()).collect()
}

#[test]
fn a_name_is_set_replaced_by_key_and_renamed_in_one_job() {
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, SAMPLE);
    let pages = first_pages(&mut host, &doc);
    let (first, second) = (pages[0].clone(), pages[1].clone());

    let created = call(
        &mut host,
        json!({ "kind": "pages.setName", "docId": doc, "opId": "name-1",
            "name": "Approved=Goedgekeurd", "page": first }),
    )["result"]
        .clone();
    assert_eq!(created["layout"]["pages"].as_array().map(Vec::len), Some(pages.len()));
    assert_eq!(target(&created, "Approved=Goedgekeurd"), json!({ "kind": "page", "page": first }));
    // `PageNameResult`: the layout and the write's meta, which names no
    // page and carries the write's opId.
    assert_eq!(
        created["meta"],
        json!({ "affectedPages": [], "cacheDelta": null, "opId": "name-1", "undoable": false })
    );
    let listed = call(&mut host, json!({ "kind": "pages.list", "docId": doc }));
    assert_eq!(listed["namedPages"], created["layout"]["namedPages"]);
    // The layout is in page space, as `pages.list` answers it.
    assert_eq!(listed["pages"][0]["boxes"], created["layout"]["pages"][0]["boxes"]);

    let replaced = call(
        &mut host,
        json!({ "kind": "pages.setName", "docId": doc, "name": "Approved=Goedgekeurd",
            "page": second }),
    )["result"]
        .clone();
    assert_eq!(
        target(&replaced, "Approved=Goedgekeurd"),
        json!({ "kind": "page", "page": second })
    );
    assert_eq!(replaced["layout"]["namedPages"].as_array().map(Vec::len), Some(1));

    let renamed = call(
        &mut host,
        json!({ "kind": "pages.setName", "docId": doc, "name": "Approved=Approved",
            "page": second, "replace": "Approved=Goedgekeurd" }),
    )["result"]
        .clone();
    assert_eq!(target(&renamed, "Approved=Goedgekeurd"), Value::Null);
    assert_eq!(target(&renamed, "Approved=Approved"), json!({ "kind": "page", "page": second }));
    assert_eq!(renamed["layout"]["namedPages"].as_array().map(Vec::len), Some(1));
}

#[test]
fn a_non_ascii_name_round_trips() {
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, SAMPLE);
    let page = first_pages(&mut host, &doc)[0].clone();
    let key = "Stämpel=Stämpel 日本";
    let set = call(
        &mut host,
        json!({ "kind": "pages.setName", "docId": doc, "name": key, "page": page }),
    );
    assert_eq!(target(&set["result"], key), json!({ "kind": "page", "page": page }));
    let listed = call(&mut host, json!({ "kind": "pages.list", "docId": doc }));
    assert!(listed["namedPages"].as_array().unwrap().iter().any(|entry| entry["name"] == key));
}

#[test]
fn a_removed_name_leaves_its_page_and_a_second_removal_is_not_found() {
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, SAMPLE);
    let pages = first_pages(&mut host, &doc);
    call(
        &mut host,
        json!({ "kind": "pages.setName", "docId": doc, "name": "Draft=Concept", "page": pages[0] }),
    );
    let removed = call(
        &mut host,
        json!({ "kind": "pages.removeName", "docId": doc, "name": "Draft=Concept" }),
    );
    assert_eq!(target(&removed["result"], "Draft=Concept"), Value::Null);
    assert_eq!(removed["result"]["layout"]["pages"].as_array().map(Vec::len), Some(pages.len()));
    let again = error_code(
        &mut host,
        json!({ "kind": "pages.removeName", "jobId": 9, "docId": doc, "name": "Draft=Concept" }),
    );
    assert_eq!(again, json!("NotFound"));
}

#[test]
fn deleting_a_page_removes_every_name_pointing_at_it() {
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, SAMPLE);
    let pages = first_pages(&mut host, &doc);
    let (survivor, victim) = (pages[0].clone(), pages[1].clone());
    for (name, page) in [("Victim=One", &victim), ("Victim=Two", &victim), ("Survivor", &survivor)]
    {
        call(
            &mut host,
            json!({ "kind": "pages.setName", "docId": doc, "name": name, "page": page }),
        );
    }
    let deleted = call(
        &mut host,
        json!({ "kind": "document.apply", "docId": doc,
            "change": { "ops": [{ "type": "pages.delete", "pages": [victim] }] } }),
    );
    let named = &deleted["result"]["items"][0]["layout"]["namedPages"];
    let names: Vec<&str> =
        named.as_array().unwrap().iter().filter_map(|entry| entry["name"].as_str()).collect();
    assert_eq!(names, ["Survivor"], "{named}");
    assert!(
        !named.as_array().unwrap().iter().any(|entry| entry["target"]["kind"] == "dangling"),
        "the delete left a dangling name: {named}"
    );
}

#[test]
fn an_undone_delete_brings_its_names_back() {
    // The page capture holds the `/Names /Pages` keys that named the page
    // (`EPDFPage_ExportRawToOwnedBuffer`), so its restore registers them
    // again.
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, SAMPLE);
    let pages = first_pages(&mut host, &doc);
    call(
        &mut host,
        json!({ "kind": "pages.setName", "docId": doc, "name": "Kept=Kept", "page": pages[1] }),
    );
    call(
        &mut host,
        json!({ "kind": "document.apply", "docId": doc, "opId": "del-1",
            "change": { "ops": [{ "type": "pages.delete", "pages": [pages[1]] }] } }),
    );
    let undone = call(
        &mut host,
        json!({ "kind": "document.apply", "docId": doc, "opId": "undo-1",
            "change": { "undoOf": "del-1" } }),
    );
    assert_eq!(undone["result"]["items"][0]["type"], "pages.restore", "{undone}");
    let listed = call(&mut host, json!({ "kind": "pages.list", "docId": doc }));
    assert_eq!(
        listed["namedPages"],
        json!([{ "name": "Kept=Kept", "target": { "kind": "page", "page": pages[1] } }])
    );
    assert_eq!(listed["pages"][1]["ref"], pages[1], "the page is back where it was");
}
