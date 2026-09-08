//! The brain: model, messages, effects, `update`, `scene`, hit-testing.
//!
//! Pure Rust with no `#[wasm_bindgen]` anywhere. That is what lets `cargo test`
//! exercise it natively against the frozen vectors, and it is what a Swift or
//! Kotlin binding would sit on, rather than on the JS glue.

use serde::{Deserialize, Serialize};

use crate::geom::*;
// Messages carry the same `Point` the geometry layer uses; keep it reachable
// from `model` so message builders need only one import path.
pub use crate::geom::Point;

pub const DEFAULT_W: f64 = 90.0;
pub const DEFAULT_H: f64 = 64.0;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct ViewEnv {
    pub zoom: f64,
    pub rotation: f64,
}

#[derive(Clone, Debug)]
pub struct Shape {
    pub handle: u32,
    pub rect: Rect,
    pub rot: f64,
    pub anchored: bool,
    pub color: String,
}

impl Shape {
    /// Parity with typescript-baseline and rust-geometry-only, where ids were `s1`, `s2`, ...
    pub fn id(&self) -> String {
        format!("s{}", self.handle)
    }
}

#[derive(Clone, Debug)]
pub struct Drag {
    pub handle: u32,
    pub from: Point,
    pub origin: Rect,
}

#[derive(Clone, Debug, Default)]
pub struct Model {
    pub shapes: Vec<Shape>,
    pub selected: Option<u32>,
    pub drag: Option<Drag>,
    pub seq: u32,
}

/// The message union, tagged to match the TypeScript one exactly.
///
/// `#[serde(tag = "t", rename_all = "camelCase")]` is what makes
/// `{ t: 'pointerDown', at, view }` from TypeScript deserialize straight into
/// `Msg::PointerDown`. Getting this wrong is the most common way a port breaks,
/// and it breaks at runtime rather than at compile time, which is the argument
/// for generating the TypeScript side from these definitions instead of writing
/// it twice.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "camelCase")]
pub enum Msg {
    Add { at: Point, color: String },
    Select { id: Option<String> },
    PointerDown { at: Point, view: ViewEnv },
    PointerMove { at: Point },
    PointerUp,
    SetAnchored { id: String, anchored: bool },
    SetRot { id: String, rot: f64 },
    Delete { id: String },
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "camelCase")]
pub enum Effect {
    Log { message: String },
    Persist { ids: Vec<String> },
}

fn handle_of(id: &str) -> Option<u32> {
    id.strip_prefix('s')?.parse().ok()
}

// --- the anchor projection -------------------------------------------------

pub fn anchor_factors(anchored: bool, view: ViewEnv) -> Option<(f64, f64)> {
    if !anchored {
        return None;
    }
    let z = if view.zoom == 0.0 { 1.0 } else { view.zoom };
    let s = z.max(1.0);
    let r = normalize_deg(view.rotation);
    if s != 1.0 || r != 0.0 {
        Some((s, r))
    } else {
        None
    }
}

pub fn project_quad(q: Quad, anchored: bool, view: ViewEnv) -> Quad {
    let Some((s, r)) = anchor_factors(anchored, view) else {
        return q;
    };
    let b = quad_bounds(q);
    let anchor = Point { x: b.x, y: b.y };
    let mut out = q;
    if s != 1.0 {
        out = quad_scale_about(out, anchor, 1.0 / s);
    }
    if r != 0.0 {
        out = quad_rotate_about(out, anchor, normalize_deg(-r));
    }
    out
}

pub fn shape_quad(shape: &Shape, view: ViewEnv) -> Quad {
    project_quad(rect_quad(shape.rect, shape.rot), shape.anchored, view)
}

pub fn is_projected(shape: &Shape, view: ViewEnv) -> bool {
    anchor_factors(shape.anchored, view).is_some()
}

// --- hit-testing -----------------------------------------------------------

/// Topmost shape under `at`, or None. Reverse order: last drawn wins.
pub fn hit_test(model: &Model, at: Point, view: ViewEnv) -> Option<u32> {
    model
        .shapes
        .iter()
        .rev()
        .find(|s| point_in_quad(at, shape_quad(s, view)))
        .map(|s| s.handle)
}

