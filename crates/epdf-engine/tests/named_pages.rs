//! `pages.setName` and `pages.removeName`, and the registrations
//! `pages.list` reports (`PagesMutator.ts`, `PagesReader.ts`).

use epdf_engine::host::Host;
use serde_json::{Value, json};

mod common;
use common::{call, open_fixture};

const REPORT: &str = "pdfium-multiplatform-prototype/fixtures/report.pdf";

fn error_code(host: &mut Host, request: Value) -> Value {
    let response = host.call(&request.to_string(), Vec::new());
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
        json!({ "kind": "pages.setName", "docId": doc, "name": "approved=Approved", "pageObjectNumber": pon }),
    );
    let named = json!([{ "name": "approved=Approved", "target": { "kind": "page", "pageObjectNumber": pon } }]);
    assert_eq!(set["result"]["layout"]["namedPages"], named);
    let list = call(&mut host, json!({ "kind": "pages.list", "docId": doc }));
    assert_eq!(list["namedPages"], named);
}

#[test]
fn a_name_change_publishes_pages_named() {
    let mut host = Host::new();
    let (doc, pon) = open_fixture(&mut host, REPORT);
    let response = host.call(
        &json!({ "kind": "pages.setName", "jobId": 1, "docId": doc, "name": "a=A", "pageObjectNumber": pon })
            .to_string(),
        Vec::new(),
    );
    let envelope: Value = serde_json::from_str(&response.json).unwrap();
    assert_eq!(envelope["result"]["event"]["type"], json!("pages.named"), "{}", envelope["error"]);
    assert_eq!(envelope["result"]["event"]["name"], json!("a=A"));
    assert_eq!(envelope["result"]["event"]["pageObjectNumber"], json!(pon));
    let response = host.call(
        &json!({ "kind": "pages.removeName", "jobId": 2, "docId": doc, "name": "a=A" }).to_string(),
        Vec::new(),
    );
    let envelope: Value = serde_json::from_str(&response.json).unwrap();
    assert_eq!(envelope["result"]["event"]["type"], json!("pages.named"), "{}", envelope["error"]);
    assert_eq!(envelope["result"]["event"]["pageObjectNumber"], Value::Null);
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
    let set = json!({ "kind": "pages.setName", "jobId": 1, "docId": doc, "name": "", "pageObjectNumber": pon });
    assert_eq!(error_code(&mut host, set), json!("InvalidArg"));
    let remove = json!({ "kind": "pages.removeName", "jobId": 2, "docId": doc, "name": "" });
    assert_eq!(error_code(&mut host, remove), json!("InvalidArg"));
}

#[test]
fn an_unknown_page_is_not_found() {
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, REPORT);
    let set = json!({ "kind": "pages.setName", "jobId": 1, "docId": doc, "name": "a=A", "pageObjectNumber": 999_999 });
    assert_eq!(error_code(&mut host, set), json!("NotFound"));
}

#[test]
fn replace_moves_a_name() {
    let mut host = Host::new();
    let (doc, pon) = open_fixture(&mut host, REPORT);
    call(
        &mut host,
        json!({ "kind": "pages.setName", "docId": doc, "name": "a=A", "pageObjectNumber": pon }),
    );
    call(
        &mut host,
        json!({ "kind": "pages.setName", "docId": doc, "name": "a=B", "pageObjectNumber": pon, "replace": "a=A" }),
    );
    let list = call(&mut host, json!({ "kind": "pages.list", "docId": doc }));
    assert_eq!(list["namedPages"].as_array().unwrap().len(), 1);
    assert_eq!(list["namedPages"][0]["name"], json!("a=B"));
}
