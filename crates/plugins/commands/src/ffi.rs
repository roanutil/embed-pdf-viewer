//! The commands plugin through uniffi: typed records, one workspace handle,
//! one event enum, and Swift-defined commands through `CommandRunner`.
//!
//! A definition's closures are built only for the hooks Swift lists, so an
//! unlisted hook keeps the plugin's default and never crosses the boundary.

use std::rc::Rc;
use std::sync::Arc;

use epdf_ffi::{ForeignSettle, HostError, KernelPost};
use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::rt::Promise;
use serde_json::Value;

use crate::contract::{CommandCtx, CommandDef, Derivation, IconAccent, Panel, Shortcut};

fn invalid(message: String) -> Error {
    plugin_error(code::INVALID_INPUT, "commands", message, None, None)
}

/// Which of a command's hooks its runner implements.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CommandHook {
    /// `run`: the verb. Without it, the command routes to its menu, panel or
    /// modal.
    Run,
    Enabled,
    Active,
    Visible,
    IconAccent,
}

/// A shell surface the command toggles, optionally exclusive within a tag.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CommandPanel {
    pub id: String,
    #[uniffi(default = None)]
    pub exclusive: Option<String>,
}

/// What a hook sees.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CommandContext {
    pub command_id: String,
    /// The target document: explicit, else the active one, else `None`.
    pub document_id: Option<String>,
    /// The caller's arguments as JSON, when it gave any.
    pub args_json: Option<String>,
}

fn context(command_id: &str, ctx: &CommandCtx) -> CommandContext {
    CommandContext {
        command_id: command_id.to_string(),
        document_id: ctx.document_id().map(str::to_string),
        args_json: ctx.args().map(Value::to_string),
    }
}

/// A Swift-defined command's hooks. Called on the kernel thread. `derive`
/// and `icon_accent` run on every resolve and must answer from state the
/// implementation already has; a throw falls back to the default (`enabled`
/// and `visible` true, `active` false, no accent). `run` returns at once
/// and answers through `completion`, from any thread.
#[uniffi::export(with_foreign)]
pub trait CommandRunner: Send + Sync {
    fn run(&self, context: CommandContext, completion: Arc<CommandCompletion>);
    /// `hook` is `Enabled`, `Active` or `Visible`.
    fn derive(&self, context: CommandContext, hook: CommandHook) -> Result<bool, HostError>;
    fn icon_accent(&self, context: CommandContext) -> Result<Option<IconAccent>, HostError>;
}

/// The answer to one `CommandRunner::run`. The first call wins; dropping it
/// unanswered fails the command with `operation-failed`.
#[derive(uniffi::Object)]
pub struct CommandCompletion {
    settle: ForeignSettle<Value>,
}

#[uniffi::export]
impl CommandCompletion {
    pub fn finish(&self) {
        self.settle.settle(|| Ok(Value::Null));
    }

    pub fn fail(&self, detail: String) {
        self.settle.settle(move || Err(Error::new(detail).with("code", code::OPERATION_FAILED)));
    }
}

/// A command as Swift defines it.
#[derive(Clone, uniffi::Record)]
pub struct CommandDefinition {
    /// Convention: `domain:verb`.
    pub id: String,
    /// Resolved through i18n when it's installed, else shown as is.
    pub label_key: String,
    #[uniffi(default = None)]
    pub icon: Option<String>,
    /// `Mod+K` style.
    #[uniffi(default = [])]
    pub shortcuts: Vec<String>,
    /// A disabled category hides its commands everywhere.
    #[uniffi(default = [])]
    pub categories: Vec<String>,
    #[uniffi(default = None)]
    pub menu: Option<String>,
    #[uniffi(default = None)]
    pub panel: Option<CommandPanel>,
    #[uniffi(default = None)]
    pub modal: Option<String>,
    #[uniffi(default = None)]
    pub runner: Option<Arc<dyn CommandRunner>>,
    /// The hooks `runner` implements. Empty exactly when `runner` is `None`.
    #[uniffi(default = [])]
    pub hooks: Vec<CommandHook>,
}

impl CommandDefinition {
    /// On the kernel thread.
    pub fn into_def(self, post: KernelPost) -> Result<CommandDef, Error> {
        let CommandDefinition {
            id,
            label_key,
            icon,
            shortcuts,
            categories,
            menu,
            panel,
            modal,
            runner,
            hooks,
        } = self;
        let runner = match (runner, hooks.is_empty()) {
            (None, true) => None,
            (Some(runner), false) => Some(runner),
            (None, false) => {
                return Err(invalid(format!(
                    "command '{id}' lists hooks {hooks:?} but has no runner"
                )));
            }
            (Some(_), true) => {
                return Err(invalid(format!("command '{id}' has a runner but lists no hooks")));
            }
        };
        let mut def = CommandDef::new(id.clone(), label_key);
        def.icon = icon;
        def.shortcut = match shortcuts.len() {
            0 => None,
            1 => Some(Shortcut::One(shortcuts.into_iter().next().expect("one shortcut"))),
            _ => Some(Shortcut::Many(shortcuts)),
        };
        def.categories = (!categories.is_empty()).then_some(categories);
        def.menu = menu;
        def.panel = panel.map(|p| match p.exclusive {
            None => Panel::Id(p.id),
            Some(exclusive) => Panel::Target { id: p.id, exclusive: Some(exclusive) },
        });
        def.modal = modal;
        let Some(runner) = runner else { return Ok(def) };
        let derivation = |hook: CommandHook| -> Option<Derivation> {
            hooks.contains(&hook).then(|| {
                let (runner, id) = (runner.clone(), id.clone());
                Rc::new(move |ctx: &CommandCtx| {
                    runner.derive(context(&id, ctx), hook).map_err(Error::from)
                }) as Derivation
            })
        };
        def.enabled = derivation(CommandHook::Enabled);
        def.active = derivation(CommandHook::Active);
        def.visible = derivation(CommandHook::Visible);
        if hooks.contains(&CommandHook::IconAccent) {
            let (runner, id) = (runner.clone(), id.clone());
            def.icon_accent = Some(Rc::new(move |ctx: &CommandCtx| {
                runner.icon_accent(context(&id, ctx)).map_err(Error::from)
            }));
        }
        if hooks.contains(&CommandHook::Run) {
            let (runner, id) = (runner.clone(), id.clone());
            def.run = Some(Rc::new(move |ctx: &CommandCtx| {
                let (promise, resolver) = Promise::pending();
                let completion = Arc::new(CommandCompletion {
                    settle: ForeignSettle::new(
                        resolver,
                        post.clone(),
                        "a CommandRunner completion",
                    ),
                });
                runner.run(context(&id, ctx), completion);
                Ok(Some(promise))
            }));
        }
        Ok(def)
    }
}