// --- update ----------------------------------------------------------------

/// Mutates in place and reports whether anything changed, plus the effects.
///
/// Note the shape of this signature versus the TypeScript original's
/// `(Model, Msg) -> [Model, Effect[]]`. Immutability was buying reference
/// equality for React, and reference equality cannot survive the boundary
/// anyway. So the Rust core mutates and publishes a version number instead,
/// which is the thing `useSyncExternalStore` actually wants.
pub fn update(model: &mut Model, msg: Msg) -> (bool, Vec<Effect>) {
    match msg {
        Msg::Add { at, color } => {
            model.seq += 1;
            let handle = model.seq;
            model.shapes.push(Shape {
                handle,
                rect: Rect {
                    x: at.x - DEFAULT_W / 2.0,
                    y: at.y - DEFAULT_H / 2.0,
                    width: DEFAULT_W,
                    height: DEFAULT_H,
                },
                rot: 0.0,
                anchored: false,
                color,
            });
            model.selected = Some(handle);
            let id = format!("s{handle}");
            (
                true,
                vec![
                    Effect::Log {
                        message: format!("added {id}"),
                    },
                    Effect::Persist { ids: vec![id] },
                ],
            )
        }

        Msg::Select { id } => {
            // typescript-baseline stores whatever string it is handed. This
            // model holds a `u32` handle, so an id it cannot represent is a
            // caller bug; report it rather than rewriting it into a deselect.
            let next = match id.as_deref() {
                None => None,
                Some(raw) => match handle_of(raw) {
                    Some(handle) => Some(handle),
                    None => {
                        return (
                            false,
                            vec![Effect::Log {
                                message: format!("select: ignored malformed id {raw:?}"),
                            }],
                        )
                    }
                },
            };
            let changed = next != model.selected;
            model.selected = next;
            (changed, vec![])
        }

        Msg::PointerDown { at, view } => {
            let hit = hit_test(model, at, view);
            match hit {
                None => {
                    let changed = model.selected.is_some();
                    model.selected = None;
                    (changed, vec![])
                }
                Some(handle) => {
                    let origin = model
                        .shapes
                        .iter()
                        .find(|s| s.handle == handle)
                        .map(|s| s.rect)
                        .expect("hit_test returned a live handle");
                    model.selected = Some(handle);
                    model.drag = Some(Drag {
                        handle,
                        from: at,
                        origin,
                    });
                    (true, vec![])
                }
            }
        }

        Msg::PointerMove { at } => {
            let Some(drag) = model.drag.clone() else {
                return (false, vec![]);
            };
            let Some(shape) = model.shapes.iter_mut().find(|s| s.handle == drag.handle) else {
                return (false, vec![]);
            };
            // The delta is NOT scaled by zoom, not even for an anchored shape.
            // `project_quad` is a similarity about the shape's own unprojected
            // bounds top-left, which makes that point a fixed point of the
            // projection: the projected quad sits at the stored rect position
            // at every zoom. Pointer coordinates already live in that space,
            // so a 1:1 delta is what makes the shape follow the cursor.
            // Scaling by max(zoom, 1) moved it that many times too far and the
            // next pointer_down missed it.
            shape.rect = Rect {
                x: drag.origin.x + (at.x - drag.from.x),
                y: drag.origin.y + (at.y - drag.from.y),
                ..drag.origin
            };
            (true, vec![])
        }

        Msg::PointerUp => match model.drag.take() {
            None => (false, vec![]),
            Some(d) => (
                true,
                vec![Effect::Persist {
                    ids: vec![format!("s{}", d.handle)],
                }],
            ),
        },

        Msg::SetAnchored { id, anchored } => {
            let Some(handle) = handle_of(&id) else {
                return (false, vec![]);
            };
            match model.shapes.iter_mut().find(|s| s.handle == handle) {
                None => (false, vec![]),
                Some(shape) => {
                    shape.anchored = anchored;
                    (
                        true,
                        vec![
                            Effect::Log {
                                message: format!("{id} anchored={anchored}"),
                            },
                            Effect::Persist { ids: vec![id] },
                        ],
                    )
                }
            }
        }

        Msg::SetRot { id, rot } => {
            let Some(handle) = handle_of(&id) else {
                return (false, vec![]);
            };
            match model.shapes.iter_mut().find(|s| s.handle == handle) {
                None => (false, vec![]),
                Some(shape) => {
                    shape.rot = rot;
                    (true, vec![Effect::Persist { ids: vec![id] }])
                }
            }
        }

        Msg::Delete { id } => {
            let Some(handle) = handle_of(&id) else {
                return (false, vec![]);
            };
            let before = model.shapes.len();
            model.shapes.retain(|s| s.handle != handle);
            if model.shapes.len() == before {
                return (false, vec![]);
            }
            if model.selected == Some(handle) {
                model.selected = None;
            }
            if model.drag.as_ref().is_some_and(|d| d.handle == handle) {
                model.drag = None;
            }
            (
                true,
                vec![Effect::Log {
                    message: format!("deleted {id}"),
                }],
            )
        }
    }
}

