//! Confirmed document values with pending changes replayed only in the view.
use crate::{
    Error, Promise,
    changes::{DocumentChanges, PendingChange},
    engine::DocumentHandle,
    event_hook::EventHookSource,
    rt,
    signal::AbortSignal,
    store::{Report, SliceLease, Store, Unsubscribe},
};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::Rc,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceStatus {
    Idle,
    Loading,
    Ready,
    Forbidden,
    Error,
}
impl ResourceStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Loading => "loading",
            Self::Ready => "ready",
            Self::Forbidden => "forbidden",
            Self::Error => "error",
        }
    }
}
pub fn status_of_failure(e: &Error) -> ResourceStatus {
    if e.is_plugin_error(Some("permission-denied")) {
        ResourceStatus::Forbidden
    } else {
        ResourceStatus::Error
    }
}
pub fn is_resync(e: &Value) -> bool {
    matches!(e["type"].as_str(), Some("stream.desynced" | "document.versioned"))
}
pub fn change_id_of(e: &Value) -> Option<&str> {
    e.pointer("/origin/tx/id").and_then(Value::as_str)
}
pub fn last_event_of(e: &Value) -> Option<&str> {
    let tx = e.pointer("/origin/tx")?;
    let count = tx["count"].as_u64()?;
    (count > 0 && tx["index"].as_u64() == Some(count - 1)).then(|| tx["id"].as_str()).flatten()
}
#[derive(Clone)]
pub struct Loaded {
    pub value: Value,
    pub cursor: Option<u64>,
}
#[derive(Clone)]
pub enum Fold {
    Value(Rc<Value>),
    Reload,
    Pages(Vec<Value>),
}
#[derive(Clone)]
pub struct MirrorChange {
    pub cause: &'static str,
    pub event: Option<Value>,
    pub pages: Value,
    pub previous: Rc<Value>,
    pub next: Rc<Value>,
}
pub type Predict = Rc<dyn Fn(Rc<Value>, &Value) -> Result<Rc<Value>, Error>>;
pub type Load = Rc<dyn Fn(Option<Rc<dyn DocumentHandle>>, AbortSignal) -> Promise<Loaded>>;
pub type Merge = Rc<dyn Fn(Rc<Value>) -> Rc<Value>>;
pub struct MirrorSpec {
    pub name: String,
    pub initial: Value,
    pub readable: Option<Rc<dyn Fn() -> bool>>,
    pub on_demand: bool,
    pub load: Load,
    pub fold: Rc<dyn Fn(Rc<Value>, &Value) -> Result<Fold, Error>>,
    pub predict: Option<Predict>,
    pub load_pages: Option<
        Rc<dyn Fn(Option<Rc<dyn DocumentHandle>>, Vec<Value>, AbortSignal) -> Promise<Merge>>,
    >,
    pub changed: Option<Rc<dyn Fn(MirrorChange) -> Result<(), Error>>>,
}
impl MirrorSpec {
    pub fn new(
        name: &str,
        initial: Value,
        load: Load,
        fold: Rc<dyn Fn(Rc<Value>, &Value) -> Result<Fold, Error>>,
    ) -> Self {
        Self {
            name: name.into(),
            initial,
            load,
            fold,
            readable: None,
            on_demand: false,
            predict: None,
            load_pages: None,
            changed: None,
        }
    }
}
#[derive(Clone)]
pub struct MirrorEnvironment {
    pub doc: Option<Rc<dyn DocumentHandle>>,
    pub lifetime: AbortSignal,
    pub changes: DocumentChanges,
    pub events: EventHookSource<Value>,
    pub cell: Rc<dyn Fn(&str, Value) -> SliceLease>,
    pub notify: Rc<dyn Fn()>,
    pub report: Report,
}
impl MirrorEnvironment {
    /// A standalone environment for embeddings without a kernel store.
    pub fn detached(
        events: EventHookSource<Value>,
        lifetime: AbortSignal,
        changes: DocumentChanges,
    ) -> Self {
        let store = Store::new(Rc::new(|_| {}));
        Self {
            doc: None,
            lifetime,
            changes,
            events,
            cell: Rc::new(move |name, initial| {
                store.lease(
                    name,
                    Rc::new(|_, _| None),
                    Rc::new(initial),
                    Rc::new(|v| v.downcast_ref::<Value>().unwrap().clone()),
                    None,
                )
            }),
            notify: Rc::new(|| {}),
            report: Rc::new(|_| {}),
        }
    }
}
#[derive(Clone)]
pub struct Mirror(Rc<Inner>);
struct Inner {
    spec: MirrorSpec,
    env: MirrorEnvironment,
    cell: SliceLease,
    value: RefCell<Rc<Value>>,
    status: Cell<ResourceStatus>,
    started: Cell<bool>,
    running: RefCell<Option<Promise<()>>>,
    again: Cell<bool>,
    queued: RefCell<Option<Vec<Value>>>,
    loads: Cell<u64>,
    pages: RefCell<Vec<Promise<()>>>,
    kept: RefCell<HashMap<String, (PendingChange, u64)>>,
    folded: RefCell<HashSet<String>>,
    version: Cell<u64>,
    memo: RefCell<Option<(Rc<Value>, u64, u64, Rc<Value>)>>,
    subscription: RefCell<Option<Unsubscribe>>,
}
impl Mirror {
    pub fn new(spec: MirrorSpec, env: MirrorEnvironment) -> Self {
        let cell = (env.cell)(
            &format!("mirror/{}", spec.name),
            json!({"value":spec.initial,"status":"idle"}),
        );
        let initial = Rc::new(spec.initial.clone());
        let this = Self(Rc::new(Inner {
            spec,
            env: env.clone(),
            cell: cell.clone(),
            value: RefCell::new(initial),
            status: Cell::new(ResourceStatus::Idle),
            started: Cell::new(false),
            running: RefCell::new(None),
            again: Cell::new(false),
            queued: RefCell::new(None),
            loads: Cell::new(0),
            pages: RefCell::new(vec![]),
            kept: RefCell::new(HashMap::new()),
            folded: RefCell::new(HashSet::new()),
            version: Cell::new(0),
            memo: RefCell::new(None),
            subscription: RefCell::new(None),
        }));
        let weak = Rc::downgrade(&this.0);
        let off = env.events.on(move |e| {
            if let Some(inner) = weak.upgrade() {
                Mirror(inner).event(e);
            }
            Ok(())
        });
        *this.0.subscription.borrow_mut() = Some(off.clone());
        env.lifetime.on_abort(Rc::new(move |_| {
            cell.revoke();
            off.call();
        }));
        this
    }
    pub fn get(&self) -> Rc<Value> {
        self.0.value.borrow().clone()
    }
    pub fn get_status(&self) -> ResourceStatus {
        self.0.status.get()
    }
    fn live(&self) -> bool {
        self.0.cell.live() && !self.0.env.lifetime.aborted()
    }
    fn write(&self, value: Rc<Value>, status: ResourceStatus) -> bool {
        if !self.live() {
            return false;
        }
        *self.0.value.borrow_mut() = value.clone();
        self.0.status.set(status);
        self.0.cell.write(Rc::new(json!({"value":*value,"status":status.as_str()})))
    }
    fn announce(&self, change: MirrorChange) {
        if let Some(f) = &self.0.spec.changed {
            if let Err(e) = f(change) {
                (self.0.env.report)(&e);
            }
        }
    }
    pub fn view(&self) -> Rc<Value> {
        let value = self.get();
        let Some(predict) = &self.0.spec.predict else { return value };
        let version = self.0.env.changes.version();
        let kept_version = self.0.version.get();
        if let Some((old, v, k, view)) = &*self.0.memo.borrow() {
            if Rc::ptr_eq(old, &value) && *v == version && *k == kept_version {
                return view.clone();
            }
        }
        let shown = showing(
            &self.0.env.changes,
            &self.0.kept.borrow().values().map(|(c, _)| c.clone()).collect::<Vec<_>>(),
            &self.0.folded.borrow(),
        );
        let view = replay(value.clone(), shown, predict, &self.0.env.report);
        *self.0.memo.borrow_mut() = Some((value, version, kept_version, view.clone()));
        view
    }
    fn keep(&self, event: &Value, after: u64) -> Option<String> {
        if self.0.spec.predict.is_none() {
            return None;
        }
        let id = change_id_of(event)?;
        let c = self.0.env.changes.find(id)?;
        self.0.kept.borrow_mut().insert(id.into(), (c, after));
        self.0.version.set(self.0.version.get() + 1);
        Some(id.into())
    }
    fn release(&self, ids: Vec<String>) {
        let mut any = false;
        {
            let mut kept = self.0.kept.borrow_mut();
            for id in ids {
                any |= kept.remove(&id).is_some();
            }
        }
        if any {
            self.0.version.set(self.0.version.get() + 1);
            (self.0.env.notify)();
        }
    }
    fn event(&self, event: &Value) {
        if !self.live() {
            return;
        }
        if is_resync(event) {
            if self.0.started.get() {
                self.refresh();
            }
            return;
        }
        if self.0.queued.borrow().is_some() {
            self.0.queued.borrow_mut().as_mut().unwrap().push(event.clone());
            self.keep(event, self.0.loads.get().saturating_sub(1));
        } else {
            self.apply(event);
        }
    }
    fn apply(&self, event: &Value) {
        let previous = self.get();
        let folded = (self.0.spec.fold)(previous.clone(), event);
        match folded {
            Err(error) => {
                (self.0.env.report)(&error);
                if !self.0.spec.on_demand || self.0.started.get() {
                    self.refresh();
                }
            }
            Ok(Fold::Value(next)) => {
                if self.0.spec.predict.is_some() {
                    if let Some(id) = last_event_of(event) {
                        if self.0.env.changes.find(id).is_some() {
                            let mut folded = self.0.folded.borrow_mut();
                            folded.retain(|id| self.0.env.changes.find(id).is_some());
                            if folded.insert(id.into()) {
                                self.0.version.set(self.0.version.get() + 1);
                            }
                        }
                    }
                }
                if !Rc::ptr_eq(&previous, &next) && self.write(next.clone(), self.get_status()) {
                    self.announce(MirrorChange {
                        cause: "event",
                        event: Some(event.clone()),
                        pages: Value::Null,
                        previous,
                        next,
                    });
                }
            }
            Ok(reload) => {
                if self.0.spec.on_demand && !self.0.started.get() {
                    return;
                }
                let kept = self.keep(event, self.0.loads.get());
                if let (Fold::Pages(pages), Some(load)) = (reload, &self.0.spec.load_pages) {
                    let call =
                        load(self.0.env.doc.clone(), pages.clone(), self.0.env.lifetime.clone());
                    let this = self.clone();
                    let task = Promise::spawn(async move {
                        match call.await {
                            Ok(merge) => {
                                if this.live() {
                                    let previous = this.get();
                                    let next = merge(previous.clone());
                                    this.write(next.clone(), this.get_status());
                                    this.announce(MirrorChange {
                                        cause: "load",
                                        event: None,
                                        pages: json!(pages),
                                        previous,
                                        next,
                                    });
                                    if let Some(id) = kept {
                                        this.release(vec![id]);
                                    }
                                }
                            }
                            Err(e) => {
                                if this.live() && !e.is_plugin_error(Some("instance-closed")) {
                                    this.write(this.get(), status_of_failure(&e));
                                    (this.0.env.report)(&e);
                                }
                            }
                        }
                        Ok(())
                    });
                    self.0.pages.borrow_mut().push(task);
                } else {
                    self.refresh();
                }
            }
        }
    }
    async fn load_once(&self) -> Result<(), Error> {
        if self.0.spec.readable.as_ref().is_some_and(|f| !f()) {
            self.write(self.get(), ResourceStatus::Forbidden);
            return Ok(());
        }
        if self.get_status() != ResourceStatus::Ready {
            self.write(self.get(), ResourceStatus::Loading);
        }
        let load = self.0.loads.get() + 1;
        self.0.loads.set(load);
        *self.0.queued.borrow_mut() = Some(vec![]);
        let result = (self.0.spec.load)(self.0.env.doc.clone(), self.0.env.lifetime.clone()).await;
        let pending = self.0.queued.borrow_mut().take().unwrap_or_default();
        if !self.live() {
            return Ok(());
        }
        match result {
            Err(error) => {
                if error.is_plugin_error(Some("instance-closed")) {
                    return Ok(());
                }
                self.write(self.get(), status_of_failure(&error));
                for e in pending {
                    self.apply(&e);
                }
                Err(error)
            }
            Ok(loaded) => {
                let previous = self.get();
                let next = Rc::new(loaded.value);
                self.write(next.clone(), ResourceStatus::Ready);
                self.announce(MirrorChange {
                    cause: "load",
                    event: None,
                    pages: json!("all"),
                    previous,
                    next,
                });
                for e in pending {
                    let server = e.pointer("/origin/serverId").and_then(Value::as_u64);
                    if loaded.cursor.zip(server).is_some_and(|(cursor, id)| id <= cursor) {
                        continue;
                    }
                    self.apply(&e);
                }
                let release = self
                    .0
                    .kept
                    .borrow()
                    .iter()
                    .filter(|(_, (_, after))| *after < load)
                    .map(|(id, _)| id.clone())
                    .collect();
                self.release(release);
                Ok(())
            }
        }
    }
    pub fn refresh(&self) -> Promise<()> {
        self.0.started.set(true);
        if let Some(running) = self.0.running.borrow().clone() {
            self.0.again.set(true);
            return running;
        }
        let (promise, resolver) = Promise::pending();
        *self.0.running.borrow_mut() = Some(promise.clone());
        let this = self.clone();
        rt::spawn(async move {
            let result = loop {
                this.0.again.set(false);
                let result = this.load_once().await;
                if !this.0.again.get() || !this.live() {
                    break result;
                }
            };
            this.0.running.borrow_mut().take();
            resolver.settle(result);
        });
        promise
    }
    pub fn ensure_loaded(&self) -> Promise<()> {
        if let Some(r) = self.0.running.borrow().clone() {
            return r;
        }
        if self.get_status() == ResourceStatus::Ready {
            return Promise::resolved(());
        }
        self.refresh()
    }
    pub fn start(&self) {
        if !self.0.started.get() && !self.0.spec.on_demand {
            self.refresh();
        }
    }
    pub fn settled(&self) -> Promise<()> {
        let this = self.clone();
        Promise::spawn(async move {
            loop {
                let running = this.0.running.borrow().clone();
                let pages = std::mem::take(&mut *this.0.pages.borrow_mut());
                if running.is_none() && pages.is_empty() {
                    break;
                }
                if let Some(r) = running {
                    let _ = r.await;
                }
                for p in pages {
                    let _ = p.await;
                }
            }
            Ok(())
        })
    }
}
pub(crate) fn showing(
    changes: &DocumentChanges,
    kept: &[PendingChange],
    folded: &HashSet<String>,
) -> Vec<PendingChange> {
    let mut shown: Vec<_> =
        changes.shown().into_iter().filter(|e| !folded.contains(e.op_id())).collect();
    for e in kept {
        if let Some(i) = shown.iter().position(|c| c.op_id() == e.op_id()) {
            shown[i] = e.clone();
        } else {
            shown.push(e.clone());
        }
    }
    shown.sort_by_key(PendingChange::seq);
    shown
}
pub(crate) fn replay(
    value: Rc<Value>,
    changes: Vec<PendingChange>,
    predict: &Predict,
    report: &Report,
) -> Rc<Value> {
    let mut next = value.clone();
    for c in changes {
        for op in c.shows() {
            match predict(next, &op) {
                Ok(v) => next = v,
                Err(e) => {
                    report(&e);
                    return value;
                }
            }
        }
    }
    next
}
