#![cfg(feature = "uniffi")]

use std::rc::Rc;
use std::sync::{Arc, Mutex};

use epdf_ffi::{HostError, HostFailure, KernelThread, ViewerError};
use epdf_kernel::rt::Promise;
use epdf_plugin_actions::ffi::ports::{
    ActionAlertOptions, ActionExecutor, ActionExecutorCompletion, ActionSubmitHandler,
    ActionSubmitRequest, ActionUiHandler, ActionUiSupport, ActionUriOptions,
    DocumentVerbCompletion, DocumentVerbOperation, FormSubmissionValue,
};
use epdf_plugin_actions::ffi::{
    ActionContext, ActionSource, ActionsHandle, PdfActionNode, PdfActionTree,
};
use epdf_plugin_actions::{
    ACTIONS, ActionNodeStatus, ActionOrigin, ActionTriggerEvent, ActionUiContext, ActionsConfig,
    DocumentVerb, FormSubmissionEntry, OpenSequence, actions_plugin,
};
use futures::executor::block_on;
use plugin_conformance::test_kernel::{DOCUMENT, Reply, TestKernel, TestSpec};
use serde_json::{Value, json};

// named, page, spec, quiet, handle, user, tree: copied from tests/ffi.rs

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

#[derive(Clone, Copy)]
enum Answer {
    Executed,
    Drop,
}

struct Executor {
    answer: Answer,
    seen: Mutex<Vec<(String, ActionOrigin)>>,
}

impl ActionExecutor for Executor {
    fn execute(
        &self,
        node: Arc<PdfActionNode>,
        context: ActionContext,
        completion: Arc<ActionExecutorCompletion>,
    ) {
        self.seen.lock().unwrap().push((node.node_type(), context.origin));
        match self.answer {
            Answer::Executed => completion.executed(),
            Answer::Drop => drop(completion),
        }
    }
}

fn goto() -> Arc<PdfActionTree> {
    tree(json!({
        "root": { "type": "goto", "subtype": "GoTo",
                  "destination": { "kind": "fit", "page": { "kind": "objectNumber", "pageObjectNumber": 1 } },
                  "next": [] },
        "incomplete": false, "warningFlags": 0, "warnings": []
    }))
}

#[test]
fn a_swift_executor_runs_its_node_type() {
    let (_thread, actions) = handle();
    let executor = Arc::new(Executor { answer: Answer::Executed, seen: Mutex::default() });
    let _registration =
        block_on(actions.register_executor("goto".into(), executor.clone())).unwrap();
    let result = block_on(actions.execute(goto(), user())).unwrap();
    assert_eq!(result.nodes[0].status, ActionNodeStatus::Executed);
    assert_eq!(*executor.seen.lock().unwrap(), [("goto".to_string(), ActionOrigin::User)]);
}

#[test]
fn a_dropped_executor_completion_fails_the_node() {
    let (_thread, actions) = handle();
    let executor = Arc::new(Executor { answer: Answer::Drop, seen: Mutex::default() });
    let _registration = block_on(actions.register_executor("goto".into(), executor)).unwrap();
    let result = block_on(actions.execute(goto(), user())).unwrap();
    assert_eq!(result.nodes[0].status, ActionNodeStatus::Failed);
    // The queue isn't stuck: the next dispatch runs.
    let again = block_on(actions.execute(goto(), user())).unwrap();
    assert_eq!(again.nodes.len(), 1);
}

#[test]
fn an_unregistered_executor_is_not_called() {
    let (thread, actions) = handle();
    let executor = Arc::new(Executor { answer: Answer::Executed, seen: Mutex::default() });
    let registration =
        block_on(actions.register_executor("goto".into(), executor.clone())).unwrap();
    registration.unregister();
    block_on(thread.call(|_| ())).unwrap();
    let result = block_on(actions.execute(goto(), user())).unwrap();
    assert_ne!(result.nodes[0].status, ActionNodeStatus::Executed);
    assert!(executor.seen.lock().unwrap().is_empty());
}

#[derive(Default)]
struct Ui {
    opened: Mutex<Vec<String>>,
    alerts: Mutex<Vec<String>>,
}

impl ActionUiHandler for Ui {
    fn open_uri(&self, uri: String, _options: ActionUriOptions) -> Result<(), HostError> {
        self.opened.lock().unwrap().push(uri);
        Ok(())
    }
    fn print(&self, _context: Option<ActionUiContext>) -> Result<(), HostError> {
        Ok(())
    }
    fn alert(&self, message: String, _options: ActionAlertOptions) -> Result<(), HostError> {
        self.alerts.lock().unwrap().push(message);
        Ok(())
    }
    fn goto_page(&self, _page: f64, _context: ActionUiContext) -> Result<(), HostError> {
        Ok(())
    }
}

fn uri(uri: &str) -> Arc<PdfActionTree> {
    tree(json!({
        "root": { "type": "uri", "subtype": "URI", "uri": uri, "isMap": false, "next": [] },
        "incomplete": false, "warningFlags": 0, "warnings": []
    }))
}

#[test]
fn only_allowed_schemes_reach_the_swift_handler() {
    let (_thread, actions) = handle();
    let ui = Arc::new(Ui::default());
    let _registration = block_on(
        actions.set_ui_adapter(Some(ui.clone()), ActionUiSupport { alert: true, goto_page: true }),
    )
    .unwrap();
    let mut statuses = Vec::new();
    for target in ["javascript:alert(1)", "file:///etc/passwd", " https://a.test/ "] {
        statuses.push(block_on(actions.execute(uri(target), user())).unwrap().nodes[0].status);
    }
    assert_eq!(*ui.opened.lock().unwrap(), ["https://a.test/"]);
    assert_eq!(
        statuses,
        [ActionNodeStatus::Blocked, ActionNodeStatus::Blocked, ActionNodeStatus::Executed]
    );
}