// --- the display list ------------------------------------------------------

/// Floats per display item: handle, 8 quad coordinates, flags.
pub const ITEM_STRIDE: usize = 10;
pub const FLAG_SELECTED: f64 = 1.0;
pub const FLAG_PROJECTED: f64 = 2.0;

/// The per-frame projection, encoded as ONE flat buffer.
///
/// This is the whole point of rust-geometry-and-state. rust-geometry-only crossed the boundary seven times per
/// anchored shape; this crosses once per frame no matter how many shapes there
/// are, because the buffer is built entirely on this side.
///
/// Colors and ids are deliberately absent: they change when the shape set
/// changes, not when the view does, so they live in `shape_table` and the
/// TypeScript side caches them against the table version.
pub fn scene_buffer(model: &Model, view: ViewEnv) -> Vec<f64> {
    let mut out = Vec::with_capacity(1 + model.shapes.len() * ITEM_STRIDE);
    out.push(model.shapes.len() as f64);
    for shape in &model.shapes {
        let q = shape_quad(shape, view);
        out.push(shape.handle as f64);
        for p in q.0 {
            out.push(p.x);
            out.push(p.y);
        }
        let mut flags = 0.0;
        if model.selected == Some(shape.handle) {
            flags += FLAG_SELECTED;
        }
        if is_projected(shape, view) {
            flags += FLAG_PROJECTED;
        }
        out.push(flags);
    }
    out
}

#[derive(Serialize)]
pub struct ShapeRow {
    pub handle: u32,
    pub id: String,
    pub color: String,
    pub rot: f64,
    pub anchored: bool,
}

/// The slowly-changing half of the model. Fetched only when the table version
/// moves, so every field here has to be covered by `affects_shape_table`.
pub fn shape_table(model: &Model) -> Vec<ShapeRow> {
    model
        .shapes
        .iter()
        .map(|s| ShapeRow {
            handle: s.handle,
            id: s.id(),
            color: s.color.clone(),
            rot: s.rot,
            anchored: s.anchored,
        })
        .collect()
}

/// Whether a message can change anything `shape_table` returns.
///
/// The TypeScript side refetches the table only when the version this drives
/// moves, so a field in `ShapeRow` that is not covered here goes stale on the
/// other side and the UI renders the old value until something else bumps it.
/// `rot` and `anchored` are exactly that case: the first version of this
/// compared `shapes.len()` instead, so `SetRot` and `SetAnchored` were
/// invisible to the cache.
///
/// Deliberately a match on the message rather than a hash of the rows: it is
/// O(1), and it fails loudly when a variant is added because the match is
/// exhaustive.
pub fn affects_shape_table(msg: &Msg) -> bool {
    match msg {
        // Adds and removes rows.
        Msg::Add { .. } | Msg::Delete { .. } => true,
        // Writes a `ShapeRow` field.
        Msg::SetRot { .. } | Msg::SetAnchored { .. } => true,
        // Selection is a scene flag and a drag only moves the rect; neither is
        // in the table.
        Msg::Select { .. } | Msg::PointerDown { .. } | Msg::PointerMove { .. } | Msg::PointerUp => {
            false
        }
    }
}
