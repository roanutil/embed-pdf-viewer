//! The fake engine: `tooling/plugin-conformance/src/fake-engine.ts`, rule
//! for rule. `README.md` in that package states the rules both implement.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

// The shipping scope rules live in the engine, which is native only: on
// wasm32 a fake `rawScope` is not resolved.
#[cfg(not(target_arch = "wasm32"))]
use epdf_engine::auth::scope;
use epdf_kernel::engine::{DocumentHandle, DocumentSecurity, Engine, EngineCall};
use epdf_kernel::error::ErrorClass;
use epdf_kernel::store::Unsubscribe;
use epdf_kernel::{Error, Promise, undefined};
use serde_json::{Value, json};

use crate::canonical;

type Publish = Rc<dyn Fn(&Value)>;

/// Hydrate the canonical fixture at the consuming adapter boundary. The
/// JSON service transport retains the marker; methods belong to the native
/// engine TextLayout, never to an invented JSON approximation. Native only:
/// the engine does not build for wasm32.
#[cfg(not(target_arch = "wasm32"))]
pub fn hydrate_text_layout(
    value: &Value,
) -> Result<epdf_engine::page_space::text::TextLayout, Error> {
    let snapshot = value.get("$textLayout").ok_or_else(|| {
        Error::new("[fake-engine] page.text.layout needs a $textLayout geometry fixture")
    })?;
    let snapshot = serde_json::from_value(snapshot.clone()).map_err(|e| {
        Error::new(format!("[fake-engine] invalid $textLayout geometry fixture: {e}"))
    })?;
    Ok(epdf_engine::page_space::text::create_text_layout(snapshot))
}

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
    handles: Vec<(String, Rc<RefCell<Vec<(u64, Rc<dyn Fn(&Value)>)>>>, Publish)>,
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
        publish: Option<Publish>,
        validation: Option<Error>,
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
        let matched =
            if validation.is_none() { self.matching(table, path, pon, &args) } else { None };
        let events = matched
            .as_ref()
            .and_then(|entry| entry.get("events"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let settle = {
            let resolver = resolver.clone();
            let map = map.clone();
            move |result: Result<Value, Error>| {
                resolver.settle(result.and_then(|v| {
                let value = map(v)?;
                for event in &events {
                    let publish = publish.as_ref().ok_or_else(|| Error::new("[fake-engine] an engine-level response cannot publish document events"))?;
                    publish(event);
                }
                Ok(value)
            }))
            }
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
        if let Some(error) = validation {
            settle(Err(error));
            return promise;
        }
        match matched {
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
        let publish = self
            .state
            .borrow()
            .handles
            .iter()
            .rev()
            .find(|(id, _, _)| id == doc)
            .map(|(_, _, publish)| publish.clone())
            .ok_or_else(|| {
                Error::new(format!("[fake-engine] emit to unopened document \"{doc}\""))
            })?;
        publish(event);
        Ok(())
    }

    pub fn snapshot(&self) -> Value {
        let state = self.state.borrow();
        let mut out = serde_json::Map::new();
        for (id, listeners, _) in &state.handles {
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
        let listeners: Rc<RefCell<Vec<(u64, Publish)>>> = Rc::default();
        let pages = Rc::new(RefCell::new(spec["pages"].as_array().cloned().unwrap_or_default()));
        let named_pages =
            Rc::new(RefCell::new(spec.get("namedPages").cloned().unwrap_or_else(|| json!([]))));
        let cursor = Rc::new(Cell::new(None::<i64>));
        let (p, n, c, l) = (pages.clone(), named_pages.clone(), cursor.clone(), listeners.clone());
        let publish: Publish = Rc::new(move |event| {
            if let Some(layout) = event.get("layout") {
                *p.borrow_mut() = layout["pages"].as_array().cloned().unwrap_or_default();
                *n.borrow_mut() = layout.get("namedPages").cloned().unwrap_or_else(|| json!([]));
            }
            if let Some(server) = event["origin"]["serverId"].as_i64() {
                c.set(Some(c.get().unwrap_or(0).max(server)));
            }
            let listeners = l.borrow().iter().map(|(_, f)| f.clone()).collect::<Vec<_>>();
            for listener in listeners {
                listener(event);
            }
        });
        {
            let mut state = self.state.borrow_mut();
            // `handles.set(id, ...)`: a reopen replaces the entry in place.
            match state.handles.iter_mut().find(|(i, _, _)| *i == id) {
                Some(entry) => {
                    entry.1 = listeners.clone();
                    entry.2 = publish.clone();
                }
                None => state.handles.push((id.clone(), listeners.clone(), publish.clone())),
            }
        }
        Ok(Rc::new(FakeHandle {
            engine: self.clone(),
            pages,
            named_pages,
            cursor,
            publish,
            security_state: Rc::new(RefCell::new({
                let state = spec["security"].get("state").cloned().unwrap_or_else(default_security);
                let prompt = spec["security"]
                    .get("passwordPrompt")
                    .cloned()
                    .unwrap_or_else(|| password_prompt(&state));
                (state, prompt)
            })),
            table: Table::Document(index),
            id,
            spec: spec.clone(),
            next_number: Rc::new(Cell::new(
                spec["pages"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|p| p["ref"]["objectNumber"].as_u64())
                    .chain(
                        spec["objectNumbers"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(Value::as_u64),
                    )
                    .max()
                    .unwrap_or(0) as u32
                    + 1,
            )),
            numbers: Rc::new(RefCell::new(
                spec.get("objectNumbers")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .filter_map(Value::as_u64)
                    .map(|n| n as u32)
                    .collect(),
            )),
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
        let raw = self.0.call(
            None,
            Table::Engine,
            "open",
            None,
            args,
            Some(fallback),
            Some(map),
            None,
            None,
        );
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
                None,
                None,
            )
            .map(|_| Ok(()))
    }
}

/// Optional contract members every handle has unless its `lacks` names them.
const DEFAULT_PRESENT: &[&str] =
    &["forms.applyEffects", "pages.flatten", "pages.extract", "page.annotations.downloadFile"];

fn default_security() -> Value {
    json!({"encryption":{"state":"none","requiresPassword":false},"permissions":{"known":true,"bits":null,"allAllowed":true,"openedAs":"none","securityHandlerRevision":null,"canUpgradeToOwner":false},"access":{"required":false,"reasons":[]}})
}
fn password_prompt(state: &Value) -> Value {
    if state["encryption"]["state"] == "none" {
        return json!({"state":"none"});
    }
    if state["permissions"]["known"] == false {
        return json!({"state":"required","hint":null,"incorrect":false});
    }
    if state["encryption"]["requiresPassword"] == true && state["permissions"]["openedAs"] == "none"
    {
        return json!({"state":"required","hint":"user","incorrect":false});
    }
    if state["permissions"]["openedAs"] != "owner"
        && state["permissions"]["canUpgradeToOwner"] == true
    {
        return json!({"state":"optional","hint":"owner"});
    }
    json!({"state":"none"})
}

struct FakeHandle {
    security_state: Rc<RefCell<(Value, Value)>>,
    pages: Rc<RefCell<Vec<Value>>>,
    named_pages: Rc<RefCell<Value>>,
    cursor: Rc<Cell<Option<i64>>>,
    publish: Publish,
    next_number: Rc<Cell<u32>>,
    numbers: Rc<RefCell<Vec<u32>>>,
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
        self.pages.borrow().clone()
    }
}

impl DocumentHandle for FakeHandle {
    fn is_local(&self) -> bool {
        self.spec["local"].as_bool() == Some(true)
    }
    fn held_object_numbers(&self) -> usize {
        self.numbers.borrow().len()
    }
    fn take_object_number(&self) -> Result<Option<u32>, Error> {
        let mut numbers = self.numbers.borrow_mut();
        Ok(if numbers.is_empty() { None } else { Some(numbers.remove(0)) })
    }
    fn reserve_object_numbers(&self, count: u32) -> EngineCall<()> {
        // Logged first, then refused, as the TypeScript validates.
        let validation =
            (count == 0).then(|| Error::engine("InvalidArg", "cannot reserve 0 object numbers"));
        let numbers = self.numbers.clone();
        let sequence = self.next_number.clone();
        let map = Rc::new(move |value| {
            let mut numbers = numbers.borrow_mut();
            let mut next = sequence.get();
            while numbers.len() < count as usize {
                numbers.push(next);
                next += 1;
            }
            sequence.set(next);
            Ok(value)
        });
        self.engine
            .call(
                Some(&self.id),
                self.table,
                "objectNumbers.reserve",
                None,
                vec![json!(count)],
                Some(Box::new(|| Ok(undefined()))),
                Some(map),
                None,
                validation,
            )
            .convert(|_| Ok(()))
    }

    fn id(&self) -> String {
        self.id.clone()
    }

    fn capabilities(&self) -> Value {
        self.spec.get("capabilities").cloned().unwrap_or_else(|| json!({}))
    }

    fn security(&self) -> Rc<dyn DocumentSecurity> {
        Rc::new(FakeSecurity {
            state: self.security_state.clone(),
            publish: self.publish.clone(),
            engine: self.engine.clone(),
            table: self.table,
            doc: self.id.clone(),
            spec: self.spec.get("security").cloned().unwrap_or_else(|| json!({})),
        })
    }

    fn has(&self, member: &str) -> bool {
        if member == "render" {
            return true;
        }
        if member == "downloadLayer" && self.is_local() {
            return true;
        }
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
        // Every page carries a `measure` service (fake-engine.ts:678), with
        // no `lacks` or `services` gate.
        if member == "page.measure" || member.starts_with("page.measure.") {
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
                let named_pages = self.named_pages.borrow().clone();
                Some(Box::new(move || {
                    Ok(
                        json!({ "pageCount": pages.len(), "pages": pages, "namedPages": named_pages }),
                    )
                }))
            }
            ("close", None) => Some(Box::new(|| Ok(undefined()))),
            ("render.getPolicy", None) => Some(Box::new(|| Ok(json!({"kind": "continuous"})))),
            _ => None,
        };
        #[cfg(not(target_arch = "wasm32"))]
        let map: Option<Rc<dyn Fn(Value) -> Result<Value, Error>>> = if path == "page.text.layout" {
            Some(Rc::new(|value| {
                hydrate_text_layout(&value)?;
                Ok(value)
            }))
        } else {
            None
        };
        #[cfg(target_arch = "wasm32")]
        let map: Option<Rc<dyn Fn(Value) -> Result<Value, Error>>> = None;
        self.engine.call(
            Some(&self.id),
            self.table,
            path,
            pon,
            args,
            fallback,
            map,
            Some(self.publish.clone()),
            pon.and_then(|p| self.page_index(p).err()),
        )
    }

    /// `page(ref)`: the page whose `ref.pageObjectNumber` is `pon`.
    fn page_index(&self, pon: u32) -> Result<u32, Error> {
        self.pages()
            .iter()
            .find(|p| {
                p.get("ref").and_then(|r| r.get("objectNumber")).and_then(Value::as_u64)
                    == Some(pon as u64)
            })
            .and_then(|p| p.get("index").and_then(Value::as_u64))
            .map(|i| i as u32)
            .ok_or_else(|| Error::engine("NotFound", format!("no page {pon}")))
    }

    fn last_server_id(&self) -> Option<String> {
        self.cursor.get().map(|n| n.to_string())
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
    state: Rc<RefCell<(Value, Value)>>,
    publish: Publish,
    engine: Rc<FakeEngine>,
    table: Table,
    doc: String,
    spec: Value,
}

impl FakeSecurity {
    /// `security.rawScope`: resolve through the shipping scope rules.
    fn raw_scope(&self) -> Option<Vec<String>> {
        let raw = self.spec.get("rawScope")?.as_array()?;
        Some(raw.iter().filter_map(|s| s.as_str().map(str::to_string)).collect())
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// `decodePdfBits(currentSecurityState.permissions.bits)`.
    fn bits(&self) -> scope::PdfBits {
        scope::decode_pdf_bits(self.state.borrow().0["permissions"]["bits"].as_i64())
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// `checkSetGroup(entity, groupId, identity.groupId, rawScope)`.
    fn set_group(&self, entity: scope::CollabEntity, group: &str, raw: &[String]) -> bool {
        let parsed = scope::parse_all(raw).unwrap_or_default();
        let own = self.identity().get("groupId").and_then(Value::as_str).map(str::to_string);
        scope::check_set_group(entity, group, own.as_deref(), &parsed)
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// `checkCollab(action, collabTargetOf(target), rawScope, identity, bits)`.
    fn collab(&self, action: &str, target: &Value, raw: &[String]) -> bool {
        let Some(action) = scope::CollabAction::parse(action) else { return false };
        let text = |v: &Value, k: &str| {
            v.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string)
        };
        let identity = self.identity();
        let claims = scope::IdentityClaims {
            user_id: text(&identity, "userId"),
            group_id: text(&identity, "groupId"),
            groups: identity
                .get("groups")
                .and_then(Value::as_array)
                .map(|g| g.iter().filter_map(|s| s.as_str().map(str::to_string)).collect())
                .unwrap_or_default(),
            display_name: text(&identity, "displayName"),
        };
        let target = scope::CollabTarget {
            user_id: text(target, "userId"),
            group_id: text(target, "groupId"),
        };
        let parsed = scope::parse_all(raw).unwrap_or_default();
        scope::check_collab(action, &target, &parsed, &claims, self.bits())
    }
}

impl DocumentSecurity for FakeSecurity {
    #[cfg_attr(target_arch = "wasm32", allow(unused_variables))]
    fn allows_annotation(&self, action: &str, target: &Value) -> bool {
        self.spec["allowsAnnotation"][action].as_bool().unwrap_or_else(|| {
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(raw) = self.raw_scope() {
                if action == "set-group" {
                    let group = target.get("groupId").and_then(Value::as_str).unwrap_or("");
                    return self.set_group(scope::CollabEntity::Annotations, group, &raw);
                }
                if action != "create"
                    && target.get("subtype").and_then(Value::as_str) == Some("widget")
                {
                    return self.allows("doc.forms.modify");
                }
                let target = if action == "create" { self.identity() } else { target.clone() };
                return self.collab(action, &target, &raw);
            }
            if action == "set-group" {
                false
            } else {
                self.allows(if action == "create" {
                    "doc.annotate.create"
                } else {
                    "doc.annotate.modify"
                })
            }
        })
    }
    #[cfg_attr(target_arch = "wasm32", allow(unused_variables))]
    fn allows_field(&self, action: &str, target: &Value) -> bool {
        self.spec["allowsField"][action].as_bool().unwrap_or_else(|| {
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(raw) = self.raw_scope() {
                let group = target.get("groupId").and_then(Value::as_str);
                if action == "set-group" {
                    return self.set_group(scope::CollabEntity::Fields, group.unwrap_or(""), &raw);
                }
                let write = if action == "fill" {
                    scope::FieldWriteAction::Fill
                } else {
                    scope::FieldWriteAction::Sign
                };
                let parsed = scope::parse_all(&raw).unwrap_or_default();
                return scope::check_field_action(write, group, &parsed, self.bits());
            }
            if action == "set-group" {
                false
            } else {
                self.allows(if action == "fill" { "doc.forms.fill" } else { "doc.sign" })
            }
        })
    }

    fn allows(&self, capability: &str) -> bool {
        match self.spec.get("allows") {
            Some(Value::String(s)) if s == "*" => true,
            Some(Value::Array(list)) => list.iter().any(|c| c.as_str() == Some(capability)),
            Some(_) => false,
            #[cfg(target_arch = "wasm32")]
            None => false,
            #[cfg(not(target_arch = "wasm32"))]
            None => self.raw_scope().is_some_and(|raw| {
                let parsed = scope::parse_all(&raw).unwrap_or_default();
                scope::DocCapability::parse(capability)
                    .is_some_and(|c| scope::check_capability(c, &parsed, self.bits()))
            }),
        }
    }

    fn allows_annotation_create(&self) -> bool {
        self.spec.get("allowsAnnotationCreate").and_then(Value::as_bool).unwrap_or_else(|| {
            match self.raw_scope() {
                #[cfg(not(target_arch = "wasm32"))]
                Some(raw) => self.collab("create", &self.identity(), &raw),
                _ => self.allows("doc.annotate.create"),
            }
        })
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
        // `[...expandRawScope(rawScope, bits)].sort()`.
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(raw) = self.raw_scope() {
            let expanded = scope::expand_raw_scope(&raw, self.bits()).unwrap_or_default();
            let mut names: Vec<String> = expanded.iter().map(|c| c.as_str().to_string()).collect();
            names.sort();
            return names;
        }
        self.spec
            .get("scope")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect())
            .unwrap_or_default()
    }

    fn identity(&self) -> Value {
        self.spec.get("identity").cloned().unwrap_or(Value::Null)
    }

    fn current(&self) -> Value {
        self.state.borrow().0.clone()
    }
    fn password_prompt(&self) -> Value {
        self.state.borrow().1.clone()
    }

    fn unlock(&self, input: Value) -> EngineCall<Value> {
        let state = self.state.clone();
        let map = Rc::new(move |value: Value| {
            let security = value.get("security").cloned().unwrap_or_else(undefined);
            let prompt = password_prompt(&security);
            *state.borrow_mut() = (security, prompt);
            Ok(value)
        });
        self.engine.call(
            Some(&self.doc),
            self.table,
            "security.unlock",
            None,
            vec![input],
            None,
            Some(map),
            Some(self.publish.clone()),
            None,
        )
    }
}
