#![cfg(feature = "uniffi")]

use std::sync::{Arc, Mutex};

use epdf_acrojs::{
    DEFAULT_SCRIPT_BUDGET, ScriptDocumentInput, ScriptEnvironment, ScriptEventInput,
    ScriptEventKind, ScriptExecutionErrorKind, ScriptIdentity, ScriptInput,
};
use epdf_ffi::{HostError, KernelThread};
use epdf_plugin_actions::ffi::ports::{
    ActionAlertOptions, ActionUiHandler, ActionUiSupport, ActionUriOptions,
};
use epdf_plugin_actions::ffi::sandbox::{
    ScriptEngine, ScriptEngineFactory, ScriptEvaluation, sandbox_factory,
};
use epdf_plugin_actions::ffi::{ActionContext, ActionSource, ActionsHandle, PdfActionTree};
use epdf_plugin_actions::{
    ACTIONS, ActionNodeStatus, ActionOrigin, ActionTriggerEvent, ActionUiContext, ActionsConfig,
    JavaScriptConfig, OpenSequence, actions_plugin,
};
use futures::executor::block_on;
use plugin_conformance::test_kernel::{DOCUMENT, Reply, TestKernel, TestSpec};
use serde_json::{Value, json};

// named, page, spec, user: copied from tests/ffi.rs

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

fn user() -> ActionContext {
    ActionContext {
        origin: ActionOrigin::User,
        source: ActionSource::Api,
        event: ActionTriggerEvent::Activate,
    }
}

const PROBE: &str = "\"function\"";

fn output(ui: Value) -> String {
    json!({
        "event": { "rc": true, "value": null, "change": "", "selStart": 0, "selEnd": 0 },
        "formEffects": [], "annotEffects": [], "uiEffects": ui, "diagnostics": []
    })
    .to_string()
}

/// A scripted engine: answers the prelude probe, then whatever `run` says.
struct Engine {
    run: Box<dyn Fn(&str) -> ScriptEvaluation + Send + Sync>,
    programs: Mutex<Vec<String>>,
    disposed: Mutex<bool>,
}

impl ScriptEngine for Engine {
    fn evaluate(&self, program: String, _time_limit_ms: u32) -> ScriptEvaluation {
        self.programs.lock().unwrap().push(program.clone());
        if program.contains("typeof globalThis.__acrojsRun") {
            return ScriptEvaluation::Value { json: PROBE.into() };
        }
        (self.run)(&program)
    }
    fn dispose(&self) {
        *self.disposed.lock().unwrap() = true;
    }
}

struct Factory(Arc<Engine>);

impl ScriptEngineFactory for Factory {
    fn create(&self) -> Result<Arc<dyn ScriptEngine>, HostError> {
        Ok(self.0.clone())
    }
}

fn engine(run: impl Fn(&str) -> ScriptEvaluation + Send + Sync + 'static) -> Arc<Engine> {
    Arc::new(Engine { run: Box::new(run), programs: Mutex::default(), disposed: Mutex::new(false) })
}

fn input() -> ScriptInput {
    let mut event = ScriptEventInput::of(ScriptEventKind::WidgetActivate);
    event.event_type = Some("Doc".into());
    event.name = Some("Open".into());
    ScriptInput {
        document: ScriptDocumentInput {
            id: "d".into(),
            file_name: "d.pdf".into(),
            page_count: 1.0,
            page_number: 0.0,
        },
        identity: ScriptIdentity::default(),
        environment: ScriptEnvironment { now_ms: 0.0, utc_offset_minutes: 0.0, random_seed: 1.0 },
        fields: vec![],
        annots: None,
        annot_pages: None,
        annots_cover_document: None,
        event,
    }
}

/// The sandbox the factory builds, on this thread: its promise is already
/// settled.
fn sandbox(engine: Arc<Engine>) -> std::rc::Rc<dyn epdf_acrojs::ScriptSandbox> {
    let made = sandbox_factory(Arc::new(Factory(engine)))();
    made.peek().expect("settled").expect("a sandbox")
}

