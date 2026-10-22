//! G13's destructive gate: applying a redaction removes what the TypeScript
//! engine removes. Each case of `redaction/apply.json` replays its recorded
//! `request` through the host (`common::replay`), against a fresh copy of
//! the fixture.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run redaction
//!
//! # What is compared, and what is not
//!
//! The result less its `meta` (whose `opId` is this run's), the pages the
//! `meta` names, and the annotations `annotations.list` reads off the page
//! afterwards. NOT the page content: both engines call the same sanitizer,
//! so comparing its output would compare it to itself, and the bytes it
//! writes are not part of this port's contract. A refusal is compared by
//! code and, since the vector records only the code, by upstream's message.

mod common;

use common::replay::Replay;
use epdf_engine::cancel::CancelToken;
use epdf_engine::features::redaction::{RedactionScope, apply};
use epdf_engine::identity::PageObjectNumber;
use epdf_engine::session::authority::SessionAuthority;
use epdf_engine::session::document::DocumentSession;
use serde::Deserialize;
use serde_json::{Value, json};

const SCHEMA_VERSION: u32 = 1;

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
    /// The request the generator sent.
    request: Value,
    #[serde(default)]
    refused: bool,
    #[serde(default)]
    refused_with: Option<String>,
    #[serde(default)]
    results: Option<Value>,
    #[serde(default)]
    removed_annotation_count: Option<u32>,
    /// What is still on the page: the subtype and name of each annotation.
    #[serde(default)]
    remaining: Option<Vec<RemainingAnnotation>>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct RemainingAnnotation {
    subtype: String,
    nm: Option<String>,
}

fn load() -> VectorFile {
    let path = test_support::corpus::vectors_dir("redaction").join("apply.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: VectorFile = serde_json::from_str(&text).expect("apply.json");
    assert_eq!(file.schema_version, SCHEMA_VERSION, "vector envelope moved");
    assert_eq!(file.kind, "redaction.apply");
    file
}

/// The page the fixture's markings are on.
const PAGE: u32 = 3;

/// Upstream's message for each refusal, which the vector does not record:
/// `RedactionApplier.buildPlan` and `preflight`, and the page lookup.
fn refusal_message(name: &str) -> &'static str {
    match name {
        "no-pages-refused" => "redaction.apply requires at least one page",
        "no-refs-refused" => "redaction.apply requires at least one annotation ref",
        "duplicate-page-refused" => "redaction.apply was given duplicate page object number 3",
        "non-redact-ref-refused" => {
            "redaction.apply ref on page 3 resolves to a 'stamp' annotation, not a redact annotation"
        }
        "unknown-page-refused" => "no page with object number 999",
        other => panic!("{other}: no recorded message for this refusal"),
    }
}

/// A fresh copy of the fixture, under the case's own document id.
fn open(file: &VectorFile, case: &Case) -> Replay {
    let doc_id = case.request["docId"].as_str().expect("a recorded docId");
    Replay::open(&file.fixture, doc_id)
}

/// What `annotations.list` reads off the page.
fn listed(replay: &mut Replay) -> Vec<Value> {
    let listing = replay
        .replay(&json!({ "kind": "annotations.list", "jobId": "<job>", "docId": "<doc>",
            "pages": [common::page_ref(PAGE)] }))
        .expect("annotations.list");
    listing["annotations"].as_array().cloned().unwrap_or_default()
}

fn remaining(replay: &mut Replay) -> Vec<RemainingAnnotation> {
    listed(replay)
        .iter()
        .map(|annotation| RemainingAnnotation {
            subtype: annotation["subtype"].as_str().expect("a subtype").to_owned(),
            nm: annotation["nm"].as_str().map(str::to_owned),
        })
        .collect()
}

