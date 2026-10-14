use epdf_ffi::{
    AnnotationFlagsPatch, AnnotationPropKey, AnnotationProps, AnnotationPropsPatch, KernelBound,
    ViewerError,
};

use super::{AnnotationSnapSettings, AnnotationSnapSettingsPatch, open};
use crate::AnnotationCapability;

/// The authoring tools and their per-tool defaults.
#[derive(uniffi::Object)]
pub struct AnnotationToolsHandle {
    pub(crate) cap: KernelBound<AnnotationCapability>,
}

/// One authoring tool. Activate it through plan 01's
/// `InteractionHandle.activate_tool(id)`.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationTool {
    pub id: String,
    pub subtype: String,
    pub preset: String,
    pub cursor: String,
    pub enables: Vec<String>,
    pub upright: bool,
    pub defaults: Option<AnnotationPropsPatch>,
    pub flags: Option<AnnotationFlagsPatch>,
}

impl From<crate::AnnotationTool> for AnnotationTool {
    fn from(t: crate::AnnotationTool) -> Self {
        AnnotationTool {
            id: t.id,
            subtype: t.subtype,
            preset: t.preset,
            cursor: t.cursor,
            enables: t.enables,
            upright: t.upright,
            defaults: t.defaults.map(props_patch_out),
            flags: t.flags.map(Into::into),
        }
    }
}

/// A core props patch as the foreign record: set values carry over, a
/// `null` `interiorColor` or `link` goes into `clear`. Vendor keys in
/// `extra` are dropped.
fn props_patch_out(p: epdf_annotation_core::AnnotationPropsPatch) -> AnnotationPropsPatch {
    use epdf_annotation_core::js::Nullable;
    let mut clear = Vec::new();
    let interior_color = match p.interior_color {
        Nullable::Value(c) => Some(c),
        Nullable::Null => {
            clear.push(AnnotationPropKey::InteriorColor);
            None
        }
        Nullable::Undefined => None,
    };
    let link = match p.link {
        Nullable::Value(l) => Some(l.into()),
        Nullable::Null => {
            clear.push(AnnotationPropKey::Link);
            None
        }
        Nullable::Undefined => None,
    };
    AnnotationPropsPatch {
        color: p.color,
        interior_color,
        stroke_width: p.stroke_width,
        opacity: p.opacity,
        blend_mode: p.blend_mode.map(Into::into),
        border: p.border.map(Into::into),
        line_endings: p.line_endings.map(|e| epdf_ffi::AnnotationLineEndingsPatch {
            start: e.start.map(Into::into),
            end: e.end.map(Into::into),
        }),
        font_family: p.font_family,
        font_size: p.font_size,
        font_color: p.font_color,
        text_align: p.text_align.map(Into::into),
        bold: p.bold,
        italic: p.italic,
        underline: p.underline,
        icon: p.icon,
        link,
        clear,
    }
}

/// One editable property of a tool or a selection, as a panel shows it.
/// The members beyond `key` and `label` depend on the key: a range for
/// `opacity`, `stroke_width` and `font_size`, `cloudy` for `border`,
/// `options` for `icon`.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationPropSpec {
    pub key: AnnotationPropKey,
    pub label: String,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
    pub cloudy: Option<bool>,
    pub options: Option<Vec<String>>,
}

impl From<epdf_annotation_core::PropSpec> for AnnotationPropSpec {
    fn from(s: epdf_annotation_core::PropSpec) -> Self {
        AnnotationPropSpec {
            key: s.key.into(),
            label: s.label.to_string(),
            min: s.min,
            max: s.max,
            step: s.step,
            cloudy: s.cloudy,
            options: s.options.map(|o| o.iter().map(|s| s.to_string()).collect()),
        }
    }
}

#[uniffi::export]
impl AnnotationToolsHandle {
    pub async fn tools(&self) -> Result<Vec<AnnotationTool>, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.list_tools().into_iter().map(Into::into).collect())).await
    }

    pub async fn tool(&self, id: String) -> Result<Option<AnnotationTool>, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.get_tool(&id).map(Into::into))).await
    }

    pub async fn defaults(&self, tool_id: String) -> Result<AnnotationProps, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.get_tool_defaults(&tool_id).into())).await
    }

    pub async fn set_defaults(
        &self,
        tool_id: String,
        patch: AnnotationPropsPatch,
    ) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?.set_tool_defaults(&tool_id, patch.try_into()?);
                Ok(())
            })
            .await
    }

    pub async fn prop_specs(
        &self,
        tool_id: String,
    ) -> Result<Vec<AnnotationPropSpec>, ViewerError> {
        self.cap
            .call(move |c| {
                Ok(open(c)?.list_prop_specs(&tool_id).into_iter().map(Into::into).collect())
            })
            .await
    }

    pub async fn snap_settings(&self) -> Result<AnnotationSnapSettings, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.get_snap_settings().into())).await
    }

    pub async fn update_snap_settings(
        &self,
        patch: AnnotationSnapSettingsPatch,
    ) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?.update_snap_settings(patch.into());
                Ok(())
            })
            .await
    }
}
