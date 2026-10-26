//! The commands settings in the kernel's settings store.
//!
//! The kernel keeps a registration's settings as one JSON value
//! (`epdf_kernel::settings`), merged as `settings.ts` merges them: plain
//! objects key by key, arrays and other values whole, and a change that
//! leaves a member equal is no change. `disabledCategories`,
//! `disabledCommands` and `notAllowed` are plain data. A definition or a
//! family holds functions, so each is stored as a handle,
//! `{"$command": n}` or `{"$commandFamily": n}`, into a table the
//! registration owns (one per `commands_plugin(config)`, shared by its
//! instances as the store is).
//!
//! The TypeScript compares the `commands` list with `sameValue`: two
//! definitions are equal when their data members are and their functions
//! are the same functions. The table hands out one handle per such value
//! (the first definition it was given), so equal JSON is exactly
//! `sameValue`, and an equal list reads back with the definitions it
//! already had, as the TypeScript merge keeps the array it had.

use std::cell::RefCell;
use std::rc::Rc;

use epdf_kernel::Error;
use serde_json::{Map, Value, json};

use crate::contract::{
    CommandDef, CommandFamily, CommandItem, CommandsConfig, CommandsSettings, NotAllowedMode,
};

const COMMAND_HANDLE: &str = "$command";
const FAMILY_HANDLE: &str = "$commandFamily";

/// The registration's definitions and families, by handle.
#[derive(Default)]
pub struct CommandTable {
    commands: RefCell<Vec<Rc<CommandDef>>>,
    families: RefCell<Vec<Rc<CommandFamily>>>,
}

fn not_allowed_json(mode: NotAllowedMode) -> Value {
    json!(match mode {
        NotAllowedMode::Hide => "hide",
        NotAllowedMode::Disable => "disable",
    })
}

impl CommandTable {
    fn item_json(&self, item: &CommandItem) -> Value {
        match item {
            CommandItem::Command(def) => {
                let mut commands = self.commands.borrow_mut();
                let n = commands.iter().position(|d| d.same_value(def)).unwrap_or_else(|| {
                    commands.push(def.clone());
                    commands.len() - 1
                });
                json!({ COMMAND_HANDLE: n })
            }
            CommandItem::Family(family) => {
                let mut families = self.families.borrow_mut();
                let n = families.iter().position(|f| f.same_value(family)).unwrap_or_else(|| {
                    families.push(family.clone());
                    families.len() - 1
                });
                json!({ FAMILY_HANDLE: n })
            }
        }
    }

    fn item_of(&self, value: &Value) -> Option<CommandItem> {
        let handle = |key: &str| value.get(key).and_then(Value::as_u64).map(|n| n as usize);
        if let Some(n) = handle(COMMAND_HANDLE) {
            return self.commands.borrow().get(n).cloned().map(CommandItem::Command);
        }
        let n = handle(FAMILY_HANDLE)?;
        self.families.borrow().get(n).cloned().map(CommandItem::Family)
    }

    /// The JSON a `DeepPartial<CommandsSettings>` change is in the store:
    /// only the members it names.
    pub fn encode(&self, config: &CommandsConfig) -> Value {
        let mut out = Map::new();
        if let Some(commands) = &config.commands {
            let items = commands.iter().map(|item| self.item_json(item)).collect();
            out.insert("commands".into(), Value::Array(items));
        }
        if let Some(categories) = &config.disabled_categories {
            out.insert("disabledCategories".into(), json!(categories));
        }
        if let Some(ids) = &config.disabled_commands {
            out.insert("disabledCommands".into(), json!(ids));
        }
        if let Some(mode) = config.not_allowed {
            out.insert("notAllowed".into(), not_allowed_json(mode));
        }
        Value::Object(out)
    }

    /// The commands a stored `commands` list names.
    pub fn items_of(&self, value: &Value) -> Option<Vec<CommandItem>> {
        value.as_array()?.iter().map(|item| self.item_of(item)).collect()
    }

