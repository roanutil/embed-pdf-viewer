//! Rust-side behaviour the vectors don't pin on their own: M14's decision
//! rule on the actual JavaScript executor, the form pipeline's print kept as
//! the TypeScript has it, a realm with no sandbox factory, a dropped
//! detached realm (A1), and the settings store's function identity.

use std::cell::RefCell;
use std::rc::Rc;

use epdf_acrojs::{
    PartialScriptIdentity, ScriptBudget, ScriptInput, ScriptOutput, ScriptSandbox,
    ScriptSandboxFactory, ScriptUiEffect,
};
use epdf_kernel::engine::DocumentHandle;
use epdf_kernel::kernel::{OpenOptions, OpenSource};
use epdf_kernel::rt::{self, Promise};
use epdf_kernel::{AnyPlugin, Error, Kernel, KernelOptions, Token, define, undefined};
use epdf_plugin_actions::{
    ACTIONS, ActionContext, ActionNodeStatus, ActionPolicyDecision, ActionPolicyPatch,
    ActionResultStatus, ActionUiAdapter, ActionUiContext, ActionsCapability, ActionsConfig,
    ActionsScriptIdentity, ActionsSettingsPatch, JavaScriptConfig, JavaScriptSettingsPatch,
    NO_DEFAULT_SANDBOX, OpenSequence, PartialActionPolicyRow, ScriptDocument, ScriptPhase,
    ScriptRealmKind, ScriptRealmTarget, ScriptSurfaceResult, actions_plugin,
};
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use serde_json::{Value, json};

// ── fixtures ──────────────────────────────────────────────────────────────────

fn page(number: u32, index: u32) -> Value {
    let box_ = json!({ "x": 0, "y": 0, "width": 600, "height": 800 });
    json!({
        "index": index,
        "ref": { "kind": "objectNumber", "objectNumber": number },
        "label": null,
        "size": { "width": 600, "height": 800 },
        "rotation": 0,
        "userUnit": 1,
        "tabOrder": null,
        "pdfCropBox": { "left": 0, "bottom": 0, "right": 600, "top": 800 },
        "boxes": { "media": box_, "crop": box_, "bleed": box_, "trim": box_, "art": box_ },
    })
}

fn document(id: &str) -> Value {
    json!({
        "id": id,
        "pages": [page(1, 0), page(2, 1)],
        "security": { "allows": "*" },
        "responses": [
            {
                "path": "forms.list",
                "value": {
                    "formKind": "acroform",
                    "needsAppearances": false,
                    "fields": [],
                    "widgets": [],
                    "calculationOrder": [],
                },
            },
            { "path": "page.annotations.list", "value": { "annotations": [] } },
            {
                "path": "actions.get",
                "value": { "nameTreeScripts": [], "openAction": null, "openDestination": null },
            },
        ],
    })
}

type Handles = Rc<RefCell<Vec<Rc<dyn DocumentHandle>>>>;

/// A sibling plugin that keeps each document's guarded handle, as the stamp
/// plugin holds the handle it mints a detached realm for.
fn handle_keeper(handles: &Handles) -> AnyPlugin {
    let handles = handles.clone();
    let token: Token<()> = Token::new("handle-keeper");
    define::<Value, Value>("handle-keeper")
        .document_scoped()
        .initial_state(undefined)
        .create(&token, move |cx| {
            handles.borrow_mut().extend(cx.doc().ok());
            Ok(((), None))
        })
        .build()
}

struct Viewer {
    kernel: Kernel,
    handles: Handles,
}

impl Viewer {
    fn new(config: ActionsConfig) -> Self {
        rt::reset();
        let fake = FakeEngine::new(json!({
            "documents": { "a": document("a"), "b": document("b") }
        }));
        let handles = Handles::default();
        let kernel = Kernel::new(
            Rc::new(AsEngine(fake)),
            vec![actions_plugin(config), handle_keeper(&handles)],
            KernelOptions::default(),
        )
        .expect("kernel");
        kernel.start();
        rt::run_until_stalled();
        Viewer { kernel, handles }
    }

