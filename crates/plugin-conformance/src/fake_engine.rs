//! The fake engine: `tooling/plugin-conformance/src/fake-engine.ts`, rule
//! for rule. `README.md` in that package states the rules both implement.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use epdf_kernel::engine::{DocumentHandle, DocumentSecurity, Engine, EngineCall};
use epdf_kernel::error::ErrorClass;
use epdf_kernel::store::Unsubscribe;
use epdf_kernel::{Error, Promise, undefined};
use serde_json::{Value, json};

use crate::canonical;

struct Pending {
    label: String,
    /// An engine call's settlement, as opposed to a scripted callback's.
    engine: bool,
    settle: Box<dyn FnOnce(Result<Value, Error>)>,
}

#[derive(Default)]
struct State {
    log: Vec<Value>,
    unmatched: Vec<String>,
    pending: Vec<Pending>,
    consumed: HashMap<(usize, usize), u64>,
    /// Live subscribers per opened handle id, in open order.
    handles: Vec<(String, Rc<RefCell<Vec<(u64, Rc<dyn Fn(&Value)>)>>>)>,
    next_listener: u64,
}

pub struct FakeEngine {
    spec: Value,
    state: RefCell<State>,
}

/// Where a response table lives: the engine's, or a document's.
#[derive(Clone, Copy)]
enum Table {
    Engine,
    Document(usize),
}

/// `makeEngineError`: an error the fake ENGINE rejects with. The kernel maps
/// engine refusals by class (`instanceof PermissionDenied` in the
/// TypeScript), and a real engine throws that class, so a refusal the table
/// names `PermissionDenied` is one, with the members it spells. Errors a
/// vector hands in as arguments (`$error`) stay plain.
pub fn engine_error(spec: &Value) -> Error {
    let error = Error::from_spec(spec);
    if error.name == "PermissionDenied" {
        return error.of_class(ErrorClass::PermissionDenied);
    }
    error
}

impl FakeEngine {
    pub fn new(spec: Value) -> Rc<FakeEngine> {
        Rc::new(FakeEngine { spec, state: RefCell::new(State::default()) })
    }

    pub fn log_len(&self) -> usize {
        self.state.borrow().log.len()
    }

    pub fn log_since(&self, from: usize) -> Vec<Value> {
        self.state.borrow().log[from..].to_vec()
    }

    pub fn unmatched(&self) -> Vec<String> {
        self.state.borrow().unmatched.clone()
    }

    fn document_keys(&self) -> Vec<String> {
        self.spec
            .get("documents")
            .and_then(Value::as_object)
            .map(|o| o.keys().cloned().collect())
            .unwrap_or_default()
    }

    fn document_spec(&self, key: &str) -> Option<&Value> {
        self.spec.get("documents")?.get(key)
    }

    fn table(&self, table: Table) -> Option<&Vec<Value>> {
        match table {
            Table::Engine => self.spec.get("responses")?.as_array(),
            Table::Document(index) => {
                let keys = self.document_keys();
                self.document_spec(keys.get(index)?)?.get("responses")?.as_array()
            }
        }
    }

    fn table_id(table: Table) -> usize {
        match table {
            Table::Engine => usize::MAX,
            Table::Document(i) => i,
        }
    }

    fn matching(
        &self,
        table: Table,
        path: &str,
        pon: Option<u32>,
        args: &[Value],
    ) -> Option<Value> {
        let entries = self.table(table)?;
        let key = canonical::key(&Value::Array(args.to_vec()));
        for (index, entry) in entries.iter().enumerate() {
            if entry.get("path").and_then(Value::as_str) != Some(path) {
                continue;
            }
            if let Some(want) = entry.get("pon")
                && want.as_u64().map(|p| p as u32) != pon
            {
                continue;
            }
            if let Some(want) = entry.get("args")
                && canonical::key(want) != key
            {
                continue;
            }
            if let Some(times) = entry.get("times").and_then(Value::as_u64) {
                let mut state = self.state.borrow_mut();
                let used = state.consumed.entry((Self::table_id(table), index)).or_insert(0);
                if *used >= times {
                    continue;
                }
                *used += 1;
            }
            return Some(entry.clone());
        }
        None
    }

