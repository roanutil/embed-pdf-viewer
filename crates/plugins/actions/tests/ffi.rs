#![cfg(feature = "uniffi")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use epdf_ffi::{KernelThread, ViewerError};
use epdf_plugin_actions::ffi::{
    ActionContext, ActionSource, ActionTreeSource, ActionTrigger, ActionsEvent,
    ActionsEventListener, ActionsHandle, PdfActionTree,
};
use epdf_plugin_actions::{
    ACTIONS, ActionDiagnosticCode, ActionOrigin, ActionPolicyDecision, ActionPolicyPatch,
    ActionResultStatus, ActionTriggerEvent, ActionsConfig, DocumentTriggerEvent, OpenSequence,
    PartialActionPolicyRow, actions_plugin,
};
use futures::executor::block_on;
use plugin_conformance::test_kernel::{DOCUMENT, Reply, TestKernel, TestSpec};
use serde_json::{Value, json};

#[derive(Default)]
struct Events(Mutex<Vec<ActionsEvent>>, AtomicUsize);

impl ActionsEventListener for Events {
    fn on_event(&self, event: ActionsEvent) {
        self.0.lock().unwrap().push(event);
    }
    fn on_end(&self) {
        self.1.fetch_add(1, Ordering::SeqCst);
    }
}

pub fn named(name: &str) -> Value {
    json!({
        "root": { "type": "named", "subtype": "Named", "name": name, "next": [] },
        "incomplete": false, "warningFlags": 0, "warnings": []
    })
}

pub fn page(pon: u32) -> Value {
    json!({
        "index": pon - 1,
        "ref": { "kind": "objectNumber", "pageObjectNumber": pon },
        "label": null,
        "size": { "width": 600, "height": 800 },
        "rotation": 0,
        "userUnit": 1,
        "boxes": {
            "media": { "left": 0, "bottom": 0, "right": 600, "top": 800 },
            "crop": { "left": 0, "bottom": 0, "right": 600, "top": 800 }
        }
    })
}

/// One page, the catalog's open and will-save trees, an empty form and no
/// annotations: what the dispatcher and the script world read.
pub fn spec() -> TestSpec {
    TestSpec::new(json!({
        "documents": {
            DOCUMENT: {
                "id": DOCUMENT, "security": { "allows": "*" },
                "services": ["actions"], "pages": [page(1)]
            }
        }
    }))
    .respond("actions.read", |_| {
        Ok(Reply::value(json!({
            "nameTreeScripts": [], "openAction": named("OpenAction"),
            "openDestination": null, "willSave": named("WS")
        })))
    })
    .respond("forms.list", |_| {
        Ok(Reply::value(json!({
            "formKind": "none", "needsAppearances": false, "fields": [], "calculationOrder": []
        })))
    })
    .respond_page("page.annotations.list", |_, _| Ok(Reply::value(json!({ "annotations": [] }))))
}

fn quiet() -> ActionsConfig {
    ActionsConfig { open_sequence: Some(OpenSequence::Off), ..Default::default() }
}

fn handle() -> (KernelThread<TestKernel>, Arc<ActionsHandle>) {
    let thread =
        KernelThread::spawn(|_| TestKernel::open(vec![actions_plugin(quiet())], spec())).unwrap();
    let post = thread.post();
    let handle =
        block_on(thread.call(move |k| ActionsHandle::new(k.capability(&ACTIONS).unwrap(), post)))
            .unwrap();
    (thread, handle)
}

fn code(error: &ViewerError) -> Option<&str> {
    match error {
        ViewerError::Kernel { code, .. } => code.as_deref(),
        ViewerError::Closed => None,
    }
}

fn user() -> ActionContext {
    ActionContext {
        origin: ActionOrigin::User,
        source: ActionSource::Api,
        event: ActionTriggerEvent::Activate,
    }
}

fn tree(json: Value) -> Arc<PdfActionTree> {
    PdfActionTree::from_json(json.to_string()).unwrap()
}

