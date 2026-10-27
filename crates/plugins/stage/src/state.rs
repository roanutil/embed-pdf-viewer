//! `state.ts`: the Stage page's state table as code, what `useStageState()`
//! returns in every framework. A second view registered with its own token
//! reads the same fields through its own capability.

use epdf_geometry::PageRotation;
use epdf_stage_core::ZoomMode;

use crate::contract::{StageStateView, StageZoomMode};
use crate::controller::StageCapability;

/// `stageState`: the read over a lens, or the empty state without one.
pub fn stage_state(stage: Option<&StageCapability>) -> StageStateView {
    match stage {
        Some(stage) => StageStateView {
            zoom_level: stage.get_zoom_level(),
            zoom_mode: stage.get_zoom_mode(),
            current_page_index: stage.get_current_page_index(),
            current_page: stage.get_current_page(),
            page_count: stage.get_page_count(),
            view_rotation: stage.get_view_rotation(),
            active_rules: stage.list_active_rules(),
        },
        None => StageStateView {
            zoom_level: 1.0,
            zoom_mode: StageZoomMode::Mode(ZoomMode::Automatic),
            current_page_index: 0,
            current_page: None,
            page_count: 0,
            view_rotation: PageRotation::DEG_0,
            active_rules: Vec::new(),
        },
    }
}
