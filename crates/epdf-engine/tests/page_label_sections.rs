//! Page label sections through `pages.list` and the page ops of
//! `document.apply`: the list carries the sections (`labels`) and reads each
//! page's label from them, and every page op moves the sections with the
//! pages (`core/src/pages/labels.ts`, `changes/internal/pageChanges.ts`);
//! an undo puts back the sections the change found.
//!
//! The document is upstream's `PAGE_PREDICTION_FIXTURE_PDF`
//! (`runPagePredictionConformance`): four pages labelled in three sections,
//! i, ii; 1; App-B.

use epdf_engine::host::Host;
use serde_json::{Value, json};

fn fixture() -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R /PageLabels << /Nums [0 << /S /r >> 2 << /S /D >> \
         3 << /S /A /P (App-) /St 2 >>] >> >>",
        "<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R 6 0 R] /Count 4 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Rotate 90 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 300] /CropBox [10 10 390 290] \
         /UserUnit 2 /Tabs /R >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 500 500] /TrimBox [20 20 480 480] \
         /Rotate 270 >>",
    ];
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

fn open() -> Host {
    let mut host = Host::new();
    let response = host.call_filled(
        &json!({ "kind": "open.fatMem", "jobId": 1, "docId": "d" }).to_string(),
        vec![fixture()],
    );
    assert!(!response.json.contains("\"error\""), "{}", response.json);
    host
}

fn send(host: &mut Host, mut request: Value) -> Value {
    request["jobId"] = json!(7);
    request["docId"] = json!("d");
    let response = host.call_filled(&request.to_string(), Vec::new());
    let envelope: Value = serde_json::from_str(&response.json).expect("JSON");
    assert!(envelope.get("error").is_none(), "{request}: {envelope}");
    envelope["result"].clone()
}

fn apply(host: &mut Host, op_id: &str, op: Value) -> Value {
    send(host, json!({ "kind": "document.apply", "opId": op_id, "change": { "ops": [op] } }))
}

fn undo(host: &mut Host, op_id: &str) -> Value {
    send(
        host,
        json!({ "kind": "document.apply", "opId": format!("undo-{op_id}"),
            "change": { "undoOf": op_id } }),
    )
}

/// Each page's object number and label, in display order.
fn labelled(layout: &Value) -> Vec<(u64, Value)> {
    layout["pages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|page| (page["ref"]["objectNumber"].as_u64().unwrap(), page["label"].clone()))
        .collect()
}

fn list(host: &mut Host) -> Value {
    send(host, json!({ "kind": "pages.list" }))
}

fn original() -> Vec<(u64, Value)> {
    vec![(3, json!("i")), (4, json!("ii")), (5, json!("1")), (6, json!("App-B"))]
}

fn page(number: u64) -> Value {
    json!({ "kind": "objectNumber", "objectNumber": number })
}

#[test]
fn the_list_carries_the_sections_and_reads_each_label_from_them() {
    let mut host = open();
    let layout = list(&mut host);
    assert_eq!(
        layout["labels"],
        json!([
            { "firstPage": 0, "style": "lowerRoman", "firstNumber": 1, "prefix": "" },
            { "firstPage": 2, "style": "decimal", "firstNumber": 1, "prefix": "" },
            { "firstPage": 3, "style": "upperLetters", "firstNumber": 2, "prefix": "App-" },
        ])
    );
    assert_eq!(labelled(&layout), original());
}

#[test]
fn a_document_without_page_labels_lists_none() {
    let mut host = Host::new();
    let response = host.call_filled(
        &json!({ "kind": "open.fatMem", "jobId": 1, "docId": "d" }).to_string(),
        vec![test_support::fixture("packages/engine/main/test/fixtures/hello_world.pdf")],
    );
    assert!(!response.json.contains("\"error\""), "{}", response.json);
    let layout = list(&mut host);
    assert_eq!(layout["labels"], json!([]));
    assert!(layout["pages"].as_array().unwrap().iter().all(|page| page["label"].is_null()));
}

#[test]
fn a_delete_drops_a_section_left_with_no_pages_and_its_undo_puts_it_back() {
    let mut host = open();
    let answer = apply(&mut host, "del", json!({ "type": "pages.delete", "pages": [page(5)] }));
    let expected = vec![(3, json!("i")), (4, json!("ii")), (6, json!("App-B"))];
    assert_eq!(labelled(&answer["result"]["items"][0]["layout"]), expected);
    assert_eq!(labelled(&list(&mut host)), expected);
    assert_eq!(list(&mut host)["labels"].as_array().unwrap().len(), 2);

    undo(&mut host, "del");
    assert_eq!(labelled(&list(&mut host)), original());
    assert_eq!(list(&mut host)["labels"].as_array().unwrap().len(), 3);
}

#[test]
fn pages_put_before_every_page_join_the_first_section() {
    let mut host = open();
    let answer = apply(
        &mut host,
        "blank",
        json!({ "type": "pages.insertBlank", "spec": { "size": { "width": 100, "height": 100 } },
            "position": "start" }),
    );
    let labels: Vec<Value> =
        labelled(&answer["result"]["items"][0]["layout"]).into_iter().map(|(_, l)| l).collect();
    assert_eq!(labels, [json!("i"), json!("ii"), json!("iii"), json!("1"), json!("App-B")]);

    undo(&mut host, "blank");
    assert_eq!(labelled(&list(&mut host)), original());
}

#[test]
fn a_move_is_a_delete_then_an_insert_and_its_undo_puts_the_sections_back() {
    let mut host = open();
    let answer = apply(
        &mut host,
        "move",
        json!({ "type": "pages.reorder", "pages": [page(3)], "position": "end" }),
    );
    let expected = vec![(4, json!("i")), (5, json!("1")), (6, json!("App-B")), (3, json!("App-C"))];
    assert_eq!(labelled(&answer["result"]["items"][0]["layout"]), expected);
    assert_eq!(labelled(&list(&mut host)), expected);

    undo(&mut host, "move");
    assert_eq!(labelled(&list(&mut host)), original());
}

#[test]
fn an_op_that_leaves_the_sections_as_they_are_writes_none() {
    let mut host = open();
    // A page put after the last joins the last section.
    let answer = apply(
        &mut host,
        "blank",
        json!({ "type": "pages.insertBlank", "spec": { "size": { "width": 100, "height": 100 } } }),
    );
    assert_eq!(answer["result"]["items"][0]["layout"]["labels"], list(&mut host)["labels"]);
    assert_eq!(labelled(&list(&mut host)).last().unwrap().1, json!("App-C"));
    assert_eq!(list(&mut host)["labels"].as_array().unwrap().len(), 3);
}
