//! G11's delete gate: the same annotation goes, and the same id comes back.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run annotation-delete
//!
//! Every case deletes from `annotation_base.pdf`, the fixture with one
//! annotation of every identity shape, so the three reference kinds are
//! exercised against annotations that really differ rather than against three
//! spellings of one.
//!
//! What is compared is the reported id, the envelope, AND the `/NM` of every
//! annotation left on the page in order. That last one is what catches a
//! delete that removed the wrong entry: the id and the envelope would both
//! look right.

use epdf_engine::cancel::CancelToken;
use epdf_engine::error::EngineErrorCode;
use epdf_engine::features::actions::vocabulary::ActionReadBudgetTracker;
use epdf_engine::features::annotation::read::page::read_page_annotations;
use epdf_engine::features::annotation::write::delete::{WidgetAttachment, delete_annotation};
use epdf_engine::identity::{AnnotationRef, PageObjectNumber};
use epdf_engine::revision::{PageState, RevisionToken, WeakAnnotationState};
use epdf_engine::session::authority::SessionAuthority;
use epdf_engine::session::document::DocumentSession;
use serde::Deserialize;

const SCHEMA_VERSION: u32 = 1;
const SESSION: &str = "<session>";

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
    page_object_number: u32,
    #[serde(rename = "ref")]
    annotation_ref: serde_json::Value,
    deleted: serde_json::Value,
    meta: ExpectedMeta,
    /// Every surviving annotation's `/NM`, in order.
    remaining: Vec<Option<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedMeta {
    changed: serde_json::Value,
    weak_refs_invalidated: bool,
    should_refetch: serde_json::Value,
}

