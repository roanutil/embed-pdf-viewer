//! `capability.ts`: the registry and the capability over it.
//!
//! Resolution is a pure read at call time: every derivation runs against
//! live state on every `resolve`, and a derivation that throws falls back
//! to its safe default. Execution goes through the same resolution, so a
//! disabled or hidden command never runs.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use epdf_kernel::{Ctx, Error};
use epdf_plugin_i18n::I18N;
use epdf_plugin_shell::{OpenSurfaceOptions, SHELL};
use epdf_ui_core::js::trim;
use epdf_ui_core::shortcuts::{
    KeyStroke, ParsedShortcut, ShortcutOptions, match_shortcut, parse_shortcut,
};

use crate::types::{
    AccentDerivation, CommandCtx, CommandDef, CommandsAction, CommandsState, Derivation,
    IconAccent, MenuTarget, Panel, ResolvedCommand, Shortcut,
};

/// A registered command: the definition plus its pre-parsed shortcuts.
pub struct RegisteredCommand {
    pub def: CommandDef,
    pub shortcuts: Vec<String>,
    pub parsed: Vec<ParsedShortcut>,
}

/// `Map<string, RegisteredCommand>`, with a `Map`'s iteration order: by
/// insertion, a deleted key re-added at the end. A walk ([`Self::at`] by
/// position) sees what a `for...of` over a live `Map` sees when the body
/// changes it: an entry deleted ahead of the walk is skipped and one added
/// during it is visited. Deletions leave a hole while a walk is running and
/// are compacted when the last walk ends.
#[derive(Default)]
pub struct CommandRegistry {
    slots: Vec<Option<(String, Rc<RegisteredCommand>)>>,
    index: HashMap<String, usize>,
    walks: usize,
}

impl CommandRegistry {
    pub fn new() -> Self {
        CommandRegistry::default()
    }

    pub fn has(&self, id: &str) -> bool {
        self.index.contains_key(id)
    }

    pub fn get(&self, id: &str) -> Option<Rc<RegisteredCommand>> {
        self.index.get(id).and_then(|&i| self.slots[i].as_ref()).map(|(_, e)| e.clone())
    }

    /// `set` of an id not yet registered: appended.
    fn append(&mut self, id: String, entry: RegisteredCommand) {
        self.index.insert(id.clone(), self.slots.len());
        self.slots.push(Some((id, Rc::new(entry))));
    }

    /// `delete`: true when the id was registered.
    pub fn delete(&mut self, id: &str) -> bool {
        let Some(i) = self.index.remove(id) else { return false };
        self.slots[i] = None;
        self.compact();
        true
    }

    fn compact(&mut self) {
        if self.walks > 0 {
            return;
        }
        self.slots.retain(Option::is_some);
        self.index.clear();
        for (i, (id, _)) in self.slots.iter().flatten().enumerate() {
            self.index.insert(id.clone(), i);
        }
    }

    /// Ids in iteration order.
    pub fn keys(&self) -> Vec<String> {
        self.slots.iter().flatten().map(|(id, _)| id.clone()).collect()
    }

    /// Entries in iteration order.
    pub fn entries(&self) -> Vec<(String, Rc<RegisteredCommand>)> {
        self.slots.iter().flatten().cloned().collect()
    }

    /// The walk's view of position `i`: `None` past the end, `Some(None)` a
    /// hole to skip.
    fn at(&self, i: usize) -> Option<Option<String>> {
        self.slots.get(i).map(|slot| slot.as_ref().map(|(id, _)| id.clone()))
    }
}