    fn open(&self, id: &str) -> ActionsCapability {
        self.kernel.documents().open(
            OpenSource::Input(json!({ "kind": "bytes", "id": id, "bytes": { "$bytes": "" } })),
            OpenOptions::default(),
        );
        rt::run_until_stalled();
        self.kernel.capability(&ACTIONS, Some(id)).expect("actions")
    }
}

/// A sandbox whose every run asks to print; `log` records what reaches it.
struct PrintingSandbox {
    log: Rc<RefCell<Vec<String>>>,
}

impl ScriptSandbox for PrintingSandbox {
    fn disposed(&self) -> bool {
        false
    }

    fn boot(
        &self,
        _sources: &[String],
        _input: &ScriptInput,
        _budget: Option<&ScriptBudget>,
    ) -> Result<ScriptOutput, Error> {
        Ok(ScriptOutput::empty())
    }

    fn run(
        &self,
        source: &str,
        _input: &ScriptInput,
        _budget: Option<&ScriptBudget>,
    ) -> Result<ScriptOutput, Error> {
        self.log.borrow_mut().push(format!("run {source}"));
        let mut output = ScriptOutput::empty();
        output.ui_effects.push(ScriptUiEffect::Print);
        Ok(output)
    }

    fn dispose(&self) {
        self.log.borrow_mut().push("dispose".into());
    }
}

fn printing_factory(log: &Rc<RefCell<Vec<String>>>) -> ScriptSandboxFactory {
    let log = log.clone();
    Rc::new(move || {
        log.borrow_mut().push("factory".into());
        Promise::resolved(Rc::new(PrintingSandbox { log: log.clone() }) as Rc<dyn ScriptSandbox>)
    })
}

