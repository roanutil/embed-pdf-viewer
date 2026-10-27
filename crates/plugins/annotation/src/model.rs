//! `model.ts`: the plugin's state and its transitions.
//!
//! Three kinds of data make up what the user sees, and each has one owner:
//! the engine's records (the records mirror, `sync/records.rs`), this
//! session's changes the engine hasn't answered (the kernel's change queue,
//! which the mirror predicts) and the session (selection, gestures,
//! settings), produced by the core's `update`. The view (`read/view.rs`)
//! composes the session with the records as the mirror shows them.
//!
//! The state is held as an `Rc`, and every transition returns the same `Rc`
//! when nothing changes: the kernel publishes a state change by identity,
//! as the TypeScript does.

use std::rc::Rc;

use epdf_annotation_core::types::{EditableKinds, Id, Point, Session, SnapSettings};
use epdf_annotation_core::update::initial_model;
use indexmap::IndexMap;
use serde::Serialize;
use serde_json::{Map, Value};

use crate::js::{Object, num};
use crate::rich_text::TextSelection;

/// `GhostPointer`: where the active tool's ghost is.
#[derive(Clone, Debug, PartialEq)]
pub struct GhostPointer {
    pub tool_id: String,
    pub page: Value,
    pub point: Point,
    /// The page's display rotation at the pointer.
    pub display_rotation: Option<f64>,
    /// The page's zoom at the pointer.
    pub zoom: Option<f64>,
}

impl GhostPointer {
    pub fn to_json(&self) -> Value {
        let mut out = Map::new();
        out.insert("toolId".into(), Value::String(self.tool_id.clone()));
        out.insert("page".into(), self.page.clone());
        out.insert("point".into(), point_json(self.point));
        if let Some(r) = self.display_rotation {
            out.insert("displayRotation".into(), num(r));
        }
        if let Some(z) = self.zoom {
            out.insert("zoom".into(), num(z));
        }
        Value::Object(out)
    }
}

/// `ForeignPlacement`: a sibling plugin's placement gesture with one of this
/// plugin's tools.
#[derive(Clone, Debug, PartialEq)]
pub struct ForeignPlacement {
    pub tool_id: String,
    pub page: Value,
    pub from: Point,
    pub to: Point,
}

impl ForeignPlacement {
    pub fn to_json(&self) -> Value {
        let mut out = Map::new();
        out.insert("toolId".into(), Value::String(self.tool_id.clone()));
        out.insert("page".into(), self.page.clone());
        out.insert("from".into(), point_json(self.from));
        out.insert("to".into(), point_json(self.to));
        Value::Object(out)
    }
}

pub fn point_json(p: Point) -> Value {
    let mut out = Map::new();
    out.insert("x".into(), num(p.x));
    out.insert("y".into(), num(p.y));
    Value::Object(out)
}

/// `AnnotationState`.
#[derive(Clone, Debug)]
pub struct AnnotationState {
    /// The core's session: selection, hover, the gesture in progress, tool settings.
    pub session: Session,
    /// Records this session renders from their description instead of the
    /// engine's raster: the ones it edited or created, in the order they
    /// joined (a JavaScript object's insertion order).
    pub vector: Rc<IndexMap<Id, ()>>,
    /// Where the active tool's ghost is.
    pub ghost_at: Option<Rc<GhostPointer>>,
    /// A sibling plugin's placement in progress with one of this plugin's tools.
    pub placing: Option<Rc<ForeignPlacement>>,
    /// The text editor's selection inside the annotation being edited.
    pub text_selection: Option<Rc<TextSelection>>,
}

impl AnnotationState {
    pub fn to_json(&self) -> Value {
        let mut out = Map::new();
        out.insert("session".into(), Value::Object(self.session.to_object()));
        let vector: Object = self.vector.keys().map(|k| (k.clone(), Value::Bool(true))).collect();
        out.insert("vector".into(), Value::Object(vector));
        out.insert("ghostAt".into(), self.ghost_at.as_ref().map_or(Value::Null, |g| g.to_json()));
        out.insert("placing".into(), self.placing.as_ref().map_or(Value::Null, |p| p.to_json()));
        out.insert(
            "textSelection".into(),
            self.text_selection.as_ref().map_or(Value::Null, |t| t.to_json()),
        );
        Value::Object(out)
    }

    /// Whether this record renders live.
    pub fn is_vector(&self, id: &str) -> bool {
        self.vector.contains_key(id)
    }
}

impl Serialize for AnnotationState {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_json().serialize(serializer)
    }
}