struct Submit(Mutex<Vec<ActionSubmitRequest>>, bool);

impl ActionSubmitHandler for Submit {
    fn submit(&self, request: ActionSubmitRequest) -> Result<(), HostError> {
        self.0.lock().unwrap().push(request);
        if self.1 { Err(HostError::Failed { detail: "offline".into() }) } else { Ok(()) }
    }
}

fn submit_tree() -> Arc<PdfActionTree> {
    tree(json!({
        "root": { "type": "submit-form", "subtype": "SubmitForm",
                  "payload": { "url": "https://home.test/submit", "fields": null,
                               "flags": { "raw": 4, "exclude": false, "includeNoValueFields": false,
                                          "format": "html", "method": "post", "submitCoordinates": false,
                                          "includeAppendSaves": false, "includeAnnotations": false,
                                          "canonicalFormat": false, "exclNonUserAnnots": false,
                                          "exclFKey": false, "embedForm": false } },
                  "next": [] },
        "incomplete": false, "warningFlags": 0, "warnings": []
    }))
}

/// Registers the form plugin's half (the dataset resolver) from Rust, as the
/// form plugin will in plan 06.
fn with_resolver(thread: &KernelThread<TestKernel>) {
    block_on(thread.call(|k| {
        let actions = k.capability(&ACTIONS).unwrap();
        let disposer = actions.register_submit_resolver(Rc::new(
            |intent: &epdf_plugin_actions::SubmitIntent,
             ctx: &epdf_plugin_actions::ActionContext,
             _diagnose: Rc<dyn Fn(epdf_plugin_actions::ActionDiagnostic)>| {
                Ok(Promise::resolved(epdf_plugin_actions::ActionSubmitRequest {
                    url: intent.url.clone(),
                    method: intent.method.clone(),
                    format: intent.format.clone(),
                    flags_raw: intent.flags_raw,
                    char_set: None,
                    entries: vec![
                        FormSubmissionEntry { name: "name".into(), value: json!("Ana") },
                        FormSubmissionEntry {
                            name: "colors".into(),
                            value: json!(["red", "blue"]),
                        },
                        FormSubmissionEntry { name: "flag".into(), value: Value::Null },
                    ],
                    origin: ctx.origin,
                    event: ctx.event.clone(),
                }))
            },
        ));
        // The test kernel lives as long as the test; keep the resolver.
        std::mem::forget(disposer);
    }))
    .unwrap();
}

#[test]
fn a_submit_reaches_the_swift_handler_typed() {
    let (thread, actions) = handle();
    with_resolver(&thread);
    let handler = Arc::new(Submit(Mutex::default(), false));
    let _registration = block_on(actions.set_submit_handler(Some(handler.clone()))).unwrap();
    let result = block_on(actions.execute(submit_tree(), user())).unwrap();
    assert_eq!(result.nodes[0].status, ActionNodeStatus::Executed);
    let seen = handler.0.lock().unwrap();
    assert_eq!(seen[0].url.as_deref(), Some("https://home.test/submit"));
    assert_eq!(seen[0].flags_raw, 4);
    assert_eq!(
        seen[0].entries.iter().map(|e| e.value.clone()).collect::<Vec<_>>(),
        [
            FormSubmissionValue::Text { value: "Ana".into() },
            FormSubmissionValue::List { values: vec!["red".into(), "blue".into()] },
            FormSubmissionValue::NameOnly,
        ]
    );
}

#[test]
fn a_throwing_submit_handler_fails_the_node() {
    let (thread, actions) = handle();
    with_resolver(&thread);
    let _registration =
        block_on(actions.set_submit_handler(Some(Arc::new(Submit(Mutex::default(), true)))))
            .unwrap();
    let result = block_on(actions.execute(submit_tree(), user())).unwrap();
    assert_eq!(result.nodes[0].status, ActionNodeStatus::Failed);
    assert_eq!(result.nodes[0].detail.as_deref(), Some("offline"));
}

struct Save(Mutex<usize>, bool);

impl DocumentVerbOperation for Save {
    fn run(&self, completion: Arc<DocumentVerbCompletion>) {
        *self.0.lock().unwrap() += 1;
        if self.1 { completion.fail("read-only".into()) } else { completion.finish() }
    }
}

#[test]
fn a_document_verb_runs_its_operation_between_the_will_and_did_trees() {
    let (_thread, actions) = handle();
    let save = Arc::new(Save(Mutex::new(0), false));
    block_on(actions.run_document_verb(DocumentVerb::Save, save.clone())).unwrap();
    assert_eq!(*save.0.lock().unwrap(), 1);
    let failing = Arc::new(Save(Mutex::new(0), true));
    let error = block_on(actions.run_document_verb(DocumentVerb::Save, failing)).unwrap_err();
    assert!(format!("{error}").contains("read-only"), "{error}");
}

struct DeniedSave;

impl DocumentVerbOperation for DeniedSave {
    fn run(&self, completion: Arc<DocumentVerbCompletion>) {
        completion.fail_with(HostFailure {
            name: "PluginError".into(),
            code: Some("permission-denied".into()),
            detail: "no doc.download".into(),
        });
    }
}

#[test]
fn a_document_verb_failure_keeps_its_code() {
    let (_thread, actions) = handle();
    let error =
        block_on(actions.run_document_verb(DocumentVerb::Save, Arc::new(DeniedSave))).unwrap_err();
    match error {
        ViewerError::Kernel { name, code, detail } => {
            assert_eq!(code.as_deref(), Some("permission-denied"), "{detail}");
            assert_eq!((name.as_str(), detail.as_str()), ("PluginError", "no doc.download"));
        }
        other => panic!("{other:?}"),
    }
}
