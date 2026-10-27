//! `registry.ts`: a command definition with its shortcuts parsed once, and
//! the commands the settings define. [`CommandRegistry`] is a JavaScript
//! `Map` by id: iteration is by insertion, setting an id that's there keeps
//! its place, and an id deleted and set again moves to the end.

use std::rc::Rc;

use epdf_kernel::Error;
use epdf_ui_core::shortcuts::{ParsedShortcut, parse_shortcut};

use crate::contract::{CommandDef, CommandFamily, CommandItem, Shortcut};

/// `RegisteredCommand`.
pub struct RegisteredCommand {
    pub def: Rc<CommandDef>,
    pub shortcuts: Vec<String>,
    pub parsed: Vec<ParsedShortcut>,
}

/// `registeredCommand(definition)`: an unparsable shortcut throws.
pub fn registered_command(def: Rc<CommandDef>) -> Result<Rc<RegisteredCommand>, Error> {
    let shortcuts = match &def.shortcut {
        None => Vec::new(),
        Some(Shortcut::One(s)) => vec![s.clone()],
        Some(Shortcut::Many(list)) => list.clone(),
    };
    let parsed = shortcuts.iter().map(|s| parse_shortcut(s)).collect::<Result<Vec<_>, _>>()?;
    Ok(Rc::new(RegisteredCommand { def, shortcuts, parsed }))
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

    /// `set`: an id that's there keeps its place.
    pub fn set(&mut self, id: String, entry: Rc<RegisteredCommand>) {
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
    pub fn values(&self) -> impl Iterator<Item = &Rc<RegisteredCommand>> {
        self.entries.iter().map(|(_, e)| e)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// `ConfiguredCommands`: the commands the settings define, by id (a later
/// id replaces an earlier one, in the earlier one's place), and their
/// families.
#[derive(Default)]
pub struct ConfiguredCommands {
    pub commands: CommandRegistry,
    pub families: Vec<Rc<CommandFamily>>,
}

/// `configuredCommands(items)`: an unparsable shortcut throws.
pub fn configured_commands(items: &[CommandItem]) -> Result<ConfiguredCommands, Error> {
    let mut configured = ConfiguredCommands::default();
    for item in items {
        match item {
            CommandItem::Family(family) => configured.families.push(family.clone()),
            CommandItem::Command(def) => {
                configured.commands.set(def.id.clone(), registered_command(def.clone())?)
            }
        }
    }
    Ok(configured)
}
