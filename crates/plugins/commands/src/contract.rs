//! `contract.ts` and `host-contract.ts`: the contract. A definition holds
//! functions, so it lives in the settings (as a handle into the
//! registration's table, `settings.rs`) and in the controller's registry,
//! never in state: the plugin has no slice.

use std::any::Any;
use std::fmt;
use std::rc::Rc;
use std::sync::LazyLock;

use epdf_kernel::plugin::PluginContext;
use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{AnyToken, Error, Token};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::controller::CommandsCapability;

pub type CommandId = String;

/// `CommandContext`: what a command's derivations and `run` see, with
/// capabilities resolved for the command's document (the one named, else
/// the document in scope, else the active one).
#[derive(Clone)]
pub struct CommandCtx {
    pub(crate) document_id: Option<String>,
    pub(crate) args: Option<Value>,
    pub(crate) signal: Option<AbortSignal>,
    pub(crate) ctx: PluginContext,
}

impl CommandCtx {
    /// The command's document, or `None` when none is open. An explicit
    /// empty id is kept, as `documentId ?? activeId` keeps it.
    pub fn document_id(&self) -> Option<&str> {
        self.document_id.as_deref()
    }

    /// What the caller passed: `execute(id, {args})`. Absent when it passed
    /// none.
    pub fn args(&self) -> Option<&Value> {
        self.args.as_ref()
    }

    /// Fires when the caller cancels `execute()`: pass it on to the calls
    /// `run` makes.
    pub fn signal(&self) -> Option<&AbortSignal> {
        self.signal.as_ref()
    }

    /// `get`, erased. A non-empty document binds document-scoped tokens to
    /// it (`ctx.forDocument`); an empty or absent one resolves as the
    /// plugin's own context does (`ctx.get`), against the active document.
    /// Workspace tokens resolve either way.
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

    /// `get`: a capability, for the command's document when the plugin is
    /// document-scoped.
    pub fn get<C: Clone + 'static>(&self, token: &Token<C>) -> Result<C, Error> {
        epdf_kernel::plugin::downcast(self.get_any(token.any())?, token.name())
    }

    /// `tryGet`: like `get`, but `None` when the capability isn't available
    /// (no plugin, no document).
    pub fn try_get<C: Clone + 'static>(&self, token: &Token<C>) -> Option<C> {
        self.get(token).ok()
    }
}

/// `IconAccent`: up to two colors that go with a command's icon.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct IconAccent {
    /// The mark: a stroke, the markup or the text color.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary: Option<String>,
    /// The fill, when there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary: Option<String>,
}

/// `enabled`, `active`, `visible` and `allowed`. `Err` is a throw.
pub type Derivation = Rc<dyn Fn(&CommandCtx) -> Result<bool, Error>>;
/// `iconAccent`. `Ok(None)` is a `null` (or `undefined`) answer.
pub type AccentDerivation = Rc<dyn Fn(&CommandCtx) -> Result<Option<IconAccent>, Error>>;
/// `run`: `Ok(None)` is a synchronous verb, `Ok(Some)` the promise an async
/// one returns, which `execute` awaits; `Err` a throw.
pub type RunFn = Rc<dyn Fn(&CommandCtx) -> Result<Option<Promise<Value>>, Error>>;
/// A family's `names`: the same `Rc` while nothing changed.
pub type NamesFn = Rc<dyn Fn(&CommandCtx) -> Result<Rc<Vec<String>>, Error>>;
/// A family's `command`: the definition for one name. Its `id` and
/// `shortcut` are the registry's to set (`Omit<CommandDef, 'id' |
/// 'shortcut'>`), so whatever it gives for them is ignored.
pub type FamilyCommandFn = Rc<dyn Fn(&str) -> CommandDef>;

/// `shortcut`: one key or a list of them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shortcut {
    One(String),
    Many(Vec<String>),
}

/// `panel`: a surface id, or one with an exclusivity tag and `focus`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Panel {
    Id(String),
    Target { id: String, exclusive: Option<String>, focus: Option<bool> },
}