    /// One asynchronous call: logged, matched, answered.
    #[allow(clippy::too_many_arguments)]
    fn call(
        self: &Rc<Self>,
        doc: Option<&str>,
        table: Table,
        path: &str,
        pon: Option<u32>,
        args: Vec<Value>,
        fallback: Option<Box<dyn FnOnce() -> Result<Value, Error>>>,
        map: Option<Rc<dyn Fn(Value) -> Result<Value, Error>>>,
    ) -> EngineCall<Value> {
        let index = {
            let mut state = self.state.borrow_mut();
            let mut entry = serde_json::Map::new();
            entry.insert("doc".into(), doc.map_or(Value::Null, |d| Value::String(d.to_string())));
            entry.insert("path".into(), Value::String(path.to_string()));
            if let Some(pon) = pon {
                entry.insert("pon".into(), json!(pon));
            }
            entry.insert("args".into(), Value::Array(args.clone()));
            state.log.push(Value::Object(entry));
            state.log.len() - 1
        };
        let (promise, resolver) = Promise::<Value>::pending();
        let map = map.unwrap_or_else(|| Rc::new(Ok));
        let settle = {
            let resolver = resolver.clone();
            let map = map.clone();
            move |result: Result<Value, Error>| resolver.settle(result.and_then(|v| map(v)))
        };
        let engine = Rc::downgrade(self);
        let aborter = resolver.clone();
        let plain = self.spec.get("plainPromises").and_then(Value::as_bool) == Some(true);
        let promise = if plain {
            promise
        } else {
            promise.with_abort(move |reason| {
                if aborter.promise().is_settled() {
                    return;
                }
                if let Some(engine) = engine.upgrade() {
                    engine.state.borrow_mut().log.push(
                    json!({ "abort": index, "reason": reason.clone().unwrap_or_else(undefined) }),
                );
                }
                aborter.reject(Error::abort(reason));
            })
        };
        match self.matching(table, path, pon, &args) {
            None => match fallback {
                Some(fallback) => settle(fallback()),
                None => {
                    self.state.borrow_mut().unmatched.push(format!(
                        "{}: {path}{} {}",
                        doc.unwrap_or("engine"),
                        pon.map_or(String::new(), |p| format!("@{p}")),
                        Value::Array(args)
                    ));
                    settle(Err(Error::named(
                        "FakeEngineUnmatched",
                        format!("no response for {path}"),
                    )));
                }
            },
            Some(entry) => {
                if let Some(label) = entry.get("defer").and_then(Value::as_str) {
                    self.state.borrow_mut().pending.push(Pending {
                        label: label.to_string(),
                        engine: true,
                        settle: Box::new(settle),
                    });
                } else if let Some(error) = entry.get("error") {
                    settle(Err(engine_error(error)));
                } else {
                    settle(Ok(entry.get("value").cloned().unwrap_or_else(undefined)));
                }
            }
        }
        promise
    }

    /// Settle the oldest pending call or callback with this label. An error
    /// is given as its spec: an engine call rejects with `engine_error`, a
    /// scripted callback with a plain error.
    pub fn settle(&self, label: &str, outcome: Result<Value, &Value>) -> bool {
        let pending = {
            let mut state = self.state.borrow_mut();
            let Some(index) = state.pending.iter().position(|p| p.label == label) else {
                return false;
            };
            state.pending.remove(index)
        };
        let outcome = outcome.map_err(|spec| {
            if pending.engine { engine_error(spec) } else { Error::from_spec(spec) }
        });
        (pending.settle)(outcome);
        true
    }

    /// Park a settlement owned by someone else (a `$fn` callback, a probe).
    pub fn defer(&self, label: &str, settle: Box<dyn FnOnce(Result<Value, Error>)>) {
        self.state.borrow_mut().pending.push(Pending {
            label: label.to_string(),
            engine: false,
            settle,
        });
    }

    pub fn emit(&self, doc: &str, event: &Value) -> Result<(), Error> {
        let listeners = {
            let state = self.state.borrow();
            let Some((_, listeners)) = state.handles.iter().rev().find(|(id, _)| id == doc) else {
                return Err(Error::new(format!(
                    "[fake-engine] emit to unopened document \"{doc}\""
                )));
            };
            listeners.borrow().iter().map(|(_, l)| l.clone()).collect::<Vec<_>>()
        };
        for listener in listeners {
            listener(event);
        }
        Ok(())
    }

    pub fn snapshot(&self) -> Value {
        let state = self.state.borrow();
        let mut out = serde_json::Map::new();
        for (id, listeners) in &state.handles {
            out.insert(id.clone(), json!({ "subscribers": listeners.borrow().len() }));
        }
        Value::Object(out)
    }

