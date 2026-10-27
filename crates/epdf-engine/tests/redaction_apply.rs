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

// ── Ported from `runRedactionApplyConformance` ─────────────────────────────
//
// `packages/engine/core/src/conformance/runRedactionApplyConformance.ts`, as
// `packages/engine/main/test/redaction-apply.test.ts` runs it: a page with no
// annotations of its own, so the collateral counts are exact.

const HELLO_WORLD: &str = "packages/engine/main/test/fixtures/hello_world.pdf";

/// The highlight sits fully inside [`redact_rect`], so applying the
/// redaction removes it as collateral.
fn redact_rect() -> Value {
    json!({ "x": 50, "y": 50, "width": 120, "height": 100 })
}

fn collateral_quad() -> Value {
    json!([{ "upperLeft": { "x": 70, "y": 80 }, "upperRight": { "x": 150, "y": 80 },
        "lowerLeft": { "x": 70, "y": 120 }, "lowerRight": { "x": 150, "y": 120 } }])
}

/// The first page's ref, as `pages.list` names it.
fn first_page(replay: &mut Replay) -> Value {
    pages(replay)["pages"][0]["ref"].clone()
}

fn pages(replay: &mut Replay) -> Value {
    replay
        .replay(&json!({ "kind": "pages.list", "jobId": "<job>", "docId": "<doc>" }))
        .expect("pages.list")
}

/// `page.annotations.create(draft)`: the new annotation's record.
fn create(replay: &mut Replay, page: &Value, draft: Value) -> Value {
    let created = replay
        .replay(&json!({ "kind": "annotations.create", "jobId": "<job>", "opId": "<op>",
            "docId": "<doc>", "page": page, "draft": draft }))
        .unwrap_or_else(|e| panic!("create: {e}"));
    created["result"]["annotation"].clone()
}

/// `doc.redaction.apply(scope)`: the whole answer.
fn redact_scope(replay: &mut Replay, scope: Value) -> Result<Value, Value> {
    replay.replay(&json!({ "kind": "redaction.apply", "effect": "contentWrite",
        "jobId": "<job>", "opId": "<op>", "docId": "<doc>", "scope": scope }))
}

fn page_annotations(replay: &mut Replay, page: &Value) -> Vec<Value> {
    let listing = replay
        .replay(&json!({ "kind": "annotations.list", "jobId": "<job>", "docId": "<doc>",
            "pages": [page] }))
        .expect("annotations.list");
    listing["annotations"].as_array().cloned().unwrap_or_default()
}

fn page_text(replay: &mut Replay, page: &Value) -> String {
    let text = replay
        .replay(&json!({ "kind": "pages.text", "jobId": "<job>", "docId": "<doc>",
            "page": page }))
        .expect("pages.text");
    text["text"].as_str().unwrap_or_else(|| panic!("no text: {text}")).to_owned()
}

/// A fresh host with `bytes` open as `doc_id`, as `openBarePage` opens one.
fn open_bytes(bytes: Vec<u8>, doc_id: &str) -> Replay {
    let mut host = epdf_engine::host::Host::new();
    let opened = host.call_filled(
        &json!({ "kind": "open.fatMem", "effect": "open", "jobId": 1, "docId": doc_id,
            "password": null })
        .to_string(),
        vec![bytes],
    );
    assert!(!opened.json.contains("\"error\""), "{}", opened.json);
    Replay::adopt(host, doc_id)
}

