#![cfg(feature = "uniffi")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use epdf_ffi::{HostError, HostFailure, KernelThread, ViewerError};
use epdf_plugin_commands::IconAccent;
use epdf_plugin_commands::ffi::{
    CommandCompletion, CommandContext, CommandDefinition, CommandHook, CommandRunner,
};
use futures::executor::block_on;

fn code(error: &ViewerError) -> Option<&str> {
    match error {
        ViewerError::Kernel { code, .. } => code.as_deref(),
        ViewerError::Closed => None,
    }
}

/// What the test runner does when `run` is called.
#[derive(Clone, Copy)]
enum Run {
    Finish,
    Fail,
    /// `fail_with` a `permission-denied`, as a Swift handler rethrowing a
    /// capability's `PDFError` does.
    Denied,
    Drop,
}

struct Runner {
    run: Run,
    enabled: Result<bool, ()>,
    ran: Mutex<Vec<CommandContext>>,
    derived: Mutex<Vec<CommandContext>>,
}

impl Runner {
    fn new(run: Run, enabled: Result<bool, ()>) -> Arc<Self> {
        Arc::new(Runner { run, enabled, ran: Mutex::default(), derived: Mutex::default() })
    }
}

impl CommandRunner for Runner {
    fn run(&self, context: CommandContext, completion: Arc<CommandCompletion>) {
        self.ran.lock().unwrap().push(context);
        match self.run {
            Run::Finish => completion.finish(),
            Run::Fail => completion.fail("disk full".into()),
            Run::Denied => completion.fail_with(HostFailure {
                name: "PluginError".into(),
                code: Some("permission-denied".into()),
                detail: "no doc.annotate".into(),
            }),
            Run::Drop => drop(completion),
        }
    }

    fn derive(&self, context: CommandContext, hook: CommandHook) -> Result<bool, HostError> {
        self.derived.lock().unwrap().push(context);
        match hook {
            CommandHook::Enabled => {
                self.enabled.map_err(|()| HostError::Failed { detail: "derive threw".into() })
            }
            _ => Ok(true),
        }
    }

    fn locked(
        &self,
        context: CommandContext,
    ) -> Result<Option<epdf_plugin_commands::ffi::CommandSettingLock>, HostError> {
        let locked = context.app_state_json == r#"{"locked":true}"#;
        self.derived.lock().unwrap().push(context);
        Ok(locked.then(|| epdf_plugin_commands::ffi::CommandSettingLock {
            by: "app-state".into(),
            reason_key: None,
        }))
    }

    fn icon_accent(&self, context: CommandContext) -> Result<Option<IconAccent>, HostError> {
        self.derived.lock().unwrap().push(context);
        Ok(Some(IconAccent { primary: Some("#f00".into()), secondary: None }))
    }
}

fn definition(id: &str, runner: Option<Arc<Runner>>, hooks: Vec<CommandHook>) -> CommandDefinition {
    CommandDefinition {
        id: id.into(),
        label_key: format!("commands.{id}"),
        icon: None,
        shortcuts: vec![],
        categories: vec![],
        menu: None,
        panel: None,
        modal: None,
        runner: runner.map(|r| r as Arc<dyn CommandRunner>),
        hooks,
    }
}

/// `into_def` needs a kernel thread for its `KernelPost`.
fn into_def(def: CommandDefinition) -> Result<(), ViewerError> {
    let thread = KernelThread::spawn(|_| Ok(())).unwrap();
    let post = thread.post();
    block_on(thread.call(move |_| def.into_def(post).map(|_| ()).map_err(ViewerError::from)))
        .unwrap_or(Err(ViewerError::Closed))
}

#[test]
fn hooks_without_a_runner_are_refused() {
    let error = into_def(definition("save", None, vec![CommandHook::Run])).unwrap_err();
    assert_eq!(code(&error), Some("invalid-input"), "{error:?}");
}

#[test]
fn a_runner_without_hooks_is_refused() {
    let runner = Runner::new(Run::Finish, Ok(true));
    let error = into_def(definition("save", Some(runner), vec![])).unwrap_err();
    assert_eq!(code(&error), Some("invalid-input"), "{error:?}");
}