fn scripting(factory: Option<ScriptSandboxFactory>) -> ActionsConfig {
    ActionsConfig {
        open_sequence: Some(OpenSequence::Off),
        javascript: Some(JavaScriptConfig {
            enabled: Some(true),
            sandbox_factory: factory,
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// An adapter that records each print with its context.
fn print_adapter(prints: &Rc<RefCell<Vec<Option<ActionUiContext>>>>) -> Rc<ActionUiAdapter> {
    let prints = prints.clone();
    Rc::new(ActionUiAdapter {
        open_uri: Rc::new(|_, _| Ok(())),
        print: Rc::new(move |ctx| {
            prints.borrow_mut().push(ctx.cloned());
            Ok(())
        }),
        alert: None,
        goto_page: None,
    })
}

fn context(origin: &str) -> ActionContext {
    serde_json::from_value(json!({
        "origin": origin,
        "source": { "kind": "api" },
        "event": { "scope": "activate" },
    }))
    .expect("context")
}

fn script(source: &str) -> Value {
    json!({
        "root": { "type": "javascript", "subtype": "JavaScript", "script": source, "next": [] },
        "incomplete": false, "warningFlags": 0, "warnings": [],
    })
}

/// Every diagnostic the capability reports, as `code: message` where the
/// run lists one.
fn diagnostics(actions: &ActionsCapability) -> Rc<RefCell<Vec<Value>>> {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let log = seen.clone();
    let _ = actions.on_diagnostic_reported(move |event| {
        log.borrow_mut().push(serde_json::to_value(event).expect("an event"));
        Ok(())
    });
    seen
}

/// What `onDiagnosticReported` carries for a refused script print: the
/// event has no message (`packages/plugin/actions/src/contract.ts`,
/// `ActionDiagnosticReportedEvent`); the text is pinned in
/// `lifecycle::document_events::tests`.
fn blocked_script_print() -> Value {
    json!({ "code": "blocked", "action": "javascript", "source": { "kind": "api" } })
}

fn print_row(user: ActionPolicyDecision) -> ActionsSettingsPatch {
    ActionsSettingsPatch {
        policy: Some(ActionPolicyPatch {
            print: Some(PartialActionPolicyRow { user: Some(user), ..Default::default() }),
            ..Default::default()
        }),
        ..Default::default()
    }
}

// ── M14 on the actual executor ────────────────────────────────────────────────

#[test]
fn m14_a_lifecycle_script_print_under_the_default_block_never_reaches_the_adapter() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let viewer = Viewer::new(scripting(Some(printing_factory(&log))));
    let actions = viewer.open("a");
    let prints = Rc::new(RefCell::new(Vec::new()));
    let _adapter = actions.set_ui_adapter(Some(print_adapter(&prints)));
    let reported = diagnostics(&actions);

    let result = actions.execute(script("this.print()"), context("lifecycle"), None);
    rt::run_until_stalled();

    // The real executor ran the script, and its print went through the
    // print door, where the default `print.lifecycle = block` refused it.
    assert_eq!(*log.borrow(), ["factory", "run this.print()"]);
    let result = result.peek().expect("settled").expect("a result");
    assert_eq!(result.status, ActionResultStatus::Executed);
    assert!(prints.borrow().is_empty(), "a lifecycle print reached the adapter");
    assert_eq!(*reported.borrow(), [blocked_script_print()]);
}

#[test]
fn m14_a_user_script_print_follows_the_live_print_row() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let viewer = Viewer::new(scripting(Some(printing_factory(&log))));
    let actions = viewer.open("a");
    let prints = Rc::new(RefCell::new(Vec::new()));
    let _adapter = actions.set_ui_adapter(Some(print_adapter(&prints)));
    let reported = diagnostics(&actions);

    // Default `print.user = adapter`: one dialog, with the script's context.
    let _ = actions.execute(script("this.print()"), context("user"), None);
    rt::run_until_stalled();
    assert_eq!(
        *prints.borrow(),
        [Some(ActionUiContext {
            origin: epdf_plugin_actions::ActionOrigin::User,
            phase: ScriptPhase::User
        })]
    );

    // Upstream refuses only non-user prints, and only with a kernel printer.
    // M14 reads the row for every origin on the adapter path, so a user
    // print under `block` is refused too.
    actions.update_settings(&print_row(ActionPolicyDecision::Block));
    let _ = actions.execute(script("this.print()"), context("user"), None);
    rt::run_until_stalled();
    assert_eq!(prints.borrow().len(), 1);
    assert_eq!(*reported.borrow(), [blocked_script_print()]);

    // `report` refuses as well, and says so the same way.
    actions.update_settings(&print_row(ActionPolicyDecision::Report));
    let _ = actions.execute(script("this.print()"), context("user"), None);
    rt::run_until_stalled();
    assert_eq!(prints.borrow().len(), 1);
    assert_eq!(*reported.borrow(), [blocked_script_print(), blocked_script_print()]);
}

#[test]
fn the_named_print_verb_is_decided_by_the_dispatch_not_by_m14() {
    // The dispatch path carries no script context, so the print door does
    // not read the row again: the walk already did.
    let viewer =
        Viewer::new(ActionsConfig { open_sequence: Some(OpenSequence::Off), ..Default::default() });
    let actions = viewer.open("a");
    let prints = Rc::new(RefCell::new(Vec::new()));
    let _adapter = actions.set_ui_adapter(Some(print_adapter(&prints)));
    let result = actions.execute_named("Print", None, None);
    rt::run_until_stalled();
    let result = result.peek().expect("settled").expect("a result");
    assert_eq!(result.nodes[0].status, ActionNodeStatus::Executed);
    assert_eq!(*prints.borrow(), [None]);
}

// ── the form pipeline (Task 14 carry) ─────────────────────────────────────────

#[test]
fn the_form_pipeline_print_still_skips_the_print_row_as_the_typescript_does() {
    // `surface.ts` sends a K/V/C/F script's print to the adapter without
    // reading the policy. This test pins that the Rust port keeps the same
    // gap; closing it is the Task 14 decision, not M14.
    let viewer =
        Viewer::new(ActionsConfig { open_sequence: Some(OpenSequence::Off), ..Default::default() });
    let actions = viewer.open("a");
    let prints = Rc::new(RefCell::new(Vec::new()));
    let _adapter = actions.set_ui_adapter(Some(print_adapter(&prints)));
    let reported = diagnostics(&actions);
    let lifecycle = epdf_plugin_actions::ActionOrigin::Lifecycle;
    actions
        .surface_script_result(&ScriptSurfaceResult {
            ui_effects: vec![ScriptUiEffect::Print],
            diagnostics: Vec::new(),
            error: None,
            origin: lifecycle,
            phase: ScriptPhase::User,
            realm: ScriptRealmKind::Document,
            source: None,
        })
        .expect("surfaced");
    assert_eq!(
        *prints.borrow(),
        [Some(ActionUiContext { origin: lifecycle, phase: ScriptPhase::User })]
    );
    assert!(reported.borrow().is_empty());
}

// ── no sandbox factory ────────────────────────────────────────────────────────

#[test]
fn scripting_on_without_a_factory_has_a_realm_whose_runs_fail() {
    let viewer = Viewer::new(scripting(None));
    let actions = viewer.open("a");
    // The realm exists, as the TypeScript's lazily loaded default VM does.
    assert!(actions.is_scripting_enabled());
    assert!(actions.script_realm().is_some());
    let result = actions.execute(script("app.alert(1)"), context("user"), None);
    rt::run_until_stalled();
    let result = result.peek().expect("settled").expect("a result");
    assert_eq!(result.status, ActionResultStatus::Partial);
    assert_eq!(result.nodes[0].status, ActionNodeStatus::Failed);
    assert_eq!(result.nodes[0].detail.as_deref(), Some(NO_DEFAULT_SANDBOX));
}

// ── A1 through the actions plugin ─────────────────────────────────────────────

#[test]
fn a_detached_realm_dropped_without_dispose_releases_its_sandbox() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let viewer = Viewer::new(scripting(Some(printing_factory(&log))));
    let actions = viewer.open("a");
    let doc = viewer.handles.borrow().first().cloned().expect("a handle");
    let realm = actions
        .create_detached_script_realm(ScriptRealmTarget {
            doc,
            document: Rc::new(|| {
                Some(ScriptDocument { name: None, page_count: 1, pages: Rc::new(json!([])) })
            }),
            boot_sources: Rc::new(|| Ok(Promise::resolved(Vec::new()))),
        })
        .expect("scripting is on");
    let ran = realm.transaction(|txn| {
        Ok(Promise::spawn(async move {
            let world = epdf_acrojs::ScriptWorldInput {
                fields: Vec::new(),
                annots: None,
                annot_pages: None,
                annots_cover_document: None,
                page_number: None,
                event: epdf_acrojs::ScriptEventInput::of(
                    epdf_acrojs::ScriptEventKind::WidgetActivate,
                ),
            };
            txn.run("x()", world, None).await?;
            Ok(())
        }))
    });
    rt::run_until_stalled();
    assert!(ran.peek().expect("settled").is_ok());
    assert_eq!(*log.borrow(), ["factory", "run x()"]);
    drop(realm);
    rt::run_until_stalled();
    assert_eq!(*log.borrow(), ["factory", "run x()", "dispose"]);
}

// ── the settings ──────────────────────────────────────────────────────────────

#[test]
fn a_function_identity_keeps_its_identity_through_the_store() {
    let viewer =
        Viewer::new(ActionsConfig { open_sequence: Some(OpenSequence::Off), ..Default::default() });
    let actions = viewer.open("a");
    let identity: Rc<dyn Fn() -> PartialScriptIdentity> =
        Rc::new(|| PartialScriptIdentity { name: Some("Fn".into()), ..Default::default() });
    actions.update_settings(&ActionsSettingsPatch {
        javascript: Some(JavaScriptSettingsPatch {
            enabled: None,
            identity: Some(Some(ActionsScriptIdentity::Function(identity.clone()))),
        }),
        ..Default::default()
    });
    match actions.get_settings().javascript.identity {
        Some(ActionsScriptIdentity::Function(stored)) => {
            assert!(std::ptr::addr_eq(Rc::as_ptr(&stored), Rc::as_ptr(&identity)));
        }
        _ => panic!("the identity is not the function given"),
    }
    actions.reset_settings();
    assert!(actions.get_settings().javascript.identity.is_none());
}

#[test]
fn javascript_enabled_applies_to_documents_opened_after_the_change() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let viewer = Viewer::new(ActionsConfig {
        open_sequence: Some(OpenSequence::Off),
        javascript: Some(JavaScriptConfig {
            sandbox_factory: Some(printing_factory(&log)),
            ..Default::default()
        }),
        ..Default::default()
    });
    let a = viewer.open("a");
    assert!(!a.is_scripting_enabled());
    a.update_settings(&ActionsSettingsPatch {
        javascript: Some(JavaScriptSettingsPatch { enabled: Some(true), identity: None }),
        ..Default::default()
    });
    assert!(!a.is_scripting_enabled(), "an open document keeps the realm it opened with");
    let b = viewer.open("b");
    assert!(b.is_scripting_enabled());
    assert!(b.get_settings().javascript.enabled);
}

