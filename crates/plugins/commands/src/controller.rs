//! `controller.ts`: the registry, resolution against a target document
//! (pure derivations over the store, throw-safe), the one execution path,
//! category gating, and the host-only keystroke matcher.

use std::cell::RefCell;
use std::rc::Rc;

use epdf_kernel::error::to_plugin_error;
use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::rt::{self, Promise};
use epdf_kernel::store::Unsubscribe;
use epdf_kernel::{Controller, Error};
use epdf_plugin_i18n::I18N;
use epdf_plugin_shell::{OpenSurfaceOptions, SHELL};
use epdf_ui_core::js::trim;
use epdf_ui_core::shortcuts::{KeyStroke, ShortcutOptions, match_shortcut};
use serde_json::Value;

use crate::contract::{
    AccentDerivation, CommandCtx, CommandDef, CommandExecutedEvent, CommandExecutionFailedEvent,
    CommandsConfig, Derivation, ExecuteResult, IconAccent, MenuTarget, Panel,
    RegisterCommandOptions, RejectReason, ResolvedCommand, ShortcutBinding,
};
use crate::model::{CommandsAction, CommandsState};
use crate::registry::{CommandRegistry, RegisteredCommand, register_command};

/// `panelTarget`: a string panel is `{id}`, an object panel is itself.
fn panel_target(def: &CommandDef) -> Option<(&str, Option<&str>)> {
    match def.panel.as_ref()? {
        Panel::Id(id) => Some((id, None)),
        Panel::Target { id, exclusive } => Some((id, exclusive.as_deref())),
    }
}

/// A `menu` or `modal` id as a condition: absent and empty are both falsy.
fn truthy(value: &Option<String>) -> Option<&str> {
    value.as_deref().filter(|v| !v.is_empty())
}

/// Derivations run against live state; one that throws falls back to the
/// safe default.
fn derive(f: Option<&Derivation>, c: &CommandCtx, fallback: bool) -> bool {
    match f {
        None => fallback,
        Some(f) => f(c).unwrap_or(fallback),
    }
}

/// The same guard for the icon accent: a throw means no accent.
fn derive_accent(f: Option<&AccentDerivation>, c: &CommandCtx) -> Option<IconAccent> {
    f.and_then(|f| f(c).ok().flatten())
}

struct Inner {
    cx: Controller<CommandsState, CommandsAction>,
    registry: RefCell<CommandRegistry>,
    executed: EventHookSource<CommandExecutedEvent>,
    execution_failed: EventHookSource<CommandExecutionFailedEvent>,
}

/// `CommandsHostCapability`: the public `CommandsCapability` plus the host
/// lens (`match_stroke`, `get_menu_target`).
#[derive(Clone)]
pub struct CommandsCapability(Rc<Inner>);

impl CommandsCapability {
    fn state(&self) -> Rc<CommandsState> {
        self.0.cx.state()
    }

    /// Bind capability resolution to the command's target document:
    /// explicit, else the active one.
    fn command_context(&self, document_id: Option<&str>, args: Option<Value>) -> CommandCtx {
        let target = match document_id {
            Some(id) => Some(id.to_string()),
            None => self.0.cx.core().active_id.clone(),
        };
        CommandCtx { document_id: target, args, ctx: self.0.cx.plugin_context().clone() }
    }

    fn resolve_fresh(
        &self,
        entry: &RegisteredCommand,
        document_id: Option<&str>,
    ) -> ResolvedCommand {
        let def = &entry.def;
        let c = self.command_context(document_id, None);
        let state = self.state();
        let categories = def.categories.clone().unwrap_or_default();
        let category_hidden = categories.iter().any(|cat| state.disabled_categories.contains(cat));
        let label = match c.try_get(&I18N) {
            Some(i18n) => i18n.t(&def.label_key, None),
            None => def.label_key.clone(),
        };
        // Surface-target commands derive `active` from the surface's open
        // state unless the definition overrides it.
        let active = match &def.active {
            Some(active) => derive(Some(active), &c, false),
            None => match c.try_get(&SHELL) {
                None => false,
                Some(shell) => {
                    if let Some(menu) = truthy(&def.menu) {
                        shell.is_menu_open(menu)
                    } else if let Some((panel, _)) = panel_target(def) {
                        shell.is_open(panel)
                    } else if let Some(modal) = truthy(&def.modal) {
                        shell.is_open(modal)
                    } else {
                        false
                    }
                }
            },
        };
        let icon_accent = derive_accent(def.icon_accent.as_ref(), &c);
        let enabled = derive(def.enabled.as_ref(), &c, true) && !category_hidden;
        let visible = derive(def.visible.as_ref(), &c, true) && !category_hidden;
        ResolvedCommand {
            id: def.id.clone(),
            label,
            icon: def.icon.clone(),
            icon_accent,
            shortcuts: entry.shortcuts.clone(),
            menu: def.menu.clone(),
            enabled,
            active,
            visible,
            categories,
        }
    }