    /// The settings a stored JSON value holds. `previous` is the last value
    /// decoded with what it decoded to: a member whose JSON did not change
    /// keeps its `Rc`, as the merge keeps the object it did not change.
    ///
    /// Every write goes through [`encode`](Self::encode), so a member that
    /// doesn't decode means the store was written some other way. The
    /// TypeScript would carry on with whatever the member holds; this typed
    /// boundary can't, so it hands the failure to `report` and reads the
    /// member as empty (the lists) or `hide`.
    pub fn decode(
        &self,
        value: &Value,
        previous: Option<(&Value, &CommandsSettings)>,
        report: &dyn Fn(Error),
    ) -> CommandsSettings {
        let refuse = |what: &str, found: &Value| {
            report(Error::new(format!("[commands] settings.{what} does not decode: {found}")))
        };
        let unchanged = |member: &str| {
            previous.filter(|(json, _)| json.get(member) == value.get(member)).map(|(_, s)| s)
        };
        let member = |name: &str| value.get(name).cloned().unwrap_or(Value::Null);
        let commands = match unchanged("commands") {
            Some(settings) => settings.commands.clone(),
            None => {
                let raw = member("commands");
                Rc::new(self.items_of(&raw).unwrap_or_else(|| {
                    refuse("commands", &raw);
                    Vec::new()
                }))
            }
        };
        let strings = |name: &str, previous: Option<&Rc<Vec<String>>>| match previous {
            Some(list) => list.clone(),
            None => {
                let raw = member(name);
                Rc::new(serde_json::from_value::<Vec<String>>(raw.clone()).unwrap_or_else(|_| {
                    refuse(name, &raw);
                    Vec::new()
                }))
            }
        };
        let disabled_categories = strings(
            "disabledCategories",
            unchanged("disabledCategories").map(|s| &s.disabled_categories),
        );
        let disabled_commands = strings(
            "disabledCommands",
            unchanged("disabledCommands").map(|s| &s.disabled_commands),
        );
        let not_allowed = match value.get("notAllowed").and_then(Value::as_str) {
            Some("hide") => NotAllowedMode::Hide,
            Some("disable") => NotAllowedMode::Disable,
            _ => {
                refuse("notAllowed", &member("notAllowed"));
                NotAllowedMode::Hide
            }
        };
        CommandsSettings { commands, disabled_categories, disabled_commands, not_allowed }
    }
}

/// `COMMANDS_DEFAULTS` in the store.
pub fn defaults_json() -> Value {
    json!({ "commands": [], "disabledCategories": [], "disabledCommands": [], "notAllowed": "hide" })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A store value no `encode` wrote: each member that doesn't decode is
    /// reported, never dropped quietly.
    #[test]
    fn a_member_that_does_not_decode_is_reported() {
        let table = CommandTable::default();
        let reports: RefCell<Vec<String>> = RefCell::default();
        let settings = table.decode(
            &json!({ "commands": [{ "$command": 3 }], "disabledCategories": "a", "disabledCommands": [1], "notAllowed": "show" }),
            None,
            &|error| reports.borrow_mut().push(error.message),
        );
        assert!(settings.commands.is_empty() && settings.disabled_categories.is_empty());
        assert!(settings.disabled_commands.is_empty());
        assert_eq!(settings.not_allowed, NotAllowedMode::Hide);
        assert_eq!(
            *reports.borrow(),
            [
                "[commands] settings.commands does not decode: [{\"$command\":3}]",
                "[commands] settings.disabledCategories does not decode: \"a\"",
                "[commands] settings.disabledCommands does not decode: [1]",
                "[commands] settings.notAllowed does not decode: \"show\"",
            ]
        );
        // What `encode` writes decodes without a report.
        reports.borrow_mut().clear();
        let mut stored = defaults_json();
        stored["commands"] = table.encode(&CommandsConfig {
            commands: Some(vec![CommandDef::new("a", "a").into()]),
            ..CommandsConfig::default()
        })["commands"]
            .clone();
        let settings =
            table.decode(&stored, None, &|error| reports.borrow_mut().push(error.message));
        assert!(reports.borrow().is_empty());
        assert_eq!(settings.commands.len(), 1);
    }

    /// Equal definitions share one handle: the first one given.
    #[test]
    fn equal_definitions_share_a_handle() {
        let table = CommandTable::default();
        let run: crate::contract::RunFn = Rc::new(|_| Ok(None));
        let first = Rc::new(CommandDef { run: Some(run.clone()), ..CommandDef::new("a", "a") });
        let equal = Rc::new(CommandDef { run: Some(run), ..CommandDef::new("a", "a") });
        let other_fn =
            Rc::new(CommandDef { run: Some(Rc::new(|_| Ok(None))), ..CommandDef::new("a", "a") });
        let config = |defs: Vec<Rc<CommandDef>>| CommandsConfig {
            commands: Some(defs.into_iter().map(CommandItem::Command).collect()),
            ..CommandsConfig::default()
        };
        let one = table.encode(&config(vec![first.clone()]));
        let two = table.encode(&config(vec![equal]));
        let three = table.encode(&config(vec![other_fn]));
        assert_eq!(one, two);
        assert_ne!(one, three);
        let items = table.items_of(&two["commands"]).unwrap();
        assert!(matches!(&items[0], CommandItem::Command(def) if Rc::ptr_eq(def, &first)));
    }
}
