//! `contract.ts`: the contract. A command definition is app content and
//! holds functions, so it lives in the controller's registry, never in the
//! store; the store slice is only the serializable `disabledCategories`.

use std::any::Any;
use std::fmt;
use std::rc::Rc;
use std::sync::LazyLock;

use epdf_kernel::plugin::PluginContext;
use epdf_kernel::types::CoreState;
use epdf_kernel::{AnyToken, Error, Token};
use serde::{Deserialize, Serialize};

use epdf_kernel::rt::Promise;
use serde_json::Value;

use crate::controller::CommandsCapability;

/// What a derivation or `run` sees: capability resolution bound to the
/// command's target document (explicit, else the active one).
#[derive(Clone)]
pub struct CommandCtx {
    pub(crate) document_id: Option<String>,
    pub(crate) args: Option<Value>,
    pub(crate) ctx: PluginContext,
}

impl CommandCtx {
    /// The target document, or `None` when no document is open. An explicit
    /// empty id is kept, as `documentId ?? activeId` keeps it.
    pub fn document_id(&self) -> Option<&str> {
        self.document_id.as_deref()
    }

    /// Caller-supplied arguments (`execute(id, {args})`); absent when the
    /// caller gave none.
    pub fn args(&self) -> Option<&Value> {
        self.args.as_ref()
    }

    pub fn core(&self) -> Rc<CoreState> {
        self.ctx.core()
    }

    /// `get`, erased. A non-empty target binds document-scoped tokens to it;
    /// an empty or absent one resolves as the plugin's own context does,
    /// against the active document. Workspace tokens resolve either way.
    pub fn get_any(&self, token: &AnyToken) -> Result<Rc<dyn Any>, Error> {
        match self.document_id.as_deref() {
            Some(target) if !target.is_empty() => self.ctx.for_document_any(token, target),
            _ => self.ctx.get_any(token),
        }
    }

    /// `tryGet`, erased: `None` wherever `get` throws.
    pub fn try_get_any(&self, token: &AnyToken) -> Option<Rc<dyn Any>> {
        self.get_any(token).ok()
    }

    /// Resolve a capability; document-scoped tokens bind to the target document.
    pub fn get<C: Clone + 'static>(&self, token: &Token<C>) -> Result<C, Error> {
        epdf_kernel::plugin::downcast(self.get_any(token.any())?, token.name())
    }

    /// Like `get`, but `None` when unavailable (no provider, no document).
    pub fn try_get<C: Clone + 'static>(&self, token: &Token<C>) -> Option<C> {
        self.get(token).ok()
    }
}

/// Up to two theme colors accompanying a command's icon.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct IconAccent {
    /// The mark: stroke / markup / font color.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary: Option<String>,
    /// The fill, when there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary: Option<String>,
}

/// `enabled`, `active` and `visible`. `Err` is a throw.
pub type Derivation = Rc<dyn Fn(&CommandCtx) -> Result<bool, Error>>;
/// `iconAccent`. `Ok(None)` is a `null` (or `undefined`) answer.
pub type AccentDerivation = Rc<dyn Fn(&CommandCtx) -> Result<Option<IconAccent>, Error>>;
/// `run`: `Ok(None)` is a synchronous verb, `Ok(Some)` the promise an async
/// one returns, which `execute` awaits; `Err` a throw.
pub type RunFn = Rc<dyn Fn(&CommandCtx) -> Result<Option<Promise<Value>>, Error>>;

/// `shortcut`: one binding or several.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shortcut {
    One(String),
    Many(Vec<String>),
}

/// `panel`: a surface id, or one with an exclusivity tag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Panel {
    Id(String),
    Target { id: String, exclusive: Option<String> },
}

