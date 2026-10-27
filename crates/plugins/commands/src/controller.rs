//! `controller.ts`: the commands (from the settings, added while the app
//! runs, and from families), resolution for a document (derivations over
//! live state, safe to throw), the one way to run a command, what the
//! session may run, and the host-only keystroke matcher. Turning commands
//! off is `turning_off.rs`.
//!
//! Definitions hold functions, so the commands added while the app runs
//! are a map outside state; every change to it calls `ctx.notify()` so
//! readers read again.
//!
//! The TypeScript's `memo` and `memoByKey` reads are ported as caches that
//! hold their inputs and compare them as the TypeScript's `Object.is` does:
//! an `Rc` by `Rc::ptr_eq` (the cache keeps it alive, so its address is
//! never reused), a string or a boolean by value. None is bounded, as none
//! is in the TypeScript.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use epdf_kernel::cancellable::cancellable;
use epdf_kernel::error::{code, plugin_error, to_plugin_error};
use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::rt::{self, Promise};
use epdf_kernel::settings::InstanceSettings;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::store::Unsubscribe;
use epdf_kernel::{Controller, DOCUMENTS, Error};
use epdf_plugin_shell::{OpenSurfaceOptions, SHELL};
use epdf_ui_core::js::trim;
use epdf_ui_core::shortcuts::{KeyStroke, ShortcutOptions, match_shortcut};
use serde_json::Value;

use crate::availability::{
    is_turned_off, label_of, not_allowed_key_of, not_allowed_message, refusal_of,
};
use crate::contract::{
    AccentDerivation, CommandCtx, CommandDef, CommandExecutedEvent, CommandExecutionFailedEvent,
    CommandFamily, CommandItem, CommandRefusal, CommandsConfig, CommandsSettings,
    CommandsSettingsChanged, Derivation, ExecuteOptions, ExecuteResult, IconAccent,
    MatchStrokeOptions, MenuTarget, NotAllowedMode, Panel, RegisterCommandOptions, RejectReason,
    ResolvedCommand, ShortcutBinding, ShortcutMatch,
};
use crate::registry::{
    CommandRegistry, ConfiguredCommands, RegisteredCommand, configured_commands, registered_command,
};
use crate::settings::CommandTable;

/// The context the controller receives: the plugin has no slice.
pub type CommandsContext = Controller<Value, Value>;

/// `panelTarget`: a string panel is `{id}`, an object panel is itself.
fn panel_target(def: &CommandDef) -> Option<(&str, Option<&str>, Option<bool>)> {
    match def.panel.as_ref()? {
        Panel::Id(id) => Some((id, None, None)),
        Panel::Target { id, exclusive, focus } => Some((id, exclusive.as_deref(), *focus)),
    }
}

/// A `menu` or `modal` id as a condition: absent and empty are both falsy.
fn truthy(value: &Option<String>) -> Option<&str> {
    value.as_deref().filter(|v| !v.is_empty())
}

/// `derive`: a derivation that throws falls back to the safe default.
fn derive(f: Option<&Derivation>, c: &CommandCtx, fallback: bool) -> bool {
    match f {
        None => fallback,
        Some(f) => f(c).unwrap_or(fallback),
    }
}

/// `deriveAccent`: a throw means no accent.
fn derive_accent(f: Option<&AccentDerivation>, c: &CommandCtx) -> Option<IconAccent> {
    f.and_then(|f| f(c).ok().flatten())
}

fn same_entry(a: &Option<Rc<RegisteredCommand>>, b: &Option<Rc<RegisteredCommand>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        _ => false,
    }
}

fn same_resolved(a: &Option<Rc<ResolvedCommand>>, b: &Option<Rc<ResolvedCommand>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        _ => false,
    }
}

/// `Derivations`: one command's live derivations for a document, as values
/// a memo can compare.
#[derive(Clone)]
struct Derivations {
    entry: Option<Rc<RegisteredCommand>>,
    label: String,
    accent_primary: Option<String>,
    accent_secondary: Option<String>,
    enabled: bool,
    active: bool,
    visible: bool,
    disabled_reason: Option<String>,
}

impl Derivations {
    /// `NOT_REGISTERED`.
    fn not_registered() -> Self {
        Derivations {
            entry: None,
            label: String::new(),
            accent_primary: None,
            accent_secondary: None,
            enabled: false,
            active: false,
            visible: false,
            disabled_reason: None,
        }
    }