#[test]
fn a_policy_change_reaches_every_open_document() {
    let viewer =
        Viewer::new(ActionsConfig { open_sequence: Some(OpenSequence::Off), ..Default::default() });
    let a = viewer.open("a");
    let b = viewer.open("b");
    let changed = Rc::new(RefCell::new(Vec::new()));
    let log = changed.clone();
    let _off = b.on_settings_changed(move |event| {
        log.borrow_mut().push(event.changed.clone());
        Ok(())
    });
    a.update_settings(&print_row(ActionPolicyDecision::Block));
    assert_eq!(b.get_settings().policy.print.user, ActionPolicyDecision::Block);
    assert_eq!(*changed.borrow(), [vec!["policy".to_string()]]);
    assert!(!b.can_execute_named("Print", None));
}

fn identity_patch(identity: ActionsScriptIdentity) -> ActionsSettingsPatch {
    ActionsSettingsPatch {
        javascript: Some(JavaScriptSettingsPatch { enabled: None, identity: Some(Some(identity)) }),
        ..Default::default()
    }
}

fn fields(name: Option<&str>, email: Option<&str>) -> ActionsScriptIdentity {
    ActionsScriptIdentity::Value(PartialScriptIdentity {
        name: name.map(str::to_string),
        email: email.map(str::to_string),
        ..Default::default()
    })
}

