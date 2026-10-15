//! G10's finalization gate: what happens after every successful write.
//!
//! No vectors. The two things a finalizer does are a sequence bump and a
//! layer save, and neither is in any operation's return value: the bump is
//! session state a cache keys on, and the save is bytes the host persists.
//! A vector of DTOs would see neither.

use epdf_engine::cancel::CancelToken;
use epdf_engine::features::annotation::write::context::AnnotationWriteContext;
use epdf_engine::features::annotation::write::create::create_annotation;
use epdf_engine::features::annotation::write::draft::{AnnotationActor, AnnotationDraft};
use epdf_engine::geom::PdfRotation;
use epdf_engine::identity::PageObjectNumber;
use epdf_engine::session::authority::SessionAuthority;
use epdf_engine::session::document::DocumentSession;
use epdf_engine::session::save::{
    DocumentEventInit, EventOrigin, MutationFamily, OriginKind, finish_mutation, publish,
};

const FIXTURE: &str = "tooling/engine-conformance/fixtures/rich_metadata.pdf";

fn page(n: u32) -> PageObjectNumber {
    PageObjectNumber::new(n).expect("a page object number")
}

fn open() -> DocumentSession {
    let bytes = std::fs::read(test_support::workspace_root().join(FIXTURE))
        .unwrap_or_else(|e| panic!("read {FIXTURE}: {e}"));
    let mut session = DocumentSession::new();
    session.open_with(bytes, None, SessionAuthority::unrestricted()).expect("open");
    session
}

#[test]
fn an_ordinary_mutation_bumps_the_sequence_and_a_self_sequenced_one_does_not() {
    // The whole judgement the finalizer makes. A form write re-reads
    // through a version-keyed model before it returns, so it bumps first
    // and a second bump here would rebuild every such cache for nothing.
    let session = open();
    let before = session.mutation_sequence();

    let ordinary = finish_mutation(&session, MutationFamily::Ordinary, &CancelToken::never())
        .expect("finalize");
    assert_eq!(ordinary.mutation_sequence, before + 1, "an ordinary mutation did not bump");

    let already = finish_mutation(&session, MutationFamily::SelfSequenced, &CancelToken::never())
        .expect("finalize");
    assert_eq!(
        already.mutation_sequence,
        before + 1,
        "a self-sequenced mutation was bumped a second time",
    );
}

#[test]
fn a_base_session_has_no_artifact_to_persist() {
    // A base session's writes are in the document and go out with the next
    // save. Only a layer has something to persist separately, and reporting
    // an empty artifact for a base one would give a host bytes to write
    // that mean nothing.
    let session = open();
    let finished = finish_mutation(&session, MutationFamily::Ordinary, &CancelToken::never())
        .expect("finalize");
    assert!(finished.artifact.is_none(), "a base session produced an artifact");
}

#[test]
fn the_family_comes_from_the_event_rather_than_from_the_caller() {
    // Derived rather than passed, so a caller cannot finalize a form write
    // as an ordinary one. Ten event types are self-sequenced and the rest
    // are not, and getting one wrong is a stale cache nobody notices.
    let nothing = serde_json::Value::Null;
    for (event, expected) in [
        (
            DocumentEventInit::AnnotationCreated {
                page_object_number: page(3),
                result: serde_json::json!({}),
            },
            MutationFamily::Ordinary,
        ),
        (
            DocumentEventInit::PagesMoved {
                page_object_numbers: vec![page(3)],
                dest_index: 0,
                result: serde_json::json!({}),
            },
            MutationFamily::Ordinary,
        ),
        (DocumentEventInit::MetadataUpdated(nothing.clone()), MutationFamily::Ordinary),
        (DocumentEventInit::AttachmentCreated(nothing.clone()), MutationFamily::Ordinary),
        (DocumentEventInit::PagesFlattened(nothing.clone()), MutationFamily::SelfSequenced),
        (DocumentEventInit::RedactionApplied(nothing.clone()), MutationFamily::SelfSequenced),
        (DocumentEventInit::FormValueChanged(nothing.clone()), MutationFamily::SelfSequenced),
        (DocumentEventInit::FormFieldCreated(nothing.clone()), MutationFamily::SelfSequenced),
        (DocumentEventInit::FormWidgetDetached(nothing.clone()), MutationFamily::SelfSequenced),
        (DocumentEventInit::FormEffectsApplied(nothing.clone()), MutationFamily::SelfSequenced),
    ] {
        assert_eq!(event.family(), expected, "{}", event.type_name());
    }
}

