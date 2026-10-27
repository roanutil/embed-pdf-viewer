//! `write/turning-off.ts`: turning commands off and on, by category (with
//! every category under it) and one by one. Both lists are settings, so
//! they change live and `reset_settings` restores what the app registered.
//! Adding a name that's there, or removing one that isn't, changes nothing
//! and fires nothing; replacing a list with an equal one is no change
//! either, and keeps the list's `Rc`.

use std::rc::Rc;

use epdf_kernel::Error;

use crate::availability::{is_category_off, is_turned_off};
use crate::contract::CommandsConfig;
use crate::controller::CommandsCapability;

#[derive(Clone, Copy)]
enum OffList {
    Categories,
    Commands,
}

impl CommandsCapability {
    fn list_of(&self, list: OffList) -> Rc<Vec<String>> {
        let settings = self.current_settings();
        match list {
            OffList::Categories => settings.disabled_categories.clone(),
            OffList::Commands => settings.disabled_commands.clone(),
        }
    }

    fn replace(&self, list: OffList, names: Vec<String>) {
        self.update_settings(&match list {
            OffList::Categories => {
                CommandsConfig { disabled_categories: Some(names), ..CommandsConfig::default() }
            }
            OffList::Commands => {
                CommandsConfig { disabled_commands: Some(names), ..CommandsConfig::default() }
            }
        });
    }

    fn add(&self, list: OffList, name: &str) {
        let current = self.list_of(list);
        if !current.iter().any(|n| n == name) {
            let mut names = current.to_vec();
            names.push(name.to_string());
            self.replace(list, names);
        }
    }

    fn remove(&self, list: OffList, name: &str) {
        let current = self.list_of(list);
        if current.iter().any(|n| n == name) {
            self.replace(list, current.iter().filter(|other| *other != name).cloned().collect());
        }
    }

    /// `getDisabledCategories`: the categories turned off.
    pub fn get_disabled_categories(&self) -> Rc<Vec<String>> {
        self.list_of(OffList::Categories)
    }

    /// `isCategoryDisabled`: it, or a category it's under (`annotations` for
    /// `annotations.highlight`).
    pub fn is_category_disabled(&self, category: &str) -> bool {
        is_category_off(category, &self.list_of(OffList::Categories))
    }

    /// `disableCategory`: the `disabledCategories` setting gains it.
    pub fn disable_category(&self, category: &str) {
        self.add(OffList::Categories, category);
    }

    /// `enableCategory`: every copy of it leaves the setting.
    pub fn enable_category(&self, category: &str) {
        self.remove(OffList::Categories, category);
    }

    /// `setDisabledCategories`: replace the categories that are off.
    pub fn set_disabled_categories(&self, categories: &[String]) {
        self.replace(OffList::Categories, categories.to_vec());
    }

    /// `getDisabledCommands`: the commands turned off one by one.
    pub fn get_disabled_commands(&self) -> Rc<Vec<String>> {
        self.list_of(OffList::Commands)
    }

    /// `isCommandDisabled`: by its id, or through one of its categories. An
    /// id no command has is off when the list names it.
    pub fn is_command_disabled(&self, id: &str) -> Result<bool, Error> {
        Ok(match self.get_command(id)? {
            Some(def) => is_turned_off(&def, &self.current_settings()),
            None => self.list_of(OffList::Commands).iter().any(|n| n == id),
        })
    }

    /// `disableCommand`: the `disabledCommands` setting gains it.
    pub fn disable_command(&self, id: &str) {
        self.add(OffList::Commands, id);
    }

    /// `enableCommand`: it stays off while one of its categories is.
    pub fn enable_command(&self, id: &str) {
        self.remove(OffList::Commands, id);
    }

    /// `setDisabledCommands`: replace the commands that are off one by one.
    pub fn set_disabled_commands(&self, ids: &[String]) {
        self.replace(OffList::Commands, ids.to_vec());
    }
}