#[test]
fn an_applied_redaction_removes_what_the_typescript_engine_removed() {
    let file = load();
    let applied: Vec<&Case> = file.cases.iter().filter(|c| !c.refused).collect();
    assert_eq!(applied.len(), 4, "the applied cases changed");

    for case in applied {
        let mut replay = open(&file, case);
        // The unchanged case needs a page whose markings are already gone,
        // so it applies everything once first.
        if case.name == "page-with-no-markings" {
            replay.replay(&case.request).expect("the first apply clears the page");
        }
        let answer = replay.send(&case.request, Vec::new());
        let op_id = if case.name == "page-with-no-markings" { "op-3" } else { "op-2" };
        let outcome = answer.outcome.unwrap_or_else(|e| panic!("{}: {e}", case.name));
        let result = &outcome["result"];

        assert_eq!(result["scope"], case.request["scope"], "{}: the scope echoes", case.name);
        assert_eq!(
            test_support::canonical_numbers(&result["results"]),
            test_support::canonical_numbers(case.results.as_ref().expect("recorded results")),
            "{}: {}",
            case.name,
            case.covers,
        );
        assert_eq!(
            result["removedAnnotationCount"],
            json!(case.removed_annotation_count.expect("a recorded count")),
            "{}: the collateral count differs",
            case.name,
        );

        // The pages applied, and only those. The fixture's annotations are
        // inline, so the apply promotes the page too, but every page here
        // that is promoted is also applied, and the unchanged case's page
        // was promoted by its first apply.
        let meta = &result["meta"];
        assert_eq!(meta["opId"], op_id, "{}: {meta}", case.name);
        assert_eq!(meta["undoable"], false, "{}: a redaction is final", case.name);
        assert_eq!(meta["cacheDelta"], Value::Null, "{}", case.name);
        let applied_pages: Vec<Value> = result["results"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|r| r["status"] == "applied")
            .map(|r| r["page"].clone())
            .collect();
        assert_eq!(meta["affectedPages"], json!(applied_pages), "{}", case.name);
        let wrote = !applied_pages.is_empty();
        assert_eq!(outcome["wrote"], wrote, "{}", case.name);
        assert!(outcome["mutationSequence"].is_u64(), "{}: {outcome}", case.name);
        match outcome.get("event") {
            Some(event) => {
                assert!(wrote, "{}: an event for a write that wrote nothing", case.name);
                assert_eq!(event["type"], "redaction.applied", "{}", case.name);
                assert_eq!(event["origin"]["tx"]["id"], op_id, "{}: {event}", case.name);
                assert_eq!(event["results"], result["results"], "{}", case.name);
            }
            None => assert!(!wrote, "{}: a write with no event", case.name),
        }

        assert_eq!(
            remaining(&mut replay),
            *case.remaining.as_ref().expect("recorded survivors"),
            "{}: the annotations left on the page differ",
            case.name,
        );
    }
}

#[test]
fn a_refused_apply_is_refused_for_the_same_reason() {
    let file = load();
    let mut refusals = 0usize;

    for case in file.cases.iter().filter(|c| c.refused) {
        refusals += 1;
        let mut replay = open(&file, case);
        let error = replay.replay(&case.request).expect_err(&case.name);
        assert_eq!(
            error["code"].as_str(),
            case.refused_with.as_deref(),
            "{}: {} ({error})",
            case.name,
            case.covers,
        );
        assert_eq!(error["message"], refusal_message(&case.name), "{}", case.name);
    }

    assert_eq!(refusals, 5, "the refused cases changed");
}

#[test]
fn a_refused_apply_redacts_nothing() {
    // The half the vector cannot show, and the one that matters most here:
    // a refusal that had already removed content would be a document
    // somebody believes is intact. A reference to a stamp is the case,
    // because the whole call is refused rather than that one page; a scope
    // naming one good page and one missing one is the other.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "non-redact-ref-refused").expect("the case");
    let mut replay = open(&file, case);
    let before = listed(&mut replay);

    replay.replay(&case.request).expect_err("a stamp is not a redaction marking");
    assert_eq!(listed(&mut replay), before, "the refusal removed something");

    let both = json!({ "kind": "redaction.apply", "effect": "contentWrite", "jobId": "<job>",
        "opId": "<op>", "docId": "<doc>",
        "scope": { "pages": [common::page_ref(PAGE), common::page_ref(999u32)] } });
    let error = replay.replay(&both).expect_err("no page 999");
    assert_eq!(error["code"], "NotFound", "{error}");
    assert_eq!(listed(&mut replay), before, "the good page was redacted before the bad one");
}

#[test]
fn applying_a_redaction_rebuilds_the_page_content() {
    // The apply takes objects out of the content stream, and the stream has
    // to be rebuilt for the removal to appear.
    use epdf_engine::access::DocumentAccess;

    let file = load();
    let page = PageObjectNumber::new(PAGE).expect("a page object number");

    let render = |apply_first: bool| -> Vec<u8> {
        let bytes = std::fs::read(test_support::workspace_root().join(&file.fixture))
            .unwrap_or_else(|e| panic!("read {}: {e}", file.fixture));
        let mut session = DocumentSession::new();
        session.open_with(bytes, None, SessionAuthority::unrestricted()).expect("open");
        if apply_first {
            session.begin_transaction("apply").expect("a transaction");
            apply(&session, &RedactionScope::Pages { pages: vec![page] }, &CancelToken::never())
                .expect("the apply runs");
            session.commit_transaction().expect("the apply commits");
        }
        let owned = session.document().expect("open");
        epdf_engine::features::render::raster::render_page(
            &owned.page(page).expect("page"),
            &epdf_engine::features::render::raster::RenderOptions::default(),
            &CancelToken::never(),
        )
        .expect("the page renders")
        .rgba
    };

    assert_ne!(
        render(false),
        render(true),
        "the page renders the same before and after, so nothing was taken out of its content",
    );
}