/// `CommandDef`: one command, as you define it. The registry owns the
/// closures, so a closure that holds the [`CommandsCapability`] (or the
/// remover its registration returned) keeps the registry alive for good;
/// hold a [`WeakCommandsCapability`](crate::WeakCommandsCapability)
/// instead.
#[derive(Clone, Default)]
pub struct CommandDef {
    /// A unique name, by convention `area:action`.
    pub id: String,
    /// What the user sees. With `label_key` too, what shows when no
    /// language has the key.
    pub label: Option<String>,
    /// A translation key for the label, read through the i18n plugin.
    pub label_key: Option<String>,
    pub icon: Option<String>,
    pub icon_accent: Option<AccentDerivation>,
    /// `Mod+K` style (ui-core grammar).
    pub shortcut: Option<Shortcut>,
    /// Whether its shortcut also runs while the cursor is in a text field.
    pub shortcut_in_text_fields: Option<bool>,
    /// The features it belongs to: turning one off turns it off, on whole
    /// dotted parts.
    pub categories: Option<Vec<String>>,
    /// The scope permissions it needs, checked against its document.
    pub requires: Option<Vec<String>>,
    /// Whether the session may run it, asked only while a document is open;
    /// one that throws allows it.
    pub allowed: Option<Derivation>,
    /// The key of the sentence that says why `allowed` refused it.
    pub not_allowed_key: Option<String>,
    /// Opens and closes a menu of yours.
    pub menu: Option<String>,
    /// Opens and closes a panel.
    pub panel: Option<Panel>,
    /// Opens and closes a dialog.
    pub modal: Option<String>,
    pub enabled: Option<Derivation>,
    pub active: Option<Derivation>,
    pub visible: Option<Derivation>,
    /// What it does. Not needed for a `menu`, `panel` or `modal`.
    pub run: Option<RunFn>,
}

impl CommandDef {
    /// A definition with an id and a label key.
    pub fn new(id: impl Into<String>, label_key: impl Into<String>) -> Self {
        CommandDef { id: id.into(), label_key: Some(label_key.into()), ..Default::default() }
    }

    /// The settings' `sameValue` over two definitions: the data members
    /// equal and every function the same one.
    pub fn same_value(&self, other: &CommandDef) -> bool {
        fn same<T: ?Sized>(a: &Option<Rc<T>>, b: &Option<Rc<T>>) -> bool {
            match (a, b) {
                (None, None) => true,
                (Some(a), Some(b)) => Rc::ptr_eq(a, b),
                _ => false,
            }
        }
        self.id == other.id
            && self.label == other.label
            && self.label_key == other.label_key
            && self.icon == other.icon
            && same(&self.icon_accent, &other.icon_accent)
            && self.shortcut == other.shortcut
            && self.shortcut_in_text_fields == other.shortcut_in_text_fields
            && self.categories == other.categories
            && self.requires == other.requires
            && same(&self.allowed, &other.allowed)
            && self.not_allowed_key == other.not_allowed_key
            && self.menu == other.menu
            && self.panel == other.panel
            && self.modal == other.modal
            && same(&self.enabled, &other.enabled)
            && same(&self.active, &other.active)
            && same(&self.visible, &other.visible)
            && same(&self.run, &other.run)
    }
}

impl fmt::Debug for CommandDef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandDef")
            .field("id", &self.id)
            .field("label", &self.label)
            .field("label_key", &self.label_key)
            .field("icon", &self.icon)
            .field("shortcut", &self.shortcut)
            .field("categories", &self.categories)
            .field("requires", &self.requires)
            .field("menu", &self.menu)
            .field("panel", &self.panel)
            .field("modal", &self.modal)
            .finish_non_exhaustive()
    }
}

/// `CommandFamily`: a command for each of a set of names, made from one
/// definition, such as `tool:<id>` for every tool a document has.
#[derive(Clone)]
pub struct CommandFamily {
    /// Where the commands' ids start: `tool:` makes `tool:highlight`.
    pub prefix: String,
    /// The names the command's document has, in order.
    pub names: NamesFn,
    /// The command for one name.
    pub command: FamilyCommandFn,
}

impl CommandFamily {
    /// The settings' `sameValue`: the same prefix and the same functions.
    pub fn same_value(&self, other: &CommandFamily) -> bool {
        self.prefix == other.prefix
            && Rc::ptr_eq(&self.names, &other.names)
            && Rc::ptr_eq(&self.command, &other.command)
    }
}

impl fmt::Debug for CommandFamily {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandFamily").field("prefix", &self.prefix).finish_non_exhaustive()
    }
}

/// One entry of the `commands` setting: `CommandDef | CommandFamily`.
#[derive(Clone, Debug)]
pub enum CommandItem {
    Command(Rc<CommandDef>),
    Family(Rc<CommandFamily>),
}

impl From<CommandDef> for CommandItem {
    fn from(def: CommandDef) -> Self {
        CommandItem::Command(Rc::new(def))
    }
}