    /// `sameInputs`: the entry by identity, the rest by value.
    fn same(&self, other: &Derivations) -> bool {
        same_entry(&self.entry, &other.entry)
            && self.label == other.label
            && self.accent_primary == other.accent_primary
            && self.accent_secondary == other.accent_secondary
            && self.enabled == other.enabled
            && self.active == other.active
            && self.visible == other.visible
            && self.disabled_reason == other.disabled_reason
    }

    /// The memo's compute: the resolved command, or `None` for no entry.
    fn resolved(&self) -> Option<Rc<ResolvedCommand>> {
        let entry = self.entry.as_ref()?;
        let def = &entry.def;
        let icon_accent = match (&self.accent_primary, &self.accent_secondary) {
            (None, None) => None,
            (primary, secondary) => {
                Some(IconAccent { primary: primary.clone(), secondary: secondary.clone() })
            }
        };
        Some(Rc::new(ResolvedCommand {
            id: def.id.clone(),
            label: self.label.clone(),
            icon: def.icon.clone(),
            icon_accent,
            shortcuts: entry.shortcuts.clone(),
            menu: def.menu.clone(),
            enabled: self.enabled,
            active: self.active,
            visible: self.visible,
            categories: def.categories.clone().unwrap_or_default(),
            disabled_reason: self.disabled_reason.clone(),
        }))
    }
}

/// `idsByDocument`'s inputs: the defined commands, the configured ones and
/// each family's names, all by identity.
struct IdsInputs {
    defined: Defined,
    configured: Rc<ConfiguredCommands>,
    names: Vec<Rc<Vec<String>>>,
}

impl IdsInputs {
    fn same(&self, other: &IdsInputs) -> bool {
        Rc::ptr_eq(&self.defined, &other.defined)
            && Rc::ptr_eq(&self.configured, &other.configured)
            && self.names.len() == other.names.len()
            && self.names.iter().zip(&other.names).all(|(a, b)| Rc::ptr_eq(a, b))
    }
}

type Defined = Rc<Vec<Rc<RegisteredCommand>>>;
type ResolutionKey = (String, Option<String>);
type ConfiguredMemo = (Rc<Vec<CommandItem>>, Rc<ConfiguredCommands>);
type DefinedMemo = (u64, Rc<ConfiguredCommands>, Defined);
type FamilyMemo = (Rc<CommandFamily>, Vec<(String, Rc<RegisteredCommand>)>);
type ResolvedMemo = (Derivations, Option<Rc<ResolvedCommand>>);
type IdsMemo = (IdsInputs, Rc<Vec<String>>);
type ListMemo = (Vec<Option<Rc<ResolvedCommand>>>, Rc<Vec<Rc<ResolvedCommand>>>);
type ShortcutsMemo = (Defined, Rc<Vec<ShortcutBinding>>);

struct Inner {
    cx: CommandsContext,
    /// `ctx.settings()`: the instance's view of the registration's store.
    settings: InstanceSettings,
    /// The registration's definitions, which the store's JSON names by
    /// handle.
    table: Rc<CommandTable>,
    /// The last settings JSON decoded, and what it decoded to.
    decoded: RefCell<Option<(Rc<Value>, Rc<CommandsSettings>)>>,
    /// Commands added while the app runs, in the order they were added.
    registered: RefCell<CommandRegistry>,
    /// Bumped on every change to `registered`.
    registered_version: Cell<u64>,
    configured: RefCell<Option<ConfiguredMemo>>,
    defined: RefCell<Option<DefinedMemo>>,
    /// A family's command for one name, made once so it keeps its
    /// identity. The TypeScript's `WeakMap` lets a family's entries go with
    /// the family; these stay for the controller's life.
    family_commands: RefCell<Vec<FamilyMemo>>,
    resolved: RefCell<HashMap<ResolutionKey, ResolvedMemo>>,
    ids: RefCell<HashMap<String, IdsMemo>>,
    lists: RefCell<HashMap<String, ListMemo>>,
    shortcuts: RefCell<Option<ShortcutsMemo>>,
    /// `NO_NAMES`.
    no_names: Rc<Vec<String>>,
    executed: EventHookSource<CommandExecutedEvent>,
    execution_failed: EventHookSource<CommandExecutionFailedEvent>,
}

/// `CommandsHostCapability`: the public `CommandsCapability` plus the host
/// lens (`match_stroke`, `get_menu_target`). Cheap to clone; clones share
/// the registry. [`in_scope`](Self::in_scope) gives the view a document's
/// scope sees.
#[derive(Clone)]
pub struct CommandsCapability {
    inner: Rc<Inner>,
    /// The document in scope (`inScope`): what a call that leaves out the
    /// document is for.
    scope: Option<Rc<str>>,
}

