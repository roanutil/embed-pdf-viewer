//! The interaction settings in the kernel's settings store.
//!
//! The kernel keeps a registration's settings as one JSON value
//! (`epdf_kernel::settings`), and a tool can carry pointer callbacks, which
//! JSON can't hold. So a tool given through the typed API is stored as a
//! handle, `{"$tool": n}`, into a table that belongs to the registration
//! (one per `interaction_plugin(config)`), shared by its instances as the
//! settings store is. A handle names one `Rc<Tool>`, so the same tool keeps
//! its identity through the store and comparing two settings compares tools
//! by identity. A tool written as plain JSON (`{id, cursor, enables, ...}`,
//! from a host that only has JSON) is read as a tool without callbacks.
//!
//! The table holds only the tools the current or the registered settings
//! name: every typed change prunes the rest.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use epdf_kernel::is_undefined;
use epdf_kernel::settings::Settings;
use serde_json::{Map, Value, json};

use crate::contract::{InteractionConfig, InteractionSettings, Tool, ToolTouch};

#[derive(Default)]
pub(crate) struct ToolTable {
    tools: RefCell<Vec<(u64, Rc<Tool>)>>,
    next: Cell<u64>,
    /// The registered settings, whose tools stay for `reset_settings`.
    registered: RefCell<Value>,
    /// The last decoded settings, by the identity of the JSON they came from.
    cache: RefCell<Option<(Rc<Value>, InteractionSettings)>>,
}

impl ToolTable {
    fn handle(&self, tool: &Rc<Tool>) -> u64 {
        if let Some((n, _)) = self.tools.borrow().iter().find(|(_, t)| Rc::ptr_eq(t, tool)) {
            return *n;
        }
        let n = self.next.get();
        self.next.set(n + 1);
        self.tools.borrow_mut().push((n, tool.clone()));
        n
    }

    fn tool(&self, value: &Value) -> Option<Rc<Tool>> {
        match value.get("$tool").and_then(Value::as_u64) {
            Some(n) => self.tools.borrow().iter().find(|(m, _)| *m == n).map(|(_, t)| t.clone()),
            None => plain_tool(value).map(Rc::new),
        }
    }

    /// The JSON a `DeepPartial<InteractionSettings>` change is in the store:
    /// only the members it names.
    pub(crate) fn encode(&self, config: &InteractionConfig) -> Value {
        let mut out = Map::new();
        if let Some(default_tool) = &config.default_tool {
            out.insert("defaultTool".into(), json!(default_tool));
        }
        if let Some(tools) = &config.tools {
            let tools = tools.iter().map(|t| json!({ "$tool": self.handle(t) })).collect();
            out.insert("tools".into(), Value::Array(tools));
        }
        Value::Object(out)
    }

    /// The registration's settings as `interaction_plugin(config)` declares them.
    pub(crate) fn register(&self, config: &InteractionConfig) -> Value {
        let registered = self.encode(config);
        *self.registered.borrow_mut() = registered.clone();
        registered
    }

    /// The settings a JSON value holds, memoized by the value's identity so
    /// the same store value reads back as the same tools.
    pub(crate) fn decode(&self, value: &Rc<Value>) -> InteractionSettings {
        if let Some((cached, settings)) = &*self.cache.borrow()
            && Rc::ptr_eq(cached, value)
        {
            return settings.clone();
        }
        let settings = InteractionSettings {
            default_tool: value
                .get("defaultTool")
                .and_then(Value::as_str)
                .unwrap_or("pointer")
                .to_string(),
            tools: value
                .get("tools")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|t| self.tool(t))
                .collect(),
        };
        *self.cache.borrow_mut() = Some((value.clone(), settings.clone()));
        settings
    }

    /// Forget every tool neither the store's current settings nor the
    /// registered ones name.
    pub(crate) fn prune(&self, store: &Settings) {
        let current = store.get_settings();
        let named = |n: u64| {
            [&*current, &*self.registered.borrow()].iter().any(|settings| {
                settings
                    .get("tools")
                    .and_then(Value::as_array)
                    .is_some_and(|tools| tools.iter().any(|t| t["$tool"].as_u64() == Some(n)))
            })
        };
        self.tools.borrow_mut().retain(|(n, _)| named(*n));
    }
}

/// The settings' defaults in the store: `INTERACTION_DEFAULTS`.
pub(crate) fn defaults_json() -> Value {
    json!({ "defaultTool": "pointer", "tools": [] })
}

/// A tool written as JSON: `id`, `cursor`, `touch`, `gapCursor` and
/// `enables` (a list, or a canonical `{"$set": [...]}`). No callbacks.
pub fn plain_tool(value: &Value) -> Option<Tool> {
    let text = |name: &str| {
        value.get(name).filter(|v| !is_undefined(v)).and_then(Value::as_str).map(str::to_string)
    };
    let tags = value.get("enables").map(|e| e.get("$set").unwrap_or(e));
    let tags: Vec<&str> =
        tags.and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
    let mut tool = Tool::new(text("id")?, text("cursor").unwrap_or_default(), tags);
    tool.gap_cursor = text("gapCursor");
    tool.touch = match text("touch").as_deref() {
        Some("draw") => Some(ToolTouch::Draw),
        Some("tap") => Some(ToolTouch::Tap),
        _ => None,
    };
    Some(tool)
}
