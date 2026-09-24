//! G11's move gate: the same block ends up in the same place.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run annotation-move
//!
//! `annotation_base.pdf` is the page, because a move is about ORDER and that
//! fixture's nine annotations have identities that differ: reordering three
//! identical ones would pass for almost any implementation.
//!
//! A `/NM` the engine minted is compared as a placeholder rather than
//! verbatim. A move strengthens every weak annotation in its batch, the
//! stamp is a fresh UUID, and its value is opaque to everyone including the
//! engine; what matters is that a name appeared where there was none.

use epdf_engine::cancel::CancelToken;
use epdf_engine::error::EngineErrorCode;
use epdf_engine::features::actions::vocabulary::ActionReadBudgetTracker;
use epdf_engine::features::annotation::read::page::read_page_annotations;
use epdf_engine::features::annotation::write::move_::move_annotations;
use epdf_engine::identity::{AnnotationRef, PageObjectNumber};
use epdf_engine::ids::SeededIdSource;
use epdf_engine::revision::{PageState, RevisionToken, WeakAnnotationState};
use epdf_engine::session::authority::SessionAuthority;
use epdf_engine::session::document::DocumentSession;
use serde::Deserialize;

const SCHEMA_VERSION: u32 = 1;
const SESSION: &str = "<session>";
const STAMPED: &str = "<stamped>";

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
    from: Vec<u32>,
    to_index: u32,
    page_object_number: u32,
    before_order: Vec<Option<String>>,
    after_order: Vec<Option<String>>,
    moved_order: Vec<Option<String>>,
    moved_indices: Vec<u32>,
    meta: ExpectedMeta,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedMeta {
    changed: serde_json::Value,
    weak_refs_invalidated: bool,
    should_refetch: serde_json::Value,
}

fn load() -> VectorFile {
    let path = test_support::corpus::vectors_dir("annotation").join("move.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: VectorFile = serde_json::from_str(&text).expect("move.json");
    assert_eq!(file.schema_version, SCHEMA_VERSION, "vector envelope moved");
    assert_eq!(file.kind, "annotation.move");
    file
}

/// A minted name, replaced by the placeholder the vector carries.
fn normalize_name(nm: Option<String>) -> Option<String> {
    nm.map(|nm| if is_uuid_v4(&nm) { STAMPED.to_owned() } else { nm })
}

fn is_uuid_v4(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes[14] == b'4'
        && value.chars().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

fn normalize_json(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::String(s) if is_uuid_v4(s) => {
            serde_json::Value::String(STAMPED.to_owned())
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(normalize_json).collect())
        }
        serde_json::Value::Object(fields) => serde_json::Value::Object(
            fields.iter().map(|(k, v)| (k.clone(), normalize_json(v))).collect(),
        ),
        _ => value.clone(),
    }
}

fn session(file: &VectorFile) -> DocumentSession {
    let bytes = std::fs::read(test_support::workspace_root().join(&file.fixture))
        .unwrap_or_else(|e| panic!("read {}: {e}", file.fixture));
    let mut session = DocumentSession::new();
    session.open_with(bytes, None, SessionAuthority::unrestricted()).expect("open");
    session
}

fn order(session: &DocumentSession, page: PageObjectNumber) -> Vec<Option<String>> {
    let listing = read_page_annotations(
        session.document().expect("open").document(),
        0,
        page,
        &RevisionToken::new(SESSION, page, 0),
        &mut ActionReadBudgetTracker::default(),
        &CancelToken::never(),
    )
    .expect("list");
    listing.annotations.into_iter().map(|a| normalize_name(a.base.nm)).collect()
}

/// The states a move is measured against: one generation apart, on a page
/// that has weak annotations, which is what makes the refetch verdict a test.
fn states(page: PageObjectNumber) -> (PageState, PageState) {
    let make = |generation: u64| PageState {
        page_object_number: page,
        revision: RevisionToken::new(SESSION, page, generation),
        weak_annotation_state: WeakAnnotationState::Known { has_any_weak_annotations: true },
    };
    (make(0), make(1))
}

#[derive(Debug)]
struct Moved {
    order: Vec<Option<String>>,
    names: Vec<Option<String>>,
    indices: Vec<u32>,
    meta: serde_json::Value,
}

fn run(file: &VectorFile, case: &Case) -> Result<Moved, epdf_engine::error::EngineError> {
    let session = session(file);
    let page = PageObjectNumber::new(case.page_object_number).expect("page number");
    let (before, after) = states(page);

    let refs: Vec<AnnotationRef> = case
        .from
        .iter()
        .map(|index| AnnotationRef::Index {
            page,
            index: *index,
            revision: before.revision.clone(),
        })
        .collect();

    let result = {
        let owned = session.document().expect("open");
        let view = owned.document().page(0).expect("page");
        move_annotations(
            owned.document(),
            &view,
            0,
            page,
            &refs,
            case.to_index,
            &before,
            &after,
            &SeededIdSource::new(11),
            &CancelToken::never(),
        )?
    };

    Ok(Moved {
        order: order(&session, page),
        names: result.moved.iter().map(|a| normalize_name(a.base.nm.clone())).collect(),
        indices: result.moved.iter().map(|a| a.base.index).collect(),
        meta: normalize_json(&serde_json::to_value(&result.meta).expect("serialize")),
    })
}

