//! `settings.ts`: the actions settings in the kernel's settings store.
//!
//! The kernel keeps a registration's settings as one JSON value
//! (`epdf_kernel::settings`). Every actions setting is plain data except
//! `javascript.identity`, which may be a function. A function identity is
//! stored as a handle, `{"$identity": n}`, into a table the registration
//! owns (one per `actions_plugin(config)`, shared by its instances as the
//! store is), so the same function reads back as the same `Rc`. Identity
//! fields are stored as the object they are. The table keeps only the
//! functions the current or the registered settings name: every change of
//! the store prunes the rest.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use epdf_acrojs::PartialScriptIdentity;
use epdf_kernel::settings::Settings;
use serde_json::{Map, Value, json};

use crate::contract::{
    ActionPolicy, ActionTriggers, ActionsConfig, ActionsScriptIdentity, ActionsSettings,
    ActionsSettingsPatch, JavaScriptSettings, OpenSequence, actions_defaults,
};

#[derive(Default)]
pub(crate) struct IdentityTable {
    functions: RefCell<Vec<(u64, ActionsScriptIdentity)>>,
    next: Cell<u64>,
    /// The registered settings, whose identity stays for `reset_settings`.
    registered: RefCell<Value>,
    /// The last decoded settings, by the identity of the JSON they came from.
    cache: RefCell<Option<(Rc<Value>, ActionsSettings)>>,
    watching: Cell<bool>,
}

fn same_identity(a: &ActionsScriptIdentity, b: &ActionsScriptIdentity) -> bool {
    match (a, b) {
        (ActionsScriptIdentity::Function(a), ActionsScriptIdentity::Function(b)) => {
            std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
        }
        _ => false,
    }
}

impl IdentityTable {
    fn handle(&self, identity: &ActionsScriptIdentity) -> u64 {
        if let Some((n, _)) =
            self.functions.borrow().iter().find(|(_, f)| same_identity(f, identity))
        {
            return *n;
        }
        let n = self.next.get();
        self.next.set(n + 1);
        self.functions.borrow_mut().push((n, identity.clone()));
        n
    }

    fn identity_json(&self, identity: &Option<ActionsScriptIdentity>) -> Value {
        match identity {
            None => Value::Null,
            Some(ActionsScriptIdentity::Value(fields)) => {
                serde_json::to_value(fields).unwrap_or(Value::Null)
            }
            Some(function) => json!({ "$identity": self.handle(function) }),
        }
    }

    fn identity_of(&self, value: &Value) -> Option<ActionsScriptIdentity> {
        if value.is_null() || epdf_kernel::is_undefined(value) {
            return None;
        }
        if let Some(n) = value.get("$identity").and_then(Value::as_u64) {
            return self.functions.borrow().iter().find(|(m, _)| *m == n).map(|(_, f)| f.clone());
        }
        let fields: PartialScriptIdentity = serde_json::from_value(value.clone()).ok()?;
        Some(ActionsScriptIdentity::Value(fields))
    }

    /// The JSON a `DeepPartial<ActionsSettings>` change is in the store: only
    /// the members it names.
    pub(crate) fn encode(&self, patch: &ActionsSettingsPatch) -> Value {
        let mut out = Map::new();
        if let Some(policy) = &patch.policy {
            out.insert("policy".into(), serde_json::to_value(policy).unwrap_or(Value::Null));
        }
        if let Some(triggers) = &patch.triggers {
            out.insert("triggers".into(), serde_json::to_value(triggers).unwrap_or(Value::Null));
        }
        if let Some(open_sequence) = patch.open_sequence {
            out.insert("openSequence".into(), json!(open_sequence.as_str()));
        }
        if let Some(javascript) = &patch.javascript {
            let mut js = Map::new();
            if let Some(enabled) = javascript.enabled {
                js.insert("enabled".into(), json!(enabled));
            }
            if let Some(identity) = &javascript.identity {
                js.insert("identity".into(), self.identity_json(identity));
            }
            out.insert("javascript".into(), Value::Object(js));
        }
        Value::Object(out)
    }

    /// `registeredSettings(config)`: `config` without the script
    /// environment, as `actions_plugin(config)` registers it.
    pub(crate) fn register(&self, config: &ActionsConfig) -> Value {
        let patch = ActionsSettingsPatch {
            policy: config.policy.clone(),
            triggers: config.triggers.clone(),
            open_sequence: config.open_sequence,
            javascript: config.javascript.as_ref().map(|js| crate::JavaScriptSettingsPatch {
                enabled: js.enabled,
                identity: js.identity.clone(),
            }),
        };
        let registered = self.encode(&patch);
        *self.registered.borrow_mut() = registered.clone();
        registered
    }

    /// The settings a JSON value holds, memoized by the value's identity so
    /// the same store value reads back as the same settings.
    pub(crate) fn decode(&self, value: &Rc<Value>) -> ActionsSettings {
        if let Some((cached, settings)) = &*self.cache.borrow()
            && Rc::ptr_eq(cached, value)
        {
            return settings.clone();
        }
        let defaults = actions_defaults();
        let policy: ActionPolicy = value
            .get("policy")
            .and_then(|policy| serde_json::from_value(policy.clone()).ok())
            .unwrap_or(defaults.policy);
        let triggers: ActionTriggers = value
            .get("triggers")
            .and_then(|triggers| serde_json::from_value(triggers.clone()).ok())
            .unwrap_or(defaults.triggers);
        let open_sequence: OpenSequence = value
            .get("openSequence")
            .and_then(|mode| serde_json::from_value(mode.clone()).ok())
            .unwrap_or(defaults.open_sequence);
        let javascript = value.get("javascript");
        let settings = ActionsSettings {
            policy,
            triggers,
            open_sequence,
            javascript: JavaScriptSettings {
                enabled: javascript
                    .and_then(|js| js.get("enabled"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                identity: javascript
                    .and_then(|js| js.get("identity"))
                    .and_then(|identity| self.identity_of(identity)),
            },
        };
        *self.cache.borrow_mut() = Some((value.clone(), settings.clone()));
        settings
    }

    /// Prune on every change of the registration's store, from the first
    /// instance on.
    pub(crate) fn watch(self: &Rc<Self>, store: &Settings) {
        if self.watching.replace(true) {
            return;
        }
        let table = Rc::downgrade(self);
        // The store's events end with the kernel; the listener holds the
        // table weakly, so dropping its unsubscribe keeps it that long.
        let _ = store.on_settings_changed().on(move |event| {
            if let Some(table) = table.upgrade() {
                table.prune(&event.settings);
            }
            Ok(())
        });
    }

    fn prune(&self, current: &Value) {
        let named = |n: u64| {
            [current, &*self.registered.borrow()]
                .iter()
                .any(|settings| settings["javascript"]["identity"]["$identity"].as_u64() == Some(n))
        };
        self.functions.borrow_mut().retain(|(n, _)| named(*n));
    }
}

/// The settings' defaults in the store: `ACTIONS_DEFAULTS`.
pub(crate) fn defaults_json() -> Value {
    let defaults = actions_defaults();
    json!({
        "policy": serde_json::to_value(&defaults.policy).unwrap_or(Value::Null),
        "triggers": serde_json::to_value(defaults.triggers).unwrap_or(Value::Null),
        "openSequence": defaults.open_sequence.as_str(),
        "javascript": { "enabled": false, "identity": null },
    })
}
