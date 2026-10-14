//! `measure.viewports` and `measure.setScale` over the wire, against the
//! TypeScript's `MeasureReader`/`MeasureMutator` behaviour.

mod common;

use common::{call, call_err, open_fixture};
use epdf_engine::host::Host;
use serde_json::{Value, json};

const FOREIGN: &str = test_support::MEASURE_FOREIGN_VIEWPORT;

/// What `measureFromKnownLength` writes for 1 in = 1 m, plus every
/// optional member a number format can carry, so the codec's round trip
/// covers them.
fn rl_measure() -> Value {
    json!({
        "subtype": "RL",
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
        call(host, json!({ "kind": "measure.viewports", "docId": doc, "pageObjectNumber": pon }));
    result["viewports"].as_array().expect("viewports").clone()
}

#[test]
fn viewports_read_another_producers_scale_as_not_owned() {
    let mut host = Host::new();
    let (doc, pon) = open_fixture(&mut host, FOREIGN);
    let viewports = viewports(&mut host, &doc, pon);
    assert_eq!(viewports.len(), 1);
    assert_eq!(viewports[0]["owned"], json!(false));
    assert_eq!(viewports[0]["name"], json!("Acrobat"));
    assert_eq!(
        viewports[0]["bbox"],
        json!({ "left": 0.0, "right": 612.0, "bottom": 0.0, "top": 792.0 })
    );
    let measure = &viewports[0]["measure"];
    assert_eq!(measure["subtype"], json!("RL"));
    assert_eq!(measure["ratio"], json!("1 in = 1 m"));
    assert_eq!(measure["distance"][0]["unit"], json!("m"));
    assert_eq!(measure["area"][0]["unit"], json!("sq m"));
    assert!(measure.get("y").is_none(), "an empty y list is left out: {measure}");
}

#[test]
fn set_scale_keeps_a_foreign_viewport() {
    let mut host = Host::new();
    let (doc, pon) = open_fixture(&mut host, FOREIGN);
    let written = call(
        &mut host,
        json!({
            "kind": "measure.setScale", "docId": doc, "pageObjectNumber": pon, "measure": rl_measure()
        }),
    );
    assert_eq!(
        written["result"],
        json!({
            "pageObjectNumber": pon, "meta": { "affectedPages": [], "cacheDelta": null }
        })
    );
    assert_eq!(written["event"]["type"], json!("page.viewportsChanged"), "{written}");
    assert!(written["mutationSequence"].is_u64(), "{written}");

    let after = viewports(&mut host, &doc, pon);
    assert_eq!(after.len(), 2);
    assert_eq!(after[0]["name"], json!("Acrobat"));
    assert_eq!(after[1]["name"], json!("EmbedPDF"));
    assert_eq!(after[1]["owned"], json!(true));
    assert_eq!(
        after[1]["bbox"],
        json!({ "left": 0.0, "right": 612.0, "bottom": 0.0, "top": 792.0 })
    );
    let m = &after[1]["measure"];
    assert_eq!(m["ratio"], json!("1 in = 1 m"));
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
    call(
        &mut host,
        json!({
            "kind": "measure.setScale", "docId": doc, "pageObjectNumber": pon, "measure": rl_measure()
        }),
    );
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
        json!({ "kind": "measure.setScale", "docId": doc, "pageObjectNumber": pon }),
    );
    assert_eq!(error["code"], json!("InvalidArg"), "{error}");
    assert_eq!(viewports(&mut host, &doc, pon).len(), 2);

    // An explicit `null` removes the owned viewport only.
    call(
        &mut host,
        json!({
            "kind": "measure.setScale", "docId": doc, "pageObjectNumber": pon, "measure": null
        }),
    );
    let cleared = viewports(&mut host, &doc, pon);
    assert_eq!(cleared.len(), 1);
    assert_eq!(cleared[0]["name"], json!("Acrobat"));
}

#[test]
fn set_scale_refuses_an_unwritable_measure() {
    let mut host = Host::new();
    let (doc, pon) = open_fixture(&mut host, FOREIGN);
    for (measure, message) in [
        (json!({ "subtype": "GEO" }), "Only rectilinear measures can be authored"),
        (json!({ "subtype": "RL", "ratio": "1 in = 1 m", "x": [] }), "Missing distance formats"),
        (
            json!({ "subtype": "RL", "x": [{ "unit": "m", "conversion": 0 }], "distance": [], "area": [] }),
            "Invalid number format",
        ),
    ] {
        let error = call_err(
            &mut host,
            json!({
                "kind": "measure.setScale", "docId": doc, "pageObjectNumber": pon, "measure": measure
            }),
        );
        assert_eq!(error["code"], json!("InvalidArg"), "{error}");
        assert_eq!(error["message"], json!(message), "{error}");
    }
    assert_eq!(viewports(&mut host, &doc, pon).len(), 1, "a refused write leaves /VP alone");
}

#[test]
fn a_saved_scale_survives_a_reopen() {
    let mut host = Host::new();
    let (doc, pon) = open_fixture(&mut host, FOREIGN);
    call(
        &mut host,
        json!({
            "kind": "measure.setScale", "docId": doc, "pageObjectNumber": pon, "measure": rl_measure()
        }),
    );
    let response = host.call(
        &json!({ "kind": "document.saveBuffer", "jobId": 9, "docId": doc, "mode": "full" })
            .to_string(),
        Vec::new(),
    );
    let saved = response.payloads.into_iter().next().expect("the saved bytes");
    let mut reopened = Host::new();
    let open = reopened
        .call(&json!({ "kind": "open.fatMem", "jobId": 1, "docId": "r" }).to_string(), vec![saved]);
    assert!(!open.json.contains("\"error\""), "{}", open.json);
    let pages = call(&mut reopened, json!({ "kind": "pages.list", "docId": "r" }));
    let pon = pages["pages"][0]["pageObjectNumber"].as_u64().unwrap();
    let after = viewports(&mut reopened, "r", pon);
    assert_eq!(after.len(), 2);
    assert_eq!(after[1]["owned"], json!(true));
}