    // ── registry ──

    /// Add a command; the remover unregisters it, and only while this
    /// definition is the one registered under its id. A duplicate id errs
    /// `conflict` unless `replace`.
    pub fn register_command(
        &self,
        def: CommandDef,
        options: RegisterCommandOptions,
    ) -> Result<Rc<dyn Fn()>, Error> {
        let id = def.id.clone();
        let entry = register_command(&mut self.0.registry.borrow_mut(), def, options)?;
        let inner = Rc::downgrade(&self.0);
        Ok(Rc::new(move || {
            let Some(inner) = inner.upgrade() else { return };
            let mut registry = inner.registry.borrow_mut();
            if registry.get(&id).is_some_and(|current| Rc::ptr_eq(&current, &entry)) {
                registry.delete(&id);
            }
        }))
    }

    /// Register each definition in order; the one remover removes them all.
    /// A failure stops there, with the earlier ones registered.
    pub fn register_commands(
        &self,
        defs: Vec<CommandDef>,
        options: RegisterCommandOptions,
    ) -> Result<Rc<dyn Fn()>, Error> {
        let mut removers = Vec::new();
        for def in defs {
            removers.push(self.register_command(def, options)?);
        }
        Ok(Rc::new(move || removers.iter().for_each(|remove| remove())))
    }

    pub fn has_command(&self, id: &str) -> bool {
        self.0.registry.borrow().has(id)
    }

    pub fn get_command(&self, id: &str) -> Option<CommandDef> {
        self.0.registry.borrow().get(id).map(|entry| entry.def.clone())
    }

    pub fn list_command_ids(&self) -> Vec<String> {
        self.0.registry.borrow().keys()
    }

    // ── resolution ──

    /// Label, icon, enabled, active and visible for a target document;
    /// `None` for an unknown id. Every derivation runs on every call.
    pub fn resolve_command(&self, id: &str, document_id: Option<&str>) -> Option<ResolvedCommand> {
        let entry = self.0.registry.borrow().get(id)?;
        Some(self.resolve_fresh(&entry, document_id))
    }

    /// Every command resolved, over the ids registered when the call began:
    /// one a derivation removes meanwhile is skipped, one it adds is not
    /// visited.
    pub fn list_commands(&self, document_id: Option<&str>) -> Vec<ResolvedCommand> {
        let ids = self.list_command_ids();
        ids.iter().filter_map(|id| self.resolve_command(id, document_id)).collect()
    }

    /// Palette query: visible commands whose resolved label (lowercased) or
    /// id contains the trimmed, lowercased query.
    pub fn search_commands(&self, query: &str, document_id: Option<&str>) -> Vec<ResolvedCommand> {
        let q = trim(query).to_lowercase();
        self.list_commands(document_id)
            .into_iter()
            .filter(|r| {
                r.visible
                    && (q.is_empty() || r.label.to_lowercase().contains(&q) || r.id.contains(&q))
            })
            .collect()
    }

    /// Every binding, for a keyboard help sheet, in registry order.
    pub fn list_shortcuts(&self) -> Vec<ShortcutBinding> {
        let entries = self.0.registry.borrow().entries();
        entries
            .iter()
            .flat_map(|(_, entry)| {
                entry.shortcuts.iter().map(|shortcut| ShortcutBinding {
                    command_id: entry.def.id.clone(),
                    shortcut: shortcut.clone(),
                })
            })
            .collect()
    }

    // ── execution ──

    /// Run a command, the only path to a verb. A refused command resolves
    /// `Rejected` with its reason. Otherwise `run` is called and awaited
    /// (or, without one, a surface target is routed through the target
    /// document's shell); a failure is mapped through `toPluginError`,
    /// announced on `onExecutionFailed` and rejected with.
    pub fn execute(
        &self,
        id: &str,
        document_id: Option<&str>,
        args: Option<Value>,
    ) -> Promise<ExecuteResult> {
        let this = self.clone();
        let id = id.to_string();
        let document_id = document_id.map(str::to_string);
        Promise::spawn(async move {
            let entry = this.0.registry.borrow().get(&id);
            let Some(entry) = entry else {
                return Ok(ExecuteResult::Rejected { reason: RejectReason::NotFound });
            };
            let resolved = this.resolve_command(&id, document_id.as_deref());
            let Some(resolved) = resolved.filter(|r| r.visible) else {
                return Ok(ExecuteResult::Rejected { reason: RejectReason::Hidden });
            };
            if !resolved.enabled {
                return Ok(ExecuteResult::Rejected { reason: RejectReason::Disabled });
            }
            let c = this.command_context(document_id.as_deref(), args.clone());
            let outcome = match &entry.def.run {
                Some(run) => match run(&c) {
                    Err(error) => Err(error),
                    Ok(None) => {
                        rt::tick().await; // `await undefined`
                        Ok(())
                    }
                    Ok(Some(pending)) => pending.await.map(|_| ()),
                },
                None => {
                    this.route(&entry.def, &c);
                    Ok(())
                }
            };
            if let Err(error) = outcome {
                let failure = to_plugin_error("commands", &error);
                this.0.execution_failed.emit(&CommandExecutionFailedEvent {
                    command_id: id.clone(),
                    document_id: c.document_id.clone(),
                    error: failure.plugin_error_info(),
                });
                return Err(failure);
            }
            this.0.executed.emit(&CommandExecutedEvent {
                command_id: id,
                document_id: c.document_id.clone(),
                args,
            });
            Ok(ExecuteResult::Executed)
        })
    }