/// A PDF file of `objects`, numbered from 1, with its cross-reference table
/// (`pdfOf`).
fn pdf_of(objects: &[&str]) -> Vec<u8> {
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

/// "annotations scope applies the marked redaction and counts collateral".
#[test]
fn an_annotations_scope_applies_the_mark_and_counts_its_collateral() {
    let mut replay = Replay::open(HELLO_WORLD, "redaction-apply-local");
    let layout_before = pages(&mut replay);
    let page = first_page(&mut replay);

    let collateral = create(
        &mut replay,
        &page,
        json!({ "subtype": "highlight", "quadPoints": collateral_quad() }),
    );
    assert_eq!(collateral["subtype"], "highlight");
    let marked = create(
        &mut replay,
        &page,
        json!({ "subtype": "redact", "rect": redact_rect(), "interiorColor": "#000000",
            "overlayText": "REDACTED", "fontColor": "#ffffff" }),
    );
    assert_eq!(marked["subtype"], "redact");
    // The mark's own popup, away from the region: it goes with the mark.
    create(
        &mut replay,
        &page,
        json!({ "subtype": "popup", "rect": { "x": 300, "y": 300, "width": 100, "height": 60 },
            "parent": marked["ref"] }),
    );
    let before = page_annotations(&mut replay, &page);

    let applied =
        redact_scope(&mut replay, json!({ "annotations": [marked["ref"]] })).expect("applied");
    let result = &applied["result"];
    assert_eq!(result["results"].as_array().map(Vec::len), Some(1), "{result}");
    assert_eq!(result["results"][0]["page"], page);
    assert_eq!(result["results"][0]["status"], "applied");
    // The highlight and the mark's popup count: the consumed redact never does.
    assert_eq!(result["results"][0]["removedAnnotationCount"], 2, "{result}");
    assert_eq!(result["removedAnnotationCount"], 2);
    assert_eq!(result["meta"]["affectedPages"], json!([page]));
    // One event, and it is the redaction's.
    assert_eq!(applied["event"]["type"], "redaction.applied", "{applied}");
    assert!(applied["event"].is_object(), "{applied}");

    // The redaction, its popup and its collateral are gone; layout is not a
    // casualty.
    assert_eq!(page_annotations(&mut replay, &page).len(), before.len() - 3);
    assert_eq!(pages(&mut replay), layout_before);
    // The label is part of the page now.
    assert!(page_text(&mut replay, &page).contains("REDACTED"));
}

/// "annotations scope applies several marks of a page together and leaves
/// its others".
#[test]
fn several_marks_of_a_page_apply_together_and_leave_the_others() {
    let mut replay = Replay::open(HELLO_WORLD, "redaction-apply-local");
    let page = first_page(&mut replay);
    let mark = |replay: &mut Replay, x: f64| {
        create(
            replay,
            &page,
            json!({ "subtype": "redact", "rect": { "x": x, "y": 20, "width": 40, "height": 30 },
                "interiorColor": "#000000" }),
        )
    };
    let first = mark(&mut replay, 10.0);
    let kept = mark(&mut replay, 70.0);
    let last = mark(&mut replay, 130.0);
    // Under the first mark: collateral of the apply.
    create(
        &mut replay,
        &page,
        json!({ "subtype": "highlight", "quadPoints": [{
            "upperLeft": { "x": 15, "y": 25 }, "upperRight": { "x": 45, "y": 25 },
            "lowerLeft": { "x": 15, "y": 45 }, "lowerRight": { "x": 45, "y": 45 } }] }),
    );

    let applied = redact_scope(&mut replay, json!({ "annotations": [last["ref"], first["ref"]] }))
        .expect("applied");
    let result = &applied["result"];
    assert_eq!(result["results"].as_array().map(Vec::len), Some(1), "{result}");
    assert_eq!(result["results"][0]["status"], "applied");
    assert_eq!(result["results"][0]["removedAnnotationCount"], 1, "{result}");
    let left = page_annotations(&mut replay, &page);
    assert_eq!(left.iter().map(|a| a["subtype"].clone()).collect::<Vec<_>>(), [json!("redact")]);
    assert_eq!(left[0]["ref"], kept["ref"], "the kept mark's ref moved");
}

/// "a label fitted to its region (fontSize 0) is burned in".
#[test]
fn a_label_fitted_to_its_region_is_burned_in() {
    let mut replay = Replay::open(HELLO_WORLD, "redaction-apply-local");
    let page = first_page(&mut replay);
    // Marked first and labelled after, as a redaction UI does.
    let marked = create(
        &mut replay,
        &page,
        json!({ "subtype": "redact", "rect": redact_rect(), "interiorColor": "#000000" }),
    );
    replay
        .replay(&json!({ "kind": "annotations.update", "jobId": "<job>", "opId": "<op>",
            "docId": "<doc>", "ref": marked["ref"], "patch": { "subtype": "redact",
            "overlayText": "CLASSIFIED", "fontSize": 0, "fontColor": "#ffffff" } }))
        .expect("labelled");

    redact_scope(&mut replay, json!({ "annotations": [marked["ref"]] })).expect("applied");
    assert!(page_text(&mut replay, &page).contains("CLASSIFIED"));
}

/// "pages scope applies everything once and validates its input".
#[test]
fn a_pages_scope_applies_everything_once_and_validates_its_input() {
    let mut replay = Replay::open(HELLO_WORLD, "redaction-apply-local");
    let page = first_page(&mut replay);
    create(
        &mut replay,
        &page,
        json!({ "subtype": "redact", "rect": redact_rect(), "interiorColor": "#000000" }),
    );

    let applied = redact_scope(&mut replay, json!({ "pages": [page] })).expect("applied");
    assert_eq!(applied["result"]["results"][0]["status"], "applied", "{applied}");
    assert_eq!(applied["result"]["results"].as_array().map(Vec::len), Some(1));
    assert_eq!(applied["result"]["removedAnnotationCount"], 0);
    assert_eq!(applied["event"]["type"], "redaction.applied");

    // A page with no redactions left is unchanged: no artifact, no event.
    let no_op = redact_scope(&mut replay, json!({ "pages": [page] })).expect("a no-op");
    assert_eq!(no_op["result"]["results"][0]["status"], "unchanged", "{no_op}");
    let meta = &no_op["result"]["meta"];
    assert_eq!(meta["affectedPages"], json!([]), "{no_op}");
    assert_eq!(meta["cacheDelta"], Value::Null);
    assert_eq!(meta["undoable"], false);
    assert!(no_op.get("event").is_none(), "{no_op}");

    let twice = redact_scope(&mut replay, json!({ "pages": [page, page] })).expect_err("twice");
    assert_eq!(twice["code"], "InvalidArg", "{twice}");

    // Preflight rejects a ref that is not a redact annotation before
    // anything is written.
    let not_redact = create(
        &mut replay,
        &page,
        json!({ "subtype": "highlight", "quadPoints": collateral_quad() }),
    );
    let refused = redact_scope(&mut replay, json!({ "annotations": [not_redact["ref"]] }))
        .expect_err("a highlight");
    assert_eq!(refused["code"], "InvalidArg", "{refused}");
}

/// "applies on a page without /Contents or /Resources".
#[test]
fn a_page_without_contents_or_resources_is_redacted() {
    let bare = pdf_of(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] >>",
    ]);
    let mut replay = open_bytes(bare, "redaction-bare-page-local");
    let page = first_page(&mut replay);
    create(
        &mut replay,
        &page,
        json!({ "subtype": "redact", "rect": redact_rect(), "interiorColor": "#000000",
            "overlayText": "REDACTED", "fontColor": "#ffffff" }),
    );

    // The overlay becomes the page's first content, its form named in the
    // /Resources the page gets on the way.
    let applied = redact_scope(&mut replay, json!({ "pages": [page] })).expect("applied");
    assert_eq!(applied["result"]["results"][0]["status"], "applied", "{applied}");
    assert_eq!(applied["result"]["meta"]["affectedPages"], json!([page]));
    assert!(page_annotations(&mut replay, &page).is_empty());
    assert!(page_text(&mut replay, &page).contains("REDACTED"));
}

