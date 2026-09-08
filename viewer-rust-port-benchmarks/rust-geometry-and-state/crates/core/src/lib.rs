//! `@poc/core`: geometry, the shape brain, and the store, all in Rust.
//!
//! The boundary is deliberately narrow. TypeScript can do exactly four things:
//! dispatch a message, read a version number, ask for one frame's display list,
//! and hit-test a point. Everything else stays on this side.
//!
//! That narrowness is the design. rust-geometry-only exported a dozen geometry helpers and
//! paid seven crossings per anchored shape per frame. This exports four calls
//! and pays one per frame, because the loop over shapes runs in Rust.

use std::cell::RefCell;

use wasm_bindgen::prelude::*;

pub mod geom;
pub mod model;

use crate::model::{
    affects_shape_table, hit_test, scene_buffer, shape_table, update, Model, Msg, Point, ViewEnv,
    FLAG_PROJECTED, FLAG_SELECTED, ITEM_STRIDE,
};

/// The layout of `scene()`'s buffer, handed to TypeScript so the decoder is not
/// a magic number on the other side.
///
/// All three are read once at store construction (`packages/core/src/store.ts`)
/// and never per frame, so the drift they prevent costs three crossings for the
/// lifetime of a store.
#[wasm_bindgen(js_name = itemStride)]
pub fn item_stride() -> usize {
    ITEM_STRIDE
}

#[wasm_bindgen(js_name = flagSelected)]
pub fn flag_selected() -> f64 {
    FLAG_SELECTED
}

#[wasm_bindgen(js_name = flagProjected)]
pub fn flag_projected() -> f64 {
    FLAG_PROJECTED
}

/// The Rust-owned store.
///
/// State lives here. TypeScript holds nothing but this handle and a cache.
#[wasm_bindgen]
pub struct Core {
    model: Model,
    /// Bumped on every change. This is what `useSyncExternalStore` subscribes
    /// to, because the model itself lives in linear memory and there is no
    /// object identity for React to compare.
    version: u32,
    /// Bumped only when something `shape_table()` returns changes, so the
    /// TypeScript side knows when its row cache is stale. Panning, zooming,
    /// selecting and dragging never move it.
    ///
    /// This used to track the shape COUNT, which was wrong: `rot` and
    /// `anchored` are table fields that no count change accompanies, so
    /// `SetRot` and `SetAnchored` left the cache serving stale rows.
    /// `affects_shape_table` is the list, and it lives next to `ShapeRow`.
    table: u32,
    /// Per-Core, not a module thread_local.
    ///
    /// A module-level slot is one slot for the whole wasm instance: a second
    /// `createCoreStore({log})` silently replaced the first store's logger, and
    /// `destroy()` calling `core.free()` left the instance holding a
    /// `js_sys::Function` that closed over an unmounted component's
    /// `setHostLines`. Owned by the Core, it goes away when the Core does.
    ///
    /// `RefCell` and not a plain field, so `set_host_logger` can keep `&self`.
    /// A `&mut self` export holds wasm-bindgen's borrow for the whole call, and
    /// calling into JS under it is the recursive-borrow failure documented on
    /// `DispatchResult` below.
    host_log: RefCell<Option<js_sys::Function>>,
}

/// What `dispatch` reports back. `changed` is what drives invalidation on the
/// TypeScript side.
///
/// The core deliberately does NOT hold a JS change listener and call it from
/// inside `dispatch`. It used to, and that was a bug: `dispatch` takes
/// `&mut self`, so wasm-bindgen holds a mutable borrow for its whole duration,
/// and the listener synchronously re-rendered React, which read `core.version`
/// and hit a recursive borrow. The error surfaced much later as
/// "attempted to take ownership of Rust value while it was borrowed" on
/// `free()`.
///
/// The rule that falls out: a `&mut self` export must not call back into JS.
/// Return what happened and let the host notify after the call has returned.
#[derive(serde::Serialize)]
struct DispatchResult {
    changed: bool,
    effects: Vec<crate::model::Effect>,
}

impl Default for Core {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl Core {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Core {
        Core {
            model: Model::default(),
            version: 1,
            table: 1,
            host_log: RefCell::new(None),
        }
    }

    /// Rust -> TypeScript, for host services the core cannot implement itself.
    ///
    /// `&self`, so the callback may READ this Core (`version`, `scene`, ...).
    /// It must not dispatch: that needs `&mut self` and would be a recursive
    /// borrow. Every host callback in a real core wants that constraint written
    /// down next to it.
    #[wasm_bindgen(js_name = setHostLogger)]
    pub fn set_host_logger(&self, f: &js_sys::Function) {
        *self.host_log.borrow_mut() = Some(f.clone());
        self.host_log(&format!("poc-core {} ready", env!("CARGO_PKG_VERSION")));
    }

    /// Drops the host callback. The store calls this from `destroy()`, before
    /// `free()`, so a torn-down store stops holding the closure it registered
    /// even if something else still holds the Core.
    #[wasm_bindgen(js_name = clearHostLogger)]
    pub fn clear_host_logger(&self) {
        *self.host_log.borrow_mut() = None;
    }