fn load() -> VectorFile {
    let path = test_support::corpus::vectors_dir("annotation").join("delete.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: VectorFile = serde_json::from_str(&text).expect("delete.json");
    assert_eq!(file.schema_version, SCHEMA_VERSION, "vector envelope moved");
    assert_eq!(file.kind, "annotation.delete");
    file
}

fn session(file: &VectorFile) -> DocumentSession {
    let bytes = std::fs::read(test_support::workspace_root().join(&file.fixture))
        .unwrap_or_else(|e| panic!("read {}: {e}", file.fixture));
    let mut session = DocumentSession::new();
    session.open_with(bytes, None, SessionAuthority::unrestricted()).expect("open");
    session
}

fn list(session: &DocumentSession, page: PageObjectNumber) -> Vec<Option<String>> {
    let listing = read_page_annotations(
        session.document().expect("open").document(),
        0,
        page,
        &RevisionToken::new(SESSION, page, 0),
        &mut ActionReadBudgetTracker::default(),
        &CancelToken::never(),
    )
    .expect("list");
    listing.annotations.into_iter().map(|a| a.base.nm).collect()
}

/// The page state a delete is measured against.
///
/// The BEFORE state carries the page's weak flag as it was, which is what
/// decides the refetch; the after state carries the bumped generation. The
/// session's own revision store is not used, because this test drives the
/// mutator directly rather than through a session facade that does not exist
/// yet.
fn states(page: PageObjectNumber, weak_before: bool) -> (PageState, PageState) {
    let make = |generation: u64, weak: bool| PageState {
        page_object_number: page,
        revision: RevisionToken::new(SESSION, page, generation),
        weak_annotation_state: WeakAnnotationState::Known { has_any_weak_annotations: weak },
    };
    (make(0, weak_before), make(1, weak_before))
}

#[test]
fn a_delete_removes_the_annotation_the_typescript_engine_removed() {
    let file = load();
    assert!(file.cases.len() >= 5, "the case list shrank");
    let mut weak_results = 0usize;
    let mut durable_results = 0usize;

    for case in &file.cases {
        let session = session(&file);
        let page = PageObjectNumber::new(case.page_object_number).expect("page number");
        let (before, after) = states(page, true);

        let annotation_ref: AnnotationRef = serde_json::from_value(case.annotation_ref.clone())
            .unwrap_or_else(|e| panic!("{}: the ref does not decode: {e}", case.name));

        let result = {
            let owned = session.document().expect("open");
            let view = owned.document().page(0).expect("page");
            delete_annotation(
                &view,
                &annotation_ref,
                &before,
                &after,
                // No form in this fixture, so nothing is attached. The
                // closure exists so the form model is not built for a
                // document that has none.
                &|_| WidgetAttachment::Deletable,
                &CancelToken::never(),
            )
            .unwrap_or_else(|e| panic!("{}: delete: {e}", case.name))
        };

        assert_eq!(
            serde_json::to_value(&result.deleted).expect("serialize"),
            case.deleted,
            "{}: the reported id: {}",
            case.name,
            case.covers,
        );
        if result.deleted.is_some() {
            durable_results += 1;
        } else {
            weak_results += 1;
        }

        let meta = serde_json::to_value(&result.meta).expect("serialize");
        assert_eq!(meta["changed"], case.meta.changed, "{}: the changed list", case.name);
        assert_eq!(
            meta["weakRefsInvalidated"].as_bool(),
            Some(case.meta.weak_refs_invalidated),
            "{}: the weak flag",
            case.name,
        );
        assert_eq!(meta["shouldRefetch"], case.meta.should_refetch, "{}", case.name);

        // The part an id comparison cannot see.
        assert_eq!(
            list(&session, page),
            case.remaining,
            "{}: the wrong annotation was removed: {}",
            case.name,
            case.covers,
        );
    }

    assert!(durable_results > 0, "no case reports a durable id");
    assert!(weak_results > 0, "no case reports none");
}

#[test]
fn a_reference_that_names_nothing_is_an_invalid_reference() {
    // Probed before anything is removed, so the caller is told which of the
    // two things went wrong rather than being handed a bare failure.
    let file = load();
    let session = session(&file);
    let page = PageObjectNumber::new(file.cases[0].page_object_number).expect("page number");
    let (before, after) = states(page, true);
    let count = list(&session, page).len();

    let owned = session.document().expect("open");
    let view = owned.document().page(0).expect("page");
    for (annotation_ref, what) in [
        (AnnotationRef::ObjectNumber { page, annot: 9_999 }, "an object number nothing has"),
        (AnnotationRef::Nm { page, nm: "no-such-name".into() }, "an /NM nothing has"),
        (
            AnnotationRef::Index {
                page,
                index: 9_999,
                revision: RevisionToken::new(SESSION, page, 0),
            },
            "an index past the end",
        ),
    ] {
        let refused = delete_annotation(
            &view,
            &annotation_ref,
            &before,
            &after,
            &|_| WidgetAttachment::Deletable,
            &CancelToken::never(),
        )
        .expect_err(what);
        assert!(refused.is(EngineErrorCode::InvalidReference), "{what}: {refused}");
    }
    let _ = view;
    let _ = owned;

    // And nothing was removed by any of the three.
    assert_eq!(list(&session, page).len(), count);
}

#[test]
fn an_attached_widget_is_not_deleted_through_the_annotation_plane() {
    // A widget attached to a field is half of that field, and removing it
    // here would leave the field pointing at nothing. An INERT widget is an
    // ordinary annotation and deletes like one, which is the distinction the
    // guard exists to make rather than refusing every widget.
    let bytes = std::fs::read(
        test_support::workspace_root()
            .join("tooling/engine-conformance/fixtures/annotation_appearances.pdf"),
    )
    .expect("read");
    let mut session = DocumentSession::new();
    session.open_with(bytes, None, SessionAuthority::unrestricted()).expect("open");

    let page = PageObjectNumber::new(
        session.document().expect("open").document().page_object_number_by_index(0),
    )
    .expect("page number");
    let (before, after) = states(page, true);

    // The fixture's attached widget is the one reachable from /AcroForm.
    let listing = read_page_annotations(
        session.document().expect("open").document(),
        0,
        page,
        &RevisionToken::new(SESSION, page, 0),
        &mut ActionReadBudgetTracker::default(),
        &CancelToken::never(),
    )
    .expect("list");
    let attached = listing
        .annotations
        .iter()
        .find(|a| a.base.nm.as_deref() == Some("a-real-field-widget"))
        .expect("the attached widget");
    let orphan = listing
        .annotations
        .iter()
        .find(|a| a.base.nm.as_deref() == Some("an-orphan-widget"))
        .expect("the orphan widget");
    let attached_ref = attached.base.annotation_ref.clone();
    let orphan_ref = orphan.base.annotation_ref.clone();
    let before_count = listing.annotations.len();

    let owned = session.document().expect("open");
    let view = owned.document().page(0).expect("page");

    // The resolver answers from the form model, the same way the reader's
    // join does.
    let model = owned.document().form_model().expect("a form model");
    let resolve = |widget: u32| match model.field_index_for_widget(widget) {
        Some(_) => WidgetAttachment::Attached,
        None => WidgetAttachment::Deletable,
    };

    let refused =
        delete_annotation(&view, &attached_ref, &before, &after, &resolve, &CancelToken::never())
            .expect_err("an attached widget");
    assert!(refused.is(EngineErrorCode::InvalidArg), "{refused}");

    // The inert one goes.
    delete_annotation(&view, &orphan_ref, &before, &after, &resolve, &CancelToken::never())
        .expect("an inert widget deletes like any other annotation");

    // And so does a widget with no object number at all, which the join key
    // cannot even address. `resolve_widget` is asked about zero there, and
    // the guard above it means it is not asked at all.
    let bare_ref = listing
        .annotations
        .iter()
        .find(|a| a.base.nm.as_deref() == Some("a-bare-widget"))
        .expect("the direct widget")
        .base
        .annotation_ref
        .clone();
    delete_annotation(&view, &bare_ref, &before, &after, &resolve, &CancelToken::never())
        .expect("a widget with no object number is an ordinary annotation");
    let _ = model;
    let _ = view;
    let _ = owned;

    let after_list = list(&session, page);
    assert_eq!(after_list.len(), before_count - 2, "exactly the two inert widgets were removed");
    assert!(
        after_list.iter().any(|nm| nm.as_deref() == Some("a-real-field-widget")),
        "the attached widget is still there",
    );
    for gone in ["an-orphan-widget", "a-bare-widget"] {
        assert!(!after_list.iter().any(|nm| nm.as_deref() == Some(gone)), "{gone} is still there",);
    }
}
