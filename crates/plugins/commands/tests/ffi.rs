#![cfg(feature = "uniffi")]

use std::sync::{Arc, Mutex};

use epdf_ffi::{HostError, KernelThread, ViewerError};
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
