//! `tools/registry.ts`: the resolved tool table (built-ins plus config
//! overrides). The config tools are kept so `registerTool` re-resolves
//! `extends` against the same base pool.

use std::rc::Rc;

use epdf_annotation_core::{
    AnnotationProps, AnnotationPropsPatch, Msg, PropSpec, defaults_for, props_for, upright_rotation,
};
use epdf_kernel::Error;
use epdf_plugin_interaction::{INTERACTION, RegisterToolOptions, Tool, Unregister};

use crate::contract::AnnotationTool;
use crate::controller::AnnotationCapability;
use crate::tools::definitions::{
    AnnotationToolDef, ResolvedTool, build_tool_registry, is_touch_direct,
};

impl AnnotationCapability {
    /// `tools.get(id)`.
    pub(crate) fn tool(&self, id: &str) -> Option<ResolvedTool> {
        self.0.registry.borrow().get(id).cloned()
    }

    /// `tools.activeTool()`: the hub's active tool, through this registry.
    pub(crate) fn active_tool(&self) -> Option<ResolvedTool> {
        let ix = self.cx().try_get(&INTERACTION).ok().flatten()?;
        self.tool(&ix.get_active_tool_id())
    }

    /// `tools.uprightRotFor(displayRotation)`: the active tool's upright
    /// counter-rotation, 0 when it asks for none or the display is upright.
    pub(crate) fn upright_rot_for(&self, display_rotation: Option<f64>) -> f64 {
        let Some(dr) = display_rotation.filter(|r| *r != 0.0 && !r.is_nan()) else { return 0.0 };
        match self.active_tool() {
            Some(tool) if tool.upright => upright_rotation(dr),
            _ => 0.0,
        }
    }

    fn project_tool(t: &ResolvedTool) -> AnnotationTool {
        AnnotationTool {
            id: t.id.clone(),
            subtype: t.subtype.clone(),
            preset: t.preset.clone(),
            cursor: t.cursor.clone(),
            enables: t.enables.clone(),
            defaults: t.defaults.clone(),
            flags: t.flags,
            upright: t.upright,
        }
    }

    /// `listTools()`.
    pub fn list_tools(&self) -> Vec<AnnotationTool> {
        self.0.registry.borrow().values().map(Self::project_tool).collect()
    }

    /// `getTool(id)`.
    pub fn get_tool(&self, id: &str) -> Option<AnnotationTool> {
        self.tool(id).as_ref().map(Self::project_tool)
    }

    /// `registerTool(def)`: register (or replace by id) at runtime; the
    /// returned remover removes it again.
    pub fn register_tool(&self, def: AnnotationToolDef) -> Result<Unregister, Error> {
        let mut all = self.0.config_tools.clone();
        all.push(def.clone());
        let registry = build_tool_registry(&all)?;
        let resolved = registry.get(&def.id).cloned().ok_or_else(|| {
            Error::new(format!("[annotation] could not resolve tool '{}'", def.id))
        })?;
        self.0.registry.borrow_mut().set(resolved.clone());
        let un = match self.cx().try_get(&INTERACTION)? {
            Some(ix) => {
                let mut tool = Tool::new(
                    resolved.id.clone(),
                    resolved.cursor.clone(),
                    resolved.enables.iter().map(String::as_str),
                );
                tool.touch_direct = Some(is_touch_direct(&resolved.enables));
                Some(ix.register_tool(tool, RegisterToolOptions { replace: true })?)
            }
            None => None,
        };
        if let Some(defaults) = resolved.defaults.clone() {
            self.commit(Msg::SetDefaults { subtype: resolved.preset.clone(), patch: defaults });
        }
        let weak = Rc::downgrade(&self.0);
        let id = resolved.id.clone();
        Ok(Unregister::new(move || {
            if let Some(core) = weak.upgrade() {
                core.registry.borrow_mut().delete(&id);
            }
            if let Some(un) = &un {
                un.call();
            }
        }))
    }

    /// `getToolDefaults(toolId)`: through the tool's preset.
    pub fn get_tool_defaults(&self, tool_id: &str) -> AnnotationProps {
        let preset = self.tool(tool_id).map_or_else(|| tool_id.to_string(), |t| t.preset);
        defaults_for(&self.model(), &preset)
    }

    /// `setToolDefaults(toolId, patch)`.
    pub fn set_tool_defaults(&self, tool_id: &str, patch: AnnotationPropsPatch) {
        let preset = self.tool(tool_id).map_or_else(|| tool_id.to_string(), |t| t.preset);
        self.commit(Msg::SetDefaults { subtype: preset, patch });
    }

    /// `listPropSpecs(toolId)`: the specs of the tool's props kind.
    pub fn list_prop_specs(&self, tool_id: &str) -> Vec<PropSpec> {
        let kind = self.tool(tool_id).map_or_else(|| tool_id.to_string(), |t| t.props_kind);
        props_for(&kind).to_vec()
    }

    /// `listResolvedTools()` (host): every resolved tool, in registry order.
    pub fn list_resolved_tools(&self) -> Vec<ResolvedTool> {
        self.0.registry.borrow().values().cloned().collect()
    }

    /// `getResolvedTool(id)` (host).
    pub fn get_resolved_tool(&self, id: &str) -> Option<ResolvedTool> {
        self.tool(id)
    }

    /// `getToolSubtype(id)` (host): the routing subtype, else the id.
    pub fn get_tool_subtype(&self, id: &str) -> String {
        self.tool(id).map_or_else(|| id.to_string(), |t| t.subtype)
    }
}