/// A [`CommandsCapability`] that doesn't keep the registry alive. Rust
/// only: a definition's closures live in the registry, so one that holds
/// the capability itself forms an `Rc` cycle no garbage collector reclaims.
/// Hold this instead, or resolve `COMMANDS` through the `CommandCtx`.
#[derive(Clone)]
pub struct WeakCommandsCapability {
    inner: Weak<Inner>,
    scope: Option<Rc<str>>,
}

impl WeakCommandsCapability {
    /// The capability, while anything else still holds it.
    pub fn upgrade(&self) -> Option<CommandsCapability> {
        self.inner.upgrade().map(|inner| CommandsCapability { inner, scope: self.scope.clone() })
    }
}

impl CommandsCapability {
    /// A handle for a definition's closures; see [`WeakCommandsCapability`].
    pub fn downgrade(&self) -> WeakCommandsCapability {
        WeakCommandsCapability { inner: Rc::downgrade(&self.inner), scope: self.scope.clone() }
    }

    /// `inScope(commands, documentId)`: the capability as a document's
    /// scope sees it. A call that leaves out the document is for that one:
    /// `resolve_command`, `list_commands`, `search_commands`, `execute`,
    /// `can_execute` and `get_refusal`.
    pub fn in_scope(&self, document_id: &str) -> CommandsCapability {
        CommandsCapability { inner: self.inner.clone(), scope: Some(document_id.into()) }
    }

    /// The document a call is for: the one it names, else the one in scope.
    fn target<'a>(&'a self, document_id: Option<&'a str>) -> Option<&'a str> {
        document_id.or(self.scope.as_deref())
    }

    // ── settings ──

    /// `settings.get()`, decoded once per settings value.
    pub(crate) fn current_settings(&self) -> Rc<CommandsSettings> {
        self.settings_of(self.inner.settings.get())
    }

    fn settings_of(&self, json: Rc<Value>) -> Rc<CommandsSettings> {
        let mut decoded = self.inner.decoded.borrow_mut();
        if let Some((cached, settings)) = &*decoded
            && Rc::ptr_eq(cached, &json)
        {
            return settings.clone();
        }
        let previous = decoded.as_ref().map(|(json, settings)| (json.as_ref(), settings.as_ref()));
        let cx = self.inner.cx.clone();
        let settings = Rc::new(
            self.inner.table.decode(&json, previous, &move |error| cx.report_error(&error)),
        );
        *decoded = Some((json, settings.clone()));
        settings
    }

    /// `getSettings()`: the settings in effect, the same `Rc` until a
    /// setting changes.
    pub fn get_settings(&self) -> Rc<CommandsSettings> {
        self.current_settings()
    }

    /// `updateSettings(changes)`: merge `changes` into the settings. Fires
    /// `on_settings_changed` once when a setting changed.
    pub fn update_settings(&self, changes: &CommandsConfig) {
        self.inner.settings.store.update_settings(&self.inner.table.encode(changes));
        self.inner.table.release_unnamed(&self.inner.settings.store);
    }

    /// How many definitions the registration's table holds (M37's guard).
    #[doc(hidden)]
    pub fn retained_commands(&self) -> usize {
        self.inner.table.retained()
    }

    /// `resetSettings()`: back to what the app registered.
    pub fn reset_settings(&self) {
        self.inner.settings.store.reset_settings();
        self.inner.table.release_unnamed(&self.inner.settings.store);
    }

    /// `onSettingsChanged(listener)`.
    pub fn on_settings_changed(
        &self,
        listener: impl Fn(&CommandsSettingsChanged) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        let this = self.downgrade();
        self.inner.settings.events.on(move |event| {
            let Some(this) = this.upgrade() else { return Ok(()) };
            listener(&CommandsSettingsChanged {
                settings: this.settings_of(event.settings.clone()),
                changed: event.changed.clone(),
            })
        })
    }

    // ── the commands ──

    /// `commandContext`: capabilities for the command's document, the one
    /// named, else the active one. Reading the active one throws on a
    /// destroyed kernel.
    fn command_context(
        &self,
        document_id: Option<&str>,
        args: Option<Value>,
        signal: Option<AbortSignal>,
    ) -> Result<CommandCtx, Error> {
        let ctx = self.inner.cx.plugin_context();
        let target = match document_id {
            Some(id) => Some(id.to_string()),
            None => ctx.get(&DOCUMENTS)?.get_active_id(),
        };
        Ok(CommandCtx { document_id: target, args, signal, ctx: ctx.clone() })
    }