// ── Promotion, and the partial-write gap ───────────────────────────────────

/// A page whose annotations are inline and that has no markings: the apply
/// promotes it, so it is affected and the call wrote, though its status is
/// `unchanged`.
#[test]
fn a_promoted_page_with_no_markings_is_unchanged_yet_written() {
    let inline = pdf_of(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] \
           /Annots [<< /Type /Annot /Subtype /Square /Rect [10 10 50 50] >>] >>",
    ]);
    let mut replay = open_bytes(inline, "redaction-promote-only");
    let page = first_page(&mut replay);

    let applied = redact_scope(&mut replay, json!({ "pages": [page] })).expect("applied");
    let result = &applied["result"];
    assert_eq!(result["results"][0]["status"], "unchanged", "{applied}");
    assert_eq!(result["meta"]["affectedPages"], json!([page]), "{applied}");
    assert_eq!(applied["wrote"], true, "{applied}");
    assert_eq!(applied["event"]["type"], "redaction.applied", "{applied}");
    assert_eq!(page_annotations(&mut replay, &page).len(), 1, "the square is still there");
}

/// Two pages, each with an inline marking over a blue square of content.
fn two_marked_pages() -> Vec<u8> {
    let content = "0 0 1 rg 50 50 100 100 re f";
    let stream = format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len());
    let page = "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Contents 5 0 R \
                /Annots [<< /Type /Annot /Subtype /Redact /Rect [40 40 160 160] /IC [0 0 0] >>] >>";
    pdf_of(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
        page,
        page,
        &stream,
    ])
}

/// What a refused write must leave as it found: each page's listing and
/// pixels, and the session's sequence.
#[derive(Debug, PartialEq)]
struct Snapshot {
    listings: Vec<Vec<Value>>,
    pixels: Vec<Vec<u8>>,
    sequence: u64,
}

fn snapshot(replay: &mut Replay, refs: &[Value]) -> Snapshot {
    use epdf_engine::access::DocumentAccess;
    let listings = refs.iter().map(|page| page_annotations(replay, page)).collect();
    let session = replay
        .host
        .sessions()
        .session(&epdf_engine::session::registry::SessionKey::document(replay.doc_id.clone()))
        .expect("the session");
    let owned = session.document().expect("open");
    let pixels = refs
        .iter()
        .map(|page| {
            let pon = PageObjectNumber::new(page["objectNumber"].as_u64().unwrap() as u32).unwrap();
            epdf_engine::features::render::raster::render_page(
                &owned.page(pon).expect("page"),
                &epdf_engine::features::render::raster::RenderOptions::default(),
                &CancelToken::never(),
            )
            .expect("the page renders")
            .rgba
        })
        .collect();
    Snapshot { listings, pixels, sequence: session.mutation_sequence() }
}