    fn open_handle(self: &Rc<Self>, key: &str) -> Result<Rc<dyn DocumentHandle>, Error> {
        let keys = self.document_keys();
        let Some(index) = keys.iter().position(|k| k == key) else {
            return Err(Error::new(format!("[fake-engine] no document \"{key}\"")));
        };
        let spec = self.document_spec(key).cloned().unwrap_or(Value::Null);
        let id = spec.get("id").and_then(Value::as_str).unwrap_or(key).to_string();
        let listeners = Rc::new(RefCell::new(Vec::new()));
        {
            let mut state = self.state.borrow_mut();
            // `handles.set(id, ...)`: a reopen replaces the entry in place.
            match state.handles.iter_mut().find(|(i, _)| *i == id) {
                Some(entry) => entry.1 = listeners.clone(),
                None => state.handles.push((id.clone(), listeners.clone())),
            }
        }
        Ok(Rc::new(FakeHandle {
            engine: self.clone(),
            table: Table::Document(index),
            id,
            spec,
            listeners,
        }))
    }
}

/// The fake as an `Engine` (the orphan rule keeps the impl off `Rc<FakeEngine>`).
pub struct AsEngine(pub Rc<FakeEngine>);

impl Engine for AsEngine {
    fn open(&self, input: Value, options: Option<Value>) -> EngineCall<Rc<dyn DocumentHandle>> {
        let args = match options {
            Some(options) => vec![input.clone(), options],
            None => vec![input.clone()],
        };
        let input_id = input.get("id").and_then(Value::as_str).map(str::to_string);
        let this = self.0.clone();
        let fallback: Box<dyn FnOnce() -> Result<Value, Error>> =
            Box::new(move || match input_id {
                Some(id) if this.document_spec(&id).is_some() => Ok(json!({ "$document": id })),
                _ => match this.spec.get("defaultDocument").and_then(Value::as_str) {
                    Some(key) => Ok(json!({ "$document": key })),
                    None => Err(Error::named("FakeEngineUnmatched", "no document for open")),
                },
            });
        // The handle is made as the answer arrives, even when the call was
        // aborted first: the TypeScript's `resolve(map(value))` runs `map`
        // before the settled promise ignores the resolve. So an open that a
        // close aborted still registers its document, with no subscribers.
        let opened: Rc<RefCell<Option<Rc<dyn DocumentHandle>>>> = Rc::default();
        let this = self.0.clone();
        let slot = opened.clone();
        let map: Rc<dyn Fn(Value) -> Result<Value, Error>> =
            Rc::new(move |value: Value| match value.get("$document").and_then(Value::as_str) {
                Some(key) => {
                    *slot.borrow_mut() = Some(this.open_handle(key)?);
                    Ok(json!({ "$handle": true }))
                }
                None => {
                    Err(Error::new("[fake-engine] open answered something that is not a document"))
                }
            });
        let raw = self.0.call(None, Table::Engine, "open", None, args, Some(fallback), Some(map));
        let abort = raw.abort_handle();
        let (promise, resolver) = Promise::<Rc<dyn DocumentHandle>>::pending();
        raw.on_settle(move |result| {
            resolver.settle(result.clone().map(|_| opened.borrow().clone().expect("opened handle")))
        });
        match abort {
            Some(abort) => promise.with_abort(move |reason| abort(reason)),
            None => promise,
        }
    }

    fn destroy(&self) -> EngineCall<()> {
        self.0
            .call(
                None,
                Table::Engine,
                "destroy",
                None,
                Vec::new(),
                Some(Box::new(|| Ok(undefined()))),
                None,
            )
            .map(|_| Ok(()))
    }
}

/// Optional contract members every handle has unless its `lacks` names them.
const DEFAULT_PRESENT: &[&str] =
    &["forms.applyEffects", "pages.flatten", "pages.extract", "page.annotations.downloadFile"];

struct FakeHandle {
    engine: Rc<FakeEngine>,
    table: Table,
    id: String,
    spec: Value,
    listeners: Rc<RefCell<Vec<(u64, Rc<dyn Fn(&Value)>)>>>,
}

impl FakeHandle {
    fn services(&self) -> Vec<&str> {
        self.spec
            .get("services")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default()
    }

    fn lacks(&self) -> Vec<&str> {
        self.spec
            .get("lacks")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default()
    }

    fn pages(&self) -> Vec<Value> {
        self.spec.get("pages").and_then(Value::as_array).cloned().unwrap_or_default()
    }
}

impl DocumentHandle for FakeHandle {
    fn id(&self) -> String {
        self.id.clone()
    }

    fn capabilities(&self) -> Value {
        self.spec.get("capabilities").cloned().unwrap_or_else(|| json!({}))
    }

