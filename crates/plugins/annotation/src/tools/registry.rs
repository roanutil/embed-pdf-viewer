//! `tools/registry.ts`: the resolved tool table (built-ins and the `tools`
//! setting). A tool maps its id to a routing subtype, a defaults key
//! (`preset`), the kind whose properties it shows and, for stamps, a source
//! spec. The setting's tools are kept so `tools.register` can re-resolve
//! `extends` against the same base pool.

use std::cell::RefCell;
use std::rc::Rc;

use epdf_annotation_core::geometry::upright_rotation;
use epdf_annotation_core::kinds::kind_named;
use epdf_annotation_core::props::properties_of;
use epdf_annotation_core::record::{read_of_defaults, text_of};
use epdf_annotation_core::update::defaults_for;
use epdf_kernel::{Error, Unsubscribe};
use epdf_plugin_interaction::{INTERACTION, RegisterToolOptions, Tool};
use indexmap::IndexMap;
use serde_json::{Map, Value, json};

use crate::controller::Core;
use crate::js::Object;
use crate::read::property_values::{PropertySources, property_values};
use crate::tools::definitions::{ResolvedTool, build_tool_registry, tool_touch_of};

/// A tool's defaults and style panel, cached while its defaults stay the same.
#[derive(Clone)]
struct ToolProperties {
    own: Object,
    defaults: Value,
    properties: Value,
}

#[derive(Default)]
pub struct ToolsState {
    registry: RefCell<IndexMap<String, Rc<ResolvedTool>>>,
    config_tools: RefCell<Vec<Value>>,
    properties: RefCell<IndexMap<String, ToolProperties>>,
    projections: RefCell<Vec<(Rc<ResolvedTool>, Value)>>,
}

/// The tool as the hub registers it.
pub fn hub_tool(tool: &ResolvedTool) -> Rc<Tool> {
    let mut hub =
        Tool::new(tool.id.clone(), tool.cursor.clone(), tool.enables.iter().map(String::as_str));
    hub.touch = Some(tool_touch_of(&tool.enables));
    // The conformance harness prints a tool as it was written; this one is
    // written `{ id, cursor, enables, touch }` (`registry.ts`).
    #[cfg(feature = "harness")]
    {
        let spelled = json!({
            "id": hub.id,
            "cursor": hub.cursor,
            "enables": { "$set": hub.enables },
            "touch": hub.touch,
        });
        return epdf_plugin_interaction::harness::tool_from_json(&spelled, None);
    }
    #[cfg_attr(feature = "harness", allow(unreachable_code))]
    Rc::new(hub)
}

impl Core {
    /// `createToolRegistry(ctx, settingTools, store, events)`: every tool's
    /// next annotation starts from its defaults.
    pub(crate) fn create_tool_registry(&self) -> Result<(), Error> {
        let config = self.parsed().tools();
        let registry = build_tool_registry(&config)?;
        *self.tools.config_tools.borrow_mut() = config;
        *self.tools.registry.borrow_mut() = registry.clone();
        for tool in registry.values() {
            if let Some(defaults) = &tool.defaults {
                self.commit_json(
                    json!({ "type": "setDefaults", "preset": tool.preset, "patch": defaults }),
                )?;
            }
        }
        Ok(())
    }

    /// `tools.get(id)` on the registry.
    pub(crate) fn tool(&self, id: &str) -> Option<Rc<ResolvedTool>> {
        self.tools.registry.borrow().get(id).cloned()
    }

    fn tool_values(&self) -> Vec<Rc<ResolvedTool>> {
        self.tools.registry.borrow().values().cloned().collect()
    }

    /// `activeTool()`: the hub's active tool, resolved through this registry.
    pub(crate) fn active_tool(&self) -> Option<Rc<ResolvedTool>> {
        let interaction = self.cx.try_get(&INTERACTION).ok().flatten()?;
        self.tool(&interaction.get_active_tool_id())
    }

    /// `uprightRotFor(displayRotation)`: the active tool's upright counter-rotation.
    pub(crate) fn upright_rot_for(&self, display_rotation: Option<f64>) -> f64 {
        let Some(rotation) = display_rotation.filter(|r| *r != 0.0 && !r.is_nan()) else {
            return 0.0;
        };
        match self.active_tool() {
            Some(tool) if tool.upright => upright_rotation(rotation),
            _ => 0.0,
        }
    }

    fn tool_properties_of(&self, tool_id: &str) -> ToolProperties {
        let tool = self.tool(tool_id);
        let kind = tool.as_ref().map_or_else(|| tool_id.to_string(), |t| t.subtype.clone());
        let preset = tool.as_ref().map_or_else(|| tool_id.to_string(), |t| t.preset.clone());
        let own = defaults_for(&self.model(), &preset);
        if let Some(cached) = self.tools.properties.borrow().get(tool_id)
            && cached.own == own
        {
            return cached.clone();
        }
        let read = read_of_defaults(&kind, &own);
        let mut defaults = read.clone();
        defaults.shift_remove("subtype");
        let fields_kind =
            tool.as_ref().map_or_else(|| tool_id.to_string(), |t| t.fields_kind.clone());
        let properties = properties_of(&kind_named(&fields_kind));
        let text = text_of(&read);
        let target = if read.get("subtype").and_then(Value::as_str) == Some("link") {
            read.get("target")
                .cloned()
                .filter(|t| !epdf_kernel::is_undefined(t))
                .unwrap_or(Value::Null)
        } else {
            Value::Null
        };
        let values = property_values(
            &properties,
            &PropertySources { data: &defaults, text: text.as_ref(), link: Some(target) },
        );
        let entry = ToolProperties {
            own,
            defaults: Value::Object(defaults),
            properties: json!({ "properties": properties, "values": values, "mixed": [] }),
        };
        self.tools.properties.borrow_mut().insert(tool_id.to_string(), entry.clone());
        entry
    }

