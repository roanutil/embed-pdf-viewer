#![cfg(feature = "uniffi")]

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
}

impl Runner {
    fn new(run: Run, enabled: Result<bool, ()>) -> Arc<Self> {
        Arc::new(Runner { run, enabled, ran: Mutex::default() })
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

    fn derive(&self, _context: CommandContext, hook: CommandHook) -> Result<bool, HostError> {
        match hook {
            CommandHook::Enabled => {
                self.enabled.map_err(|()| HostError::Failed { detail: "derive threw".into() })
            }
            _ => Ok(true),
        }
    }

    fn icon_accent(&self, _context: CommandContext) -> Result<Option<IconAccent>, HostError> {
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
fn a_throwing_derivation_falls_back() {
    let (_thread, commands) = handle();
    let runner = Runner::new(Run::Finish, Err(()));
    block_on(commands.register_command(definition("doc:save", Some(runner), run_hooks()), false))
        .unwrap();
    let resolved = block_on(commands.resolve_command("doc:save".into(), None)).unwrap().unwrap();
    assert!(resolved.enabled);
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
    assert_eq!(block_on(commands.match_stroke(stroke, true)).unwrap().as_deref(), Some("zoom:in"));
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