    fn security(&self) -> Rc<dyn DocumentSecurity> {
        Rc::new(FakeSecurity {
            engine: self.engine.clone(),
            table: self.table,
            doc: self.id.clone(),
            spec: self.spec.get("security").cloned().unwrap_or_else(|| json!({})),
        })
    }

    fn has(&self, member: &str) -> bool {
        if DEFAULT_PRESENT.contains(&member) {
            return !self.lacks().contains(&member);
        }
        // The TypeScript fake answers every `pages.*` member through a proxy
        // that honours `lacks`, and every `page.annotations.*` member but a
        // lacked `downloadFile` (above) through one that does not.
        if member.starts_with("pages.") {
            return !self.lacks().contains(&member);
        }
        if member.starts_with("page.annotations.") {
            return true;
        }
        // `forms` is a proxy too, with `submit` present only when `services`
        // lists it and every other member present unless `lacks` names it.
        if member.starts_with("forms.") && member != "forms.submit" {
            return !self.lacks().contains(&member);
        }
        self.services().contains(&member)
    }

    fn call(&self, path: &str, pon: Option<u32>, args: Vec<Value>) -> EngineCall<Value> {
        let fallback: Option<Box<dyn FnOnce() -> Result<Value, Error>>> = match (path, pon) {
            ("pages.list", None) => {
                let pages = self.pages();
                Some(Box::new(move || Ok(json!({ "pageCount": pages.len(), "pages": pages }))))
            }
            ("close", None) => Some(Box::new(|| Ok(undefined()))),
            _ => None,
        };
        self.engine.call(Some(&self.id), self.table, path, pon, args, fallback, None)
    }

    /// `page(ref)`: the page whose `ref.pageObjectNumber` is `pon`.
    fn page_index(&self, pon: u32) -> Result<u32, Error> {
        self.pages()
            .iter()
            .find(|p| {
                p.get("ref").and_then(|r| r.get("pageObjectNumber")).and_then(Value::as_u64)
                    == Some(pon as u64)
            })
            .and_then(|p| p.get("index").and_then(Value::as_u64))
            .map(|i| i as u32)
            .ok_or_else(|| Error::engine("NotFound", format!("no page {pon}")))
    }

    fn subscribe(&self, listener: Rc<dyn Fn(&Value)>) -> Unsubscribe {
        let id = {
            let mut state = self.engine.state.borrow_mut();
            state.next_listener += 1;
            state.next_listener
        };
        self.listeners.borrow_mut().push((id, listener));
        let listeners = Rc::downgrade(&self.listeners);
        Unsubscribe::new(move || {
            if let Some(listeners) = listeners.upgrade() {
                listeners.borrow_mut().retain(|(i, _)| *i != id);
            }
        })
    }
}

struct FakeSecurity {
    engine: Rc<FakeEngine>,
    table: Table,
    doc: String,
    spec: Value,
}

impl DocumentSecurity for FakeSecurity {
    fn allows(&self, capability: &str) -> bool {
        match self.spec.get("allows") {
            Some(Value::String(s)) if s == "*" => true,
            Some(Value::Array(list)) => list.iter().any(|c| c.as_str() == Some(capability)),
            _ => false,
        }
    }

    fn allows_annotation_create(&self) -> bool {
        self.spec
            .get("allowsAnnotationCreate")
            .and_then(Value::as_bool)
            .unwrap_or_else(|| self.allows("doc.annotate.create"))
    }

    fn allows_annotation_mutation(&self, action: &str, _target: &Value) -> bool {
        self.spec
            .get("allowsAnnotationMutation")
            .and_then(|m| m.get(action))
            .and_then(Value::as_bool)
            .unwrap_or_else(|| self.allows("doc.annotate.modify"))
    }

    fn allows_annotation_group_assignment(&self, _group_id: &str) -> bool {
        self.spec.get("allowsAnnotationGroupAssignment").and_then(Value::as_bool).unwrap_or(false)
    }

    fn effective_scope(&self) -> Vec<String> {
        self.spec
            .get("effectiveScope")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect())
            .unwrap_or_default()
    }

    fn identity(&self) -> Value {
        self.spec.get("identity").cloned().unwrap_or(Value::Null)
    }

    fn current(&self) -> Value {
        self.spec.get("current").cloned().unwrap_or(Value::Null)
    }

    fn password_prompt(&self) -> Value {
        self.spec.get("passwordPrompt").cloned().unwrap_or_else(|| json!({ "state": "none" }))
    }

    fn unlock(&self, input: Value) -> EngineCall<Value> {
        self.engine.call(
            Some(&self.doc),
            self.table,
            "security.unlock",
            None,
            vec![input],
            None,
            None,
        )
    }
}
