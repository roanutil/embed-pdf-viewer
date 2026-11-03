//! `pages.copy` against upstream's `runPageCopyConformance`
//! (`core/src/conformance/`, through `main/test/page-copy.test.ts`) over the
//! same `CHANGE_FIXTURE_PDF`, and its `pages.copy` round trip
//! (`runChangeConformance`'s `ROUND_TRIPS`):
//!
//!   1. A copy reads as its page does, under the number it was given, and
//!      brings the page's annotations, and its form field as a field of its
//!      own: a name the document uses gets the first free `name_2`.
//!   2. Undo takes the copy and its field away; redo brings both back under
//!      the same numbers; undo again takes them away again.
//!   3. A page named twice is copied twice.
//!   4. A page the document doesn't have, or a number the session doesn't
//!      hold, is refused, and nothing changes.

use std::collections::BTreeMap;

use epdf_engine::host::Host;
use serde_json::{Value, json};

/// `CHANGE_FIXTURE_PDF`: two pages; the first holds annotations and the
/// field `name`, the second is empty.
fn fixture() -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [9 0 R] /DA (/Helv 0 Tf 0 g) \
         /DR << /Font << /Helv << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >> >> >>",
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Contents 10 0 R /Resources << >> \
         /Annots [5 0 R 6 0 R 7 0 R 8 0 R 9 0 R] >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Contents 10 0 R /Resources << >> >>",
        "<< /Type /Annot /Subtype /Square /Rect [20 200 80 260] /C [0 0 1] /T (Ann) /P 3 0 R >>",
        "<< /Type /Annot /Subtype /Text /Rect [120 220 140 240] /Contents (Note) /T (Ann) \
         /Popup 7 0 R /P 3 0 R >>",
        "<< /Type /Annot /Subtype /Popup /Rect [150 180 250 240] /Parent 6 0 R /P 3 0 R >>",
        "<< /Type /Annot /Subtype /Text /Rect [120 220 140 240] /Contents (Reply) /T (Bea) \
         /IRT 6 0 R /P 3 0 R >>",
        "<< /Type /Annot /Subtype /Widget /FT /Tx /T (name) /V (Ann) /Rect [20 20 180 44] \
         /P 3 0 R >>",
        "<< /Length 0 >>\nstream\n\nendstream",
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

const FIRST: u64 = 3;
const SECOND: u64 = 4;

/// The document, with `count` object numbers reserved; answers the first.
fn open(count: u64) -> (Host, u64) {
    let mut host = Host::new();
    let response = host.call_filled(
        &json!({ "kind": "open.fatMem", "jobId": 1, "docId": "d", "reserveObjectNumbers": count })
            .to_string(),
        vec![fixture()],
    );
    let opened: Value = serde_json::from_str(&response.json).expect("JSON");
    assert!(opened.get("error").is_none(), "{opened}");
    let first = opened["result"]["objectNumbers"]["first"].as_u64().expect("a reserved range");
    (host, first)
}

fn send(host: &mut Host, mut request: Value) -> Result<Value, Value> {
    request["jobId"] = json!(7);
    request["docId"] = json!("d");
    let response = host.call_filled(&request.to_string(), Vec::new());
    let mut envelope: Value = serde_json::from_str(&response.json).expect("JSON");
    match envelope.get_mut("error") {
        Some(error) => Err(error.take()),
        None => Ok(envelope["result"].take()),
    }
}

fn apply(host: &mut Host, op_id: &str, change: Value) -> Result<Value, Value> {
    send(host, json!({ "kind": "document.apply", "opId": op_id, "change": change }))
}

fn copy(host: &mut Host, op_id: &str, op: Value) -> Result<Value, Value> {
    let mut op = op;
    op["type"] = json!("pages.copy");
    apply(host, op_id, json!({ "ops": [op] }))
}

fn page(number: u64) -> Value {
    json!({ "kind": "objectNumber", "objectNumber": number })
}

fn list(host: &mut Host) -> Value {
    send(host, json!({ "kind": "pages.list" })).expect("pages.list")
}

fn order_of(host: &mut Host) -> Vec<u64> {
    list(host)["pages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|page| page["ref"]["objectNumber"].as_u64().unwrap())
        .collect()
}

/// The fields by name: their values, and the pages their widgets are on.
fn fields_of(host: &mut Host) -> BTreeMap<String, (Value, Vec<Value>)> {
    let listing = send(host, json!({ "kind": "forms.list" })).expect("forms.list");
    listing["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|field| {
            let pages = field["widgets"]
                .as_array()
                .unwrap()
                .iter()
                .map(|widget| widget["page"]["objectNumber"].clone())
                .collect();
            (field["name"].as_str().unwrap().to_owned(), (field["value"].clone(), pages))
        })
        .collect()
}