#[test]
fn a_definition_with_neither_is_a_routing_command() {
    into_def(definition("zoom-in", None, vec![])).unwrap();
}

use epdf_plugin_commands::ffi::{
    CommandExecuteResult, CommandKeyStroke, CommandRejectReason, CommandsEvent,
    CommandsEventListener, CommandsHandle,
};
use epdf_plugin_commands::{COMMANDS, CommandsConfig, commands_plugin};
use plugin_conformance::test_kernel::{TestKernel, one_document};

#[derive(Default)]
struct Events(Mutex<Vec<CommandsEvent>>);

impl CommandsEventListener for Events {
    fn on_event(&self, event: CommandsEvent) {
        self.0.lock().unwrap().push(event);
    }
}

fn handle() -> (KernelThread<TestKernel>, Arc<CommandsHandle>) {
    let thread = KernelThread::spawn(|_| {
        TestKernel::open(vec![commands_plugin(CommandsConfig::default())], one_document())
    })
    .unwrap();
    let post = thread.post();
    let handle =
        block_on(thread.call(move |k| CommandsHandle::new(k.capability(&COMMANDS).unwrap(), post)))
            .unwrap();
    (thread, handle)
}

fn run_hooks() -> Vec<CommandHook> {
    vec![CommandHook::Run, CommandHook::Enabled, CommandHook::IconAccent]
}

#[test]
fn direct_command_controls_preserve_categories_and_permanent_refusals() {
    use epdf_plugin_commands::ffi::CommandRefusalReason;
    let (thread, commands) = handle();
    let mut def = definition("edit:note", None, vec![]);
    def.categories = vec!["edit.annotations".into()];
    let _registration = block_on(commands.register_command(def, false)).unwrap();
    assert!(block_on(commands.disabled_commands()).unwrap().is_empty());
    assert!(block_on(commands.refusal("missing".into(), None)).unwrap().is_none());
    assert!(block_on(commands.refusal("edit:note".into(), None)).unwrap().is_none());
    block_on(commands.disable_command("edit:note".into())).unwrap();
    let revision = block_on(thread.call(|k| k.kernel.state_revision())).unwrap();
    block_on(commands.disable_command("edit:note".into())).unwrap();
    assert_eq!(block_on(thread.call(|k| k.kernel.state_revision())).unwrap(), revision);
    assert_eq!(block_on(commands.disabled_commands()).unwrap(), vec!["edit:note"]);
    assert_eq!(
        block_on(commands.refusal("edit:note".into(), None)).unwrap().unwrap().reason,
        CommandRefusalReason::TurnedOff
    );
    block_on(commands.disable_category("edit".into())).unwrap();
    block_on(commands.enable_command("edit:note".into())).unwrap();
    assert!(block_on(commands.disabled_commands()).unwrap().is_empty());
    assert!(block_on(commands.is_command_disabled("edit:note".into())).unwrap());
    block_on(commands.enable_category("edit".into())).unwrap();
    assert!(!block_on(commands.is_command_disabled("edit:note".into())).unwrap());
    block_on(commands.set_disabled_commands(vec!["missing".into(), "".into()])).unwrap();
    assert!(block_on(commands.is_command_disabled("missing".into())).unwrap());
    assert!(block_on(commands.is_command_disabled("".into())).unwrap());
    assert!(block_on(commands.refusal("missing".into(), None)).unwrap().is_none());
    assert!(!block_on(commands.can_execute("missing".into(), None)).unwrap());
    block_on(commands.set_disabled_commands(vec![])).unwrap();
    block_on(thread.call(|k| plugin_conformance::test_kernel::settle(&k.kernel.destroy())))
        .unwrap()
        .unwrap();
    assert_eq!(code(&block_on(commands.disabled_commands()).unwrap_err()), Some("instance-closed"));
    assert!(block_on(commands.disable_command("x".into())).is_err());
    assert!(block_on(commands.enable_command("x".into())).is_err());
    assert!(block_on(commands.set_disabled_commands(vec![])).is_err());
    assert!(block_on(commands.is_command_disabled("x".into())).is_err());
    assert!(block_on(commands.refusal("x".into(), None)).is_err());
}

