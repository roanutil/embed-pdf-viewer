//! G13's destructive gate: applying a redaction removes what the TypeScript
//! engine removes.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run redaction
//!
//! # What is compared, and what is not
//!
//! The per-page status, the collateral count, and the annotations left on
//! the page. NOT the page content: both engines call the same sanitizer, so
//! comparing its output would compare it to itself, and the bytes it writes
//! are not part of this port's contract.

use epdf_engine::access::DocumentAccess;
use epdf_engine::cancel::CancelToken;
use epdf_engine::error::EngineErrorCode;
use epdf_engine::features::redaction::{RedactionScope, apply};
use epdf_engine::identity::PageObjectNumber;
use epdf_engine::session::authority::SessionAuthority;
use epdf_engine::session::document::DocumentSession;
use serde::Deserialize;

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
    scope: serde_json::Value,
    #[serde(default)]
    refused: bool,
    #[serde(default)]
    refused_with: Option<String>,
    #[serde(default)]
    results: Option<serde_json::Value>,
    #[serde(default)]
    removed_annotation_count: Option<u32>,
    /// What is still on the page: the subtype and name of each annotation.
    #[serde(default)]
    remaining: Option<Vec<RemainingAnnotation>>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, serde::Serialize)]
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

#[derive(Debug)]
struct Applied {
    results: serde_json::Value,
    removed: u32,
    remaining: Vec<RemainingAnnotation>,
}

/// Run one case against a fresh copy of the fixture.
fn run(file: &VectorFile, case: &Case) -> Result<Applied, epdf_engine::error::EngineError> {
    let bytes = std::fs::read(test_support::workspace_root().join(&file.fixture))
        .unwrap_or_else(|e| panic!("{}: read {}: {e}", case.name, file.fixture));
    let mut session = DocumentSession::new();
    session.open_with(bytes, None, SessionAuthority::unrestricted()).expect("open");
    let owned = session.document().expect("open");
    let page = PageObjectNumber::new(PAGE).expect("a page object number");

    // The unchanged case needs a page whose markings are already gone, so
    // it applies everything once first.
    if case.name == "page-with-no-markings" {
        apply(
            owned.document(),
            |p| owned.page(p),
            &RedactionScope::Pages { page_object_numbers: vec![page] },
            &CancelToken::never(),
        )
        .expect("the first apply clears the page");
    }

    let scope: RedactionScope = serde_json::from_value(case.scope.clone())
        .unwrap_or_else(|e| panic!("{}: the scope does not decode: {e}", case.name));
    let result = apply(owned.document(), |p| owned.page(p), &scope, &CancelToken::never())?;

    let listed = epdf_engine::features::annotation::read::page::read_page_annotations(
        owned.document(),
        0,
        page,
        &epdf_engine::revision::RevisionToken::new("<session>", page, 0),
        &mut epdf_engine::features::actions::vocabulary::ActionReadBudgetTracker::default(),
        &CancelToken::never(),
    )
    .expect("the page lists");
    let remaining = listed
        .annotations
        .iter()
        .map(|annotation| {
            let value = serde_json::to_value(annotation).expect("serialize");
            RemainingAnnotation {
                subtype: value["subtype"].as_str().expect("a subtype").to_owned(),
                nm: value["nm"].as_str().map(str::to_owned),
            }
        })
        .collect();

    Ok(Applied {
        results: serde_json::to_value(&result.results).expect("serialize"),
        removed: result.removed_annotation_count,
        remaining,
    })
}

#[test]
fn an_applied_redaction_removes_what_the_typescript_engine_removed() {
    let file = load();
    assert!(file.cases.len() >= 8, "the case list shrank");
    let mut applied = 0usize;

    for case in &file.cases {
        if case.refused {
            continue;
        }
        applied += 1;
        let mine = run(&file, case).unwrap_or_else(|e| panic!("{}: {e}", case.name));

        assert_eq!(
            test_support::canonical_numbers(&mine.results),
            test_support::canonical_numbers(case.results.as_ref().expect("recorded results")),
            "{}: {}",
            case.name,
            case.covers,
        );
        assert_eq!(
            mine.removed,
            case.removed_annotation_count.expect("a recorded count"),
            "{}: the collateral count differs",
            case.name,
        );
        assert_eq!(
            mine.remaining,
            *case.remaining.as_ref().expect("recorded survivors"),
            "{}: the annotations left on the page differ",
            case.name,
        );
    }

    assert!(applied >= 4, "the applied list shrank");
}

#[test]
fn a_refused_apply_is_refused_for_the_same_reason() {
    let file = load();
    let mut refusals = 0usize;

    for case in &file.cases {
        if !case.refused {
            continue;
        }
        refusals += 1;
        let error = run(&file, case).expect_err(&case.name);
        let expected = match case.refused_with.as_deref() {
            Some("NotFound") => EngineErrorCode::NotFound,
            Some("InvalidArg") => EngineErrorCode::InvalidArg,
            other => panic!("{}: unhandled refusal code {other:?}", case.name),
        };
        assert!(error.is(expected), "{}: {} (got {error})", case.name, case.covers);
    }

    assert!(refusals >= 5, "the refusal list shrank");
}

