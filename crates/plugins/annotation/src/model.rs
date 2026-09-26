//! `model.ts`: the slice holds the core model, the hydration status, the
//! chrome settings, the tool ghost, the stamp arm epoch and the editor's
//! text selection. The pure `update` runs in the store service; the reducer
//! only stores.

use std::rc::Rc;

use epdf_annotation_core::{Model, SnapSettings, initial_model};
use serde::Serialize;

use crate::contract::*;

/// `AnnotationState`. The model and chrome sit behind `Rc` so the memoized
/// selectors can key on identity, as the TypeScript's `===` does.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationState {
    pub model: Rc<Model>,
    pub hydration: AnnotationHydration,
    pub chrome: Rc<ChromeSettings>,
    pub tool_ghost: Option<Rc<ToolGhost>>,
    pub stamp_arm_epoch: f64,
    pub text_selection: Option<Rc<TextSelection>>,
}

/// The hydration error makes one variant large; actions are transient.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type")]
pub enum AnnotationAction {
    #[serde(rename = "SET_MODEL")]
    SetModel { model: Rc<Model> },
    #[serde(rename = "SET_HYDRATION")]
    SetHydration { hydration: AnnotationHydration },
    #[serde(rename = "SET_CHROME")]
    SetChrome { patch: ChromeSettingsPatch },
    #[serde(rename = "SET_TOOL_GHOST")]
    SetToolGhost { ghost: Option<Rc<ToolGhost>> },
    #[serde(rename = "SET_TEXT_SELECTION")]
    SetTextSelection { selection: Option<Rc<TextSelection>> },
    #[serde(rename = "STAMP_ARM_CHANGED")]
    StampArmChanged,
}

/// `DEFAULT_CHROME`.
pub fn default_chrome() -> ChromeSettings {
    ChromeSettings {
        accent: "#3858e9".into(),
        outline: ChromeOutline { style: StrokeStyle::Solid, width: 1.0, color: None },
        handles: ChromeHandles { size: 8.0, hit_size: 24.0, fill: "#ffffff".into(), stroke: None },
        knob: ChromeKnob {
            size: 10.0,
            hit_size: 24.0,
            offset: 32.0,
            stalk: true,
            fill: "#ffffff".into(),
            stroke: None,
        },
        guides: ChromeGuides {
            enabled: true,
            style: StrokeStyle::Solid,
            width: 1.0,
            axis_color: None,
            axis_opacity: 0.35,
            indicator_color: None,
            indicator_opacity: 0.8,
        },
    }
}

/// `mergeChrome(base, patch)`: one level per piece.
pub fn merge_chrome(base: &ChromeSettings, patch: &ChromeSettingsPatch) -> ChromeSettings {
    let o = patch.outline.clone().unwrap_or_default();
    let h = patch.handles.clone().unwrap_or_default();
    let k = patch.knob.clone().unwrap_or_default();
    let g = patch.guides.clone().unwrap_or_default();
    ChromeSettings {
        accent: patch.accent.clone().unwrap_or_else(|| base.accent.clone()),
        outline: ChromeOutline {
            style: o.style.unwrap_or(base.outline.style),
            width: o.width.unwrap_or(base.outline.width),
            color: o.color.or_else(|| base.outline.color.clone()),
        },
        handles: ChromeHandles {
            size: h.size.unwrap_or(base.handles.size),
            hit_size: h.hit_size.unwrap_or(base.handles.hit_size),
            fill: h.fill.unwrap_or_else(|| base.handles.fill.clone()),
            stroke: h.stroke.or_else(|| base.handles.stroke.clone()),
        },
        knob: ChromeKnob {
            size: k.size.unwrap_or(base.knob.size),
            hit_size: k.hit_size.unwrap_or(base.knob.hit_size),
            offset: k.offset.unwrap_or(base.knob.offset),
            stalk: k.stalk.unwrap_or(base.knob.stalk),
            fill: k.fill.unwrap_or_else(|| base.knob.fill.clone()),
            stroke: k.stroke.or_else(|| base.knob.stroke.clone()),
        },
        guides: ChromeGuides {
            enabled: g.enabled.unwrap_or(base.guides.enabled),
            style: g.style.unwrap_or(base.guides.style),
            width: g.width.unwrap_or(base.guides.width),
            axis_color: g.axis_color.or_else(|| base.guides.axis_color.clone()),
            axis_opacity: g.axis_opacity.unwrap_or(base.guides.axis_opacity),
            indicator_color: g.indicator_color.or_else(|| base.guides.indicator_color.clone()),
            indicator_opacity: g.indicator_opacity.unwrap_or(base.guides.indicator_opacity),
        },
    }
}

/// `{ ...initialModel.snap, ...config.snap }`.
fn snap_over(base: &SnapSettings, patch: &epdf_annotation_core::SnapSettingsPatch) -> SnapSettings {
    SnapSettings {
        guides: patch.guides.unwrap_or(base.guides),
        guide_threshold: patch.guide_threshold.unwrap_or(base.guide_threshold),
        rotation: patch.rotation.unwrap_or(base.rotation),
        rotation_angles: patch
            .rotation_angles
            .clone()
            .unwrap_or_else(|| base.rotation_angles.clone()),
        rotation_threshold: patch.rotation_threshold.unwrap_or(base.rotation_threshold),
    }
}

/// `initialAnnotationState(config)`.
pub fn initial_annotation_state(config: &AnnotationConfig) -> AnnotationState {
    let base = initial_model();
    let snap = match &config.snap {
        Some(patch) => snap_over(&base.snap, patch),
        None => base.snap.clone(),
    };
    AnnotationState {
        model: Rc::new(Model { snap, ..base }),
        hydration: AnnotationHydration::Loading,
        chrome: Rc::new(merge_chrome(
            &default_chrome(),
            &config.chrome.clone().unwrap_or_default(),
        )),
        tool_ghost: None,
        stamp_arm_epoch: 0.0,
        text_selection: None,
    }
}

/// `annotationReducer`. Every action replaces the state.
pub fn annotation_reducer(
    state: &AnnotationState,
    action: &AnnotationAction,
) -> Option<AnnotationState> {
    let mut next = state.clone();
    match action {
        AnnotationAction::SetModel { model } => next.model = model.clone(),
        AnnotationAction::SetHydration { hydration } => next.hydration = hydration.clone(),
        AnnotationAction::SetChrome { patch } => {
            next.chrome = Rc::new(merge_chrome(&state.chrome, patch));
        }
        AnnotationAction::SetToolGhost { ghost } => next.tool_ghost = ghost.clone(),
        AnnotationAction::SetTextSelection { selection } => {
            next.text_selection = selection.clone();
        }
        AnnotationAction::StampArmChanged => {
            next.stamp_arm_epoch = state.stamp_arm_epoch + 1.0;
            next.tool_ghost = None;
        }
    }
    Some(next)
}