#[test]
fn direct_refusals_distinguish_permission_lock_and_temporary_enablement() {
    use epdf_plugin_commands::CommandDef;
    use epdf_plugin_commands::ffi::CommandRefusalReason;
    let thread = KernelThread::spawn(|_| {
        let mut spec = one_document();
        spec["documents"]["a"]["security"]["allows"] = serde_json::json!(["doc.open"]);
        spec["documents"]["b"] = serde_json::json!({"id":"b","security":{"allows":"*"},"pages":[]});
        let mut print = CommandDef::new("print", "Print");
        print.requires = Some(vec!["doc.print".into()]);
        let mut locked = CommandDef::new("locked", "Locked");
        locked.locked = Some(std::rc::Rc::new(|_| {
            Ok(Some(epdf_kernel::settings::SettingLock { by: "policy".into(), reason_key: None }))
        }));
        let mut temporary = CommandDef::new("temporary", "Temporary");
        temporary.enabled = Some(std::rc::Rc::new(|_| Ok(false)));
        TestKernel::open(
            vec![commands_plugin(CommandsConfig {
                commands: Some(vec![print.into(), locked.into(), temporary.into()]),
                ..Default::default()
            })],
            spec,
        )
    })
    .unwrap();
    let post = thread.post();
    let commands =
        block_on(thread.call(move |k| CommandsHandle::new(k.capability(&COMMANDS).unwrap(), post)))
            .unwrap();
    let refused = block_on(commands.refusal("print".into(), None)).unwrap().unwrap();
    assert_eq!(refused.reason, CommandRefusalReason::NotAllowed);
    assert!(!refused.message.is_empty());
    assert_eq!(
        block_on(commands.refusal("locked".into(), None)).unwrap().unwrap().reason,
        CommandRefusalReason::Locked
    );
    assert!(block_on(commands.refusal("temporary".into(), None)).unwrap().is_none());
    assert!(!block_on(commands.can_execute("temporary".into(), None)).unwrap());
    // Permanent refusal reflects the document passed to the workspace registry.
    let document_id = block_on(thread.call(|k| k.document_id.clone())).unwrap();
    assert_eq!(
        block_on(commands.refusal("print".into(), Some(document_id))).unwrap().unwrap().reason,
        CommandRefusalReason::NotAllowed
    );
    let other = block_on(thread.call(|k| {
        let opened = plugin_conformance::test_kernel::settle(&k.kernel.documents().open(
            epdf_kernel::OpenSource::Input(
                serde_json::json!({"kind":"bytes","id":"b","bytes":{"$bytes":""}}),
            ),
            epdf_kernel::OpenOptions::default(),
        ))
        .unwrap();
        opened["document"]["id"].as_str().unwrap().to_string()
    }))
    .unwrap();
    assert!(block_on(commands.refusal("print".into(), Some(other))).unwrap().is_none());
}