    /// `registeredChanged`.
    fn registered_changed(&self) {
        let inner = &self.inner;
        inner.registered_version.set(inner.registered_version.get() + 1);
        inner.cx.notify();
    }

    /// `configured`: the commands the settings define, rebuilt when the
    /// setting changes. A list with an unparsable shortcut throws on every
    /// read.
    fn configured(&self) -> Result<Rc<ConfiguredCommands>, Error> {
        let items = self.current_settings().commands.clone();
        if let Some((held, configured)) = &*self.inner.configured.borrow()
            && Rc::ptr_eq(held, &items)
        {
            return Ok(configured.clone());
        }
        let configured = Rc::new(configured_commands(&items)?);
        *self.inner.configured.borrow_mut() = Some((items, configured.clone()));
        Ok(configured)
    }

    /// `familyCommand`: a family's command for one name, made once.
    fn family_command(&self, family: &Rc<CommandFamily>, name: &str) -> Rc<RegisteredCommand> {
        let cached = self.inner.family_commands.borrow().iter().find_map(|(f, by_name)| {
            Rc::ptr_eq(f, family)
                .then(|| by_name.iter().find(|(n, _)| n == name).map(|(_, e)| e.clone()))
                .flatten()
        });
        if let Some(entry) = cached {
            return entry;
        }
        let mut def = (family.command)(name);
        def.id = format!("{}{name}", family.prefix);
        def.shortcut = None;
        let entry = Rc::new(RegisteredCommand {
            def: Rc::new(def),
            shortcuts: Vec::new(),
            parsed: Vec::new(),
        });
        let mut families = self.inner.family_commands.borrow_mut();
        match families.iter_mut().find(|(f, _)| Rc::ptr_eq(f, family)) {
            Some((_, by_name)) => match by_name.iter_mut().find(|(n, _)| n == name) {
                Some(slot) => slot.1 = entry.clone(),
                None => by_name.push((name.to_string(), entry.clone())),
            },
            None => families.push((family.clone(), vec![(name.to_string(), entry.clone())])),
        }
        entry
    }

    /// `entryOf`: one added while the app runs, else the settings', else a
    /// family's.
    fn entry_of(&self, id: &str) -> Result<Option<Rc<RegisteredCommand>>, Error> {
        let configured = self.configured()?;
        let registered = self.inner.registered.borrow().get(id);
        if let Some(entry) = registered.or_else(|| configured.commands.get(id)) {
            return Ok(Some(entry));
        }
        let family = configured.families.iter().find(|family| {
            id.starts_with(family.prefix.as_str()) && id.len() > family.prefix.len()
        });
        Ok(family.map(|family| self.family_command(family, &id[family.prefix.len()..])))
    }

    /// `definedCommands`: every command with its own definition (not a
    /// family's), each id once, the settings' first.
    fn defined_commands(&self) -> Result<Defined, Error> {
        let version = self.inner.registered_version.get();
        let configured = self.configured()?;
        if let Some((held_version, held, defined)) = &*self.inner.defined.borrow()
            && *held_version == version
            && Rc::ptr_eq(held, &configured)
        {
            return Ok(defined.clone());
        }
        let mut entries = CommandRegistry::new();
        for entry in configured.commands.values() {
            entries.set(entry.def.id.clone(), entry.clone());
        }
        for entry in self.inner.registered.borrow().values() {
            entries.set(entry.def.id.clone(), entry.clone());
        }
        let defined = Rc::new(entries.values().cloned().collect::<Vec<_>>());
        *self.inner.defined.borrow_mut() = Some((version, configured, defined.clone()));
        Ok(defined)
    }

    /// `namesOf`: a family's names, none when it throws.
    fn names_of(&self, family: &CommandFamily, context: &CommandCtx) -> Rc<Vec<String>> {
        (family.names)(context).unwrap_or_else(|_| self.inner.no_names.clone())
    }

