//! The commands plugin through uniffi: typed records, one workspace handle,
//! one event enum, and Swift-defined commands through `CommandRunner`.
//!
//! A definition's closures are built only for the hooks Swift lists, so an
//! unlisted hook keeps the plugin's default and never crosses the boundary.

use std::rc::Rc;
use std::sync::Arc;

use epdf_ffi::{
    ForeignSettle, HostError, KernelBound, KernelPost, PluginErrorInfo, RegistrationHandle,
    Subscription, ViewerError,
};
use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::rt::Promise;
use epdf_kernel::store::Unsubscribe;
use epdf_ui_core::shortcuts::{KeyStroke, ShortcutOptions};
use serde_json::Value;

use crate::contract::{
    CommandCtx, CommandDef, CommandExecutedEvent, CommandExecutionFailedEvent, Derivation,
    ExecuteResult, IconAccent, MenuTarget, Panel, RegisterCommandOptions, RejectReason,
    ResolvedCommand, Shortcut, ShortcutBinding,
};
use crate::controller::CommandsCapability;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CommandRejectReason {
    NotFound,
    Disabled,
    Hidden,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CommandExecuteResult {
    Executed,
    Rejected { reason: CommandRejectReason },
}

impl From<ExecuteResult> for CommandExecuteResult {
    fn from(result: ExecuteResult) -> Self {
        match result {
            ExecuteResult::Executed => CommandExecuteResult::Executed,
            ExecuteResult::Rejected { reason } => CommandExecuteResult::Rejected {
                reason: match reason {
                    RejectReason::NotFound => CommandRejectReason::NotFound,
                    RejectReason::Disabled => CommandRejectReason::Disabled,
                    RejectReason::Hidden => CommandRejectReason::Hidden,
                },
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CommandShortcutBinding {
    pub command_id: String,
    pub shortcut: String,
}

impl From<ShortcutBinding> for CommandShortcutBinding {
    fn from(binding: ShortcutBinding) -> Self {
        CommandShortcutBinding { command_id: binding.command_id, shortcut: binding.shortcut }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CommandMenuTarget {
    pub menu: Option<String>,
}

impl From<MenuTarget> for CommandMenuTarget {
    fn from(target: MenuTarget) -> Self {
        CommandMenuTarget { menu: target.menu }
    }
}

/// A key press, as `matchStroke` reads it.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CommandKeyStroke {
    pub key: String,
    #[uniffi(default = None)]
    pub code: Option<String>,
    #[uniffi(default = false)]
    pub ctrl: bool,
    #[uniffi(default = false)]
    pub meta: bool,
    #[uniffi(default = false)]
    pub alt: bool,
    #[uniffi(default = false)]
    pub shift: bool,
}

impl From<CommandKeyStroke> for KeyStroke {
    fn from(stroke: CommandKeyStroke) -> Self {
        KeyStroke {
            key: stroke.key,
            code: stroke.code,
            ctrl_key: stroke.ctrl,
            meta_key: stroke.meta,
            alt_key: stroke.alt,
            shift_key: stroke.shift,
        }
    }
}

/// A registered definition, without its runner.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CommandSummary {
    pub id: String,
    pub label_key: String,
    pub icon: Option<String>,
    pub shortcuts: Vec<String>,
    pub categories: Vec<String>,
    pub menu: Option<String>,
    pub panel: Option<CommandPanel>,
    pub modal: Option<String>,
    pub has_runner: bool,
}

impl From<CommandDef> for CommandSummary {
    fn from(def: CommandDef) -> Self {
        CommandSummary {
            id: def.id,
            label_key: def.label_key,
            icon: def.icon,
            shortcuts: match def.shortcut {
                None => vec![],
                Some(Shortcut::One(s)) => vec![s],
                Some(Shortcut::Many(list)) => list,
            },
            categories: def.categories.unwrap_or_default(),
            menu: def.menu,
            panel: def.panel.map(|panel| match panel {
                Panel::Id(id) => CommandPanel { id, exclusive: None },
                Panel::Target { id, exclusive } => CommandPanel { id, exclusive },
            }),
            modal: def.modal,
            has_runner: def.run.is_some()
                || def.enabled.is_some()
                || def.active.is_some()
                || def.visible.is_some()
                || def.icon_accent.is_some(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CommandExecuted {
    pub command_id: String,
    pub document_id: Option<String>,
    pub args_json: Option<String>,
}

impl From<&CommandExecutedEvent> for CommandExecuted {
    fn from(event: &CommandExecutedEvent) -> Self {
        CommandExecuted {
            command_id: event.command_id.clone(),
            document_id: event.document_id.clone(),
            args_json: event.args.as_ref().map(Value::to_string),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CommandExecutionFailure {
    pub command_id: String,
    pub document_id: Option<String>,
    pub error: PluginErrorInfo,
}

impl TryFrom<&CommandExecutionFailedEvent> for CommandExecutionFailure {
    type Error = Error;

    fn try_from(event: &CommandExecutionFailedEvent) -> Result<Self, Error> {
        let field = |key: &str| {
            event.error.get(key).and_then(Value::as_str).map(str::to_string).ok_or_else(|| {
                Error::named("InvalidState", format!("[commands] failure without \"{key}\""))
                    .with("code", "invalid-state")
            })
        };
        Ok(CommandExecutionFailure {
            command_id: event.command_id.clone(),
            document_id: event.document_id.clone(),
            error: PluginErrorInfo {
                code: field("code")?,
                message: field("message")?,
                capability: field("capability")?,
            },
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CommandsEvent {
    Executed { event: CommandExecuted },
    ExecutionFailed { event: CommandExecutionFailure },
}

/// Workspace events: the stream lasts as long as the viewer.
#[uniffi::export(with_foreign)]
pub trait CommandsEventListener: Send + Sync {
    fn on_event(&self, event: CommandsEvent);
}

// A `register_command` or `register_commands` call answers with an
// `epdf_ffi::RegistrationHandle` (umbrella Task 0c). Dropping it keeps the
// commands; `unregister` removes them, each only while it is still the
// definition registered under its id.

fn args(args_json: Option<String>) -> Result<Option<Value>, Error> {
    args_json
        .map(|json| {
            serde_json::from_str(&json)
                .map_err(|e| invalid(format!("command args are not JSON: {e}")))
        })
        .transpose()
}

#[derive(uniffi::Object)]
pub struct CommandsHandle {
    cap: KernelBound<CommandsCapability>,
    post: KernelPost,
}

impl CommandsHandle {
    /// On the kernel thread.
    pub fn new(cap: CommandsCapability, post: KernelPost) -> Arc<Self> {
        Arc::new(CommandsHandle { cap: KernelBound::new(cap, post.clone()), post })
    }
}

#[uniffi::export]
impl CommandsHandle {
    /// A duplicate id throws `conflict` unless `replace`; an unparsable
    /// shortcut throws too.
    pub async fn register_command(
        &self,
        definition: CommandDefinition,
        replace: bool,
    ) -> Result<Arc<RegistrationHandle>, ViewerError> {
        let post = self.post.clone();
        self.cap
            .call(move |c| {
                let def = definition.into_def(post.clone())?;
                let remove = c.register_command(def, RegisterCommandOptions { replace })?;
                Ok(RegistrationHandle::new(post, move || remove()))
            })
            .await
    }

    /// In order; a failure stops there, with the earlier ones registered.
    pub async fn register_commands(
        &self,
        definitions: Vec<CommandDefinition>,
        replace: bool,
    ) -> Result<Arc<RegistrationHandle>, ViewerError> {
        let post = self.post.clone();
        self.cap
            .call(move |c| {
                let defs = definitions
                    .into_iter()
                    .map(|d| d.into_def(post.clone()))
                    .collect::<Result<Vec<_>, _>>()?;
                let remove = c.register_commands(defs, RegisterCommandOptions { replace })?;
                Ok(RegistrationHandle::new(post, move || remove()))
            })
            .await
    }

    pub async fn has_command(&self, id: String) -> Result<bool, ViewerError> {
        self.cap.call(move |c| Ok(c.has_command(&id))).await
    }

    pub async fn command(&self, id: String) -> Result<Option<CommandSummary>, ViewerError> {
        self.cap.call(move |c| Ok(c.get_command(&id).map(CommandSummary::from))).await
    }

    pub async fn command_ids(&self) -> Result<Vec<String>, ViewerError> {
        self.cap.call(|c| Ok(c.list_command_ids())).await
    }

    /// Every derivation runs on every call.
    pub async fn resolve_command(
        &self,
        id: String,
        document_id: Option<String>,
    ) -> Result<Option<ResolvedCommand>, ViewerError> {
        self.cap.call(move |c| Ok(c.resolve_command(&id, document_id.as_deref()))).await
    }

    pub async fn commands(
        &self,
        document_id: Option<String>,
    ) -> Result<Vec<ResolvedCommand>, ViewerError> {
        self.cap.call(move |c| Ok(c.list_commands(document_id.as_deref()))).await
    }

    /// Visible commands whose label or id contains the trimmed, lowercased
    /// query.
    pub async fn search_commands(
        &self,
        query: String,
        document_id: Option<String>,
    ) -> Result<Vec<ResolvedCommand>, ViewerError> {
        self.cap.call(move |c| Ok(c.search_commands(&query, document_id.as_deref()))).await
    }

    pub async fn shortcuts(&self) -> Result<Vec<CommandShortcutBinding>, ViewerError> {
        self.cap
            .call(|c| {
                Ok(c.list_shortcuts().into_iter().map(CommandShortcutBinding::from).collect())
            })
            .await
    }

    /// `args_json` that isn't JSON throws `invalid-input` before anything
    /// runs.
    pub async fn execute(
        &self,
        id: String,
        document_id: Option<String>,
        args_json: Option<String>,
    ) -> Result<CommandExecuteResult, ViewerError> {
        self.cap
            .call_promise(
                move |c| Ok(c.execute(&id, document_id.as_deref(), args(args_json)?)),
                CommandExecuteResult::from,
            )
            .await
    }

    pub async fn can_execute(
        &self,
        id: String,
        document_id: Option<String>,
    ) -> Result<bool, ViewerError> {
        self.cap.call(move |c| Ok(c.can_execute(&id, document_id.as_deref()))).await
    }

    pub async fn disabled_categories(&self) -> Result<Vec<String>, ViewerError> {
        self.cap.call(|c| Ok(c.get_disabled_categories())).await
    }

    pub async fn is_category_disabled(&self, category: String) -> Result<bool, ViewerError> {
        self.cap.call(move |c| Ok(c.is_category_disabled(&category))).await
    }

    pub async fn disable_category(&self, category: String) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                c.disable_category(&category);
                Ok(())
            })
            .await
    }

    pub async fn enable_category(&self, category: String) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                c.enable_category(&category);
                Ok(())
            })
            .await
    }

    pub async fn set_disabled_categories(
        &self,
        categories: Vec<String>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                c.set_disabled_categories(&categories);
                Ok(())
            })
            .await
    }

    /// The first command with a matching shortcut, enabled or not.
    pub async fn match_stroke(
        &self,
        stroke: CommandKeyStroke,
        is_mac: bool,
    ) -> Result<Option<String>, ViewerError> {
        self.cap.call(move |c| Ok(c.match_stroke(&stroke.into(), ShortcutOptions { is_mac }))).await
    }

    pub async fn menu_target(&self, id: String) -> Result<Option<CommandMenuTarget>, ViewerError> {
        self.cap.call(move |c| Ok(c.get_menu_target(&id).map(CommandMenuTarget::from))).await
    }

    /// The listener is registered on the kernel thread after this returns;
    /// a cancel before then still takes effect.
    pub fn subscribe(&self, listener: Arc<dyn CommandsEventListener>) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.cap.fire(move |c| {
            let executed = {
                let listener = listener.clone();
                c.on_executed(move |event| {
                    listener.on_event(CommandsEvent::Executed { event: event.into() });
                    Ok(())
                })
            };
            let failed = c.on_execution_failed(move |event| {
                let event = CommandExecutionFailure::try_from(event)?;
                listener.on_event(CommandsEvent::ExecutionFailed { event });
                Ok(())
            });
            slot.fill(Unsubscribe::new(move || {
                executed.call();
                failed.call();
            }));
        });
        subscription
    }
}