/// Visit every entry of a shared registry the way a live `Map` iteration
/// does, with no borrow held while `visit` runs (it may change the registry).
fn walk(
    registry: &RefCell<CommandRegistry>,
    mut visit: impl FnMut(&str) -> Result<(), Error>,
) -> Result<(), Error> {
    registry.borrow_mut().walks += 1;
    let mut result = Ok(());
    let mut i = 0;
    loop {
        // Bound first, so no borrow is held while `visit` runs.
        let slot = registry.borrow().at(i);
        let Some(slot) = slot else { break };
        i += 1;
        let Some(id) = slot else { continue };
        if let Err(error) = visit(&id) {
            result = Err(error);
            break;
        }
    }
    let mut registry = registry.borrow_mut();
    registry.walks -= 1;
    registry.compact();
    result
}

pub fn register_command(registry: &mut CommandRegistry, def: CommandDef) -> Result<(), Error> {
    if registry.has(&def.id) {
        return Err(Error::new(format!("[commands] duplicate command: {}", def.id)));
    }
    let shortcuts = match &def.shortcut {
        None => Vec::new(),
        Some(Shortcut::One(s)) => vec![s.clone()],
        Some(Shortcut::Many(list)) => list.clone(),
    };
    let parsed = shortcuts.iter().map(|s| parse_shortcut(s)).collect::<Result<Vec<_>, _>>()?;
    let id = def.id.clone();
    registry.append(id, RegisteredCommand { def, shortcuts, parsed });
    Ok(())
}

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

/// `ctx.getState()` read after the slice is gone is `undefined`, and every
/// reader goes on to read `.disabledCategories` of it.
fn state_gone() -> Error {
    Error::named("TypeError", "Cannot read properties of undefined (reading 'disabledCategories')")
}

#[derive(Clone)]
pub struct CommandsCapability {
    ctx: Ctx<CommandsState, CommandsAction>,
    registry: Rc<RefCell<CommandRegistry>>,
}

impl CommandsCapability {
    pub(crate) fn new(
        ctx: Ctx<CommandsState, CommandsAction>,
        registry: Rc<RefCell<CommandRegistry>>,
    ) -> Self {
        CommandsCapability { ctx, registry }
    }

    /// Bind capability resolution to the command's target document.
    fn command_ctx(&self, document_id: Option<&str>) -> CommandCtx {
        let target = match document_id {
            Some(id) => Some(id.to_string()),
            None => self.ctx.core().active_id.clone(),
        };
        CommandCtx { document_id: target, ctx: self.ctx.ctx.clone() }
    }

    fn state(&self) -> Result<Rc<CommandsState>, Error> {
        self.ctx.try_state().ok_or_else(state_gone)
    }

    // ── registry ──

    pub fn register(&self, def: CommandDef) -> Result<(), Error> {
        register_command(&mut self.registry.borrow_mut(), def)
    }

    pub fn unregister(&self, id: &str) {
        self.registry.borrow_mut().delete(id);
    }

    pub fn has(&self, id: &str) -> bool {
        self.registry.borrow().has(id)
    }

    pub fn ids(&self) -> Vec<String> {
        self.registry.borrow().keys()
    }

    // ── resolution ──

    /// `Ok(None)` for an unknown id. `Err` only when the plugin's slice is
    /// gone (the kernel was destroyed).
    pub fn resolve(
        &self,
        id: &str,
        document_id: Option<&str>,
    ) -> Result<Option<ResolvedCommand>, Error> {
        let entry = self.registry.borrow().get(id);
        let Some(entry) = entry else { return Ok(None) };
        let def = &entry.def;
        let c = self.command_ctx(document_id);

        let state = self.state()?;
        let disabled = &state.disabled_categories;
        let categories = def.categories.clone().unwrap_or_default();
        let category_hidden = categories.iter().any(|cat| disabled.contains(cat));

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
        Ok(Some(ResolvedCommand {
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
        }))
    }

    /// Palette query: visible commands whose resolved label (lowercased) or
    /// id contains the trimmed, lowercased query, in registry order.
    pub fn search(
        &self,
        query: &str,
        document_id: Option<&str>,
    ) -> Result<Vec<ResolvedCommand>, Error> {
        let q = trim(query).to_lowercase();
        let mut hits = Vec::new();
        walk(&self.registry, |id| {
            if let Some(r) = self.resolve(id, document_id)?
                && r.visible
                && (q.is_empty() || r.label.to_lowercase().contains(&q) || r.id.contains(&q))
            {
                hits.push(r);
            }
            Ok(())
        })?;
        Ok(hits)
    }

