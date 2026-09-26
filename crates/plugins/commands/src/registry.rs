//! `registry.ts`: definitions plus their pre-parsed shortcuts, in a `Map`'s
//! iteration order (by insertion; replacing a definition keeps its place,
//! and an id deleted and registered again moves to the end).

use std::rc::Rc;

use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error};
use epdf_ui_core::shortcuts::{ParsedShortcut, parse_shortcut};

use crate::contract::{CommandDef, RegisterCommandOptions, Shortcut};

/// A registered command. The remover `registerCommand` returns deletes an
/// id only while this very entry is still registered under it.
pub struct RegisteredCommand {
    pub def: CommandDef,
    pub shortcuts: Vec<String>,
    pub parsed: Vec<ParsedShortcut>,
}

/// `Map<string, RegisteredCommand>`.
#[derive(Default)]
pub struct CommandRegistry {
    entries: Vec<(String, Rc<RegisteredCommand>)>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        CommandRegistry::default()
    }

    pub fn has(&self, id: &str) -> bool {
        self.entries.iter().any(|(k, _)| k == id)
    }

    pub fn get(&self, id: &str) -> Option<Rc<RegisteredCommand>> {
        self.entries.iter().find(|(k, _)| k == id).map(|(_, e)| e.clone())
    }

    /// `set`: an existing id keeps its place.
    fn set(&mut self, id: String, entry: Rc<RegisteredCommand>) {
        match self.entries.iter_mut().find(|(k, _)| *k == id) {
            Some(slot) => slot.1 = entry,
            None => self.entries.push((id, entry)),
        }
    }

    /// `delete`.
    pub fn delete(&mut self, id: &str) {
        self.entries.retain(|(k, _)| k != id);
    }

    /// Ids in iteration order.
    pub fn keys(&self) -> Vec<String> {
        self.entries.iter().map(|(id, _)| id.clone()).collect()
    }

    /// Entries in iteration order.
    pub fn entries(&self) -> Vec<(String, Rc<RegisteredCommand>)> {
        self.entries.clone()
    }
}

/// `registerCommand(registry, def, options)`: a duplicate id throws
/// `conflict` unless `replace`, and an unparsable shortcut throws before
/// anything is registered.
pub fn register_command(
    registry: &mut CommandRegistry,
    def: CommandDef,
    options: RegisterCommandOptions,
) -> Result<Rc<RegisteredCommand>, Error> {
    if registry.has(&def.id) && !options.replace {
        return Err(plugin_error(
            code::CONFLICT,
            "commands",
            format!("duplicate command '{}'", def.id),
            None,
            None,
        ));
    }
    let shortcuts = match &def.shortcut {
        None => Vec::new(),
        Some(Shortcut::One(s)) => vec![s.clone()],
        Some(Shortcut::Many(list)) => list.clone(),
    };
    let parsed = shortcuts.iter().map(|s| parse_shortcut(s)).collect::<Result<Vec<_>, _>>()?;
    let id = def.id.clone();
    let entry = Rc::new(RegisteredCommand { def, shortcuts, parsed });
    registry.set(id, entry.clone());
    Ok(entry)
}