    /// `derivationsOf`.
    fn derivations_of(
        &self,
        entry: &Rc<RegisteredCommand>,
        document_id: Option<&str>,
    ) -> Result<Derivations, Error> {
        let def = &entry.def;
        let context = self.command_context(document_id, None, None)?;
        let label = label_of(def, &context);
        // Turned off hides it; not allowed hides it, or disables it with the
        // reason.
        let turned_off = is_turned_off(def, &self.current_settings());
        let not_allowed = if turned_off { None } else { not_allowed_key_of(def, &context) };
        let shown_when_not_allowed = self.current_settings().not_allowed == NotAllowedMode::Disable;
        let ruled_out = turned_off || not_allowed.is_some();
        // A command that opens something is active while it's open, unless
        // it says otherwise.
        let active = match &def.active {
            Some(active) => derive(Some(active), &context, false),
            None => match context.try_get(&SHELL) {
                None => false,
                Some(shell) => {
                    if let Some(menu) = truthy(&def.menu) {
                        shell.is_menu_open(menu)
                    } else if let Some((panel, _, _)) = panel_target(def) {
                        shell.is_open(panel)
                    } else if let Some(modal) = truthy(&def.modal) {
                        shell.is_open(modal)
                    } else {
                        false
                    }
                }
            },
        };
        let accent = derive_accent(def.icon_accent.as_ref(), &context);
        let enabled = !ruled_out && derive(def.enabled.as_ref(), &context, true);
        let visible = !turned_off
            && (not_allowed.is_none() || shown_when_not_allowed)
            && derive(def.visible.as_ref(), &context, true);
        let disabled_reason = match &not_allowed {
            Some(key) if shown_when_not_allowed => Some(not_allowed_message(key, &label, &context)),
            _ => None,
        };
        let (accent_primary, accent_secondary) = match accent {
            Some(accent) => (accent.primary, accent.secondary),
            None => (None, None),
        };
        Ok(Derivations {
            entry: Some(entry.clone()),
            label,
            accent_primary,
            accent_secondary,
            enabled,
            active,
            visible,
            disabled_reason,
        })
    }

    /// `resolvedByKey`: a resolved command, the same `Rc` until one of its
    /// derivations or its entry changes.
    fn resolved_by_key(
        &self,
        id: &str,
        document_id: Option<&str>,
    ) -> Result<Option<Rc<ResolvedCommand>>, Error> {
        let derivations = match self.entry_of(id)? {
            Some(entry) => self.derivations_of(&entry, document_id)?,
            None => Derivations::not_registered(),
        };
        let key = (id.to_string(), document_id.map(str::to_string));
        if let Some((held, resolved)) = self.inner.resolved.borrow().get(&key)
            && held.same(&derivations)
        {
            return Ok(resolved.clone());
        }
        let resolved = derivations.resolved();
        self.inner.resolved.borrow_mut().insert(key, (derivations, resolved.clone()));
        Ok(resolved)
    }

    /// The controller's own `resolveCommand`, with no document in scope.
    fn resolve_for(
        &self,
        id: &str,
        document_id: Option<&str>,
    ) -> Result<Option<Rc<ResolvedCommand>>, Error> {
        if self.entry_of(id)?.is_none() {
            return Ok(None);
        }
        self.resolved_by_key(id, document_id)
    }

    /// `idsByDocument`: every command's id for a document (`""` is the
    /// active one): the defined ones in the order they were added, then
    /// each family's names that no defined command took.
    fn ids_by_document(&self, document_key: &str) -> Result<Rc<Vec<String>>, Error> {
        let configured = self.configured()?;
        let target = (!document_key.is_empty()).then_some(document_key);
        let context = self.command_context(target, None, None)?;
        let inputs = IdsInputs {
            defined: self.defined_commands()?,
            names: configured.families.iter().map(|f| self.names_of(f, &context)).collect(),
            configured,
        };
        if let Some((held, ids)) = self.inner.ids.borrow().get(document_key)
            && held.same(&inputs)
        {
            return Ok(ids.clone());
        }
        let mut ids: Vec<String> = inputs.defined.iter().map(|e| e.def.id.clone()).collect();
        let mut taken: std::collections::HashSet<String> = ids.iter().cloned().collect();
        for (family, names) in inputs.configured.families.iter().zip(&inputs.names) {
            for name in names.iter() {
                let id = format!("{}{name}", family.prefix);
                if taken.insert(id.clone()) {
                    ids.push(id);
                }
            }
        }
        let ids = Rc::new(ids);
        self.inner.ids.borrow_mut().insert(document_key.to_string(), (inputs, ids.clone()));
        Ok(ids)
    }