    /// `tools.updateDefaults(id, changes)`: a change to the values they have changes nothing.
    pub fn tools_update_defaults(&self, tool_id: &str, changes: &Object) -> Result<(), Error> {
        let own = self.tool_properties_of(tool_id).own;
        let changed = changes
            .iter()
            .any(|(k, v)| crate::js::stringify(own.get(k)) != crate::js::stringify(Some(v)));
        if !changed {
            return Ok(());
        }
        let preset = self.tool(tool_id).map_or_else(|| tool_id.to_string(), |t| t.preset.clone());
        self.commit_json(json!({ "type": "setDefaults", "preset": preset, "patch": changes }))?;
        let defaults = self.tool_properties_of(tool_id).defaults;
        self.events.tool_defaults_changed.emit(&json!({ "toolId": tool_id, "defaults": defaults }));
        Ok(())
    }

    /// `projectTool(tool)`: the public shape of a tool, the same object per resolved tool.
    fn project_tool(&self, tool: &Rc<ResolvedTool>) -> Value {
        if let Some((_, v)) =
            self.tools.projections.borrow().iter().find(|(t, _)| Rc::ptr_eq(t, tool))
        {
            return v.clone();
        }
        let mut out = Map::new();
        out.insert("id".into(), json!(tool.id));
        out.insert("subtype".into(), json!(tool.subtype));
        out.insert("preset".into(), json!(tool.preset));
        out.insert("cursor".into(), json!(tool.cursor));
        out.insert("enables".into(), json!(tool.enables));
        if let Some(d) = &tool.defaults {
            out.insert("defaults".into(), Value::Object(d.clone()));
        }
        out.insert("ghost".into(), tool.ghost_json());
        if let Some(m) = &tool.meta {
            out.insert("meta".into(), Value::Object(m.clone()));
        }
        let value = Value::Object(out);
        let mut projections = self.tools.projections.borrow_mut();
        projections.retain(|(t, _)| Rc::strong_count(t) > 1);
        projections.push((tool.clone(), value.clone()));
        value
    }

    /// `tools.list()`.
    pub fn tools_list(&self) -> Vec<Value> {
        self.tool_values().iter().map(|t| self.project_tool(t)).collect()
    }

    /// `tools.get(id)`.
    pub fn tools_get(&self, id: &str) -> Value {
        self.tool(id).map_or(Value::Null, |t| self.project_tool(&t))
    }

    /// `tools.register(definition)`: re-resolved against the same base pool,
    /// registered with the hub, its defaults seeded. Returns the remover.
    pub fn tools_register(&self, definition: Value) -> Result<Unsubscribe, Error> {
        let id = definition.get("id").and_then(Value::as_str).unwrap_or("undefined").to_string();
        let mut pool = self.tools.config_tools.borrow().clone();
        pool.push(definition);
        let resolved = build_tool_registry(&pool)?
            .get(&id)
            .cloned()
            .ok_or_else(|| Error::new(format!("[annotation] could not resolve tool '{id}'")))?;
        self.tools.registry.borrow_mut().insert(resolved.id.clone(), resolved.clone());
        let unregister = match self.cx.try_get(&INTERACTION).ok().flatten() {
            Some(interaction) => Some(
                interaction
                    .register_tool(hub_tool(&resolved), RegisterToolOptions { replace: true })?,
            ),
            None => None,
        };
        if let Some(defaults) = &resolved.defaults {
            self.commit_json(
                json!({ "type": "setDefaults", "preset": resolved.preset, "patch": defaults }),
            )?;
        }
        let me = self.weak();
        let tool_id = resolved.id.clone();
        Ok(Unsubscribe::new(move || {
            if let Some(core) = me.upgrade() {
                core.tools.registry.borrow_mut().shift_remove(&tool_id);
            }
            if let Some(unregister) = &unregister {
                unregister.call();
            }
        }))
    }

    /// `tools.getDefaults(id)`.
    pub fn tools_get_defaults(&self, id: &str) -> Value {
        self.tool_properties_of(id).defaults
    }

    /// `tools.getProperties(id)`.
    pub fn tools_get_properties(&self, id: &str) -> Value {
        self.tool_properties_of(id).properties
    }

    /// `listResolvedTools()`.
    pub fn list_resolved_tools(&self) -> Vec<Rc<ResolvedTool>> {
        self.tool_values()
    }

    /// `getResolvedTool(id)`.
    pub fn get_resolved_tool(&self, id: &str) -> Option<Rc<ResolvedTool>> {
        self.tool(id)
    }

    /// `getToolSubtype(id)`: the tool's routing subtype, or the id itself.
    pub fn get_tool_subtype(&self, id: &str) -> String {
        self.tool(id).map_or_else(|| id.to_string(), |t| t.subtype.clone())
    }
}