#[test]
fn a_move_leaves_the_order_the_typescript_engine_left() {
    let file = load();
    assert!(file.cases.len() >= 7, "the case list shrank");

    for case in &file.cases {
        // The starting order, so a case whose fixture drifted fails here
        // rather than being blamed on the move.
        let start = order(&session(&file), PageObjectNumber::new(case.page_object_number).unwrap());
        assert_eq!(
            start, case.before_order,
            "{}: the fixture no longer starts this way",
            case.name,
        );

        let moved = run(&file, case).unwrap_or_else(|e| panic!("{}: move: {e}", case.name));
        assert_eq!(
            moved.order, case.after_order,
            "{}: the resulting order: {}",
            case.name, case.covers,
        );
        assert_eq!(moved.names, case.moved_order, "{}: the moved block", case.name);
        assert_eq!(moved.indices, case.moved_indices, "{}: where the block landed", case.name);
    }
}

#[test]
fn the_block_lands_where_it_was_asked_to_in_the_order_it_was_given() {
    // The two properties an order comparison could satisfy by accident. The
    // block is CONTIGUOUS at the destination, and its internal order is the
    // caller's rather than the document's.
    let file = load();
    for case in &file.cases {
        let moved = run(&file, case).unwrap_or_else(|e| panic!("{}: {e}", case.name));
        let expected: Vec<u32> =
            (0..case.from.len() as u32).map(|offset| case.to_index + offset).collect();
        assert_eq!(moved.indices, expected, "{}: the block is not contiguous", case.name);

        // And what landed there is what was asked for, in that order.
        for (offset, name) in moved.names.iter().enumerate() {
            assert_eq!(
                moved.order.get(case.to_index as usize + offset),
                Some(name),
                "{}: the block's order does not match the page",
                case.name,
            );
        }
    }

    // The case that makes the order claim non-vacuous: the same three
    // annotations, asked for in the opposite order, come out reversed.
    let forward =
        file.cases.iter().find(|c| c.name == "a-scattered-block").expect("the scattered case");
    let backward = file
        .cases
        .iter()
        .find(|c| c.name == "the-callers-order-not-the-documents")
        .expect("the reversed case");
    let mut reversed = run(&file, backward).expect("move").names;
    reversed.reverse();
    assert_eq!(run(&file, forward).expect("move").names, reversed);
}

#[test]
fn a_move_is_structural_and_says_so() {
    let file = load();
    for case in &file.cases {
        let moved = run(&file, case).unwrap_or_else(|e| panic!("{}: {e}", case.name));
        let meta = moved.meta.as_object().expect("an object");
        assert_eq!(
            meta["changed"],
            normalize_json(&case.meta.changed),
            "{}: the changed list",
            case.name,
        );
        assert_eq!(
            meta["weakRefsInvalidated"].as_bool(),
            Some(case.meta.weak_refs_invalidated),
            "{}",
            case.name,
        );
        assert_eq!(meta["shouldRefetch"], case.meta.should_refetch, "{}", case.name);
        assert!(
            !meta["shouldRefetch"].is_null(),
            "{}: the page has weak annotations and a move did not ask for a refetch",
            case.name,
        );
    }
}

#[test]
fn a_move_that_changes_nothing_is_still_structural() {
    // Moving a block to where it already is leaves the order alone and
    // still bumps and still asks for a refetch. The engine does not inspect
    // whether the result happened to be the same, and it should not: a
    // revision that sometimes advances is not one a weak reference can be
    // checked against.
    let file = load();
    let case = file
        .cases
        .iter()
        .find(|c| c.name == "a-move-that-changes-nothing")
        .expect("the no-op case");
    let moved = run(&file, case).expect("move");

    // The order is the same except for the names the move stamped, which is
    // why the comparison is against the recorded after-order rather than
    // against the before-order.
    assert_eq!(moved.order, case.after_order);
    assert!(!moved.meta["shouldRefetch"].is_null());
}

#[test]
fn a_move_refuses_what_it_cannot_do_and_changes_nothing() {
    let file = load();
    let case = &file.cases[0];
    let page = PageObjectNumber::new(case.page_object_number).expect("page number");
    let (before, after) = states(page);
    let ids = SeededIdSource::new(11);

    let reference =
        |index: u32| AnnotationRef::Index { page, index, revision: before.revision.clone() };
    let other_page = PageObjectNumber::new(page.get() + 1).expect("another page number");

    for (refs, to_index, what) in [
        (vec![], 0u32, "an empty batch"),
        (vec![reference(0), reference(0)], 1, "two references to one annotation"),
        (vec![reference(0)], 9, "a destination past the post-removal count"),
        (
            vec![AnnotationRef::Index {
                page: other_page,
                index: 0,
                revision: before.revision.clone(),
            }],
            0,
            "a reference to another page",
        ),
    ] {
        let session = session(&file);
        let start = order(&session, page);
        {
            let owned = session.document().expect("open");
            let view = owned.document().page(0).expect("page");
            let refused = move_annotations(
                owned.document(),
                &view,
                0,
                page,
                &refs,
                to_index,
                &before,
                &after,
                &ids,
                &CancelToken::never(),
            )
            .expect_err(what);
            assert!(refused.is(EngineErrorCode::InvalidArg), "{what}: {refused}");
        }
        assert_eq!(order(&session, page), start, "{what}: the page moved anyway");
    }

    // And a reference that names nothing is an invalid reference rather than
    // an invalid argument: the caller's request was well formed and its
    // target was not there.
    let session = session(&file);
    let owned = session.document().expect("open");
    let view = owned.document().page(0).expect("page");
    let refused = move_annotations(
        owned.document(),
        &view,
        0,
        page,
        &[reference(9_999)],
        0,
        &before,
        &after,
        &ids,
        &CancelToken::never(),
    )
    .expect_err("an index past the end");
    assert!(refused.is(EngineErrorCode::InvalidReference), "{refused}");
}
