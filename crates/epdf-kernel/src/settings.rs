//! One settings store per registration, shared by its document instances.
use crate::{event_hook::EventHookSource, scope::Scope, store::Report};
use serde_json::{Value, json};
use std::{cell::RefCell, rc::Rc};
#[derive(Clone)]
pub struct SettingsDeclaration {
    pub defaults: Value,
    pub registered: Option<Value>,
    pub whole: Vec<String>,
}
#[derive(Clone)]
pub struct SettingsChanged {
    pub settings: Rc<Value>,
    pub changed: Vec<String>,
}
#[derive(Clone)]
pub struct Settings(Rc<Inner>);
struct Inner {
    report: Report,
    initial: Rc<Value>,
    current: RefCell<Rc<Value>>,
    whole: Vec<String>,
    notify: Rc<dyn Fn()>,
    events: EventHookSource<SettingsChanged>,
}
impl Settings {
    pub fn new(declaration: SettingsDeclaration, notify: Rc<dyn Fn()>, report: Report) -> Self {
        let initial = Rc::new(merge_settings(
            &declaration.defaults,
            declaration.registered.as_ref(),
            &declaration.whole,
        ));
        Self(Rc::new(Inner {
            report: report.clone(),
            initial: initial.clone(),
            current: RefCell::new(initial),
            whole: declaration.whole,
            notify,
            events: EventHookSource::new(Some(report)),
        }))
    }
    pub fn get(&self) -> Rc<Value> {
        self.0.current.borrow().clone()
    }
    pub fn get_settings(&self) -> Rc<Value> {
        self.get()
    }
    /// A handle that doesn't keep the store alive.
    pub fn downgrade(&self) -> WeakSettings {
        WeakSettings(Rc::downgrade(&self.0))
    }
    /// The registered settings, which `reset_settings` goes back to.
    pub fn initial(&self) -> Rc<Value> {
        self.0.initial.clone()
    }
    /// Every handle `{ [member]: n }` the settings name, in the registered
    /// value or the current one: the stand-ins a plugin's table must keep,
    /// as the TypeScript store keeps the objects those two values hold and
    /// no others.
    pub fn named_handles(&self, member: &str) -> std::collections::HashSet<u64> {
        let mut out = std::collections::HashSet::new();
        handles_in(&self.0.initial, member, &mut out);
        handles_in(&self.get(), member, &mut out);
        out
    }
    fn commit(&self, next: Rc<Value>) {
        let current = self.get();
        let mut keys = Vec::new();
        for value in [&*current, &*next] {
            if let Some(o) = value.as_object() {
                for key in o.keys() {
                    if !keys.contains(key) {
                        keys.push(key.clone());
                    }
                }
            }
        }
        let changed =
            keys.into_iter().filter(|k| current.get(k) != next.get(k)).collect::<Vec<_>>();
        if changed.is_empty() {
            return;
        }
        *self.0.current.borrow_mut() = next.clone();
        (self.0.notify)();
        self.0.events.emit(&SettingsChanged { settings: next, changed });
    }
    pub fn update_settings(&self, changes: &Value) {
        self.commit(Rc::new(merge_settings(&self.get(), Some(changes), &self.0.whole)));
    }
    pub fn reset_settings(&self) {
        self.commit(self.0.initial.clone());
    }
    pub fn on_settings_changed(&self) -> EventHookSource<SettingsChanged> {
        self.0.events.clone()
    }
    pub fn for_instance(&self, scope: &Scope) -> InstanceSettings {
        let events = EventHookSource::new(Some(self.0.report.clone()));
        let publish = events.clone();
        let off = self.0.events.on(move |e| {
            publish.emit(e);
            Ok(())
        });
        scope.defer_fn(move || off.call());
        let end = events.clone();
        scope.defer_fn(move || end.dispose());
        InstanceSettings { store: self.clone(), events }
    }
    pub fn dispose(&self) {
        self.0.events.dispose();
    }
}
/// A [`Settings`] store held weakly ([`Settings::downgrade`]).
#[derive(Clone)]
pub struct WeakSettings(std::rc::Weak<Inner>);
impl WeakSettings {
    pub fn upgrade(&self) -> Option<Settings> {
        self.0.upgrade().map(Settings)
    }
}
#[derive(Clone)]
pub struct InstanceSettings {
    pub store: Settings,
    pub events: EventHookSource<SettingsChanged>,
}
impl InstanceSettings {
    pub fn get(&self) -> Rc<Value> {
        self.store.get()
    }
}
fn handles_in(value: &Value, member: &str, out: &mut std::collections::HashSet<u64>) {
    match value {
        Value::Object(object) => {
            if let Some(n) = object.get(member).and_then(Value::as_u64) {
                out.insert(n);
            }
            for value in object.values() {
                handles_in(value, member, out);
            }
        }
        Value::Array(items) => {
            for value in items {
                handles_in(value, member, out);
            }
        }
        _ => {}
    }
}
pub fn merge_settings(base: &Value, change: Option<&Value>, whole: &[String]) -> Value {
    let Some(change) = change else {
        return base.clone();
    };
    if crate::is_undefined(change) {
        return base.clone();
    }
    let Some(object) = change.as_object() else {
        return change.clone();
    };
    let mut result = base.as_object().cloned().unwrap_or_default();
    for (key, value) in object {
        if crate::is_undefined(value) {
            continue;
        }
        let next = if whole.contains(key) {
            value.clone()
        } else {
            merge_settings(result.get(key).unwrap_or(&crate::undefined()), Some(value), &[])
        };
        result.insert(key.clone(), next);
    }
    if result.is_empty() && !base.is_object() { base.clone() } else { json!(result) }
}
