//! `model.ts`: the stage's session state, the flat settings bag plus the
//! camera, the viewport, the cursor and the render-commit latch, and the
//! pure transitions. The controller's areas apply them with
//! `update_state`; nothing else changes stage state.
//!
//! The TypeScript transitions return the same state object when nothing
//! changed, and compare by identity (`state.camera === camera`). Two of
//! the objects the state holds are compared that way by the change hooks
//! (`previous.camera !== next.camera`, `previous.viewport !==
//! next.viewport`), so here each carries a hidden identity: a camera is a
//! [`CameraObject`], the value plus the identity and member order of the
//! object literal that built it, and a viewport write counts.

use std::cell::Cell;
use std::rc::Rc;

use epdf_geometry::Size;
use epdf_stage_core::Camera;
use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize, Serializer};

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

/// The member order of a camera object literal. The `Camera` interface and
/// `zoomAround`/`panByScreen` write `x, y, zoom`; stage-core's clamp, scroll,
/// placement and anchor functions write `zoom, x, y`. A camera from outside
/// keeps the order it came in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CameraOrder(pub(crate) [CameraKey; 3]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CameraKey {
    X,
    Y,
    Zoom,
}

impl CameraOrder {
    /// `{x, y, zoom}`.
    pub const XY_ZOOM: CameraOrder = CameraOrder([CameraKey::X, CameraKey::Y, CameraKey::Zoom]);
    /// `{zoom, x, y}`.
    pub const ZOOM_XY: CameraOrder = CameraOrder([CameraKey::Zoom, CameraKey::X, CameraKey::Y]);

    /// The order of a camera's JSON members; `XY_ZOOM` for anything else.
    pub fn of_json(value: &serde_json::Value) -> CameraOrder {
        let Some(map) = value.as_object() else { return CameraOrder::XY_ZOOM };
        let keys: Vec<CameraKey> = map
            .keys()
            .filter_map(|k| match k.as_str() {
                "x" => Some(CameraKey::X),
                "y" => Some(CameraKey::Y),
                "zoom" => Some(CameraKey::Zoom),
                _ => None,
            })
            .collect();
        match keys.as_slice() {
            [a, b, c] => CameraOrder([*a, *b, *c]),
            _ => CameraOrder::XY_ZOOM,
        }
    }

    /// The camera printed in this order.
    pub fn view<'a>(&'a self, camera: &'a Camera) -> OrderedCamera<'a> {
        OrderedCamera { camera, order: self }
    }
}

/// A camera serialized in a given member order.
pub struct OrderedCamera<'a> {
    camera: &'a Camera,
    order: &'a CameraOrder,
}

impl Serialize for OrderedCamera<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(Some(3))?;
        for key in self.order.0 {
            match key {
                CameraKey::X => map.serialize_entry("x", &self.camera.x)?,
                CameraKey::Y => map.serialize_entry("y", &self.camera.y)?,
                CameraKey::Zoom => map.serialize_entry("zoom", &self.camera.zoom)?,
            }
        }
        map.end()
    }
}

thread_local! {
    static NEXT_OBJECT: Cell<u64> = const { Cell::new(1) };
}

fn next_object_id() -> u64 {
    NEXT_OBJECT.with(|next| {
        let id = next.get();
        next.set(id + 1);
        id
    })
}

/// A camera object: its value, the identity of the literal that built it
/// (two writes of the same object are no change), and its member order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraObject {
    pub camera: Camera,
    pub(crate) id: u64,
    pub(crate) order: CameraOrder,
}

impl CameraObject {
    /// A new camera object literal.
    pub fn new(camera: Camera, order: CameraOrder) -> Self {
        CameraObject { camera, id: next_object_id(), order }
    }

    /// `{x, y, zoom}`.
    pub fn xy_zoom(camera: Camera) -> Self {
        Self::new(camera, CameraOrder::XY_ZOOM)
    }

    /// `{zoom, x, y}`.
    pub fn zoom_xy(camera: Camera) -> Self {
        Self::new(camera, CameraOrder::ZOOM_XY)
    }

    /// Whether the two are the same object (`===`).
    pub fn same(&self, other: &CameraObject) -> bool {
        self.id == other.id
    }
}

/// The stage slice.
#[derive(Clone, Debug, PartialEq)]
pub struct StageState {
    pub camera: Camera,
    /// One-way render-commit latch: false until the initial placement's
    /// camera and settings writes have completed.
    pub placed: bool,
    /// False while the zoom is in motion, true once it has rested.
    pub camera_resting: bool,
    /// The container size the host reported, in viewport px.
    pub viewport: Size,
    /// Device px per view px.
    pub dpr: f64,
    /// The current page index, valid in both flows.
    pub cursor: usize,
    pub motion_cause: MotionCause,
    /// Names of the matching responsive rules, in source order.
    pub active_rules: Vec<String>,
    pub settings: StageSettings,
    pub(crate) camera_id: u64,
    pub(crate) camera_order: CameraOrder,
    /// Counts viewport writes: each is a new object.
    pub(crate) viewport_writes: u64,
}

impl StageState {
    /// The camera as the object the state holds.
    pub fn camera_object(&self) -> CameraObject {
        CameraObject { camera: self.camera, id: self.camera_id, order: self.camera_order }
    }

    /// The camera member order the state holds.
    pub fn camera_order(&self) -> CameraOrder {
        self.camera_order
    }
}