#[test]
fn the_reported_type_name_is_the_one_on_the_wire() {
    // Two spellings of the same thing, and they have to agree: the tag is
    // what a consumer switches on and `type_name` is what this engine says
    // it sent. Every variant is listed, so a new one cannot be added
    // without deciding both.
    let nothing = serde_json::Value::Null;
    let empty = || serde_json::json!({});
    let every: Vec<DocumentEventInit> = vec![
        DocumentEventInit::AnnotationCreated { page_object_number: page(3), result: empty() },
        DocumentEventInit::AnnotationUpdated { page_object_number: page(3), result: empty() },
        DocumentEventInit::AnnotationDeleted { page_object_number: page(3), result: empty() },
        DocumentEventInit::AnnotationMoved { page_object_number: page(3), result: empty() },
        DocumentEventInit::AnnotationsFlattened(nothing.clone()),
        DocumentEventInit::PagesMoved {
            page_object_numbers: vec![page(3)],
            dest_index: 0,
            result: empty(),
        },
        DocumentEventInit::PagesRotated {
            page_object_numbers: vec![page(3)],
            rotation: PdfRotation::Clockwise90,
            result: empty(),
        },
        DocumentEventInit::PagesDeleted { page_object_numbers: vec![page(3)], result: empty() },
        DocumentEventInit::PagesInserted { dest_index: None, result: empty() },
        DocumentEventInit::PagesFlattened(nothing.clone()),
        DocumentEventInit::PagesNamed {
            name: String::from("cover"),
            page_object_number: None,
            result: empty(),
        },
        DocumentEventInit::RedactionApplied(nothing.clone()),
        DocumentEventInit::MetadataUpdated(nothing.clone()),
        DocumentEventInit::AttachmentCreated(nothing.clone()),
        DocumentEventInit::AttachmentDeleted(nothing.clone()),
        DocumentEventInit::FormValueChanged(nothing.clone()),
        DocumentEventInit::FormImported(nothing.clone()),
        DocumentEventInit::FormRepaired(nothing.clone()),
        DocumentEventInit::FormFieldCreated(nothing.clone()),
        DocumentEventInit::FormFieldUpdated(nothing.clone()),
        DocumentEventInit::FormFieldDeleted(nothing.clone()),
        DocumentEventInit::FormWidgetAttached(nothing.clone()),
        DocumentEventInit::FormWidgetDetached(nothing.clone()),
        DocumentEventInit::FormEffectsApplied(nothing.clone()),
        DocumentEventInit::PageViewportsChanged(nothing.clone()),
        DocumentEventInit::SignaturePrepared { signing_id: String::from("s"), field: empty() },
        DocumentEventInit::SignatureCompleted(nothing),
        DocumentEventInit::SignatureAborted { signing_id: String::from("s") },
    ];
    // The TypeScript union (`core/src/events/DocumentEvent.ts`) has thirty
    // members. Two are not here: `stream.desynced` is a transport notice no
    // engine publishes locally, and `document.versioned` follows
    // `signature.completed` from `epdf-viewer-host`'s publisher, because a
    // host answer carries one event.
    assert_eq!(every.len(), 28, "the event list and the TypeScript union disagree");

    for event in &every {
        let wire = serde_json::to_value(event).expect("serialize");
        assert_eq!(
            wire["type"].as_str(),
            Some(event.type_name()),
            "the tag and the reported name disagree",
        );
    }
}

#[test]
fn an_event_carries_its_result_verbatim() {
    // The invariant the whole stream rests on: a consumer that acted on an
    // operation's return value and one that acted on its event acted on the
    // same thing. So the result rides as it came, not as a summary.
    let session = open();
    let owned = session.document().expect("open");
    let page = PageObjectNumber::new(3).expect("a page object number");
    let listed = epdf_engine::features::annotation::read::page::read_page_annotations(
        owned.document(),
        0,
        page,
        &epdf_engine::revision::RevisionToken::new("<session>", page, 0),
        &mut epdf_engine::features::actions::vocabulary::ActionReadBudgetTracker::default(),
        &CancelToken::never(),
    )
    .expect("the page lists");

    let draft: AnnotationDraft = serde_json::from_value(serde_json::json!({
        "subtype": "square",
        "rect": { "left": 10.0, "bottom": 10.0, "right": 100.0, "top": 100.0 },
    }))
    .expect("the draft decodes");
    let view = owned.document().page(0).expect("page");
    let created = create_annotation(
        owned.document(),
        &view,
        page,
        &listed.page_state,
        &listed.page_state,
        &draft,
        &AnnotationActor::default(),
        &AnnotationWriteContext::empty(),
        &epdf_engine::clock::FixedClock(epdf_engine::clock::Timestamp::utc(1_700_000_000_000)),
        &CancelToken::never(),
    )
    .expect("the annotation is created");

    let result = serde_json::to_value(&created).expect("serialize");
    let event = publish(
        DocumentEventInit::AnnotationCreated { page_object_number: page, result: result.clone() },
        EventOrigin {
            kind: OriginKind::Local,
            session_id: String::from("engine-1"),
            sub: None,
            ts: 1_700_000_000_000,
            server_id: None,
        },
    );

    let wire = serde_json::to_value(&event).expect("serialize");
    assert_eq!(wire["type"], "annotation.created", "the tag is not on the envelope");
    assert_eq!(wire["origin"]["kind"], "local", "the provenance is not on the envelope");
    // The result, unchanged. Every field the operation returned is on the
    // envelope beside the tag and the provenance, which is what lets a
    // consumer of the event and a caller of the operation act on the same
    // thing. A summary here would let the two disagree.
    let returned = result.as_object().expect("the result is an object");
    assert!(!returned.is_empty(), "the operation returned nothing to carry");
    for (key, value) in returned {
        assert_eq!(&wire[key], value, "the event changed '{key}' on its way out");
    }
}

#[test]
fn a_local_origin_has_no_subject_and_no_server_id() {
    // Provenance, not transport. A local engine has nobody authenticated
    // and no audit log, so both are absent rather than invented; a
    // consumer using `sub` for attribution can tell.
    let origin = EventOrigin {
        kind: OriginKind::Local,
        session_id: String::from("engine-1"),
        sub: None,
        ts: 0,
        server_id: None,
    };
    let wire = serde_json::to_value(&origin).expect("serialize");
    assert_eq!(wire["sub"], serde_json::Value::Null);
    assert_eq!(wire["serverId"], serde_json::Value::Null);
    assert_eq!(wire["sessionId"], "engine-1");
}