    /// `None` for an unknown id.
    pub fn menu_target(&self, id: &str) -> Option<MenuTarget> {
        self.registry.borrow().get(id).map(|entry| MenuTarget { menu: entry.def.menu.clone() })
    }

    // ── execution ──

    /// The only path to a verb, guarded by `enabled` and `visible`. `run`,
    /// when present, replaces the default routing of a surface target. A
    /// `run` that throws throws out of here.
    pub fn execute(&self, id: &str, document_id: Option<&str>) -> Result<(), Error> {
        let entry = self.registry.borrow().get(id);
        let Some(entry) = entry else { return Ok(()) };
        let Some(resolved) = self.resolve(id, document_id)? else { return Ok(()) };
        if !resolved.enabled || !resolved.visible {
            return Ok(());
        }
        let c = self.command_ctx(document_id);

        if let Some(run) = &entry.def.run {
            return run(&c);
        }
        // Default routing for declarative surface targets.
        let Some(shell) = c.try_get(&SHELL) else { return Ok(()) };
        let def = &entry.def;
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
        Ok(())
    }

    /// Match a keystroke against every registered shortcut, in registry
    /// order: the first command with a matching binding, whether or not it
    /// is enabled.
    pub fn match_stroke(&self, stroke: &KeyStroke, opts: ShortcutOptions) -> Option<String> {
        let entries = self.registry.borrow().entries();
        entries
            .into_iter()
            .find(|(_, entry)| entry.parsed.iter().any(|p| match_shortcut(p, stroke, opts)))
            .map(|(id, _)| id)
    }

    // ── category gating ──

    pub fn disabled_categories(&self) -> Result<Vec<String>, Error> {
        Ok(self.state()?.disabled_categories.clone())
    }

    pub fn is_category_disabled(&self, category: &str) -> Result<bool, Error> {
        Ok(self.state()?.disabled_categories.iter().any(|c| c == category))
    }

    pub fn disable_category(&self, category: &str) {
        self.ctx.dispatch(CommandsAction::DisableCategory { category: category.to_string() });
    }

    pub fn enable_category(&self, category: &str) {
        self.ctx.dispatch(CommandsAction::EnableCategory { category: category.to_string() });
    }

    pub fn set_disabled_categories(&self, categories: &[String]) {
        self.ctx
            .dispatch(CommandsAction::SetDisabledCategories { categories: categories.to_vec() });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry(ids: &[&str]) -> CommandRegistry {
        let mut registry = CommandRegistry::new();
        for id in ids {
            register_command(&mut registry, CommandDef::new(*id, *id)).unwrap();
        }
        registry
    }

    /// Compaction is not behaviour a vector can see, only memory: a delete
    /// outside a walk drops its slot, and one inside a walk drops it when
    /// the walk ends.
    #[test]
    fn deleted_slots_are_reclaimed() {
        let mut outside = registry(&["a", "b", "c"]);
        outside.delete("b");
        assert_eq!(outside.slots.len(), 2);
        assert!(outside.get("c").is_some());

        let shared = RefCell::new(registry(&["a", "b", "c"]));
        let mut seen = Vec::new();
        walk(&shared, |id| {
            seen.push(id.to_string());
            if id == "b" {
                shared.borrow_mut().delete("a");
                assert_eq!(shared.borrow().slots.len(), 3, "no compaction mid-walk");
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(seen, ["a", "b", "c"]);
        let shared = shared.into_inner();
        assert_eq!(shared.walks, 0);
        assert_eq!(shared.slots.len(), 2);
        assert_eq!(shared.keys(), ["b", "c"]);
        assert!(shared.get("c").is_some());
    }
}