/// The state as the TypeScript object prints: the camera in its literal's
/// member order, then the fields in `initialStageState`'s order.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StateOut<'a> {
    camera: OrderedCamera<'a>,
    placed: bool,
    camera_resting: bool,
    viewport: Size,
    dpr: f64,
    cursor: usize,
    motion_cause: MotionCause,
    active_rules: &'a [String],
    #[serde(flatten)]
    settings: &'a StageSettings,
}

impl Serialize for StageState {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        StateOut {
            camera: self.camera_order.view(&self.camera),
            placed: self.placed,
            camera_resting: self.camera_resting,
            viewport: self.viewport,
            dpr: self.dpr,
            cursor: self.cursor,
            motion_cause: self.motion_cause,
            active_rules: &self.active_rules,
            settings: &self.settings,
        }
        .serialize(s)
    }
}

/// The JSON shape a state is read from (vectors, a host's restore).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StateJson {
    camera: Camera,
    placed: bool,
    camera_resting: bool,
    viewport: Size,
    dpr: f64,
    cursor: usize,
    motion_cause: MotionCause,
    active_rules: Vec<String>,
    #[serde(flatten)]
    settings: StageSettings,
}

impl StageState {
    /// Read a state from its JSON, keeping the camera's member order.
    pub fn from_json(value: &serde_json::Value) -> Result<StageState, serde_json::Error> {
        let json: StateJson = serde_json::from_value(value.clone())?;
        Ok(Self::from_decoded(json, CameraOrder::of_json(&value["camera"])))
    }

    /// A decoded state, its camera literal in `order`.
    pub(crate) fn from_decoded(json: StateJson, order: CameraOrder) -> StageState {
        StageState {
            camera: json.camera,
            placed: json.placed,
            camera_resting: json.camera_resting,
            viewport: json.viewport,
            dpr: json.dpr,
            cursor: json.cursor,
            motion_cause: json.motion_cause,
            active_rules: json.active_rules,
            settings: json.settings,
            camera_id: next_object_id(),
            camera_order: order,
            viewport_writes: 0,
        }
    }
}

/// `initialStageState(config)`: the defaults under the config's settings
/// overrides. The responsive rules are capability config, not state.
pub fn initial_stage_state(overrides: &StageSettingsPatch) -> StageState {
    let camera = CameraObject::xy_zoom(Camera { x: 0.0, y: 0.0, zoom: 1.0 });
    StageState {
        camera: camera.camera,
        placed: false,
        camera_resting: true,
        viewport: Size { width: 0.0, height: 0.0 },
        dpr: 1.0,
        cursor: 0,
        motion_cause: MotionCause::User,
        active_rules: Vec::new(),
        settings: merge_settings(&default_settings(), overrides),
        camera_id: camera.id,
        camera_order: camera.order,
        viewport_writes: 0,
    }
}

fn with(state: &Rc<StageState>, change: impl FnOnce(&mut StageState)) -> Rc<StageState> {
    let mut next = (**state).clone();
    change(&mut next);
    Rc::new(next)
}

/// `Object.is` on numbers: `NaN` is itself, `0` is not `-0`.
pub(crate) fn object_is(a: f64, b: f64) -> bool {
    if a.is_nan() && b.is_nan() {
        return true;
    }
    a == b && a.is_sign_negative() == b.is_sign_negative()
}

pub fn set_camera(state: &Rc<StageState>, camera: CameraObject) -> Rc<StageState> {
    if state.camera_id == camera.id {
        return state.clone();
    }
    with(state, |next| {
        next.camera = camera.camera;
        next.camera_id = camera.id;
        next.camera_order = camera.order;
    })
}

pub fn set_camera_resting(state: &Rc<StageState>, resting: bool) -> Rc<StageState> {
    if state.camera_resting == resting {
        return state.clone();
    }
    with(state, |next| next.camera_resting = resting)
}

pub fn mark_placed(state: &Rc<StageState>) -> Rc<StageState> {
    if state.placed {
        return state.clone();
    }
    with(state, |next| next.placed = true)
}

/// A reported viewport is always a new object, so it always lands.
pub fn set_viewport(state: &Rc<StageState>, viewport: Size) -> Rc<StageState> {
    with(state, |next| {
        next.viewport = viewport;
        next.viewport_writes += 1;
    })
}

pub fn set_dpr(state: &Rc<StageState>, dpr: f64) -> Rc<StageState> {
    // `state.dpr === dpr`.
    if state.dpr == dpr {
        return state.clone();
    }
    with(state, |next| next.dpr = dpr)
}

pub fn set_cursor(state: &Rc<StageState>, cursor: usize) -> Rc<StageState> {
    if state.cursor == cursor {
        return state.clone();
    }
    with(state, |next| next.cursor = cursor)
}

pub fn set_motion_cause(state: &Rc<StageState>, cause: MotionCause) -> Rc<StageState> {
    if state.motion_cause == cause {
        return state.clone();
    }
    with(state, |next| next.motion_cause = cause)
}

/// Merge a settings patch; absent values are skipped. A key lands when it
/// is not `Object.is` the current value: a primitive by value, an object
/// setting (a page frame, an alignment, a zoom spec, a `{px}` gap) always,
/// since a patch carries a new object.
pub fn patch_settings(state: &Rc<StageState>, patch: &StageSettingsPatch) -> Rc<StageState> {
    if !crate::settings::patch_changes(&state.settings, patch) {
        return state.clone();
    }
    with(state, |next| next.settings = merge_settings(&state.settings, patch))
}

/// An active-rule list is always a new array, so it always lands.
pub fn set_active_rules(state: &Rc<StageState>, active: Vec<String>) -> Rc<StageState> {
    with(state, |next| next.active_rules = active)
}