    /// The borrow ends BEFORE the call into JS, and that ordering is the point.
    ///
    /// Holding a `RefCell` borrow across a call into JavaScript is how a Rust
    /// core kills itself: the host callback can re-enter Rust, and
    /// `set_host_logger` takes `borrow_mut()`. A host logger that re-registered
    /// a logger, or a second `createCoreStore()` running inside one, panicked
    /// with `BorrowMutError`, and a wasm panic leaves the instance dead for the
    /// rest of the page.
    fn host_log(&self, message: &str) {
        let logger = self.host_log.borrow().clone();
        if let Some(f) = logger {
            let _ = f.call1(&JsValue::NULL, &JsValue::from_str(message));
        }
    }

    #[wasm_bindgen(getter)]
    pub fn version(&self) -> u32 {
        self.version
    }

    #[wasm_bindgen(getter, js_name = tableVersion)]
    pub fn table_version(&self) -> u32 {
        self.table
    }

    #[wasm_bindgen(getter, js_name = shapeCount)]
    pub fn shape_count(&self) -> usize {
        self.model.shapes.len()
    }

    /// Monotonic: every `Add` bumps it and nothing lowers it. This is what the
    /// demo's colour cycle needs, and `shapeCount` is not: a delete lowers the
    /// count, so cycling on the count reissues the colour just handed out.
    /// typescript-baseline and rust-geometry-only cycle on `model.seq`, which is this.
    #[wasm_bindgen(getter)]
    pub fn seq(&self) -> u32 {
        self.model.seq
    }

    #[wasm_bindgen(getter, js_name = selectedHandle)]
    pub fn selected_handle(&self) -> Option<u32> {
        self.model.selected
    }

    #[wasm_bindgen(getter, js_name = isDragging)]
    pub fn is_dragging(&self) -> bool {
        self.model.drag.is_some()
    }

    /// One message in, `{ changed, effects }` out. One crossing per user action,
    /// and no callback into JS while the borrow is held.
    #[wasm_bindgen]
    pub fn dispatch(&mut self, msg: JsValue) -> Result<JsValue, JsValue> {
        let parsed: Msg = serde_wasm_bindgen::from_value(msg)
            .map_err(|e| JsValue::from_str(&format!("bad message: {e}")))?;

        let touches_table = affects_shape_table(&parsed);
        let (changed, effects) = update(&mut self.model, parsed);

        if changed {
            self.version = self.version.wrapping_add(1);
            if touches_table {
                self.table = self.table.wrapping_add(1);
            }
        }

        Ok(serde_wasm_bindgen::to_value(&DispatchResult { changed, effects })?)
    }

    /// ONE crossing for the whole frame. The loop over shapes is in Rust.
    #[wasm_bindgen]
    pub fn scene(&self, zoom: f64, rotation: f64) -> Vec<f64> {
        scene_buffer(&self.model, ViewEnv { zoom, rotation })
    }

    /// The slowly-changing half: ids, colors, flags. Read on a topology bump.
    #[wasm_bindgen(js_name = shapeTable)]
    pub fn shape_table(&self) -> Result<JsValue, JsValue> {
        Ok(serde_wasm_bindgen::to_value(&shape_table(&self.model))?)
    }

    /// The handle under the point, or -1. Hit-testing walks every shape, and it
    /// walks them here rather than crossing once per shape.
    #[wasm_bindgen(js_name = hitTest)]
    pub fn hit_test(&self, x: f64, y: f64, zoom: f64, rotation: f64) -> i32 {
        hit_test(
            &self.model,
            Point { x, y },
            ViewEnv { zoom, rotation },
        )
        .map_or(-1, |h| h as i32)
    }

    /// Drops every shape. Exists so the demo can rebuild a large scene for the
    /// crossing comparison without a hundred dispatches.
    #[wasm_bindgen(js_name = seedShapes)]
    pub fn seed_shapes(&mut self, count: u32, anchored: bool) {
        self.model = Model::default();
        for i in 0..count {
            let col = (i % 10) as f64;
            let row = (i / 10) as f64;
            update(
                &mut self.model,
                Msg::Add {
                    at: Point {
                        x: 60.0 + col * 52.0,
                        y: 60.0 + row * 46.0,
                    },
                    color: PALETTE[(i as usize) % PALETTE.len()].to_string(),
                },
            );
            if anchored {
                let id = format!("s{}", self.model.seq);
                update(&mut self.model, Msg::SetAnchored { id, anchored: true });
            }
        }
        self.model.selected = None;
        self.version = self.version.wrapping_add(1);
        self.table = self.table.wrapping_add(1);
        // No host_log here. `seed_shapes` takes `&mut self`, and calling into JS
        // under a mutable borrow is the trap documented on DispatchResult. The
        // wrapper logs this instead.
    }
}

const PALETTE: [&str; 6] = ["#c0392b", "#2980b9", "#27ae60", "#8e44ad", "#d35400", "#16a085"];
