use epdf_ffi::{KernelBound, ViewerError};
use serde_json::Value;

use super::{object_of, open};
use crate::AnnotationCapability;

/// The authoring tools and their per-tool defaults.
#[derive(uniffi::Object)]
pub struct AnnotationToolsHandle {
    pub(crate) cap: KernelBound<AnnotationCapability>,
}

/// One authoring tool (`ResolvedTool`). Activate it through the interaction
/// handle's `activate_tool(id)`.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationTool {
    pub id: String,
    pub subtype: String,
    pub preset: String,
    pub cursor: String,
    pub enables: Vec<String>,
    pub upright: bool,
    /// The tool as the plugin resolves it, every member.
    pub tool_json: String,
}

fn tool_out(tool: &Value) -> AnnotationTool {
    let s = |k: &str| tool.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    // `enables` is a set; its JSON is a list.
    let enables = tool
        .get("enables")
        .and_then(|e| e.as_array().or_else(|| e.get("$set").and_then(Value::as_array)))
        .map(|list| list.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default();
    AnnotationTool {
        id: s("id"),
        subtype: s("subtype"),
        preset: s("preset"),
        cursor: s("cursor"),
        enables,
        upright: tool.get("upright").and_then(Value::as_bool).unwrap_or(false),
        tool_json: tool.to_string(),
    }
}

/// One editable property of a tool or a selection, as a panel shows it
/// (`AnnotationProperty`): its key, control, label and, by control, a range
/// or options. The rest of the description is in `property_json`.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationProperty {
    pub key: String,
    pub control: String,
    pub label: String,
    pub label_key: Option<String>,
    pub property_json: String,
}

pub(crate) fn properties_out(list: &Value) -> Vec<AnnotationProperty> {
    list.as_array()
        .map(|list| {
            list.iter()
                .map(|p| {
                    let s = |k: &str| p.get(k).and_then(Value::as_str).map(str::to_string);
                    AnnotationProperty {
                        key: s("key").unwrap_or_default(),
                        control: s("control").unwrap_or_default(),
                        label: s("label").unwrap_or_default(),
                        label_key: s("labelKey"),
                        property_json: p.to_string(),
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

#[uniffi::export]
impl AnnotationToolsHandle {
    /// `tools.list()`.
    pub async fn tools(&self) -> Result<Vec<AnnotationTool>, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.tools_list().iter().map(tool_out).collect())).await
    }

    /// `tools.get(id)`.
    pub async fn tool(&self, id: String) -> Result<Option<AnnotationTool>, ViewerError> {
        self.cap
            .call(move |c| {
                let tool = open(c)?.tools_get(&id);
                Ok((!tool.is_null()).then(|| tool_out(&tool)))
            })
            .await
    }

    /// `tools.getDefaults(id)`: the engine's field bag a new annotation from
    /// this tool starts with.
    pub async fn defaults(&self, tool_id: String) -> Result<String, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.tools_get_defaults(&tool_id).to_string())).await
    }

    /// `tools.updateDefaults(id, changes)`.
    pub async fn update_defaults(
        &self,
        tool_id: String,
        changes_json: String,
    ) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                let changes = object_of(&changes_json)?;
                open(c)?.tools_update_defaults(&tool_id, &changes)
            })
            .await
    }

    /// `tools.getProperties(id)`: what the tool's style panel shows.
    pub async fn properties(
        &self,
        tool_id: String,
    ) -> Result<super::AnnotationSelectionProps, ViewerError> {
        self.cap
            .call(move |c| {
                let read = open(c)?.tools_get_properties(&tool_id);
                Ok(super::AnnotationSelectionProps {
                    properties: properties_out(read.get("properties").unwrap_or(&Value::Null)),
                    values_json: read.get("values").cloned().unwrap_or(Value::Null).to_string(),
                    mixed: Vec::new(),
                })
            })
            .await
    }
}