fn stored_fields(actions: &ActionsCapability) -> Option<PartialScriptIdentity> {
    match actions.get_settings().javascript.identity {
        Some(ActionsScriptIdentity::Value(fields)) => Some(fields),
        _ => None,
    }
}

#[test]
fn fields_over_a_function_identity_replace_it_as_the_typescript_merge_does() {
    // `mergeValue` (packages/core/main/src/settings.ts): a plain object over
    // a function merges into `{}`, so only the new fields remain.
    let viewer =
        Viewer::new(ActionsConfig { open_sequence: Some(OpenSequence::Off), ..Default::default() });
    let actions = viewer.open("a");
    let function: Rc<dyn Fn() -> PartialScriptIdentity> = Rc::new(PartialScriptIdentity::default);
    let weak = Rc::downgrade(&function);
    actions.update_settings(&identity_patch(fields(Some("A"), None)));
    actions.update_settings(&identity_patch(ActionsScriptIdentity::Function(function)));
    actions.update_settings(&identity_patch(fields(None, Some("b@x.test"))));
    assert_eq!(
        stored_fields(&actions),
        Some(PartialScriptIdentity { email: Some("b@x.test".into()), ..Default::default() })
    );
    // The store no longer names the function, so the table released it.
    assert!(weak.upgrade().is_none(), "the replaced function identity is still held");
}

#[test]
fn a_function_over_fields_replaces_them() {
    let viewer =
        Viewer::new(ActionsConfig { open_sequence: Some(OpenSequence::Off), ..Default::default() });
    let actions = viewer.open("a");
    let function: Rc<dyn Fn() -> PartialScriptIdentity> = Rc::new(PartialScriptIdentity::default);
    actions.update_settings(&identity_patch(fields(Some("A"), None)));
    actions.update_settings(&identity_patch(ActionsScriptIdentity::Function(function.clone())));
    match actions.get_settings().javascript.identity {
        Some(ActionsScriptIdentity::Function(stored)) => {
            assert!(std::ptr::addr_eq(Rc::as_ptr(&stored), Rc::as_ptr(&function)));
        }
        _ => panic!("the function did not replace the fields"),
    }
}