#[test]
fn a_swift_command_runs_with_its_context() {
    let (_thread, commands) = handle();
    let runner = Runner::new(Run::Finish, Ok(true));
    let events = Arc::new(Events::default());
    let _sub = commands.subscribe(events.clone());
    block_on(
        commands.register_command(definition("doc:save", Some(runner.clone()), run_hooks()), false),
    )
    .unwrap();
    let result =
        block_on(commands.execute("doc:save".into(), None, Some(r#"{"copy":true}"#.into())))
            .unwrap();
    assert_eq!(result, CommandExecuteResult::Executed);
    let ran = runner.ran.lock().unwrap();
    assert_eq!(ran.len(), 1);
    assert_eq!(ran[0].command_id, "doc:save");
    assert_eq!(ran[0].args_json.as_deref(), Some(r#"{"copy":true}"#));
    let seen = events.0.lock().unwrap();
    assert!(
        matches!(seen.last(), Some(CommandsEvent::Executed { event }) if event.command_id == "doc:save"
            && event.args_json.as_deref() == Some(r#"{"copy":true}"#)),
        "{seen:?}"
    );
}

#[test]
fn a_failing_command_rejects_and_reports() {
    let (_thread, commands) = handle();
    let events = Arc::new(Events::default());
    let _sub = commands.subscribe(events.clone());
    let runner = Runner::new(Run::Fail, Ok(true));
    block_on(commands.register_command(definition("doc:save", Some(runner), run_hooks()), false))
        .unwrap();
    let error = block_on(commands.execute("doc:save".into(), None, None)).unwrap_err();
    assert_eq!(code(&error), Some("operation-failed"), "{error:?}");
    let seen = events.0.lock().unwrap();
    assert!(
        matches!(seen.last(), Some(CommandsEvent::ExecutionFailed { event }) if event.error.message.contains("disk full")),
        "{seen:?}"
    );
}

#[test]
fn a_failure_with_a_code_keeps_it_through_execute_and_the_event() {
    let (_thread, commands) = handle();
    let events = Arc::new(Events::default());
    let _sub = commands.subscribe(events.clone());
    let runner = Runner::new(Run::Denied, Ok(true));
    block_on(commands.register_command(definition("doc:save", Some(runner), run_hooks()), false))
        .unwrap();
    let error = block_on(commands.execute("doc:save".into(), None, None)).unwrap_err();
    assert_eq!(code(&error), Some("permission-denied"), "{error:?}");
    assert!(format!("{error:?}").contains("no doc.annotate"), "{error:?}");
    let seen = events.0.lock().unwrap();
    assert!(
        matches!(seen.last(), Some(CommandsEvent::ExecutionFailed { event }) if event.error.code == "permission-denied" && event.error.message == "no doc.annotate"),
        "{seen:?}"
    );
}

#[test]
fn a_dropped_completion_fails_the_command() {
    let (_thread, commands) = handle();
    let events = Arc::new(Events::default());
    let _sub = commands.subscribe(events.clone());
    let runner = Runner::new(Run::Drop, Ok(true));
    block_on(commands.register_command(definition("doc:save", Some(runner), run_hooks()), false))
        .unwrap();
    let error = block_on(commands.execute("doc:save".into(), None, None)).unwrap_err();
    assert_eq!(code(&error), Some("operation-failed"), "{error:?}");
    assert!(matches!(events.0.lock().unwrap().last(), Some(CommandsEvent::ExecutionFailed { .. })));
}

#[test]
fn a_disabled_command_is_rejected_without_running() {
    let (_thread, commands) = handle();
    let runner = Runner::new(Run::Finish, Ok(false));
    block_on(
        commands.register_command(definition("doc:save", Some(runner.clone()), run_hooks()), false),
    )
    .unwrap();
    let result = block_on(commands.execute("doc:save".into(), None, None)).unwrap();
    assert_eq!(result, CommandExecuteResult::Rejected { reason: CommandRejectReason::Disabled });
    assert!(runner.ran.lock().unwrap().is_empty());
    assert!(!block_on(commands.can_execute("doc:save".into(), None)).unwrap());
    let missing = block_on(commands.execute("nope".into(), None, None)).unwrap();
    assert_eq!(missing, CommandExecuteResult::Rejected { reason: CommandRejectReason::NotFound });
}

#[test]
fn a_throwing_derivation_says_no() {
    let (_thread, commands) = handle();
    let runner = Runner::new(Run::Finish, Err(()));
    block_on(commands.register_command(definition("doc:save", Some(runner), run_hooks()), false))
        .unwrap();
    let resolved = block_on(commands.resolve_command("doc:save".into(), None)).unwrap().unwrap();
    // `enabled` throws and counts as false; `visible` is absent and true.
    assert!(!resolved.enabled);
    assert!(resolved.visible);
    assert_eq!(resolved.icon_accent.and_then(|a| a.primary).as_deref(), Some("#f00"));
}

#[test]
fn unparsable_args_are_invalid_input() {
    let (_thread, commands) = handle();
    let events = Arc::new(Events::default());
    let _sub = commands.subscribe(events.clone());
    let runner = Runner::new(Run::Finish, Ok(true));
    block_on(
        commands.register_command(definition("doc:save", Some(runner.clone()), run_hooks()), false),
    )
    .unwrap();
    let error =
        block_on(commands.execute("doc:save".into(), None, Some("{not json".into()))).unwrap_err();
    assert_eq!(code(&error), Some("invalid-input"), "{error:?}");
    assert!(runner.ran.lock().unwrap().is_empty());
    assert!(events.0.lock().unwrap().is_empty());
}

#[test]
fn registry_reads_categories_and_shortcuts() {
    let (_thread, commands) = handle();
    let mut zoom = definition("zoom:in", None, vec![]);
    zoom.shortcuts = vec!["Mod+=".into()];
    zoom.categories = vec!["zoom".into()];
    zoom.menu = Some("zoom-menu".into());
    block_on(commands.register_commands(vec![zoom, definition("zoom:out", None, vec![])], false))
        .unwrap();
    assert_eq!(block_on(commands.command_ids()).unwrap(), ["zoom:in", "zoom:out"]);
    assert!(block_on(commands.has_command("zoom:in".into())).unwrap());
    let summary = block_on(commands.command("zoom:in".into())).unwrap().unwrap();
    assert_eq!(summary.shortcuts, ["Mod+="]);
    assert!(!summary.has_runner);
    let shortcuts = block_on(commands.shortcuts()).unwrap();
    assert_eq!(
        shortcuts.iter().map(|s| (s.command_id.as_str(), s.shortcut.as_str())).collect::<Vec<_>>(),
        [("zoom:in", "Mod+=")]
    );
    let stroke = CommandKeyStroke {
        key: "=".into(),
        code: None,
        ctrl: false,
        meta: true,
        alt: false,
        shift: false,
    };
    let found = block_on(commands.match_stroke(stroke, true, false)).unwrap().unwrap();
    assert_eq!((found.command_id.as_str(), found.refusal), ("zoom:in", None));
    assert_eq!(
        block_on(commands.menu_target("zoom:in".into())).unwrap().and_then(|m| m.menu).as_deref(),
        Some("zoom-menu")
    );
    block_on(commands.disable_category("zoom".into())).unwrap();
    assert!(block_on(commands.is_category_disabled("zoom".into())).unwrap());
    let found = block_on(commands.search_commands("zoom".into(), None)).unwrap();
    assert_eq!(found.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["zoom:out"]);
    block_on(commands.set_disabled_categories(vec![])).unwrap();
    assert_eq!(block_on(commands.disabled_categories()).unwrap(), Vec::<String>::new());
    block_on(commands.enable_category("zoom".into())).unwrap();
    assert_eq!(block_on(commands.commands(None)).unwrap().len(), 2);
}

#[test]
fn a_duplicate_id_conflicts_unless_replaced() {
    let (_thread, commands) = handle();
    block_on(commands.register_command(definition("zoom:in", None, vec![]), false)).unwrap();
    let error = block_on(commands.register_command(definition("zoom:in", None, vec![]), false))
        .err()
        .unwrap();
    assert_eq!(code(&error), Some("conflict"), "{error:?}");
    block_on(commands.register_command(definition("zoom:in", None, vec![]), true)).unwrap();
}

#[test]
fn an_unparsable_shortcut_is_refused() {
    let (_thread, commands) = handle();
    let mut bad = definition("zoom:in", None, vec![]);
    bad.shortcuts = vec!["Mod+Hyper+".into()];
    assert!(block_on(commands.register_command(bad, false)).is_err());
}

#[test]
fn unregistering_a_replaced_command_keeps_the_replacement() {
    let (thread, commands) = handle();
    let first =
        block_on(commands.register_command(definition("zoom:in", None, vec![]), false)).unwrap();
    let mut second = definition("zoom:in", None, vec![]);
    second.icon = Some("plus".into());
    block_on(commands.register_command(second, true)).unwrap();
    first.unregister();
    block_on(thread.call(|_| ())).unwrap();
    let summary = block_on(commands.command("zoom:in".into())).unwrap().unwrap();
    assert_eq!(summary.icon.as_deref(), Some("plus"));
    // A registration dropped without `unregister` leaves its command alone.
    drop(block_on(commands.register_command(definition("zoom:out", None, vec![]), false)).unwrap());
    block_on(thread.call(|_| ())).unwrap();
    assert!(block_on(commands.has_command("zoom:out".into())).unwrap());
}

struct LockRunner(std::sync::atomic::AtomicBool);
impl CommandRunner for LockRunner {
    fn run(&self, _: CommandContext, completion: Arc<CommandCompletion>) {
        completion.finish();
    }
    fn derive(&self, _: CommandContext, _: CommandHook) -> Result<bool, HostError> {
        Ok(true)
    }
    fn icon_accent(&self, _: CommandContext) -> Result<Option<IconAccent>, HostError> {
        Ok(None)
    }
    fn locked(
        &self,
        _: CommandContext,
    ) -> Result<Option<epdf_plugin_commands::ffi::CommandSettingLock>, HostError> {
        Ok(self.0.load(std::sync::atomic::Ordering::SeqCst).then(|| {
            epdf_plugin_commands::ffi::CommandSettingLock {
                by: "native-kiosk".into(),
                reason_key: None,
            }
        }))
    }
}
#[test]
fn a_native_command_lock_disables_resolution_and_releases() {
    let (thread, commands) = handle();
    let runner = Arc::new(LockRunner(std::sync::atomic::AtomicBool::new(true)));
    let mut def = definition("view:single", None, vec![CommandHook::Locked]);
    def.runner = Some(runner.clone());
    block_on(commands.register_command(def, false)).unwrap();
    assert!(
        !block_on(commands.resolve_command("view:single".into(), None)).unwrap().unwrap().enabled
    );
    assert_eq!(
        block_on(commands.execute("view:single".into(), None, None)).unwrap(),
        CommandExecuteResult::Rejected { reason: CommandRejectReason::Disabled }
    );
    runner.0.store(false, std::sync::atomic::Ordering::SeqCst);
    // A command's derivations are asked once per store change (upstream
    // 7af98f4df), so the released lock shows after the next one.
    block_on(thread.call(|kernel| kernel.kernel.store().notify())).unwrap();
    assert!(
        block_on(commands.resolve_command("view:single".into(), None)).unwrap().unwrap().enabled
    );
}

#[test]
fn every_hook_receives_current_app_state_after_whole_replacement_and_failed_runs() {
    for run in [Run::Finish, Run::Fail] {
        let (thread, commands) = handle();
        let wakes = Arc::new(AtomicUsize::new(0));
        let count = wakes.clone();
        block_on(thread.call(move |kernel| {
            kernel.kernel.subscribe(std::rc::Rc::new(move || {
                count.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }));
        }))
        .unwrap();
        let runner = Runner::new(run, Ok(true));
        block_on(commands.register_command(
            definition(
                "app:inspect",
                Some(runner.clone()),
                vec![
                    CommandHook::Run,
                    CommandHook::Enabled,
                    CommandHook::Active,
                    CommandHook::Visible,
                    CommandHook::IconAccent,
                    CommandHook::Locked,
                ],
            ),
            true,
        ))
        .unwrap();
        for state in [
            epdf_kernel::undefined(),
            serde_json::json!({"done": [], "status": "review"}),
            serde_json::json!({"done": ["a"]}),
            serde_json::Value::Null,
        ] {
            // The settings merge treats the undefined marker as no change.
            // Read the initial value before applying JSON replacements.
            if !epdf_kernel::is_undefined(&state) {
                block_on(commands.set_app_state_json(state.to_string())).unwrap();
            }
            runner.derived.lock().unwrap().clear();
            let before = wakes.load(Ordering::SeqCst);
            let result = block_on(commands.execute("app:inspect".into(), None, None));
            assert!(wakes.load(Ordering::SeqCst) > before, "finished and failed runs wake readers");
            match run {
                Run::Finish => assert_eq!(result.unwrap(), CommandExecuteResult::Executed),
                Run::Fail => assert_eq!(code(&result.unwrap_err()), Some("operation-failed")),
                _ => unreachable!(),
            }
            let ran = runner.ran.lock().unwrap();
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&ran.last().unwrap().app_state_json)
                    .unwrap(),
                state
            );
            drop(ran);
            block_on(commands.resolve_command("app:inspect".into(), None)).unwrap().unwrap();
            let derived = runner.derived.lock().unwrap();
            assert!(derived.len() >= 5);
            for context in derived.iter() {
                assert_eq!(
                    serde_json::from_str::<serde_json::Value>(&context.app_state_json).unwrap(),
                    state
                );
            }
        }
    }
}

#[test]
fn invalid_app_state_keeps_the_previous_state_and_other_settings() {
    let (thread, commands) = handle();
    let runner = Runner::new(Run::Finish, Ok(true));
    block_on(
        commands
            .register_command(definition("app:inspect", Some(runner.clone()), run_hooks()), false),
    )
    .unwrap();
    block_on(thread.call(|kernel| {
        kernel.capability(&COMMANDS).unwrap().update_settings(&CommandsConfig {
            commands: Some(vec![
                epdf_plugin_commands::CommandDef::new("configured:keep", "Keep").into(),
            ]),
            ..CommandsConfig::default()
        });
    }))
    .unwrap();
    block_on(commands.disable_category("hidden".into())).unwrap();
    block_on(commands.set_app_state_json(r#"{"kept":true}"#.into())).unwrap();
    let wakes = Arc::new(AtomicUsize::new(0));
    let count = wakes.clone();
    block_on(thread.call(move |kernel| {
        kernel.kernel.subscribe(std::rc::Rc::new(move || {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }));
    }))
    .unwrap();
    let error = block_on(commands.set_app_state_json("{broken".into())).unwrap_err();
    assert_eq!(wakes.load(Ordering::SeqCst), 0, "invalid input changes nothing");
    assert_eq!(code(&error), Some("invalid-input"));
    assert!(block_on(commands.has_command("app:inspect".into())).unwrap());
    assert!(block_on(commands.has_command("configured:keep".into())).unwrap());
    assert_eq!(block_on(commands.disabled_categories()).unwrap(), ["hidden"]);
    block_on(commands.execute("app:inspect".into(), None, None)).unwrap();
    assert_eq!(runner.ran.lock().unwrap().last().unwrap().app_state_json, r#"{"kept":true}"#);
    let state =
        block_on(thread.call(|kernel| {
            (*kernel.capability(&COMMANDS).unwrap().get_settings().app_state).clone()
        }))
        .unwrap();
    assert_eq!(state, serde_json::json!({"kept":true}));
}

#[test]
fn current_app_state_controls_native_lock_availability_and_shortcut_refusal() {
    use epdf_plugin_commands::ffi::{CommandKeyStroke, CommandRefusalReason};
    let (_thread, commands) = handle();
    let runner = Runner::new(Run::Finish, Ok(true));
    let mut def =
        definition("app:locked", Some(runner.clone()), vec![CommandHook::Run, CommandHook::Locked]);
    def.shortcuts = vec!["Ctrl+L".into()];
    block_on(commands.register_command(def, false)).unwrap();
    block_on(commands.set_app_state_json(r#"{"locked":true}"#.into())).unwrap();
    assert!(
        !block_on(commands.resolve_command("app:locked".into(), None)).unwrap().unwrap().enabled
    );
    let matched = block_on(commands.match_stroke(
        CommandKeyStroke {
            key: "l".into(),
            code: None,
            ctrl: true,
            meta: false,
            alt: false,
            shift: false,
        },
        false,
        false,
    ))
    .unwrap()
    .unwrap();
    assert_eq!(matched.refusal.unwrap().reason, CommandRefusalReason::Locked);
    assert_eq!(
        block_on(commands.execute("app:locked".into(), None, None)).unwrap(),
        CommandExecuteResult::Rejected { reason: CommandRejectReason::Disabled }
    );
    assert!(runner.ran.lock().unwrap().is_empty());
    assert!(
        runner.derived.lock().unwrap().iter().all(|c| c.app_state_json == r#"{"locked":true}"#)
    );
    block_on(commands.set_app_state_json("null".into())).unwrap();
    assert!(
        block_on(commands.resolve_command("app:locked".into(), None)).unwrap().unwrap().enabled
    );
    assert_eq!(
        block_on(commands.execute("app:locked".into(), None, None)).unwrap(),
        CommandExecuteResult::Executed
    );
    assert_eq!(runner.ran.lock().unwrap().last().unwrap().app_state_json, "null");
}
