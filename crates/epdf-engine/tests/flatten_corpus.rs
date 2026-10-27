//! The flatten and appearance-export cases of `pages/mutate.json`, replayed
//! through the host: each case's recorded `request` (`common::replay`),
//! against a fresh copy of its fixture.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run pages-mutate
//!
//! `pages_mutate.rs` replays the rest of the file (8.18). What is compared:
//! a flatten's result less its `meta` (whose `opId` is this run's), the
//! pages the `meta` names, how many annotations the page lists afterwards,
//! and the layout of an exported document. A refusal is compared by code
//! and, since the vector records only the code, by upstream's message.

mod common;

use common::replay::Replay;
use serde::Deserialize;
use serde_json::{Value, json};

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VectorFile {
    schema_version: u32,
    kind: String,
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    name: String,
    covers: String,
    #[serde(default)]
    flatten: Option<Input>,
    #[serde(default)]
    flatten_annotations: Option<Input>,
    #[serde(default)]
    export_appearance: Option<Input>,
    /// The request the generator sent.
    #[serde(default)]
    request: Option<Value>,
    #[serde(default)]
    refused: bool,
    #[serde(default)]
    refused_with: Option<String>,
    /// `pages.flatten`'s result less its `meta`.
    #[serde(default)]
    flattened: Option<Value>,
    /// `annotations.flatten`'s result less its `meta`.
    #[serde(default)]
    annotations_flattened: Option<Value>,
    /// `annotations.flatten`'s `meta.affectedPages`.
    #[serde(default)]
    affected_pages: Option<Value>,
    /// How many annotations the page lists after `annotations.flatten`.
    #[serde(default)]
    remaining: Option<usize>,
    /// The page list of the document `annotations.exportAppearance` made.
    #[serde(default)]
    exported: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct Input {
    fixture: String,
}

impl Case {
    fn input(&self) -> Option<&Input> {
        self.flatten
            .as_ref()
            .or(self.flatten_annotations.as_ref())
            .or(self.export_appearance.as_ref())
    }
}

/// Upstream's message for each refusal, which the vector does not record:
/// `PagesFlattener.requireUniquePages`, `AnnotationFlattener.requireRefs`
/// and `exportAppearance`, and `resolveAnnotIndexRaw`.
fn refusal_message(name: &str) -> &'static str {
    match name {
        "flatten-duplicate-refused" => "pages.flatten was given duplicate page object number 3",
        "flatten-annotations-empty-refused" => "annotations.flatten requires at least one ref",
        "flatten-annotations-other-page-refused" => {
            "annotations.flatten refs must all target page 3; got ref on page 4"
        }
        "flatten-annotations-unknown-ref-refused" => {
            "no annotation with object number 9999 on page 3"
        }
        "export-appearance-without-appearance-refused" => {
            "annotations.exportAppearance: every ref must be a visible annotation of this page with a normal appearance"
        }
        other => panic!("{other}: no recorded message for this refusal"),
    }
}