    /// `commandsByDocument`: every command resolved for a document, the
    /// same `Rc` while every one of them is.
    fn commands_by_document(
        &self,
        document_key: &str,
    ) -> Result<Rc<Vec<Rc<ResolvedCommand>>>, Error> {
        let ids = self.ids_by_document(document_key)?;
        let target = (!document_key.is_empty()).then_some(document_key);
        let resolved =
            ids.iter().map(|id| self.resolve_for(id, target)).collect::<Result<Vec<_>, Error>>()?;
        if let Some((held, list)) = self.inner.lists.borrow().get(document_key)
            && held.len() == resolved.len()
            && held.iter().zip(&resolved).all(|(a, b)| same_resolved(a, b))
        {
            return Ok(list.clone());
        }
        let list = Rc::new(resolved.iter().flatten().cloned().collect::<Vec<_>>());
        self.inner.lists.borrow_mut().insert(document_key.to_string(), (resolved, list.clone()));
        Ok(list)
    }

    /// `registerCommand`: add a command while the app runs, and get a
    /// function that removes it. A taken id errs `conflict` unless
    /// `replace`; an unparsable shortcut errs before anything is
    /// registered. The remover removes this registration only: a later
    /// replacement is not its to remove. It holds the registry and its
    /// entry weakly, so a remover the definition's own closures hold
    /// doesn't keep either alive.
    pub fn register_command(
        &self,
        def: impl Into<Rc<CommandDef>>,
        options: RegisterCommandOptions,
    ) -> Result<Rc<dyn Fn()>, Error> {
        let def = def.into();
        let taken =
            self.inner.registered.borrow().has(&def.id) || self.configured()?.commands.has(&def.id);
        if taken && !options.replace {
            return Err(plugin_error(
                code::CONFLICT,
                "commands",
                format!("duplicate command '{}'", def.id),
                None,
                None,
            ));
        }
        let id = def.id.clone();
        let entry = registered_command(def)?;
        self.inner.registered.borrow_mut().set(id.clone(), entry.clone());
        self.registered_changed();
        let this = self.downgrade();
        let entry = Rc::downgrade(&entry);
        Ok(Rc::new(move || {
            let Some(this) = this.upgrade() else { return };
            let ours = this
                .inner
                .registered
                .borrow()
                .get(&id)
                .is_some_and(|current| Weak::ptr_eq(&Rc::downgrade(&current), &entry));
            if !ours {
                return;
            }
            this.inner.registered.borrow_mut().delete(&id);
            this.registered_changed();
        }))
    }

    /// `registerCommands`: `register_command` for each definition, in
    /// order; the function removes all of them. A failure stops there, with
    /// the earlier ones registered and no remover for them, as in the
    /// TypeScript.
    pub fn register_commands(
        &self,
        defs: impl IntoIterator<Item = Rc<CommandDef>>,
        options: RegisterCommandOptions,
    ) -> Result<Rc<dyn Fn()>, Error> {
        let removers = defs
            .into_iter()
            .map(|def| self.register_command(def, options))
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(Rc::new(move || removers.iter().for_each(|remove| remove())))
    }

    /// `hasCommand`: with a family, every id that starts with its prefix
    /// does.
    pub fn has_command(&self, id: &str) -> Result<bool, Error> {
        Ok(self.entry_of(id)?.is_some())
    }

    /// `getCommand`: a command as you defined it.
    pub fn get_command(&self, id: &str) -> Result<Option<Rc<CommandDef>>, Error> {
        Ok(self.entry_of(id)?.map(|entry| entry.def.clone()))
    }

    /// `listCommandIds`: every command's id for the active document, in the
    /// order they were added. The same `Rc` until one changes.
    pub fn list_command_ids(&self) -> Result<Rc<Vec<String>>, Error> {
        self.ids_by_document("")
    }

    // ── what a button shows ──

    /// `resolveCommand`: a command as a button shows it, or `None` for an
    /// unknown id. The same `Rc` while nothing about it changed. Every
    /// derivation runs on every call.
    pub fn resolve_command(
        &self,
        id: &str,
        document_id: Option<&str>,
    ) -> Result<Option<Rc<ResolvedCommand>>, Error> {
        self.resolve_for(id, self.target(document_id))
    }

    /// `listCommands`: every command, resolved, in the order they were
    /// added. The same `Rc` while nothing changed. A derivation that
    /// removes a command ahead of the walk makes it skip that one, and one
    /// it registers is not visited.
    pub fn list_commands(
        &self,
        document_id: Option<&str>,
    ) -> Result<Rc<Vec<Rc<ResolvedCommand>>>, Error> {
        self.commands_by_document(self.target(document_id).unwrap_or(""))
    }

