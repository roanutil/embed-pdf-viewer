//! `model.ts`: the stage slice. The flat settings bag plus the camera,
//! viewport, cursor and the render-commit latch, and the pure reducer.
//! Every transition is here; nothing else mutates stage state.

use epdf_geometry::Size;
use epdf_stage_core::Camera;
use serde::{Deserialize, Serialize};

use crate::contract::*;
use crate::responsive::merge_settings;
use crate::settings::default_settings;

/// What last drove the camera or cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MotionCause {
    User,
    Programmatic,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageState {
    pub camera: Camera,
    /// The one-way render-commit latch.
    pub placed: bool,
    /// False while the zoom is in motion.
    pub camera_resting: bool,
    pub vp: Size,
    /// Device px per view px.
    pub dpr: f64,
    /// The current page, valid in both flows.
    pub cursor: usize,
    pub motion_cause: MotionCause,
    /// Names of the matching responsive rules, in source order.
    pub active_rules: Vec<String>,
    #[serde(flatten)]
    pub settings: StageSettings,
}

// A PATCH carries a whole settings bag; actions are short-lived values, so
// the size difference costs nothing a box would save.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum StageAction {
    #[serde(rename = "CAMERA")]
    Camera { camera: Camera },
    #[serde(rename = "CAMERA_REST")]
    CameraRest { resting: bool },
    #[serde(rename = "PLACED")]
    Placed,
    #[serde(rename = "VP")]
    Vp { vp: Size },
    #[serde(rename = "DPR")]
    Dpr { dpr: f64 },
    #[serde(rename = "CURSOR")]
    Cursor { cursor: usize },
    #[serde(rename = "MOTION_CAUSE")]
    MotionCause { cause: MotionCause },
    #[serde(rename = "PATCH")]
    Patch { patch: PartialSettings },
    #[serde(rename = "RESPONSIVE")]
    Responsive { active: Vec<String> },
}

/// `initialStageState(config)`: the defaults under the config's settings
/// overrides. The scheduler and the rules are capability config, not state.
pub fn initial_stage_state(overrides: &PartialSettings) -> StageState {
    StageState {
        settings: merge_settings(&default_settings(), overrides),
        camera: Camera { x: 0.0, y: 0.0, zoom: 1.0 },
        placed: false,
        active_rules: Vec::new(),
        camera_resting: true,
        vp: Size { width: 0.0, height: 0.0 },
        dpr: 1.0,
        cursor: 0,
        motion_cause: MotionCause::User,
    }
}

/// Pure. `None` is the TypeScript's `return state`: no change, no
/// notification.
pub fn stage_reducer(state: &StageState, action: &StageAction) -> Option<StageState> {
    let mut next = state.clone();
    match action {
        StageAction::Camera { camera } => next.camera = *camera,
        StageAction::CameraRest { resting } => {
            if state.camera_resting == *resting {
                return None;
            }
            next.camera_resting = *resting;
        }
        StageAction::Placed => {
            if state.placed {
                return None;
            }
            next.placed = true;
        }
        StageAction::Vp { vp } => next.vp = *vp,
        StageAction::Dpr { dpr } => next.dpr = *dpr,
        StageAction::Cursor { cursor } => next.cursor = *cursor,
        StageAction::MotionCause { cause } => next.motion_cause = *cause,
        StageAction::Patch { patch } => next.settings = merge_settings(&state.settings, patch),
        StageAction::Responsive { active } => next.active_rules = active.clone(),
    }
    Some(next)
}
