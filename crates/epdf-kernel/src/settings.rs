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
    defaults: Value,
    whole: Vec<String>,
    notify: Rc<dyn Fn()>,
    events: EventHookSource<SettingsChanged>,
}
impl Settings {
    pub fn new(declaration: SettingsDeclaration, notify: Rc<dyn Fn()>, report: Report) -> Self {
        let initial = Rc::new(merge_declared_settings(
            &declaration.defaults,
            declaration.registered.as_ref(),
            &declaration.defaults,
            &declaration.whole,
        ));
        Self(Rc::new(Inner {
            report: report.clone(),
            initial: initial.clone(),
            current: RefCell::new(initial),
            defaults: declaration.defaults,
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
        // `changedKeys`: `sameValue` per top-level key, so a setting the
        // host writes as `1.0` is the `1` it was.
        let changed = keys
            .into_iter()
            .filter(|k| match (current.get(k), next.get(k)) {
                (Some(before), Some(after)) => !crate::js::same_value(before, after),
                (None, None) => false,
                _ => true,
            })
            .collect::<Vec<_>>();
        if changed.is_empty() {
            return;
        }
        *self.0.current.borrow_mut() = next.clone();
        (self.0.notify)();
        self.0.events.emit(&SettingsChanged { settings: next, changed });
    }
    pub fn update_settings(&self, changes: &Value) {
        self.commit(Rc::new(merge_declared_settings(
            &self.get(),
            Some(changes),
            &self.0.defaults,
            &self.0.whole,
        )));
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
    // `mergeValue`: arrays and other values replace, but an equal one keeps
    // the value already there.
    let Some(object) = change.as_object() else {
        return if crate::js::same_value(base, change) { base.clone() } else { change.clone() };
    };
    let mut result = base.as_object().cloned().unwrap_or_default();
    for (key, value) in object {
        if crate::is_undefined(value) {
            continue;
        }
        let next = if whole.contains(key) {
            // `mergeSettings` keeps a `whole` value equal to the one there.
            match result.get(key) {
                Some(current) if crate::js::same_value(current, value) => continue,
                _ => value.clone(),
            }
        } else {
            merge_settings(result.get(key).unwrap_or(&crate::undefined()), Some(value), &[])
        };
        result.insert(key.clone(), next);
    }
    if result.is_empty() && !base.is_object() { base.clone() } else { json!(result) }
}

/// `mergeSettings(settings, changes, { defaults, whole })`: `changes` merged
/// into `settings`, the settings at the `whole` paths (`"identity"`,
/// `"snapshot.box"`) replaced instead, what such a change leaves out taking
/// its default.
pub fn merge_declared_settings(
    settings: &Value,
    changes: Option<&Value>,
    defaults: &Value,
    whole: &[String],
) -> Value {
    let Some(changes) = changes.filter(|c| c.is_object() && !crate::is_undefined(c)) else {
        return merge_settings(settings, changes, &[]);
    };
    if whole.is_empty() {
        return merge_settings(settings, Some(changes), &[]);
    }
    let paths: Vec<Vec<&str>> = whole.iter().map(|path| path.split('.').collect()).collect();
    let mut rest = changes.clone();
    for path in &paths {
        if value_at(&rest, path).is_some() {
            rest = with_value_at(&rest, path, crate::undefined());
        }
    }
    let mut merged = merge_settings(settings, Some(&rest), &[]);
    for path in &paths {
        let Some(change) = value_at(changes, path) else { continue };
        let value = filled_from(value_at(defaults, path), change);
        let same = value_at(&merged, path).is_some_and(|now| crate::js::same_value(now, &value));
        if !same {
            merged = with_value_at(&merged, path, value);
        }
    }
    merged
}

/// `valueAt`: the value at `path`, `None` where the path ends early or the
/// value is `undefined`.
fn value_at<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut at = value;
    for key in path {
        at = at.as_object()?.get(*key)?;
    }
    (!crate::is_undefined(at)).then_some(at)
}

/// `withValueAt`: `value` with `next` at `path`, the objects along it copied.
fn with_value_at(value: &Value, path: &[&str], next: Value) -> Value {
    let Some((key, rest)) = path.split_first() else { return next };
    let mut into =
        value.as_object().filter(|_| !crate::is_undefined(value)).cloned().unwrap_or_default();
    let inner = into.get(*key).cloned().unwrap_or_else(crate::undefined);
    into.insert((*key).to_string(), with_value_at(&inner, rest, next));
    Value::Object(into)
}

/// `filledFrom`: a whole value, a plain object's keys left out (or
/// `undefined`) taking their defaults.
fn filled_from(default: Option<&Value>, change: &Value) -> Value {
    let (Some(change_object), Some(Value::Object(default_object))) = (change.as_object(), default)
    else {
        return change.clone();
    };
    let mut filled = default_object.clone();
    for (key, value) in change_object {
        if !crate::is_undefined(value) {
            filled.insert(key.clone(), value.clone());
        }
    }
    Value::Object(filled)
}

/// Who holds a document setting and the sentence its controls show.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingLock {
    pub by: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason_key: Option<String>,
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    /// `changedKeys` compares with `sameValue`: a setting written again as
    /// `1.0` where it was `1` changed nothing, and nothing is announced.
    #[test]
    fn a_setting_written_again_in_floats_changes_nothing() {
        let notified = Rc::new(Cell::new(0));
        let count = notified.clone();
        let settings = Settings::new(
            SettingsDeclaration {
                defaults: json!({ "zoom": { "level": 1, "steps": [1, 2] }, "page": 3 }),
                registered: None,
                whole: vec![],
            },
            Rc::new(move || count.set(count.get() + 1)),
            Rc::new(|e| panic!("{e}")),
        );
        let changed = Rc::new(RefCell::new(Vec::new()));
        let log = changed.clone();
        settings.on_settings_changed().on(move |e: &SettingsChanged| {
            log.borrow_mut().push(e.changed.clone());
            Ok(())
        });
        settings.update_settings(
            &json!({ "zoom": { "level": 1.0, "steps": [1.0, 2.0] }, "page": 3.0 }),
        );
        assert_eq!(notified.get(), 0);
        assert!(changed.borrow().is_empty());
        settings.update_settings(&json!({ "page": 4.0 }));
        assert_eq!(*changed.borrow(), vec![vec!["page".to_string()]]);
    }

    /// `mergeValue` keeps the base value when the change is `sameValue` to it,
    /// so a value written again in floats reads as it was, `whole` ones too.
    #[test]
    fn a_change_equal_to_the_value_there_keeps_that_value() {
        let base = json!({ "zoom": { "level": 1, "steps": [1, 2] }, "page": 3, "tools": [1] });
        let whole = vec!["tools".to_string()];
        let merged = merge_settings(
            &base,
            Some(
                &json!({ "zoom": { "level": 1.0, "steps": [1.0, 2.0] }, "page": 3.0, "tools": [1.0] }),
            ),
            &whole,
        );
        assert_eq!(merged.to_string(), base.to_string());
        assert_eq!(merge_settings(&json!(1), Some(&json!(1.0)), &[]).to_string(), "1");
        // A different value still replaces.
        let merged = merge_settings(&base, Some(&json!({ "page": 4.0, "tools": [2] })), &whole);
        assert_eq!(merged["page"], json!(4.0));
        assert_eq!(merged["tools"], json!([2]));
    }
}
