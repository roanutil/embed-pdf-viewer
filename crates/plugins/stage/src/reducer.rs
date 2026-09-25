//! `reducer.ts`: the initial state and the pure reducer.

use epdf_geometry::Size;
use epdf_stage_core::Camera;

use crate::responsive::merge_settings;
use crate::settings::default_settings;
use crate::types::*;

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
