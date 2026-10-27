//! A document's staged changes. Confirmed state belongs to mirrors; refusal
//! removes predictions rather than restoring an old snapshot.
use crate::{
    Error, Promise,
    error::{plugin_error, to_plugin_error},
    event_hook::EventHookSource,
    rt,
    store::Report,
};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    rc::Rc,
};

type Apply = Rc<dyn Fn(Value, Value) -> Promise<Value>>;
#[derive(Clone)]
pub struct StagedChange {
    pub label: Value,
    pub ops: Vec<Value>,
    pub undo_of: Option<String>,
    pub shows: Vec<Value>,
    pub undo: Vec<Value>,
    pub history: bool,
}
impl StagedChange {
    pub fn ops(label: Value, ops: Vec<Value>) -> Self {
        Self { label, shows: ops.clone(), ops, undo_of: None, undo: vec![], history: true }
    }
    pub fn undo(label: Value, undo_of: String, shows: Vec<Value>) -> Self {
        Self { label, ops: vec![], undo_of: Some(undo_of), shows, undo: vec![], history: false }
    }
}
#[derive(Clone)]
pub struct PendingChange(Rc<Entry>);
struct Entry {
    seq: u64,
    op_id: String,
    capability: String,
    label: Value,
    merge: Option<String>,
    change: RefCell<StagedChange>,
    held: Cell<bool>,
    answered: Cell<bool>,
    result: Promise<Value>,
    resolve: Rc<dyn Fn(Result<Value, Error>)>,
    abort: RefCell<Option<Rc<dyn Fn(Option<Value>)>>>,
}
impl PendingChange {
    pub fn seq(&self) -> u64 {
        self.0.seq
    }
    pub fn op_id(&self) -> &str {
        &self.0.op_id
    }
    pub fn label(&self) -> &Value {
        &self.0.label
    }
    pub fn merge(&self) -> Option<&str> {
        self.0.merge.as_deref()
    }
    pub fn held(&self) -> bool {
        self.0.held.get()
    }
    pub fn shows(&self) -> Vec<Value> {
        self.0.change.borrow().shows.clone()
    }
    pub fn undo(&self) -> Vec<Value> {
        self.0.change.borrow().undo.clone()
    }
    pub fn undo_of(&self) -> Option<String> {
        self.0.change.borrow().undo_of.clone()
    }
    pub fn history(&self) -> bool {
        self.0.change.borrow().history
    }
    pub fn result(&self) -> Promise<Value> {
        self.0.result.clone()
    }
    fn wire(&self) -> Value {
        let c = self.0.change.borrow();
        match &c.undo_of {
            Some(id) => json!({"undoOf":id}),
            None => json!({"ops":c.ops}),
        }
    }
}
#[derive(Clone)]
pub enum SettledChange {
    Applied { change: PendingChange, result: Value },
    Refused { change: PendingChange, error: Error },
}
#[derive(Clone)]
pub struct DocumentChanges(Rc<Inner>);
struct Inner {
    entries: RefCell<Vec<PendingChange>>,
    seq: Cell<u64>,
    version: Cell<u64>,
    closed: Cell<bool>,
    group: RefCell<Option<(Value, String, Option<PendingChange>)>>,
    apply: Apply,
    notify: Rc<dyn Fn()>,
    take: RefCell<Rc<dyn Fn() -> Result<Option<u32>, Error>>>,
    reserve: RefCell<Rc<dyn Fn(u32) -> Promise<()>>>,
    staged: EventHookSource<PendingChange>,
    settled: EventHookSource<SettledChange>,
}
impl DocumentChanges {
    pub fn new(apply: Apply, notify: Rc<dyn Fn()>, report: Report) -> Self {
        Self(Rc::new(Inner {
            entries: RefCell::new(vec![]),
            seq: Cell::new(0),
            version: Cell::new(0),
            closed: Cell::new(false),
            group: RefCell::new(None),
            apply,
            notify,
            take: RefCell::new(Rc::new(|| Ok(None))),
            reserve: RefCell::new(Rc::new(|_| {
                Promise::rejected(Error::engine(
                    "NotImplemented",
                    "object number reservation unavailable",
                ))
            })),
            staged: EventHookSource::new(Some(report.clone())),
            settled: EventHookSource::new(Some(report)),
        }))
    }
    pub fn set_object_numbers(
        &self,
        take: Rc<dyn Fn() -> Result<Option<u32>, Error>>,
        reserve: Rc<dyn Fn(u32) -> Promise<()>>,
    ) {
        *self.0.take.borrow_mut() = take;
        *self.0.reserve.borrow_mut() = reserve;
    }
    pub fn for_plugin(&self, capability: &str) -> ChangeQueue {
        ChangeQueue { changes: self.clone(), capability: capability.into() }
    }
    pub fn shown(&self) -> Vec<PendingChange> {
        self.0.entries.borrow().clone()
    }
    pub fn find(&self, id: &str) -> Option<PendingChange> {
        self.0.entries.borrow().iter().find(|e| e.op_id() == id).cloned()
    }
    pub fn version(&self) -> u64 {
        self.0.version.get()
    }
    fn changed(&self) {
        self.0.version.set(self.version() + 1);
        (self.0.notify)();
    }
    fn entry(
        &self,
        capability: &str,
        change: StagedChange,
        held: bool,
        merge: Option<String>,
    ) -> PendingChange {
        let op_id = crate::identity::generate_uuid_v7(None);
        let (result, resolver) = Promise::pending();
        let seq = self.0.seq.get();
        self.0.seq.set(seq + 1);
        PendingChange(Rc::new(Entry {
            seq,
            op_id,
            capability: capability.into(),
            label: change.label.clone(),
            change: RefCell::new(change),
            merge,
            held: Cell::new(held),
            answered: Cell::new(false),
            result,
            resolve: Rc::new(move |r| resolver.settle(r)),
            abort: RefCell::new(None),
        }))
    }
    fn remove(&self, e: &PendingChange) {
        self.0.entries.borrow_mut().retain(|c| c.op_id() != e.op_id());
    }
    fn refuse(&self, e: PendingChange, error: Error) {
        if e.0.answered.replace(true) {
            return;
        }
        self.remove(&e);
        (e.0.resolve)(Err(error.clone()));
        self.0.settled.emit(&SettledChange::Refused { change: e.clone(), error });
        let named = named_by(&e.wire());
        for later in self.shown() {
            if later.seq() > e.seq() && references(&later.wire()).iter().any(|n| named.contains(n))
            {
                let abort = later.0.abort.borrow().clone();
                if let Some(abort) = abort {
                    abort(None);
                }
                let error = plugin_error(
                    "conflict",
                    &later.0.capability,
                    format!(
                        "a change it depends on ({}) was refused",
                        e.label()["key"].as_str().unwrap_or("")
                    ),
                    None,
                    Some(json!({"reason":"dependency-refused","dependsOn":e.op_id()})),
                );
                self.refuse(later, error);
            }
        }
        self.changed();
    }
    fn send(&self, e: PendingChange) {
        e.0.held.set(false);
        let answer = (self.0.apply)(e.wire(), json!({"opId":e.op_id()}));
        *e.0.abort.borrow_mut() = answer.abort_handle();
        let this = self.clone();
        rt::spawn(async move {
            match answer.reaction().await {
                Ok(result) => {
                    if e.0.answered.replace(true) {
                        return;
                    }
                    this.remove(&e);
                    this.changed();
                    (e.0.resolve)(Ok(result.clone()));
                    this.0.settled.emit(&SettledChange::Applied { change: e, result });
                }
                Err(error) => {
                    let error = to_plugin_error(&e.0.capability, &error);
                    this.refuse(e, error);
                }
            }
        });
    }
    fn cancel(&self, e: &PendingChange) {
        if e.0.answered.replace(true) {
            return;
        }
        self.remove(e);
        (e.0.resolve)(Err(plugin_error(
            "operation-cancelled",
            &e.0.capability,
            "the change was cancelled",
            None,
            None,
        )));
        self.changed();
    }
    fn send_before(&self, before: u64) {
        for e in self.shown() {
            if e.held() && e.seq() < before {
                if e.0.change.borrow().ops.is_empty() {
                    self.cancel(&e);
                } else {
                    self.0.staged.emit(&e);
                    self.send(e);
                }
            }
        }
    }
    pub fn send_holds(&self) {
        self.send_before(u64::MAX);
    }
    /// `whenSettled`: `await Promise.allSettled(results)` in an `async`
    /// function, so it settles two turns after the last result (the
    /// `allSettled` reaction, then the `await`), and one turn after the call
    /// with nothing pending.
    pub fn when_settled(&self) -> Promise<()> {
        let pending: Vec<Promise<()>> = self
            .shown()
            .into_iter()
            .filter(|e| !e.held())
            .map(|e| e.result().convert(|_| Ok(())))
            .collect();
        Promise::spawn(async move {
            crate::kernel::all_settled(pending).await;
            Ok(())
        })
    }
    pub fn settle(&self) -> Promise<()> {
        self.send_holds();
        self.when_settled()
    }
    pub fn close(&self) {
        if self.0.closed.replace(true) {
            return;
        }
        let entries = std::mem::take(&mut *self.0.entries.borrow_mut());
        self.0.version.set(self.version() + 1);
        for e in entries {
            e.0.answered.set(true);
            (e.0.resolve)(Err(plugin_error(
                "instance-closed",
                &e.0.capability,
                "the document closed",
                None,
                None,
            )));
        }
        self.0.staged.dispose();
        self.0.settled.dispose();
    }
}
#[derive(Clone)]
pub struct ChangeQueue {
    changes: DocumentChanges,
    capability: String,
}
impl ChangeQueue {
    fn assert_open(&self) -> Result<(), Error> {
        if self.changes.0.closed.get() {
            Err(plugin_error(
                "instance-closed",
                &self.capability,
                "the document closed",
                None,
                None,
            ))
        } else {
            Ok(())
        }
    }
    fn invalid(&self, msg: &str) -> Error {
        plugin_error("invalid-input", &self.capability, msg, None, None)
    }
    pub fn stage(&self, change: StagedChange) -> Result<PendingChange, Error> {
        self.assert_open()?;
        if change.undo_of.is_none() && change.ops.is_empty() {
            return Err(self.invalid("a change needs at least one op"));
        }
        let grouping = self.changes.0.group.borrow().clone();
        if let Some((label, capability, entry)) = grouping {
            if change.undo_of.is_some() {
                return Err(self.invalid("an undo can not join a group"));
            }
            let e = if let Some(e) = entry {
                {
                    let mut current = e.0.change.borrow_mut();
                    current.ops.extend(change.ops);
                    current.shows = current.ops.clone();
                    let mut undo = change.undo;
                    undo.append(&mut current.undo);
                    current.undo = undo;
                    current.history |= change.history;
                }
                e
            } else {
                let mut change = change;
                change.label = label;
                let e = self.changes.entry(&capability, change, false, None);
                self.changes.0.entries.borrow_mut().push(e.clone());
                self.changes.0.group.borrow_mut().as_mut().unwrap().2 = Some(e.clone());
                e
            };
            self.changes.changed();
            return Ok(e);
        }
        self.changes.send_holds();
        let e = self.changes.entry(&self.capability, change, false, None);
        self.changes.0.entries.borrow_mut().push(e.clone());
        self.changes.changed();
        self.changes.0.staged.emit(&e);
        self.changes.send(e.clone());
        Ok(e)
    }
    pub fn hold(&self, label: Value, merge: Option<String>) -> Result<HeldChange, Error> {
        self.assert_open()?;
        if self.changes.0.group.borrow().is_some() {
            return Err(self.invalid("a change can not be held inside a group"));
        }
        Ok(HeldChange(Rc::new(Held {
            queue: self.clone(),
            label,
            merge,
            entry: RefCell::new(None),
            cancelled: Cell::new(false),
        })))
    }
    pub fn group<T>(
        &self,
        label: Value,
        run: impl FnOnce() -> Result<T, Error>,
    ) -> Result<T, Error> {
        self.assert_open()?;
        if self.changes.0.group.borrow().is_some() {
            return run();
        }
        self.changes.send_holds();
        *self.changes.0.group.borrow_mut() = Some((label, self.capability.clone(), None));
        let result = run();
        let (_, _, entry) = self.changes.0.group.borrow_mut().take().unwrap();
        if let Some(e) = entry {
            if result.is_err() {
                self.changes.cancel(&e);
            } else {
                self.changes.0.staged.emit(&e);
                self.changes.send(e);
            }
        }
        result
    }
    pub fn take_object_number(&self) -> Result<Option<u32>, Error> {
        self.assert_open()?;
        (self.changes.0.take.borrow())().map_err(|e| to_plugin_error(&self.capability, &e))
    }
    pub fn reserve_object_numbers(&self, count: u32) -> Promise<()> {
        if let Err(e) = self.assert_open() {
            return Promise::rejected(e);
        }
        let call = (self.changes.0.reserve.borrow())(count);
        let capability = self.capability.clone();
        Promise::spawn(async move { call.await.map_err(|e| to_plugin_error(&capability, &e)) })
    }
    pub fn pending(&self) -> Vec<PendingChange> {
        self.changes.shown()
    }
    pub fn has_pending(&self) -> bool {
        !self.pending().is_empty()
    }
    pub fn send_holds(&self) {
        self.changes.send_holds();
    }
    pub fn when_settled(&self) -> Promise<()> {
        self.changes.when_settled()
    }
    pub fn on_staged(&self) -> EventHookSource<PendingChange> {
        self.changes.0.staged.clone()
    }
    pub fn on_settled(&self) -> EventHookSource<SettledChange> {
        self.changes.0.settled.clone()
    }
}
struct Held {
    queue: ChangeQueue,
    label: Value,
    merge: Option<String>,
    entry: RefCell<Option<PendingChange>>,
    cancelled: Cell<bool>,
}
#[derive(Clone)]
pub struct HeldChange(Rc<Held>);
impl HeldChange {
    pub fn open(&self) -> bool {
        !self.0.queue.changes.0.closed.get()
            && !self.0.cancelled.get()
            && self.0.entry.borrow().as_ref().is_none_or(|e| e.held() && !e.0.answered.get())
    }
    pub fn change(&self) -> Option<PendingChange> {
        self.0.entry.borrow().clone()
    }
    pub fn set(&self, ops: Vec<Value>, undo: Vec<Value>) {
        if !self.open() {
            return;
        }
        let existing = self.change();
        if let Some(e) = existing {
            let mut c = e.0.change.borrow_mut();
            c.ops = ops.clone();
            c.shows = ops;
            c.undo = undo;
        } else {
            let mut c = StagedChange::ops(self.0.label.clone(), ops);
            c.undo = undo;
            let e =
                self.0.queue.changes.entry(&self.0.queue.capability, c, true, self.0.merge.clone());
            self.0.queue.changes.0.entries.borrow_mut().push(e.clone());
            *self.0.entry.borrow_mut() = Some(e);
        }
        self.0.queue.changes.changed();
    }
    pub fn send(&self) -> Option<PendingChange> {
        let e = self.change();
        if let Some(e) = &e
            && e.held()
        {
            self.0.queue.changes.send_before(e.seq() + 1);
        }
        e
    }
    pub fn cancel(&self) {
        self.0.cancelled.set(true);
        if let Some(e) = self.change()
            && e.held()
        {
            self.0.queue.changes.cancel(&e);
        }
    }
}
fn named_by(change: &Value) -> HashSet<u64> {
    let mut out = HashSet::new();
    if let Some(ops) = change["ops"].as_array() {
        for op in ops {
            match op["type"].as_str().unwrap_or("") {
                "annotations.create" | "forms.create" | "forms.addWidget" | "outline.create" => {
                    if let Some(n) = op["objectNumber"].as_u64() {
                        out.insert(n);
                    }
                    if op["type"] == "forms.addWidget"
                        && let Some(n) = op["splitObjectNumber"].as_u64()
                    {
                        out.insert(n);
                    }
                    if op["type"] == "forms.create"
                        && let Some(ns) = op["widgetObjectNumbers"].as_array()
                    {
                        out.extend(ns.iter().filter_map(Value::as_u64));
                    }
                }
                "pages.insertBlank" => {
                    if let Some(ns) = op["objectNumbers"].as_array() {
                        out.extend(ns.iter().filter_map(Value::as_u64));
                    }
                }
                _ => {}
            }
        }
    }
    out
}
fn references(value: &Value) -> HashSet<u64> {
    fn visit(v: &Value, out: &mut HashSet<u64>) {
        match v {
            Value::Array(a) => {
                for v in a {
                    visit(v, out);
                }
            }
            Value::Object(o) => {
                if v["kind"] == "objectNumber"
                    && let Some(n) = v["objectNumber"].as_u64()
                {
                    out.insert(n);
                }
                for v in o.values() {
                    visit(v, out);
                }
            }
            _ => {}
        }
    }
    let mut out = HashSet::new();
    visit(value, &mut out);
    out
}
