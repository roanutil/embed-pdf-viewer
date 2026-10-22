//! G13's first gate: a page list is reordered, rotated, thinned and grown
//! the way the TypeScript engine does it. Each case's recorded `request`, a
//! `document.apply` change with one page op, is replayed through
//! `Host::call`. The file's flatten and export cases are replayed by
//! `flatten_corpus.rs` (8.17) and skipped here.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run pages-mutate
//!
//! # What the comparison watches
//!
//! Which OBJECT NUMBER ends up at which index. An index is a position and
//! only the object number is identity, so a reorder that produced the right
//! shape with the wrong pages in it would otherwise pass.

mod common;

use common::replay::Replay;
use epdf_engine::host::Host;
use serde::Deserialize;
use serde_json::{Value, json};

const SCHEMA_VERSION: u32 = 1;

/// The object numbers upstream's local engine reserves for each document it
/// opens (`OBJECT_NUMBER_POOL_SIZE`, `LocalObjectNumberPool.ts`): a page made
/// without a number of its own gets the next one past them.
const OBJECT_NUMBER_POOL_SIZE: u32 = 64;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VectorFile {
    schema_version: u32,
    kind: String,
    fixture: String,
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    name: String,
    covers: String,
    /// The `document.apply` request the TypeScript engine sent.
    request: Value,
    /// The flatten and export inputs, only to know the case is
    /// `flatten_corpus.rs`'s.
    flatten: Option<Value>,
    flatten_annotations: Option<Value>,
    export_appearance: Option<Value>,
    #[serde(default)]
    refused: bool,
    #[serde(default)]
    refused_with: Option<String>,
    #[serde(default)]
    layout: Option<Value>,
    /// The pages an insert handed out, as refs. Absent for the operations
    /// that add nothing.
    #[serde(default)]
    inserted: Option<Value>,
}

fn load() -> VectorFile {
    let path = test_support::corpus::vectors_dir("pages").join("mutate.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: VectorFile = serde_json::from_str(&text).expect("mutate.json");
    assert_eq!(file.schema_version, SCHEMA_VERSION, "vector envelope moved");
    assert_eq!(file.kind, "pages.mutate");
    file
}

impl Case {
    /// A flatten or export case, which `flatten_corpus.rs` replays.
    fn flattens(&self) -> bool {
        self.flatten.is_some()
            || self.flatten_annotations.is_some()
            || self.export_appearance.is_some()
    }
}

/// A fresh copy of the fixture, as the case's own document, opened as
/// upstream's local engine opens it: `open.fatMem`, a layer over the bytes,
/// with the pool its main thread names reserved.
fn open(file: &VectorFile, case: &Case) -> Replay {
    let doc = format!("pages-mutate:{}", case.name);
    let mut host = Host::new();
    let opened = host.call_filled(
        &json!({ "kind": "open.fatMem", "effect": "open", "jobId": 1, "docId": doc,
            "password": null, "reserveObjectNumbers": OBJECT_NUMBER_POOL_SIZE })
        .to_string(),
        vec![test_support::fixture(&file.fixture)],
    );
    assert!(!opened.json.contains("\"error\""), "{}", opened.json);
    Replay::adopt(host, &doc)
}

/// The document's pages, as `pages.list` answers them.
fn layout(replay: &mut Replay) -> Value {
    replay.replay(&json!({ "kind": "pages.list", "jobId": "<job>" })).expect("the pages list")
}

/// `request` as this wire sends it: a recorded insert holds upstream's typed
/// array as a list of numbers (`recording-engine.ts`), where this wire
/// carries bytes as a payload the op names (`{ "payload": n }`).
fn sent(replay: &mut Replay, request: &Value) -> Result<Value, Value> {
    let mut request = request.clone();
    let mut payloads = Vec::new();
    for op in request["change"]["ops"].as_array_mut().into_iter().flatten() {
        if let Some(Value::Array(bytes)) = op.get("source") {
            let bytes: Vec<u8> = bytes.iter().map(|b| b.as_u64().expect("a byte") as u8).collect();
            op["source"] = json!({ "payload": payloads.len() });
            payloads.push(bytes);
        }
    }
    replay.send(&request, payloads).outcome
}

#[test]
fn a_page_mutation_leaves_the_layout_the_typescript_engine_left() {
    let file = load();
    let mut applied = 0usize;

    for case in file.cases.iter().filter(|case| !case.refused && !case.flattens()) {
        applied += 1;
        let mut replay = open(&file, case);
        let answer =
            sent(&mut replay, &case.request).unwrap_or_else(|e| panic!("{}: {e}", case.name));
        let items = answer["result"]["items"].as_array().expect("items");
        assert_eq!(items.len(), 1, "{}: one op, one item", case.name);
        let item = &items[0];
        assert_eq!(item["type"], case.request["change"]["ops"][0]["type"], "{}", case.name);
        assert_eq!(
            test_support::canonical_numbers(&item["layout"]),
            test_support::canonical_numbers(case.layout.as_ref().expect("a recorded layout")),
            "{}: {}",
            case.name,
            case.covers,
        );
        assert_eq!(
            item.get("insertedPages"),
            case.inserted.as_ref(),
            "{}: the inserted pages differ",
            case.name,
        );
        // The layout the op answers is the one the document lists after it.
        assert_eq!(
            test_support::canonical_numbers(&layout(&mut replay)),
            test_support::canonical_numbers(&item["layout"]),
            "{}: the answer is not what the document lists",
            case.name,
        );
        // A page op names no page in `affectedPages`; the change publishes
        // one event for it, under its opId.
        assert_eq!(item["meta"]["affectedPages"], json!([]), "{}", case.name);
        assert_eq!(answer["events"].as_array().map(Vec::len), Some(1), "{}", case.name);
    }

    assert!(applied >= 13, "the applied list shrank: {applied}");
}

#[test]
fn a_refused_page_mutation_is_refused_for_the_same_reason() {
    let file = load();
    let mut refusals = 0usize;

    for case in file.cases.iter().filter(|case| case.refused && !case.flattens()) {
        refusals += 1;
        let mut replay = open(&file, case);
        let error = sent(&mut replay, &case.request).expect_err(&case.name);
        assert_eq!(
            error["code"].as_str(),
            case.refused_with.as_deref(),
            "{}: {} (got {error})",
            case.name,
            case.covers,
        );
    }

    assert!(refusals >= 10, "the refusal list shrank: {refusals}");
}

#[test]
fn a_refused_mutation_changes_nothing() {
    // The half the vector cannot show. A list naming one page that does not
    // exist must leave the others alone, and the only way to see that is to
    // ask the same document afterwards.
    let file = load();
    for case in file.cases.iter().filter(|case| case.refused && !case.flattens()) {
        let mut replay = open(&file, case);
        let before = layout(&mut replay);
        sent(&mut replay, &case.request).expect_err(&case.name);
        assert_eq!(layout(&mut replay), before, "{}: the refusal changed the document", case.name);
    }
}