fn load() -> Vec<Case> {
    let path = test_support::corpus::vectors_dir("pages").join("mutate.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: VectorFile = serde_json::from_str(&text).expect("mutate.json");
    assert_eq!(file.schema_version, SCHEMA_VERSION, "vector envelope moved");
    assert_eq!(file.kind, "pages.mutate");
    file.cases.into_iter().filter(|case| case.input().is_some()).collect()
}

fn listed(replay: &mut Replay, page: &Value) -> Vec<Value> {
    let listing = replay
        .replay(&json!({ "kind": "annotations.list", "jobId": "<job>", "docId": "<doc>",
            "pages": [page] }))
        .expect("annotations.list");
    listing["annotations"].as_array().cloned().unwrap_or_default()
}

/// `result` less its `meta`, which it must carry, stamped with this run's
/// op.
fn without_meta(case: &Case, result: &Value, op_id: &str) -> (Value, Value) {
    let mut result = result.clone();
    let meta = result.as_object_mut().and_then(|r| r.remove("meta")).expect("a meta");
    assert_eq!(meta["opId"], op_id, "{}: {meta}", case.name);
    assert_eq!(meta["undoable"], false, "{}: a flatten is final", case.name);
    assert_eq!(meta["cacheDelta"], Value::Null, "{}", case.name);
    (result, meta)
}

#[test]
fn every_flatten_and_export_case_answers_what_the_typescript_engine_answered() {
    let cases = load();
    assert_eq!(cases.len(), 12, "the flatten and export cases changed");
    let (mut applied, mut refused) = (0usize, 0usize);

    for case in &cases {
        let fixture = &case.input().expect("an input").fixture;
        let request = case.request.as_ref().unwrap_or_else(|| panic!("{}: no request", case.name));
        let mut replay = Replay::open(fixture, &format!("pages-mutate:{}", case.name));
        // `Replay` names the first replayed request's op `op-2`.
        let op_id = "op-2";
        let answer = replay.send(request, Vec::new());

        if case.refused {
            refused += 1;
            let error = answer.outcome.expect_err(&case.name);
            assert_eq!(
                Some(error["code"].as_str().unwrap_or_default()),
                case.refused_with.as_deref(),
                "{}: {} ({error})",
                case.name,
                case.covers,
            );
            assert_eq!(error["message"], refusal_message(&case.name), "{}", case.name);
            continue;
        }
        applied += 1;
        let outcome = answer.outcome.unwrap_or_else(|e| panic!("{}: {e}", case.name));

        if let Some(expected) = &case.flattened {
            let (result, meta) = without_meta(case, &outcome["result"], op_id);
            assert_eq!(&result, expected, "{}: {}", case.name, case.covers);
            // Each applied page, and only those: no fixture page here is
            // promoted without being flattened.
            let applied_pages: Vec<Value> = result["results"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|r| r["status"] == "applied")
                .map(|r| r["page"].clone())
                .collect();
            assert_eq!(meta["affectedPages"], json!(applied_pages), "{}", case.name);
            assert_eq!(outcome["wrote"], json!(!applied_pages.is_empty()), "{}", case.name);
            assert_eq!(outcome.get("event").is_some(), !applied_pages.is_empty(), "{}", case.name);
            continue;
        }
        if let Some(expected) = &case.annotations_flattened {
            let (result, meta) = without_meta(case, &outcome["result"], op_id);
            assert_eq!(&result, expected, "{}: {}", case.name, case.covers);
            assert_eq!(Some(&meta["affectedPages"]), case.affected_pages.as_ref(), "{}", case.name);
            assert_eq!(
                Some(listed(&mut replay, &result["page"]).len()),
                case.remaining,
                "{}: annotations left on the page",
                case.name,
            );
            assert_eq!(outcome["wrote"], true, "{}", case.name);
            assert_eq!(outcome["event"]["type"], "annotations.flattened", "{}", case.name);
            continue;
        }
        let expected = case.exported.as_ref().unwrap_or_else(|| panic!("{}: nothing", case.name));
        assert_eq!(answer.payloads.len(), 1, "{}", case.name);
        let mut host = epdf_engine::host::Host::new();
        let opened = host.call_filled(
            &json!({ "kind": "open.fatMem", "effect": "open", "jobId": 1, "docId": "exported",
                "password": null })
            .to_string(),
            answer.payloads,
        );
        assert!(!opened.json.contains("\"error\""), "{}: {}", case.name, opened.json);
        let mut exported = Replay::adopt(host, "exported");
        let layout = exported
            .replay(&json!({ "kind": "pages.list", "jobId": "<job>", "docId": "<doc>" }))
            .expect("pages.list");
        assert_eq!(
            test_support::canonical_numbers(&layout["pages"]),
            test_support::canonical_numbers(expected),
            "{}: {}",
            case.name,
            case.covers,
        );
    }

    assert_eq!((applied, refused), (7, 5), "the applied and refused cases changed");
}