#[test]
fn the_program_is_the_quickjs_sandbox_call_wrapped_in_stringify() {
    let engine = engine(|_| ScriptEvaluation::Value { json: output(json!([])) });
    let sandbox = sandbox(engine.clone());
    let out = sandbox.run("app.alert('hi')", &input(), None).unwrap();
    assert!(out.event.rc);
    let programs = engine.programs.lock().unwrap();
    let run = programs.last().unwrap();
    assert!(run.starts_with("JSON.stringify(globalThis.__acrojsRun(\"app.alert('hi')\","), "{run}");
    assert!(run.ends_with(",{\"maxEffects\":256}))"), "{run}");
}

#[test]
fn a_second_boot_throws() {
    let sandbox = sandbox(engine(|_| ScriptEvaluation::Value { json: output(json!([])) }));
    sandbox.boot(&[], &input(), None).unwrap();
    assert!(sandbox.boot(&[], &input(), None).is_err());
}

#[test]
fn a_timed_out_evaluation_is_a_budget_failure_and_poisons() {
    let engine = engine(|_| ScriptEvaluation::TimedOut);
    let sandbox = sandbox(engine.clone());
    let out = sandbox.run("while (true) {}", &input(), None).unwrap();
    assert!(!out.event.rc);
    assert_eq!(out.error.unwrap().kind, ScriptExecutionErrorKind::Budget);
    assert!(sandbox.disposed());
    assert!(*engine.disposed.lock().unwrap());
    assert!(sandbox.run("1", &input(), None).is_err());
}

#[test]
fn malformed_output_is_invalid_output() {
    for answer in [
        ScriptEvaluation::Value { json: "{\"event\":{}}".into() },
        ScriptEvaluation::Value { json: "not json".into() },
        ScriptEvaluation::Threw { detail: "SyntaxError".into() },
    ] {
        let answer = Mutex::new(Some(answer));
        let sandbox = sandbox(engine(move |_| answer.lock().unwrap().take().unwrap()));
        let out = sandbox.run("x", &input(), None).unwrap();
        assert_eq!(out.error.unwrap().kind, ScriptExecutionErrorKind::InvalidOutput);
        assert!(sandbox.disposed());
    }
}

#[test]
fn oversized_output_is_a_budget_failure() {
    let big = "x".repeat(2048);
    let sandbox = sandbox(engine(move |_| ScriptEvaluation::Value {
        json: output(json!([{ "kind": "alert", "message": big, "icon": 0 }])),
    }));
    let budget = epdf_acrojs::ScriptBudget { max_output_bytes: 1024.0, ..DEFAULT_SCRIPT_BUDGET };
    let out = sandbox.run("x", &input(), Some(&budget)).unwrap();
    let error = out.error.unwrap();
    assert_eq!(error.kind, ScriptExecutionErrorKind::Budget);
    assert!(error.message.contains("limit is 1024"), "{}", error.message);
}

#[test]
fn a_jsc_stack_overflow_counts_as_a_resource_limit() {
    let overflowed = json!({
        "event": { "rc": false, "value": null, "change": "", "selStart": 0, "selEnd": 0 },
        "formEffects": [], "annotEffects": [], "uiEffects": [], "diagnostics": [],
        "error": { "kind": "exception", "message": "RangeError: Maximum call stack size exceeded." }
    })
    .to_string();
    let sandbox = sandbox(engine(move |_| ScriptEvaluation::Value { json: overflowed.clone() }));
    let out = sandbox.run("f()", &input(), None).unwrap();
    assert_eq!(out.error.unwrap().kind, ScriptExecutionErrorKind::Budget);
}

