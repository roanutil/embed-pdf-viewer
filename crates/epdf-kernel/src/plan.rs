//! `order.ts`: validate `requires` and sort plugins so dependencies come
//! first.

use std::collections::HashMap;
use std::rc::Rc;

use crate::error::Error;
use crate::plugin::PluginDef;
use crate::token::AnyToken;
use crate::types::PluginScope;

pub struct PluginPlan {
    pub ordered: Vec<Rc<PluginDef>>,
    provider: HashMap<AnyToken, Rc<PluginDef>>,
    scope: HashMap<AnyToken, PluginScope>,
}

impl PluginPlan {
    pub fn provider_of(&self, token: &AnyToken) -> Option<&Rc<PluginDef>> {
        self.provider.get(token)
    }

    pub fn scope_of(&self, token: &AnyToken) -> PluginScope {
        self.scope.get(token).copied().unwrap_or(PluginScope::Workspace)
    }
}

pub fn plan_plugins(plugins: &[Rc<PluginDef>], documents: &AnyToken) -> Result<PluginPlan, Error> {
    let mut provider = HashMap::new();
    let mut scope = HashMap::new();
    scope.insert(documents.clone(), PluginScope::Workspace);
    for plugin in plugins {
        if let (Some(token), true) = (&plugin.token, plugin.capability.is_some()) {
            provider.insert(token.clone(), plugin.clone());
            scope.insert(token.clone(), plugin.scope);
        }
    }

    for plugin in plugins {
        for required in &plugin.requires {
            if required != documents && !provider.contains_key(required) {
                let fix = match required.hint() {
                    Some(hint) => format!(" — {hint}."),
                    None => ".".to_string(),
                };
                return Err(Error::new(format!(
                    "Plugin \"{}\" requires capability \"{}\", which no plugin provides{}",
                    plugin.id,
                    required.name(),
                    fix
                )));
            }
        }
    }

    #[derive(PartialEq)]
    enum Mark {
        Visiting,
        Done,
    }
    fn visit(
        plugin: &Rc<PluginDef>,
        provider: &HashMap<AnyToken, Rc<PluginDef>>,
        marks: &mut HashMap<String, Mark>,
        ordered: &mut Vec<Rc<PluginDef>>,
    ) -> Result<(), Error> {
        match marks.get(&plugin.id) {
            Some(Mark::Done) => return Ok(()),
            Some(Mark::Visiting) => {
                return Err(Error::new(format!(
                    "Dependency cycle involving plugin \"{}\".",
                    plugin.id
                )));
            }
            None => {}
        }
        marks.insert(plugin.id.clone(), Mark::Visiting);
        for token in plugin.requires.iter().chain(plugin.optional.iter()) {
            if let Some(dependency) = provider.get(token)
                && !Rc::ptr_eq(dependency, plugin)
            {
                visit(dependency, provider, marks, ordered)?;
            }
        }
        marks.insert(plugin.id.clone(), Mark::Done);
        ordered.push(plugin.clone());
        Ok(())
    }

    let mut marks = HashMap::new();
    let mut ordered = Vec::new();
    for plugin in plugins {
        visit(plugin, &provider, &mut marks, &mut ordered)?;
    }
    Ok(PluginPlan { ordered, provider, scope })
}