#[test]
fn one_positional_reference_is_allowed_and_two_are_not() {
    // The rule is about BATCHING, not about positional references. Nothing
    // has shifted when the only reference resolves; the second one in a
    // batch names a different annotation by the time it gets there.
    //
    // Rust-only, because an index reference carries a revision token minted
    // by the session that listed the page. A vector cannot hold one: the
    // session id is new on every open.
    let file = load();
    let bytes = std::fs::read(test_support::workspace_root().join(&file.fixture))
        .unwrap_or_else(|e| panic!("read {}: {e}", file.fixture));
    let mut session = DocumentSession::new();
    session.open_with(bytes, None, SessionAuthority::unrestricted()).expect("open");
    let owned = session.document().expect("open");
    let page = PageObjectNumber::new(PAGE).expect("a page object number");
    let revision = epdf_engine::revision::RevisionToken::new("<session>", page, 0);

    let listed = epdf_engine::features::annotation::read::page::read_page_annotations(
        owned.document(),
        0,
        page,
        &revision,
        &mut epdf_engine::features::actions::vocabulary::ActionReadBudgetTracker::default(),
        &CancelToken::never(),
    )
    .expect("the page lists");
    let marking_indices: Vec<u32> = listed
        .annotations
        .iter()
        .enumerate()
        .filter(|(_, annotation)| {
            serde_json::to_value(annotation).expect("serialize")["subtype"] == "redact"
        })
        .map(|(index, _)| index as u32)
        .collect();
    assert!(marking_indices.len() >= 2, "the fixture has fewer markings than it used to");

    let by_index = |index: u32| epdf_engine::identity::AnnotationRef::Index {
        page,
        index,
        revision: revision.clone(),
    };

    // Two is refused, and the message says why rather than only that.
    let error = apply(
        owned.document(),
        |p| owned.page(p),
        &RedactionScope::Annotations {
            refs: vec![by_index(marking_indices[0]), by_index(marking_indices[1])],
        },
        &CancelToken::never(),
    )
    .expect_err("a batch of positional references");
    assert!(error.is(EngineErrorCode::InvalidArg), "{error}");
    assert!(
        error.to_string().contains("applying one shifts the rest"),
        "the message does not say why: {error}",
    );

    // One is fine.
    let applied = apply(
        owned.document(),
        |p| owned.page(p),
        &RedactionScope::Annotations { refs: vec![by_index(marking_indices[0])] },
        &CancelToken::never(),
    )
    .expect("one positional reference resolves before anything has shifted");
    assert_eq!(applied.results.len(), 1);
}

#[test]
fn an_unknown_page_is_refused_before_any_page_is_opened() {
    // PDFium refuses to open the page too, with the same code. The
    // difference is when: this resolves the WHOLE scope first, so a scope
    // naming one good page and one bad one redacts neither.
    let file = load();
    let bytes = std::fs::read(test_support::workspace_root().join(&file.fixture))
        .unwrap_or_else(|e| panic!("read {}: {e}", file.fixture));
    let mut session = DocumentSession::new();
    session.open_with(bytes, None, SessionAuthority::unrestricted()).expect("open");
    let owned = session.document().expect("open");
    let page = PageObjectNumber::new(PAGE).expect("a page object number");
    let before =
        epdf_engine::access::PageAccess::view(&owned.page(page).expect("page")).annotation_count();

    apply(
        owned.document(),
        |p| owned.page(p),
        &RedactionScope::Pages {
            page_object_numbers: vec![page, PageObjectNumber::new(999).expect("a number")],
        },
        &CancelToken::never(),
    )
    .expect_err("no page 999");

    assert_eq!(
        epdf_engine::access::PageAccess::view(&owned.page(page).expect("page")).annotation_count(),
        before,
        "the good page was redacted before the bad one was noticed",
    );
}

#[test]
fn applying_a_redaction_rebuilds_the_page_content() {
    // The apply takes objects out of the content stream, and the stream has
    // to be rebuilt for the removal to appear. Every other writer in this
    // port calls `generate_content` for form's sake and changes nothing;
    // here it is the call that makes the redaction visible.
    let file = load();
    let page = PageObjectNumber::new(PAGE).expect("a page object number");

    let render = |apply_first: bool| -> Vec<u8> {
        let bytes = std::fs::read(test_support::workspace_root().join(&file.fixture))
            .unwrap_or_else(|e| panic!("read {}: {e}", file.fixture));
        let mut session = DocumentSession::new();
        session.open_with(bytes, None, SessionAuthority::unrestricted()).expect("open");
        let owned = session.document().expect("open");
        if apply_first {
            apply(
                owned.document(),
                |p| owned.page(p),
                &RedactionScope::Pages { page_object_numbers: vec![page] },
                &CancelToken::never(),
            )
            .expect("the apply runs");
        }
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

#[test]
fn a_refused_apply_redacts_nothing() {
    // The half the vector cannot show, and the one that matters most here:
    // a refusal that had already removed content would be a document
    // somebody believes is intact. A reference to a stamp is the case,
    // because the whole call is refused rather than that one page.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "non-redact-ref-refused").expect("the case");

    let bytes = std::fs::read(test_support::workspace_root().join(&file.fixture))
        .unwrap_or_else(|e| panic!("read {}: {e}", file.fixture));
    let mut session = DocumentSession::new();
    session.open_with(bytes, None, SessionAuthority::unrestricted()).expect("open");
    let owned = session.document().expect("open");
    let page = PageObjectNumber::new(PAGE).expect("a page object number");
    let before =
        epdf_engine::access::PageAccess::view(&owned.page(page).expect("page")).annotation_count();

    let scope: RedactionScope =
        serde_json::from_value(case.scope.clone()).expect("the scope decodes");
    apply(owned.document(), |p| owned.page(p), &scope, &CancelToken::never())
        .expect_err("a stamp is not a redaction marking");

    assert_eq!(
        epdf_engine::access::PageAccess::view(&owned.page(page).expect("page")).annotation_count(),
        before,
        "the refusal removed something",
    );
}