/// `CommandDef`. The registry owns the closures, so a closure that holds
/// the [`CommandsCapability`] (or the remover its registration returned)
/// keeps the registry alive for good; hold a
/// [`WeakCommandsCapability`](crate::WeakCommandsCapability) instead.
#[derive(Clone, Default)]
pub struct CommandDef {
    /// Convention: `domain:verb`.
    pub id: String,
    /// i18n key, resolved through `I18N` when present (else shown verbatim).
    pub label_key: String,
    pub icon: Option<String>,
    pub icon_accent: Option<AccentDerivation>,
    /// `Mod+K` style (ui-core grammar).
    pub shortcut: Option<Shortcut>,
    /// Feature-gating tags: a disabled category hides its commands everywhere.
    pub categories: Option<Vec<String>>,
    /// Toggles a named dropdown menu.
    pub menu: Option<String>,
    /// Toggles a named shell surface.
    pub panel: Option<Panel>,
    /// Toggles a modal surface (exclusive within the built-in `modal` tag).
    pub modal: Option<String>,
    pub enabled: Option<Derivation>,
    pub active: Option<Derivation>,
    pub visible: Option<Derivation>,
    /// The verb. When present it runs instead of the surface routing, and
    /// `execute` settles after it.
    pub run: Option<RunFn>,
}

impl CommandDef {
    pub fn new(id: impl Into<String>, label_key: impl Into<String>) -> Self {
        CommandDef { id: id.into(), label_key: label_key.into(), ..Default::default() }
    }
}

impl fmt::Debug for CommandDef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandDef")
            .field("id", &self.id)
            .field("label_key", &self.label_key)
            .field("icon", &self.icon)
            .field("shortcut", &self.shortcut)
            .field("categories", &self.categories)
            .field("menu", &self.menu)
            .field("panel", &self.panel)
            .field("modal", &self.modal)
            .finish_non_exhaustive()
    }
}

/// A command as a renderer sees it: everything resolved for the target document.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct ResolvedCommand {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_accent: Option<IconAccent>,
    pub shortcuts: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub menu: Option<String>,
    pub enabled: bool,
    pub active: bool,
    pub visible: bool,
    #[serde(default)]
    pub categories: Vec<String>,
}

/// Value equality over resolved commands, for bindings that memo by value.
/// `categories` is not compared, as in the TypeScript.
pub fn resolved_commands_equal(a: Option<&ResolvedCommand>, b: Option<&ResolvedCommand>) -> bool {
    let (a, b) = match (a, b) {
        (None, None) => return true,
        (Some(a), Some(b)) => (a, b),
        _ => return false,
    };
    let primary = |r: &ResolvedCommand| r.icon_accent.as_ref().and_then(|x| x.primary.clone());
    let secondary = |r: &ResolvedCommand| r.icon_accent.as_ref().and_then(|x| x.secondary.clone());
    a.id == b.id
        && a.label == b.label
        && a.icon == b.icon
        && primary(a) == primary(b)
        && secondary(a) == secondary(b)
        && a.menu == b.menu
        && a.enabled == b.enabled
        && a.active == b.active
        && a.visible == b.visible
        && a.shortcuts == b.shortcuts
}

#[derive(Clone, Debug, Default)]
pub struct CommandsConfig {
    /// The app's command definitions (the plugin ships none).
    pub commands: Vec<CommandDef>,
    /// Categories disabled at startup (host feature-gating).
    pub disabled_categories: Vec<String>,
}

/// `RegisterCommandOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RegisterCommandOptions {
    /// Replace an existing definition with the same id (otherwise a
    /// duplicate throws `conflict`).
    pub replace: bool,
}

/// `ExecuteResult`: `{status: 'executed'}`, or a refusal with its reason.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum ExecuteResult {
    Executed,
    Rejected { reason: RejectReason },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RejectReason {
    NotFound,
    Disabled,
    Hidden,
}

/// `CommandExecutedEvent`. `args` is absent when the caller gave none.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandExecutedEvent {
    pub command_id: String,
    pub document_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Value>,
}

/// `CommandExecutionFailedEvent`. `error` is `PluginErrorInfo`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandExecutionFailedEvent {
    pub command_id: String,
    pub document_id: Option<String>,
    pub error: Value,
}

/// A `{commandId, shortcut}` row of `listShortcuts`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutBinding {
    pub command_id: String,
    pub shortcut: String,
}

/// `getMenuTarget(id)`: the one fact the overflow projection needs. The
/// ui-core type itself (`ResolveMenuTarget`-shaped), so a
/// `get_menu_target` closure passes straight to `project_overflow`.
pub use epdf_ui_core::MenuTarget;

pub static COMMANDS: LazyLock<Token<CommandsCapability>> = LazyLock::new(|| {
    Token::with_hint(
        "commands",
        "add commandsPlugin() from '@embedpdf/plugin-commands' to your plugins list",
    )
});
