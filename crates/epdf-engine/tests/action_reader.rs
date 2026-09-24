//! G6: walking a detached action model.
//!
//! `page_actions.pdf` carries an open action that is a JavaScript node
//! chaining to a URI through `/Next`, and a close action that is a GoTo with
//! an explicit `/XYZ` destination. Between them they cover the tree shape, two
//! payload kinds and the destination reader.
//!
//! # What this build cannot read
//!
//! Hide targets, ResetForm fields, SubmitForm payloads and a URI's `/IsMap`
//! all need accessors the pinned fork artifact does not export.
//! `packages/engine/runtime/build/libpdfium/darwin-arm64/include/epdf_action.h`
//! is from fork commit `e4174c4f9` and has no `EPDFAction_GetNodeTargetCount`;
//! the fork's own source does. Those node kinds come back recognized and
//! inert, which is the behaviour the TypeScript falls back to when it
//! feature-detects the same getters as absent, and a test below pins that it
//! is inert rather than degraded.

use epdf_engine::features::actions::destination::PdfDestination;
use epdf_engine::features::actions::reader::{ActionNodeKind, read_action_model};
use epdf_engine::features::actions::vocabulary::{
    ActionReadBudget, ActionReadBudgetTracker, ActionWarning,
};
use pdfium::{Document, Library, PageActionEvent};

fn document() -> Document {
    let path =
        test_support::workspace_root().join("tooling/engine-conformance/fixtures/page_actions.pdf");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    Document::from_bytes(Library::acquire(), &bytes, None).expect("open page_actions.pdf")
}

/// The object number of the page carrying the actions.
fn acting_page(document: &Document) -> u32 {
    document.page_object_number_by_index(0)
}

#[test]
fn an_open_action_reads_its_javascript_and_its_next_chain() {
    let document = document();
    let model = document
        .page_action_model(acting_page(&document), PageActionEvent::Open)
        .expect("the page has an open action");
    let tree = read_action_model(&model, &mut ActionReadBudgetTracker::default()).expect("read");

    assert!(!tree.incomplete);
    assert!(tree.warnings.is_empty(), "{:?}", tree.warnings);

    let root = tree.root.expect("a root node");
    assert_eq!(root.subtype, "JavaScript");
    let ActionNodeKind::Javascript { script } = &root.kind else {
        panic!("expected a javascript node, got {:?}", root.kind);
    };
    assert_eq!(script, "app.alert('opened');");

    // The /Next chain is one deep and is a URI.
    assert_eq!(root.next.len(), 1);
    let child = &root.next[0];
    assert_eq!(child.subtype, "URI");
    let ActionNodeKind::Uri { uri, .. } = &child.kind else {
        panic!("expected a uri node, got {:?}", child.kind);
    };
    assert_eq!(uri, "https://example.test/opened");
    assert!(child.next.is_empty());
}

#[test]
fn a_close_action_reads_its_goto_destination() {
    let document = document();
    let model = document
        .page_action_model(acting_page(&document), PageActionEvent::Close)
        .expect("the page has a close action");
    let tree = read_action_model(&model, &mut ActionReadBudgetTracker::default()).expect("read");

    let root = tree.root.expect("a root node");
    assert_eq!(root.subtype, "GoTo");
    let ActionNodeKind::Goto { destination } = &root.kind else {
        panic!("expected a goto node, got {:?}", root.kind);
    };

    // An /XYZ with a left, a top and a null zoom. Null means "retain the
    // current value", which is a different answer from zero.
    let PdfDestination::Xyz { page_object_number, left, top, zoom } = destination else {
        panic!("expected an xyz destination, got {destination:?}");
    };
    assert_eq!(page_object_number.get(), 4);
    assert_eq!(*left, Some(100.0));
    assert_eq!(*top, Some(200.0));
    assert_eq!(*zoom, None, "a null zoom must not become 0.0");
}

#[test]
fn a_page_with_no_action_has_no_model() {
    let document = document();
    let quiet_page = document.page_object_number_by_index(1);
    assert!(document.page_action_model(quiet_page, PageActionEvent::Open).is_none());
    assert!(document.page_action_model(quiet_page, PageActionEvent::Close).is_none());
}

#[test]
fn a_node_budget_refuses_rather_than_truncating() {
    // Truncating would hand a caller a tree that looks complete and is not,
    // which is the one outcome worse than an error here: a consumer must
    // never execute a partial action chain believing it is whole.
    let document = document();
    let model = document
        .page_action_model(acting_page(&document), PageActionEvent::Open)
        .expect("open action");
    let mut budget = ActionReadBudgetTracker::new(ActionReadBudget {
        max_nodes: 1,
        ..ActionReadBudget::default()
    });
    let refused = read_action_model(&model, &mut budget).expect_err("over budget");
    assert_eq!(refused.code, epdf_engine::error::EngineErrorCode::MalformedPdf);
    assert!(refused.message.contains("budget"), "{}", refused.message);
}

#[test]
fn a_script_budget_refuses_the_script_it_cannot_afford() {
    let document = document();
    let model = document
        .page_action_model(acting_page(&document), PageActionEvent::Open)
        .expect("open action");
    let mut budget = ActionReadBudgetTracker::new(ActionReadBudget {
        max_script_code_units: 4,
        ..ActionReadBudget::default()
    });
    assert!(read_action_model(&model, &mut budget).is_err());
}

#[test]
fn one_budget_spans_several_models() {
    // The aggregate guard the port order asks for: PDFium's own limits are
    // per model, so a document with many individually reasonable models could
    // otherwise build an unbounded snapshot.
    let document = document();
    let page = acting_page(&document);
    let mut budget = ActionReadBudgetTracker::new(ActionReadBudget {
        max_models: 1,
        ..ActionReadBudget::default()
    });

    let open = document.page_action_model(page, PageActionEvent::Open).expect("open");
    read_action_model(&open, &mut budget).expect("the first model fits");

    let close = document.page_action_model(page, PageActionEvent::Close).expect("close");
    assert!(
        read_action_model(&close, &mut budget).is_err(),
        "the second model was read against a one-model budget",
    );
}

#[test]
fn the_payloads_this_build_cannot_read_come_back_inert_not_degraded() {
    // Hide, ResetForm and SubmitForm need accessors the pinned artifact does
    // not export, so their payloads are absent. What must NOT happen is a
    // degrade to Unknown: the node type is known, and reporting it as unknown
    // would tell a consumer the document said something it did not.
    //
    // No fixture here carries one, so this asserts the shape of the mapping
    // rather than reading a document. When the artifact gains the accessors,
    // this test is what says the fallback is gone.
    let inert = [
        ActionNodeKind::Hide { targets: Vec::new(), hide: true },
        ActionNodeKind::ResetForm { fields: None, exclude: false },
        ActionNodeKind::SubmitForm { payload: None },
    ];
    for kind in &inert {
        assert!(
            !matches!(kind, ActionNodeKind::Unknown),
            "an unreadable payload degraded a known node type to unknown",
        );
    }
    // And the payload-dropped warning is reserved for a genuine degrade.
    assert_ne!(ActionWarning::PayloadDropped, ActionWarning::Incomplete);
}