#[test]
fn a_budget_that_is_not_a_positive_integer_throws() {
    let sandbox = sandbox(engine(|_| ScriptEvaluation::Value { json: output(json!([])) }));
    let budget = epdf_acrojs::ScriptBudget { max_effects: 0.5, ..DEFAULT_SCRIPT_BUDGET };
    assert!(sandbox.run("x", &input(), Some(&budget)).is_err());
}

#[test]
fn a_prelude_that_fails_rejects_the_factory() {
    struct Broken;
    impl ScriptEngine for Broken {
        fn evaluate(&self, _program: String, _limit: u32) -> ScriptEvaluation {
            ScriptEvaluation::Threw { detail: "no globalThis".into() }
        }
        fn dispose(&self) {}
    }
    struct BrokenFactory;
    impl ScriptEngineFactory for BrokenFactory {
        fn create(&self) -> Result<Arc<dyn ScriptEngine>, HostError> {
            Ok(Arc::new(Broken))
        }
    }
    let made = sandbox_factory(Arc::new(BrokenFactory))();
    let error = made.peek().expect("settled").err().expect("a rejection");
    assert!(error.message.contains("no globalThis"), "{error:?}");
}

#[derive(Default)]
struct Alerts(Mutex<Vec<String>>);

impl ActionUiHandler for Alerts {
    fn open_uri(&self, _uri: String, _options: ActionUriOptions) -> Result<(), HostError> {
        Ok(())
    }
    fn print(&self, _context: Option<ActionUiContext>) -> Result<(), HostError> {
        Ok(())
    }
    fn alert(&self, message: String, _options: ActionAlertOptions) -> Result<(), HostError> {
        self.0.lock().unwrap().push(message);
        Ok(())
    }
    fn goto_page(&self, _page: f64, _context: ActionUiContext) -> Result<(), HostError> {
        Ok(())
    }
}

#[test]
fn a_javascript_node_runs_in_the_foreign_engine_and_alerts() {
    let engine = engine(|_| ScriptEvaluation::Value {
        json: output(json!([{ "kind": "alert", "message": "hi", "icon": 0 }])),
    });
    // `sandbox_factory` returns an `Rc`, so it's built on the kernel thread.
    let shared = engine.clone();
    let thread = KernelThread::spawn(move |_| {
        let factory = sandbox_factory(Arc::new(Factory(shared)));
        let config = ActionsConfig {
            open_sequence: Some(OpenSequence::Off),
            javascript: Some(JavaScriptConfig {
                enabled: Some(true),
                sandbox_factory: Some(factory),
                ..Default::default()
            }),
            ..Default::default()
        };
        TestKernel::open(vec![actions_plugin(config)], spec())
    })
    .unwrap();
    let post = thread.post();
    let actions =
        block_on(thread.call(move |k| ActionsHandle::new(k.capability(&ACTIONS).unwrap(), post)))
            .unwrap();
    assert!(block_on(actions.is_scripting_enabled()).unwrap());
    let alerts = Arc::new(Alerts::default());
    let _ui = block_on(
        actions
            .set_ui_adapter(Some(alerts.clone()), ActionUiSupport { alert: true, goto_page: true }),
    )
    .unwrap();
    let script = PdfActionTree::from_json(
        json!({
            "root": { "type": "javascript", "subtype": "JavaScript", "script": "app.alert('hi')", "next": [] },
            "incomplete": false, "warningFlags": 0, "warnings": []
        })
        .to_string(),
    )
    .unwrap();
    let result = block_on(actions.execute(script, user())).unwrap();
    assert_eq!(result.nodes[0].status, ActionNodeStatus::Executed, "{result:?}");
    assert_eq!(*alerts.0.lock().unwrap(), ["hi"]);
    let programs = engine.programs.lock().unwrap();
    assert!(programs.iter().any(|p| p.contains("__acrojsBoot")), "{programs:?}");
    assert!(programs.iter().any(|p| p.contains("__acrojsRun(\"app.alert('hi')\"")), "{programs:?}");
}
