mod common;
use common::*;
use epdf_engine::{cancel::CancelToken, host::Host};
use serde_json::json;

#[test]
fn host_checks_the_supplied_token_during_existing_page_reads() {
    let mut host = Host::new();
    let request = json!({"kind":"open.fatMem","docId":"cancel","jobId":1,"effect":"open","identity":{},"scope":["*"]});
    let opened = host.call(&request.to_string(), vec![PLAIN.to_vec()]);
    assert!(
        serde_json::from_str::<serde_json::Value>(&opened.json).unwrap().get("error").is_none()
    );
    let request =
        json!({"kind":"pages.list","docId":"cancel","jobId":2,"effect":"read"}).to_string();
    let token = CancelToken::cancelled_at_check(2);
    let response = host.call_with_cancel(&request, vec![], &token);
    let response: serde_json::Value = serde_json::from_str(&response.json).unwrap();
    assert_eq!(response["error"]["code"], "Aborted", "{response}");
    assert!(token.is_cancelled());
    let response = host.call(&request, vec![]);
    let response: serde_json::Value = serde_json::from_str(&response.json).unwrap();
    assert!(response["result"]["pages"].is_array(), "{response}");
}

#[test]
fn supplied_cancellation_stops_render_and_literal_search_without_payloads() {
    let mut host = Host::new();
    let opened=host.call(&json!({"kind":"open.fatMem","docId":"cancel-work","jobId":1,"effect":"open","identity":{},"scope":["*"]}).to_string(),vec![rotated_crop_pdf()]);
    assert!(
        serde_json::from_str::<serde_json::Value>(&opened.json).unwrap().get("error").is_none()
    );
    let listed = host.call(
        &json!({"kind":"pages.list","docId":"cancel-work","jobId":2,"effect":"read"}).to_string(),
        vec![],
    );
    let listed: serde_json::Value = serde_json::from_str(&listed.json).unwrap();
    let page = &listed["result"]["pages"][0]["ref"];
    for args in [
        json!({"kind":"pages.text","page":page}),
        json!({"kind":"pages.geometry","page":page}),
        json!({"kind":"pages.render","page":page,"options":{"viewport":{"kind":"scale","scale":2}}}),
        json!({"kind":"search.query","request":{"text":"needle","limit":{"pages":1}}}),
    ] {
        let mut request = args;
        request["docId"] = json!("cancel-work");
        request["jobId"] = json!(3);
        request["effect"] = json!("read");
        let token = CancelToken::cancelled_at_check(2);
        let response = host.call_with_cancel(&request.to_string(), vec![], &token);
        let parsed: serde_json::Value = serde_json::from_str(&response.json).unwrap();
        assert_eq!(parsed["error"]["code"], "Aborted", "{request}: {parsed}");
        assert!(response.payloads.is_empty());
        assert!(token.is_cancelled());
    }
}
