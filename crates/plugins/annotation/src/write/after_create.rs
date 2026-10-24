//! `write/after-create.ts`: what happens after a tool creates an annotation
//! (the `afterCreate` setting, and a tool's own over it): whether the new
//! annotation is selected, whether a new text box opens for typing, and
//! whether the tool stays active. Code that creates never selects unless it
//! asks, so this applies only to what a tool makes.

use std::rc::Rc;

use epdf_annotation_core::types::{Effect, Id, UpdateResult};
use epdf_plugin_interaction::INTERACTION;
use serde_json::json;

use crate::contract::AfterCreate;
use crate::controller::Core;
use crate::services::store::Adjust;

fn created_ids(result: &UpdateResult) -> Vec<Id> {
    result
        .effects
        .iter()
        .filter_map(|e| if let Effect::Create(c) = e { Some(c.id.clone()) } else { None })
        .collect()
}

impl Core {
    /// The policy for a tool: its own `afterCreate` over the setting.
    fn after_create_policy(&self, tool_id: Option<&str>) -> AfterCreate {
        let base = self.parsed().after_create;
        let own = tool_id
            .and_then(|id| self.tool(id))
            .and_then(|t| t.after_create.clone())
            .unwrap_or_default();
        base.over(&own)
    }

    /// `afterCreate.shape(toolId)`: a tool's create, as the policy shows it.
    pub(crate) fn after_create_shape(&self, tool_id: Option<String>) -> Adjust {
        let me = self.weak();
        Rc::new(move |result: UpdateResult| {
            let created = created_ids(&result);
            if created.is_empty() {
                return result;
            }
            let Some(core) = me.upgrade() else { return result };
            let policy = core.after_create_policy(tool_id.as_deref());
            let mut session = result.session.clone();
            let mut changed = false;
            if policy.edit_text != Some(true)
                && session.editing.as_ref().is_some_and(|e| created.contains(e))
            {
                session.editing = None;
                changed = true;
            }
            let typing = session.editing.as_ref().is_some_and(|e| created.contains(e));
            if policy.select != Some(true) && !typing {
                session.selected = Rc::new(Vec::new());
                changed = true;
            }
            if changed { UpdateResult { session, ..result } } else { result }
        })
    }

    /// `afterCreate.done(toolId, result)`: back to the default tool when the policy says so.
    pub(crate) fn after_create_done(&self, tool_id: Option<&str>, effects: &[Effect]) {
        if !effects.iter().any(|e| matches!(e, Effect::Create(_))) {
            return;
        }
        if self.after_create_policy(tool_id).tool.as_deref() != Some("default") {
            return;
        }
        if let Ok(Some(interaction)) = self.cx.try_get(&INTERACTION) {
            let _ = interaction.activate_default_tool();
        }
    }

    /// `afterCreate.placed(toolId, ids)`: a click-placed create.
    pub(crate) fn after_create_placed(&self, tool_id: Option<&str>, ids: &[Id]) {
        if self.after_create_policy(tool_id).select == Some(true) {
            let _ = self.commit_json(json!({ "type": "select", "ids": ids }));
        }
        if self.after_create_policy(tool_id).tool.as_deref() == Some("default")
            && let Ok(Some(interaction)) = self.cx.try_get(&INTERACTION)
        {
            let _ = interaction.activate_default_tool();
        }
    }
}