/// `initialAnnotationState()`. The session's snapping and editable rule come
/// from the settings (`with_snap`, `with_editable`).
pub fn initial_annotation_state() -> AnnotationState {
    AnnotationState {
        session: initial_model().session(),
        vector: Rc::new(IndexMap::new()),
        ghost_at: None,
        placing: None,
        text_selection: None,
    }
}

type State = Rc<AnnotationState>;

fn with(state: &State, change: impl FnOnce(&mut AnnotationState)) -> State {
    let mut next = (**state).clone();
    change(&mut next);
    Rc::new(next)
}

/// `withSnap(state, snap)`: the session snaps as the settings say.
pub fn with_snap(state: &State, snap: &Rc<SnapSettings>) -> State {
    if Rc::ptr_eq(&state.session.snap, snap) {
        return state.clone();
    }
    with(state, |s| s.session.snap = snap.clone())
}

/// `withEditable(state, editable)`: a boolean rule compares by value, a list by identity.
pub fn with_editable(state: &State, editable: &Rc<EditableKinds>) -> State {
    let same = match (&*state.session.editable, &**editable) {
        (EditableKinds::All(a), EditableKinds::All(b)) => a == b,
        _ => Rc::ptr_eq(&state.session.editable, editable),
    };
    if same {
        return state.clone();
    }
    with(state, |s| s.session.editable = editable.clone())
}

/// `withSession(state, session, vector)`: record one message's session; the
/// records it changed and drew live render live from now on.
pub fn with_session(state: &State, session: &Session, vector: &[Id]) -> State {
    let next = if state.session.same(session) {
        state.clone()
    } else {
        with(state, |s| s.session = session.clone())
    };
    prefer_vector(&next, vector)
}

/// `withObjectNumbers(state, numbers)`: the session holds these numbers too.
pub fn with_object_numbers(state: &State, numbers: &[f64]) -> State {
    if numbers.is_empty() {
        return state.clone();
    }
    with(state, |s| {
        let mut all = (*s.session.object_numbers).clone();
        all.extend_from_slice(numbers);
        s.session.object_numbers = Rc::new(all);
    })
}

/// `withoutObjectNumbers(state, lost)`.
pub fn without_object_numbers(state: &State, lost: &[f64]) -> State {
    let kept: Vec<f64> =
        state.session.object_numbers.iter().copied().filter(|n| !lost.contains(n)).collect();
    if kept.len() == state.session.object_numbers.len() {
        return state.clone();
    }
    with(state, |s| s.session.object_numbers = Rc::new(kept))
}

/// `preferVector(state, ids)`: render these records live from their description.
pub fn prefer_vector(state: &State, ids: &[Id]) -> State {
    let added: Vec<&Id> = ids.iter().filter(|id| !state.vector.contains_key(*id)).collect();
    if added.is_empty() {
        return state.clone();
    }
    with(state, |s| {
        let mut vector = (*s.vector).clone();
        for id in added {
            vector.insert(id.clone(), ());
        }
        s.vector = Rc::new(vector);
    })
}

/// `preferBaked(state, ids)`: render these records from the engine's raster again.
pub fn prefer_baked(state: &State, ids: &[Id]) -> State {
    if !ids.iter().any(|id| state.vector.contains_key(id)) {
        return state.clone();
    }
    with(state, |s| {
        let mut vector = (*s.vector).clone();
        for id in ids {
            vector.shift_remove(id);
        }
        s.vector = Rc::new(vector);
    })
}

/// `setGhostAt(state, ghostAt)`, by identity.
pub fn set_ghost_at(state: &State, ghost_at: Option<Rc<GhostPointer>>) -> State {
    if opt_ptr_eq(&state.ghost_at, &ghost_at) {
        return state.clone();
    }
    with(state, |s| s.ghost_at = ghost_at)
}

/// `setPlacing(state, placing)`, by identity.
pub fn set_placing(state: &State, placing: Option<Rc<ForeignPlacement>>) -> State {
    if opt_ptr_eq(&state.placing, &placing) {
        return state.clone();
    }
    with(state, |s| s.placing = placing)
}

/// `setTextSelection(state, textSelection)`, by identity.
pub fn set_text_selection(state: &State, selection: Option<Rc<TextSelection>>) -> State {
    if opt_ptr_eq(&state.text_selection, &selection) {
        return state.clone();
    }
    with(state, |s| s.text_selection = selection)
}

fn opt_ptr_eq<T>(a: &Option<Rc<T>>, b: &Option<Rc<T>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}
