//! `tools/markup-bridge.ts`: text-selection authoring. The bridge that turns
//! a text selection into markup or a text-edit annotation. These tools have
//! no gesture of their own: they enable the selection plugin's `text-select`
//! gesture, and this consumes its events and the tool changes. Selection is
//! the producer, annotation the consumer.

use std::collections::BTreeMap;

use epdf_plugin_interaction::InteractionCapability;
use epdf_plugin_selection::SelectionCapability;
use serde_json::{Map, Value, json};

use crate::controller::Core;

impl Core {
    /// Keep the live preview and the selection's own visual in sync with the
    /// active tool and the selection.
    fn sync_markup(&self, selection: &SelectionCapability, interaction: &InteractionCapability) {
        let tool = self.get_resolved_tool(&interaction.get_active_tool_id());
        let authoring = tool.as_ref().and_then(|t| t.selection.clone());
        let kind = authoring.as_ref().and_then(|a| a.get("kind")).and_then(Value::as_str);
        let operation = authoring.as_ref().and_then(|a| a.get("operation")).and_then(Value::as_str);
        let preview = match (kind, operation) {
            (Some("markup"), _) => tool.as_ref().map(|t| t.subtype.clone()),
            (Some("text-edit"), Some("replace")) => Some("strikeout".to_string()),
            _ => None,
        };
        selection.set_highlight_visible(preview.is_none());
        match (&preview, &tool) {
            (Some(subtype), Some(tool)) if selection.has_selection() => {
                // `Record<number, Quad[]>`: integer keys print in ascending order.
                let mut by_page: BTreeMap<u32, Value> = BTreeMap::new();
                for entry in selection.get_snapshot().pages {
                    let quads: Vec<_> = entry.segments.iter().map(|s| s.quad).collect();
                    by_page.insert(entry.page.page_object_number, json!(quads));
                }
                let quads: Map<String, Value> =
                    by_page.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
                if let Err(error) =
                    self.preview_markup(subtype, &Value::Object(quads), Some(&tool.preset))
                {
                    crate::console::error(
                        "[annotation] a markup preview failed:",
                        error.to_canonical(),
                    );
                }
            }
            _ => self.clear_markup_preview(),
        }
    }

    /// `wireMarkup(annotation, selection, interaction)`.
    pub(crate) fn wire_markup(
        &self,
        selection: &SelectionCapability,
        interaction: &InteractionCapability,
    ) {
        let (me, s, i) = (self.weak(), selection.clone(), interaction.clone());
        let off = selection.on_changed(move |_| {
            if let Some(core) = me.upgrade() {
                core.sync_markup(&s, &i);
            }
            Ok(())
        });
        self.cx.listen(off);
        let (me, i) = (self.weak(), interaction.clone());
        let off = selection.on_committed(move |_| {
            if let Some(core) = me.upgrade() {
                core.apply_tool_to_selection(&i.get_active_tool_id())?;
            }
            Ok(())
        });
        self.cx.listen(off);
        let (me, s, i) = (self.weak(), selection.clone(), interaction.clone());
        let off = interaction.on_tool_changed(move |_| {
            if let Some(core) = me.upgrade() {
                if !s.is_selecting() {
                    core.apply_tool_to_selection(&i.get_active_tool_id())?;
                }
                core.sync_markup(&s, &i);
            }
            Ok(())
        });
        self.cx.listen(off);
    }
}