fn annotation_count_of(host: &mut Host, number: u64) -> usize {
    let listing = send(host, json!({ "kind": "annotations.list", "page": page(number) }))
        .expect("annotations.list");
    listing["annotations"].as_array().unwrap().len()
}

#[test]
fn a_copy_reads_as_its_page_does_and_brings_its_annotations_and_its_field_renamed() {
    let (mut host, copied) = open(1);
    let before = list(&mut host);
    let answer = copy(
        &mut host,
        "copy",
        json!({ "pages": [page(FIRST)], "position": "end", "objectNumbers": [copied] }),
    )
    .expect("pages.copy");
    let item = &answer["result"]["items"][0];
    assert_eq!(item["type"], "pages.copy");
    assert_eq!(item["insertedPages"], json!([page(copied)]));
    assert_eq!(order_of(&mut host), [FIRST, SECOND, copied]);

    let source = before["pages"][0].clone();
    let mut copy_layout = item["layout"]["pages"][2].clone();
    copy_layout["index"] = source["index"].clone();
    copy_layout["ref"] = source["ref"].clone();
    assert_eq!(
        test_support::canonical_numbers(&copy_layout),
        test_support::canonical_numbers(&source)
    );
    let annotations = annotation_count_of(&mut host, FIRST);
    assert!(annotations > 0);
    assert_eq!(annotation_count_of(&mut host, copied), annotations);
    assert_eq!(
        fields_of(&mut host),
        BTreeMap::from([
            ("name".to_owned(), (json!("Ann"), vec![json!(FIRST)])),
            ("name_2".to_owned(), (json!("Ann"), vec![json!(copied)])),
        ])
    );
    // A copy publishes the insert's event.
    assert_eq!(answer["events"][0]["type"], "pages.inserted", "{}", answer["events"]);
}

#[test]
fn undo_takes_the_copy_and_its_field_away_redo_brings_them_back_undo_again() {
    let (mut host, copied) = open(1);
    let original = fields_of(&mut host);
    copy(&mut host, "copy", json!({ "pages": [page(FIRST)], "objectNumbers": [copied] }))
        .expect("pages.copy");
    let with_copy = fields_of(&mut host);

    apply(&mut host, "undo", json!({ "undoOf": "copy" })).expect("undo");
    assert_eq!(order_of(&mut host), [FIRST, SECOND]);
    assert_eq!(fields_of(&mut host), original);

    apply(&mut host, "redo", json!({ "undoOf": "undo" })).expect("redo");
    assert_eq!(order_of(&mut host), [FIRST, SECOND, copied]);
    assert_eq!(fields_of(&mut host), with_copy);
    assert_eq!(annotation_count_of(&mut host, copied), annotation_count_of(&mut host, FIRST));

    apply(&mut host, "undo-again", json!({ "undoOf": "redo" })).expect("undo again");
    assert_eq!(order_of(&mut host), [FIRST, SECOND]);
    assert_eq!(fields_of(&mut host), original);
}

#[test]
fn a_page_named_twice_is_copied_twice_in_the_order_given() {
    let (mut host, first) = open(3);
    let numbers = [first, first + 1, first + 2];
    copy(
        &mut host,
        "copy",
        json!({ "pages": [page(SECOND), page(FIRST), page(SECOND)],
            "position": { "after": page(FIRST) }, "objectNumbers": numbers }),
    )
    .expect("pages.copy");
    assert_eq!(order_of(&mut host), [FIRST, numbers[0], numbers[1], numbers[2], SECOND]);
}

#[test]
fn refused_a_page_the_document_does_not_have_or_a_number_the_session_does_not_hold() {
    let (mut host, copied) = open(1);
    let missing =
        copy(&mut host, "missing", json!({ "pages": [page(9_999)], "objectNumbers": [copied] }))
            .expect_err("no such page");
    assert_eq!(missing["code"], "NotFound", "{missing}");
    let unheld = copy(
        &mut host,
        "unheld",
        json!({ "pages": [page(FIRST)], "objectNumbers": [copied + 1_000] }),
    )
    .expect_err("a number the session doesn't hold");
    assert_eq!(unheld["code"], "ObjectNumberUnavailable", "{unheld}");
    assert_eq!(order_of(&mut host), [FIRST, SECOND]);
    assert_eq!(fields_of(&mut host).into_keys().collect::<Vec<_>>(), ["name"]);
}

#[test]
fn a_copy_without_numbers_takes_fresh_ones_and_no_pages_is_refused() {
    let (mut host, _) = open(1);
    let answer = copy(&mut host, "copy", json!({ "pages": [page(SECOND)], "position": "start" }))
        .expect("pages.copy");
    let inserted = answer["result"]["items"][0]["insertedPages"][0]["objectNumber"].as_u64();
    assert_eq!(order_of(&mut host)[0], inserted.unwrap());

    let empty = copy(&mut host, "empty", json!({ "pages": [] })).expect_err("no pages");
    assert_eq!(empty["code"], "InvalidArg", "{empty}");
}