impl From<CommandFamily> for CommandItem {
    fn from(family: CommandFamily) -> Self {
        CommandItem::Family(Rc::new(family))
    }
}

/// `ResolvedCommand`: a command as a button shows it.
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
    /// Why it's disabled: set when the scope doesn't allow it and the
    /// `notAllowed` setting is `disable`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled_reason: Option<String>,
}

/// `resolvedCommandsEqual`: whether two resolved commands show the same.
/// `categories` is not compared, as in the TypeScript. Every member it
/// compares is a string, a boolean or a list of strings, so the
/// TypeScript's `left === right` shortcut changes no answer.
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
        && a.disabled_reason == b.disabled_reason
        && a.shortcuts == b.shortcuts
}

/// `unregisteredCommand(id)`: what a toolbar draws for an id no plugin
/// registered.
pub fn unregistered_command(id: &str) -> ResolvedCommand {
    ResolvedCommand {
        id: id.to_string(),
        label: id.to_string(),
        icon: None,
        icon_accent: None,
        shortcuts: Vec::new(),
        menu: None,
        enabled: false,
        active: false,
        visible: false,
        categories: Vec::new(),
        disabled_reason: None,
    }
}

/// Why a command won't run whatever the moment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RefusalReason {
    TurnedOff,
    NotAllowed,
}

/// `CommandRefusal`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandRefusal {
    pub reason: RefusalReason,
    /// A sentence for the user, translated through the i18n plugin when
    /// it's there.
    pub message: String,
}

/// `ShortcutMatch` (host contract): the command a keystroke names, and why
/// it won't run when it's refused.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutMatch {
    pub command_id: String,
    pub refusal: Option<CommandRefusal>,
}

/// `matchStroke`'s options: the platform, and whether the key was typed in
/// a text field.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchStrokeOptions {
    pub is_mac: bool,
    #[serde(default)]
    pub in_text_field: bool,
}

/// `NotAllowedMode`: a command the scope doesn't allow is hidden, or shown
/// disabled with its reason.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NotAllowedMode {
    #[default]
    Hide,
    Disable,
}

/// `CommandsSettings`, as the store holds them. Each member keeps its `Rc`
/// while a change leaves it equal, as the TypeScript merge keeps the object.
#[derive(Clone, Debug, Default)]
pub struct CommandsSettings {
    /// The commands your viewer has. A later definition with an id that's
    /// taken replaces the earlier one.
    pub commands: Rc<Vec<CommandItem>>,
    /// Categories turned off, with every category under them.
    pub disabled_categories: Rc<Vec<String>>,
    /// Commands turned off one by one, by id.
    pub disabled_commands: Rc<Vec<String>>,
    pub not_allowed: NotAllowedMode,
}

/// `CommandsConfig` (`DeepPartial<CommandsSettings>`): what
/// `commands_plugin` registers and `update_settings` merges. A member left
/// `None` leaves that setting as it is.
#[derive(Clone, Debug, Default)]
pub struct CommandsConfig {
    pub commands: Option<Vec<CommandItem>>,
    pub disabled_categories: Option<Vec<String>>,
    pub disabled_commands: Option<Vec<String>>,
    pub not_allowed: Option<NotAllowedMode>,
}

/// `SettingsChangedEvent<CommandsSettings>`.
#[derive(Clone, Debug)]
pub struct CommandsSettingsChanged {
    pub settings: Rc<CommandsSettings>,
    /// The top-level settings whose value changed.
    pub changed: Vec<String>,
}

/// `RegisterCommandOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RegisterCommandOptions {
    /// Replace a command with the same id; without it, a taken id throws
    /// `conflict`.
    pub replace: bool,
}

/// `ExecuteOptions`.
#[derive(Clone, Default)]
pub struct ExecuteOptions {
    /// The document to run it for; the document in scope, else the active
    /// one, when left out.
    pub document_id: Option<String>,
    /// Passed to the command's `run` as `args`.
    pub args: Option<Value>,
    /// Rejects `execute` with `operation-cancelled` when it fires.
    pub signal: Option<AbortSignal>,
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
/// ui-core type itself (`ResolveMenuTarget`-shaped).
pub use epdf_ui_core::MenuTarget;

/// `CommandsToken`.
pub static COMMANDS: LazyLock<Token<CommandsCapability>> = LazyLock::new(|| {
    Token::with_hint(
        "commands",
        "add commandsPlugin() from '@embedpdf/plugin-commands' to your plugins list",
    )
});