    /// `searchCommands`: the visible commands whose lowercased label
    /// contains the trimmed, lowercased query, or whose id does.
    pub fn search_commands(
        &self,
        query: &str,
        document_id: Option<&str>,
    ) -> Result<Vec<Rc<ResolvedCommand>>, Error> {
        let needle = trim(query).to_lowercase();
        Ok(self
            .list_commands(document_id)?
            .iter()
            .filter(|command| {
                command.visible
                    && (needle.is_empty()
                        || command.label.to_lowercase().contains(&needle)
                        || command.id.contains(&needle))
            })
            .cloned()
            .collect())
    }

    /// `listShortcuts`: every shortcut with its command. The same `Rc`
    /// until the commands change.
    pub fn list_shortcuts(&self) -> Result<Rc<Vec<ShortcutBinding>>, Error> {
        let defined = self.defined_commands()?;
        if let Some((held, list)) = &*self.inner.shortcuts.borrow()
            && Rc::ptr_eq(held, &defined)
        {
            return Ok(list.clone());
        }
        let list = Rc::new(
            defined
                .iter()
                .flat_map(|entry| {
                    entry.shortcuts.iter().map(|shortcut| ShortcutBinding {
                        command_id: entry.def.id.clone(),
                        shortcut: shortcut.clone(),
                    })
                })
                .collect::<Vec<_>>(),
        );
        *self.inner.shortcuts.borrow_mut() = Some((defined, list.clone()));
        Ok(list)
    }

    // ── running them ──

    /// `runCommand`: what running a command does, its `run`, or opening
    /// what it opens.
    fn run_command(def: Rc<CommandDef>, context: CommandCtx) -> Promise<()> {
        Promise::spawn(async move {
            if let Some(run) = &def.run {
                match run(&context)? {
                    // `await undefined`.
                    None => rt::tick().await,
                    Some(pending) => {
                        pending.await?;
                    }
                }
                return Ok(());
            }
            let Some(shell) = context.try_get(&SHELL) else { return Ok(()) };
            if let Some(menu) = truthy(&def.menu) {
                shell.toggle_menu(menu);
            } else if let Some((panel, exclusive, focus)) = panel_target(&def) {
                let options = OpenSurfaceOptions {
                    exclusive: exclusive.map(str::to_string),
                    props: None,
                    focus: focus.unwrap_or(false),
                };
                shell.toggle(panel, Some(options));
            } else if let Some(modal) = truthy(&def.modal) {
                let options = OpenSurfaceOptions {
                    exclusive: Some("modal".into()),
                    props: None,
                    focus: false,
                };
                shell.toggle(modal, Some(options));
            }
            Ok(())
        })
    }

    /// `execute`: run a command. Resolves `Executed` once its `run` has,
    /// firing `on_executed`, or `Rejected` for a command that is unknown,
    /// hidden or disabled. A `run` that throws or rejects fires
    /// `on_execution_failed` and rejects with its `PluginError`; the signal
    /// rejects `operation-cancelled`.
    pub fn execute(&self, id: &str, options: ExecuteOptions) -> Promise<ExecuteResult> {
        let this = self.clone();
        let id = id.to_string();
        let ExecuteOptions { document_id, args, signal } = options;
        let document_id = document_id.or_else(|| self.scope.as_deref().map(str::to_string));
        Promise::spawn(async move {
            if signal.as_ref().is_some_and(AbortSignal::aborted) {
                return Err(plugin_error(
                    code::OPERATION_CANCELLED,
                    "commands",
                    "operation cancelled",
                    None,
                    None,
                ));
            }
            let Some(entry) = this.entry_of(&id)? else {
                return Ok(ExecuteResult::Rejected { reason: RejectReason::NotFound });
            };
            let resolved = this.resolve_for(&id, document_id.as_deref())?;
            let Some(resolved) = resolved.filter(|r| r.visible) else {
                return Ok(ExecuteResult::Rejected { reason: RejectReason::Hidden });
            };
            if !resolved.enabled {
                return Ok(ExecuteResult::Rejected { reason: RejectReason::Disabled });
            }
            let context =
                this.command_context(document_id.as_deref(), args.clone(), signal.clone())?;
            let task = Self::run_command(entry.def.clone(), context.clone());
            if let Err(error) = cancellable("commands", signal.clone(), task).await {
                // The caller cancelling isn't the command failing.
                if signal.as_ref().is_some_and(AbortSignal::aborted)
                    && error.is_plugin_error(Some(code::OPERATION_CANCELLED))
                {
                    return Err(error);
                }
                let failure = to_plugin_error("commands", &error);
                this.inner.execution_failed.emit(&CommandExecutionFailedEvent {
                    command_id: id,
                    document_id: context.document_id.clone(),
                    error: failure.plugin_error_info(),
                });
                return Err(failure);
            }
            this.inner.executed.emit(&CommandExecutedEvent {
                command_id: id,
                document_id: context.document_id.clone(),
                args,
            });
            Ok(ExecuteResult::Executed)
        })
    }

