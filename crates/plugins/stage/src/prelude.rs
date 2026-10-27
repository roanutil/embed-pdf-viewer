//! What every area of the controller reads: the crate's contract and model,
//! the kernel's error, stage-core, and the controller's private helpers.
//! Crate-private.

pub(crate) use std::cell::{Cell, RefCell};
pub(crate) use std::rc::Rc;

pub(crate) use epdf_geometry::{
    PageRotation, PageTransformInput, Point, Rect, Size, add_rotations, apply_point, apply_rect,
    display_size, intersect_rects, js, page_transform, rotate_scale_matrix, snap_to_device,
};
pub(crate) use epdf_kernel::Error;
pub(crate) use epdf_stage_core as sc;
pub(crate) use epdf_stage_core::{
    Align, AlignValue, AlignmentValue, Anchor, Camera, CameraConstraint, Direction, PageBox,
    PageGeom, Scene, SceneItem, ScrollMetrics, ZoomMode, ZoomSpec,
};

pub(crate) use crate::contract::*;
pub(crate) use crate::controller::{
    Grouping, StageCapability, falsy, num, read_of_undefined, serde_str, stage_error,
};
pub(crate) use crate::model::{MotionCause, StageState};
pub(crate) use crate::motion::{
    FLING_STOP, ease_out_cubic, glide_step, rubber_in, rubber_out, spring_step, zoom_lerp,
};
pub(crate) use crate::responsive::{box_of, eq_setting, merge_settings, resolve_responsive};
pub(crate) use crate::services::scene::PageRow;
pub(crate) use crate::settings::{
    SETTING_KEYS, SettingEffect, SettingKey, default_responsive, patch_has, patch_keys,
    settings_effect,
};