#[test]
fn the_policy_reads_and_patches() {
    let (_thread, actions) = handle();
    let policy = block_on(actions.policy()).unwrap();
    assert_eq!(policy.uri.user, ActionPolicyDecision::Adapter);
    assert_eq!(policy.uri.hover, ActionPolicyDecision::Report);
    let patch = ActionPolicyPatch {
        uri: Some(PartialActionPolicyRow {
            user: Some(ActionPolicyDecision::Block),
            ..Default::default()
        }),
        ..Default::default()
    };
    block_on(actions.update_policy(patch)).unwrap();
    let policy = block_on(actions.policy()).unwrap();
    assert_eq!(policy.uri.user, ActionPolicyDecision::Block);
    assert_eq!(policy.uri.hover, ActionPolicyDecision::Report);
}

#[test]
fn an_incomplete_tree_is_refused() {
    let (_thread, actions) = handle();
    let mut incomplete = named("NextPage");
    incomplete["incomplete"] = json!(true);
    let incomplete = tree(incomplete);
    assert!(!block_on(actions.can_execute(incomplete.clone(), user())).unwrap());
    let result = block_on(actions.execute(incomplete, user())).unwrap();
    assert_eq!(result.status, ActionResultStatus::Refused);
    assert!(result.diagnostics.iter().any(|d| d.code == ActionDiagnosticCode::IncompleteTree));
}

#[test]
fn a_goto_tree_is_allowed_for_a_user() {
    let (_thread, actions) = handle();
    let goto = tree(json!({
        "root": { "type": "goto", "subtype": "GoTo",
                  "destination": { "kind": "fit", "page": { "kind": "objectNumber", "pageObjectNumber": 1 } },
                  "next": [] },
        "incomplete": false, "warningFlags": 0, "warnings": []
    }));
    assert!(block_on(actions.can_execute(goto, user())).unwrap());
}

#[test]
fn the_document_open_tree_comes_from_the_catalog() {
    let (_thread, actions) = handle();
    let open =
        block_on(actions.action_tree(ActionTreeSource::Document { event: None })).unwrap().unwrap();
    assert_eq!(open.root_type().as_deref(), Some("named"));
    let none =
        block_on(actions.action_tree(ActionTreeSource::Document {
            event: Some(DocumentTriggerEvent::DidPrint),
        }))
        .unwrap();
    assert!(none.is_none());
}

#[test]
fn a_document_trigger_runs_its_tree_and_is_observed() {
    let (_thread, actions) = handle();
    let events = Arc::new(Events::default());
    let _sub = actions.subscribe(events.clone());
    let trigger = ActionTrigger::Document { event: DocumentTriggerEvent::WillSave };
    assert!(block_on(actions.can_dispatch(trigger.clone())).unwrap());
    let result = block_on(actions.dispatch(trigger)).unwrap();
    assert_eq!(result.steps.len(), 1);
    assert_eq!(result.steps[0].tree.root_type().as_deref(), Some("named"));
    assert!(matches!(result.steps[0].source, ActionSource::Document));
    let seen = events.0.lock().unwrap();
    assert!(
        seen.iter().any(|e| matches!(e, ActionsEvent::Executed { event } if event.tree.root_type().as_deref() == Some("named"))),
        "{seen:?}"
    );
}

#[test]
fn json_that_is_not_an_object_is_invalid_input() {
    let error = PdfActionTree::from_json("[1, 2".into()).err().unwrap();
    assert_eq!(code(&error), Some("invalid-input"), "{error:?}");
}

#[test]
fn a_closed_documents_capability_refuses_with_instance_closed() {
    let (thread, actions) = handle();
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    for error in [
        block_on(actions.policy()).map(|_| ()).unwrap_err(),
        block_on(actions.prepare_close()).map(|_| ()).unwrap_err(),
        block_on(actions.execute(tree(named("NextPage")), user())).map(|_| ()).unwrap_err(),
    ] {
        assert_eq!(code(&error), Some("instance-closed"), "{error:?}");
    }
}

#[test]
fn closing_the_document_ends_its_subscriptions() {
    let (thread, actions) = handle();
    let events = Arc::new(Events::default());
    let _subscription = actions.subscribe(events.clone());
    block_on(thread.call(|_| ())).unwrap();
    assert_eq!(events.1.load(Ordering::SeqCst), 0);
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    assert_eq!(events.1.load(Ordering::SeqCst), 1);
    let late = Arc::new(Events::default());
    let _late = actions.subscribe(late.clone());
    block_on(thread.call(|_| ())).unwrap();
    assert_eq!(late.1.load(Ordering::SeqCst), 1);
}