    /// `canExecute`: it exists, and is enabled and visible.
    pub fn can_execute(&self, id: &str, document_id: Option<&str>) -> Result<bool, Error> {
        Ok(self.resolve_command(id, document_id)?.is_some_and(|r| r.enabled && r.visible))
    }

    /// `getRefusal`: why a command won't run whatever the moment; `None`
    /// when it may run (or only can't right now), and for an unknown id.
    pub fn get_refusal(
        &self,
        id: &str,
        document_id: Option<&str>,
    ) -> Result<Option<CommandRefusal>, Error> {
        let Some(entry) = self.entry_of(id)? else { return Ok(None) };
        let context = self.command_context(self.target(document_id), None, None)?;
        Ok(refusal_of(&entry.def, &context, &self.current_settings()))
    }

    // ── events ──

    /// `onExecuted`: a command ran.
    pub fn on_executed(
        &self,
        listener: impl Fn(&CommandExecutedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.inner.executed.on(listener)
    }

    /// `onExecutionFailed`: a command's `run` threw.
    pub fn on_execution_failed(
        &self,
        listener: impl Fn(&CommandExecutionFailedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.inner.execution_failed.on(listener)
    }

    // ── host lens ──

    /// `matchStroke`: the command whose shortcut a keystroke matches, for
    /// the active document. Of several with the same keys, one that isn't
    /// refused wins; a refused one counts only while its `visible` says it
    /// would show. With `in_text_field`, only a command whose
    /// `shortcut_in_text_fields` says so matches.
    pub fn match_stroke(
        &self,
        stroke: &KeyStroke,
        options: MatchStrokeOptions,
    ) -> Result<Option<ShortcutMatch>, Error> {
        let platform = ShortcutOptions { is_mac: options.is_mac };
        let mut refused = None;
        for entry in self.defined_commands()?.iter() {
            let def = &entry.def;
            if options.in_text_field && def.shortcut_in_text_fields != Some(true) {
                continue;
            }
            if !entry.parsed.iter().any(|shortcut| match_shortcut(shortcut, stroke, platform)) {
                continue;
            }
            let context = self.command_context(None, None, None)?;
            let Some(refusal) = refusal_of(def, &context, &self.current_settings()) else {
                return Ok(Some(ShortcutMatch { command_id: def.id.clone(), refusal: None }));
            };
            // A refused command takes its keys only where it would show.
            if !derive(def.visible.as_ref(), &context, true) {
                continue;
            }
            if refused.is_none() {
                refused =
                    Some(ShortcutMatch { command_id: def.id.clone(), refusal: Some(refusal) });
            }
        }
        Ok(refused)
    }

    /// `getMenuTarget`: the menu a command opens, for the "More" menu's
    /// nested menus; `None` for an unknown id.
    pub fn get_menu_target(&self, id: &str) -> Result<Option<MenuTarget>, Error> {
        Ok(self.entry_of(id)?.map(|entry| MenuTarget { menu: entry.def.menu.clone() }))
    }
}

/// `createCommandsController(ctx)`: reads the settings through the
/// registration's table and returns the api; the plugin has no `connect`.
pub fn create_commands_controller(
    cx: CommandsContext,
    table: Rc<CommandTable>,
) -> Result<CommandsCapability, Error> {
    let settings = cx.settings()?;
    let executed = cx.event_source();
    let execution_failed = cx.event_source();
    Ok(CommandsCapability {
        inner: Rc::new(Inner {
            cx,
            settings,
            table,
            decoded: RefCell::new(None),
            registered: RefCell::new(CommandRegistry::new()),
            registered_version: Cell::new(0),
            configured: RefCell::new(None),
            defined: RefCell::new(None),
            family_commands: RefCell::new(Vec::new()),
            resolved: RefCell::new(HashMap::new()),
            ids: RefCell::new(HashMap::new()),
            lists: RefCell::new(HashMap::new()),
            shortcuts: RefCell::new(None),
            no_names: Rc::new(Vec::new()),
            executed,
            execution_failed,
        }),
        scope: None,
    })
}