    /// Default routing for declarative surface targets: a menu wins over a
    /// panel, a panel over a modal; with no shell nothing happens.
    fn route(&self, def: &CommandDef, c: &CommandCtx) {
        let Some(shell) = c.try_get(&SHELL) else { return };
        if let Some(menu) = truthy(&def.menu) {
            shell.toggle_menu(menu);
        } else if let Some((panel, exclusive)) = panel_target(def) {
            let options =
                OpenSurfaceOptions { exclusive: exclusive.map(str::to_string), props: None };
            shell.toggle(panel, Some(options));
        } else if let Some(modal) = truthy(&def.modal) {
            let options = OpenSurfaceOptions { exclusive: Some("modal".into()), props: None };
            shell.toggle(modal, Some(options));
        }
    }

    /// Enabled and visible for the target; false for an unknown id.
    pub fn can_execute(&self, id: &str, document_id: Option<&str>) -> bool {
        self.resolve_command(id, document_id).is_some_and(|r| r.enabled && r.visible)
    }

    // ── category gating ──

    pub fn get_disabled_categories(&self) -> Vec<String> {
        self.state().disabled_categories.clone()
    }

    pub fn is_category_disabled(&self, category: &str) -> bool {
        self.state().disabled_categories.iter().any(|c| c == category)
    }

    pub fn disable_category(&self, category: &str) {
        self.0.cx.dispatch(CommandsAction::DisableCategory { category: category.to_string() });
    }

    pub fn enable_category(&self, category: &str) {
        self.0.cx.dispatch(CommandsAction::EnableCategory { category: category.to_string() });
    }

    pub fn set_disabled_categories(&self, categories: &[String]) {
        self.0
            .cx
            .dispatch(CommandsAction::SetDisabledCategories { categories: categories.to_vec() });
    }

    // ── events ──

    pub fn on_executed(
        &self,
        listener: impl Fn(&CommandExecutedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.executed.on(listener)
    }

    pub fn on_execution_failed(
        &self,
        listener: impl Fn(&CommandExecutionFailedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.execution_failed.on(listener)
    }

    // ── host lens ──

    /// Match a keystroke against every registered shortcut, in registry
    /// order: the first command with a matching binding, whether or not it
    /// is enabled.
    pub fn match_stroke(&self, stroke: &KeyStroke, options: ShortcutOptions) -> Option<String> {
        let entries = self.0.registry.borrow().entries();
        entries
            .into_iter()
            .find(|(_, entry)| entry.parsed.iter().any(|p| match_shortcut(p, stroke, options)))
            .map(|(id, _)| id)
    }

    /// `None` for an unknown id.
    pub fn get_menu_target(&self, id: &str) -> Option<MenuTarget> {
        self.0.registry.borrow().get(id).map(|entry| MenuTarget { menu: entry.def.menu.clone() })
    }
}

/// `createCommandsController(ctx, config)`: registers the configured
/// commands (a duplicate or an unparsable shortcut fails the create, and so
/// the kernel), and returns the api; the plugin has no `connect`.
pub fn create_commands_controller(
    cx: Controller<CommandsState, CommandsAction>,
    config: &CommandsConfig,
) -> Result<CommandsCapability, Error> {
    let mut registry = CommandRegistry::new();
    for def in &config.commands {
        register_command(&mut registry, def.clone(), RegisterCommandOptions::default())?;
    }
    // The TypeScript reports a failing listener to `console.error`.
    let report: Rc<dyn Fn(&Error)> =
        Rc::new(|error| eprintln!("[commands] listener failed: {error}"));
    let executed = EventHookSource::new(Some(report.clone()));
    let execution_failed = EventHookSource::new(Some(report));
    {
        let (executed, failed) = (executed.clone(), execution_failed.clone());
        cx.cleanup_fn(move || {
            executed.dispose();
            failed.dispose();
        });
    }
    Ok(CommandsCapability(Rc::new(Inner {
        cx,
        registry: RefCell::new(registry),
        executed,
        execution_failed,
    })))
}
