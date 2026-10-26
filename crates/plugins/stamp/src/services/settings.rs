//! The stamp settings in the kernel's settings store.
//!
//! The kernel keeps a registration's settings as one JSON value
//! (`epdf_kernel::settings`). `previewWidth` and `dynamic` are plain data;
//! `assetEngine` is an engine or a function, which JSON can't hold. It is
//! stored as a handle, `{"$assetEngine": n}`, into a table the registration
//! owns (one per `stamp_plugin(config)`, shared by its instances as the
//! store is), so the same engine reads back as the same value, and the
//! asset engine service can tell a changed setting from the one it
//! resolved (`resolved.from !== configured`). `assetEngine` is declared
//! `whole`, as `stamp.plugin.ts` declares it: a change replaces it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use serde_json::{Map, Value, json};

use crate::contract::{StampAssetEngine, StampConfig, StampSettings, stamp_defaults};

/// The reserved key of a stored engine's handle.
const ENGINE_KEY: &str = "$assetEngine";

#[derive(Default)]
pub struct EngineTable {
    engines: RefCell<Vec<(u64, StampAssetEngine)>>,
    next: Cell<u64>,
}

impl EngineTable {
    fn handle(&self, engine: &StampAssetEngine) -> u64 {
        if let Some((n, _)) = self.engines.borrow().iter().find(|(_, e)| e.same(engine)) {
            return *n;
        }
        let n = self.next.get();
        self.next.set(n + 1);
        self.engines.borrow_mut().push((n, engine.clone()));
        n
    }

    fn engine_json(&self, engine: &Option<StampAssetEngine>) -> Value {
        match engine {
            None => Value::Null,
            Some(engine) => json!({ ENGINE_KEY: self.handle(engine) }),
        }
    }

    /// The stored setting's handle: `None` for `null`.
    pub fn handle_of(value: &Value) -> Option<u64> {
        value.get(ENGINE_KEY).and_then(Value::as_u64)
    }

    fn engine_of(&self, value: &Value) -> Option<StampAssetEngine> {
        let n = Self::handle_of(value)?;
        let engine = self.engines.borrow().iter().find(|(m, _)| *m == n).map(|(_, e)| e.clone());
        debug_assert!(engine.is_some(), "[stamp] stored asset engine handle {n} names no engine");
        engine
    }

    /// The JSON a `DeepPartial<StampSettings>` change is in the store: only
    /// the members it names.
    pub fn encode(&self, config: &StampConfig) -> Value {
        let mut out = Map::new();
        if let Some(engine) = &config.asset_engine {
            out.insert("assetEngine".into(), self.engine_json(engine));
        }
        if let Some(width) = config.preview_width {
            out.insert("previewWidth".into(), json!(width));
        }
        if let Some(dynamic) = config.dynamic {
            out.insert("dynamic".into(), json!(dynamic));
        }
        Value::Object(out)
    }

    /// The settings a stored JSON value holds.
    pub fn decode(&self, value: &Value) -> StampSettings {
        let defaults = stamp_defaults();
        StampSettings {
            asset_engine: value.get("assetEngine").and_then(|v| self.engine_of(v)),
            preview_width: value
                .get("previewWidth")
                .and_then(Value::as_f64)
                .unwrap_or(defaults.preview_width),
            dynamic: value.get("dynamic").and_then(Value::as_bool).unwrap_or(defaults.dynamic),
        }
    }
}

/// `STAMP_DEFAULTS` in the store.
pub fn defaults_json() -> Value {
    let defaults = stamp_defaults();
    json!({
        "assetEngine": null,
        "previewWidth": defaults.preview_width,
        "dynamic": defaults.dynamic,
    })
}

/// The registration's table and its registered settings.
pub fn register(config: &StampConfig) -> (Rc<EngineTable>, Value) {
    let table = Rc::new(EngineTable::default());
    let registered = table.encode(config);
    (table, registered)
}