/// Run `write` as the host runs a write: in the session's transaction.
fn write_with<T>(
    replay: &mut Replay,
    write: impl FnOnce(&mut DocumentSession) -> Result<T, epdf_engine::error::EngineError>,
) -> Result<T, epdf_engine::error::EngineError> {
    let key = epdf_engine::session::registry::SessionKey::document(replay.doc_id.clone());
    let session = replay.host.sessions_mut().session_mut(&key).expect("the session");
    epdf_engine::features::changes::transaction::in_transaction(session, "op-cancelled", write)
}

/// The host's call is synchronous, so nothing can cancel it partway; the
/// write runs here as the host runs it, with a token that cancels at its
/// third check. A one-page scope makes two checks (the call's and the
/// page's) and completes, so the third is the second page's, after the
/// first page was applied.
#[test]
fn a_cancel_after_the_first_page_takes_the_whole_apply_back() {
    let pages_of = |replay: &mut Replay| -> Vec<Value> {
        pages(replay)["pages"].as_array().unwrap().iter().map(|p| p["ref"].clone()).collect()
    };
    let scope = |refs: &[Value]| -> RedactionScope {
        serde_json::from_value(json!({ "pages": refs })).expect("a scope")
    };

    let mut one = open_bytes(two_marked_pages(), "redaction-one-page");
    let refs = pages_of(&mut one);
    let first_only = scope(&refs[..1]);
    write_with(&mut one, |s| apply(s, &first_only, &CancelToken::cancelled_at_check(3)))
        .expect("one page checks twice");

    let mut replay = open_bytes(two_marked_pages(), "redaction-cancelled");
    let refs = pages_of(&mut replay);
    let before = snapshot(&mut replay, &refs);
    let both = scope(&refs);
    let refused = write_with(&mut replay, |s| apply(s, &both, &CancelToken::cancelled_at_check(3)))
        .expect_err("cancelled on the second page");
    assert!(refused.is(epdf_engine::error::EngineErrorCode::Aborted), "{refused}");
    assert_eq!(snapshot(&mut replay, &refs), before, "the first page stayed redacted");

    // And the document still takes the apply afterwards.
    let applied = redact_scope(&mut replay, json!({ "pages": refs })).expect("applied");
    assert_eq!(applied["result"]["meta"]["affectedPages"], json!(refs), "{applied}");
}

/// The same for `pages.flatten`, 8.17's sibling.
#[test]
fn a_cancel_after_the_first_page_takes_the_whole_flatten_back() {
    use epdf_engine::features::pages_write::{FlattenUsage, flatten_pages};

    let blank = pdf_of(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] >>",
    ]);
    let mut replay = open_bytes(blank, "flatten-cancelled");
    let refs: Vec<Value> =
        pages(&mut replay)["pages"].as_array().unwrap().iter().map(|p| p["ref"].clone()).collect();
    for page in &refs {
        create(
            &mut replay,
            page,
            json!({ "subtype": "square", "rect": { "x": 50, "y": 50, "width": 100, "height": 100 },
                "color": "#1428dc", "strokeWidth": 4 }),
        );
    }
    let numbers: Vec<PageObjectNumber> = refs
        .iter()
        .map(|p| PageObjectNumber::new(p["objectNumber"].as_u64().unwrap() as u32).unwrap())
        .collect();

    let before = snapshot(&mut replay, &refs);
    let refused = write_with(&mut replay, |s| {
        flatten_pages(s, &numbers, FlattenUsage::Display, &CancelToken::cancelled_at_check(3))
    })
    .expect_err("cancelled on the second page");
    assert!(refused.is(epdf_engine::error::EngineErrorCode::Aborted), "{refused}");
    assert_eq!(snapshot(&mut replay, &refs), before, "the first page stayed flattened");

    // Two checks for one page, so the third was the second page's.
    let flattened = write_with(&mut replay, |s| {
        flatten_pages(s, &numbers[..1], FlattenUsage::Display, &CancelToken::cancelled_at_check(3))
    })
    .expect("one page checks twice");
    assert_eq!(
        serde_json::to_value(&flattened).unwrap()["results"][0]["status"],
        "applied",
        "the first page had something to flatten"
    );
}
