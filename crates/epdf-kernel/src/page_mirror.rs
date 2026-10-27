//! Page-scoped reads, guarded by epochs so stale loads cannot replace newer facts.
use crate::{
    Error, Promise,
    changes::PendingChange,
    engine::DocumentHandle,
    mirror::{
        Fold, MirrorEnvironment, Predict, ResourceStatus, change_id_of, is_resync, last_event_of,
        replay, showing, status_of_failure,
    },
    rt,
    signal::AbortSignal,
    store::{SliceLease, Unsubscribe},
};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::Rc,
};
#[derive(Clone)]
pub enum Affected {
    None,
    All,
    Pages(Vec<Value>),
}
#[derive(Clone)]
pub struct PageMirrorChange {
    pub page: Value,
    pub cause: &'static str,
    pub event: Option<Value>,
    pub previous: Option<Rc<Value>>,
    pub next: Option<Rc<Value>>,
}
pub struct PageMirrorSpec {
    pub name: String,
    pub load: Rc<dyn Fn(Option<Rc<dyn DocumentHandle>>, Value, AbortSignal) -> Promise<Value>>,
    pub affected: Rc<dyn Fn(&Value) -> Affected>,
    pub fold: Option<Rc<dyn Fn(Rc<Value>, &Value, &Value) -> Result<Fold, Error>>>,
    pub predict: Option<Rc<dyn Fn(Rc<Value>, &Value, &Value) -> Result<Rc<Value>, Error>>>,
    pub changed: Option<Rc<dyn Fn(PageMirrorChange) -> Result<(), Error>>>,
}
#[derive(Clone)]
pub(crate) struct Entry {
    /// Shared, so copying the entries on write copies a pointer.
    page: Rc<Value>,
    value: Option<Rc<Value>>,
    status: ResourceStatus,
}
struct Inner {
    spec: PageMirrorSpec,
    env: MirrorEnvironment,
    cell: SliceLease,
    /// Shared with the store cell's [`PageMirrorCell`], which the store
    /// keeps until the next write, so every write copies the map: each
    /// entry's key, its page and read by pointer, and its status. That is
    /// the O(n) of `page-mirror.ts`'s `{ ...rest, [key]: entry }`, and no
    /// read or page JSON is copied.
    entries: RefCell<Rc<indexmap::IndexMap<String, Entry>>>,
    epochs: RefCell<HashMap<String, u64>>,
    running: RefCell<HashMap<String, Promise<()>>>,
    kept: RefCell<HashMap<String, HashMap<String, (PendingChange, u64)>>>,
    folded: RefCell<HashSet<String>>,
    version: Cell<u64>,
    views: RefCell<HashMap<String, (Rc<Value>, u64, u64, Rc<Value>)>>,
    off: RefCell<Option<Unsubscribe>>,
}
#[derive(Clone)]
pub struct PageMirror(Rc<Inner>);
thread_local! {
    static VALUE_COPIES: Cell<usize> = const { Cell::new(0) };
}
/// How many page reads this thread has deep-copied into a page mirror's
/// store cell. The guard (`tests/current_contracts.rs`) holds it at none
/// across loads and at one per page per store dump.
#[doc(hidden)]
pub fn page_cell_value_copies() -> usize {
    VALUE_COPIES.with(Cell::get)
}
fn count_value_copy() {
    VALUE_COPIES.with(|n| n.set(n.get() + 1));
}
/// A page mirror's store cell: its entries, kept by pointer and serialised
/// only when the store is dumped (`mirror::cell_json`), as
/// `{ [key]: { page, status, value? } }`.
pub struct PageMirrorCell(Rc<indexmap::IndexMap<String, Entry>>);
impl PageMirrorCell {
    pub(crate) fn to_json(&self) -> Value {
        let cells: serde_json::Map<String, Value> = self
            .0
            .iter()
            .map(|(k, e)| {
                let mut value = json!({"page":*e.page,"status":e.status.as_str()});
                if let Some(v) = &e.value {
                    count_value_copy();
                    value["value"] = (**v).clone();
                }
                (k.clone(), value)
            })
            .collect();
        Value::Object(cells)
    }
}
fn key(page: &Value) -> String {
    format!("obj:{}", page["objectNumber"])
}
impl PageMirror {
    pub fn new(spec: PageMirrorSpec, env: MirrorEnvironment) -> Self {
        let cell = (env.cell)(&format!("page-mirror/{}", spec.name), json!({}));
        let this = Self(Rc::new(Inner {
            spec,
            env: env.clone(),
            cell: cell.clone(),
            entries: RefCell::new(Rc::new(indexmap::IndexMap::new())),
            epochs: RefCell::new(HashMap::new()),
            running: RefCell::new(HashMap::new()),
            kept: RefCell::new(HashMap::new()),
            folded: RefCell::new(HashSet::new()),
            version: Cell::new(0),
            views: RefCell::new(HashMap::new()),
            off: RefCell::new(None),
        }));
        let weak = Rc::downgrade(&this.0);
        let off = env.events.on(move |e| {
            if let Some(inner) = weak.upgrade() {
                PageMirror(inner).event(e);
            }
            Ok(())
        });
        *this.0.off.borrow_mut() = Some(off.clone());
        env.lifetime.on_abort(Rc::new(move |_| {
            cell.revoke();
            off.call();
        }));
        this
    }
    fn live(&self) -> bool {
        self.0.cell.live() && !self.0.env.lifetime.aborted()
    }
    fn entry(&self, key: &str) -> Option<Entry> {
        self.0.entries.borrow().get(key).cloned()
    }
    fn write(&self, k: String, entry: Option<Entry>) {
        if !self.live() {
            return;
        }
        let entries = {
            let mut entries = self.0.entries.borrow_mut();
            let map = Rc::make_mut(&mut entries);
            map.shift_remove(&k);
            if let Some(entry) = entry {
                map.insert(k, entry);
            }
            entries.clone()
        };
        self.0.cell.write(Rc::new(PageMirrorCell(entries)));
    }
    fn bump(&self, k: &str) -> u64 {
        let mut epochs = self.0.epochs.borrow_mut();
        let next = epochs.get(k).copied().unwrap_or(0) + 1;
        epochs.insert(k.into(), next);
        next
    }
    fn announce(&self, e: PageMirrorChange) {
        if let Some(changed) = &self.0.spec.changed {
            if let Err(error) = changed(e) {
                (self.0.env.report)(&error);
            }
        }
    }
    pub fn get(&self, page: &Value) -> Option<Rc<Value>> {
        self.entry(&key(page)).and_then(|e| e.value)
    }
    pub fn get_status(&self, page: &Value) -> ResourceStatus {
        self.entry(&key(page)).map_or(ResourceStatus::Idle, |e| e.status)
    }
    fn loaded(&self) -> Vec<Value> {
        self.0
            .entries
            .borrow()
            .values()
            .filter(|e| e.status != ResourceStatus::Idle)
            .map(|e| (*e.page).clone())
            .collect()
    }
    pub fn view(&self, page: &Value) -> Option<Rc<Value>> {
        let value = self.get(page)?;
        let Some(predict) = &self.0.spec.predict else { return Some(value) };
        let k = key(page);
        let version = self.0.env.changes.version();
        let kept_version = self.0.version.get();
        if let Some((old, v, kv, view)) = self.0.views.borrow().get(&k) {
            if Rc::ptr_eq(old, &value) && *v == version && *kv == kept_version {
                return Some(view.clone());
            }
        }
        let kept = self
            .0
            .kept
            .borrow()
            .get(&k)
            .map(|m| m.values().map(|(c, _)| c.clone()).collect::<Vec<_>>())
            .unwrap_or_default();
        let shown = showing(&self.0.env.changes, &kept, &self.0.folded.borrow());
        let (predict, page) = (predict.clone(), page.clone());
        let predict: Predict = Rc::new(move |value, op| predict(value, op, &page));
        let next = replay(value.clone(), shown, &predict, &self.0.env.report);
        self.0.views.borrow_mut().insert(k, (value, version, kept_version, next.clone()));
        Some(next)
    }
    fn keep(&self, k: &str, event: &Value) {
        if self.0.spec.predict.is_none() {
            return;
        }
        if let Some(id) = change_id_of(event) {
            if let Some(change) = self.0.env.changes.find(id) {
                let epoch = self.0.epochs.borrow().get(k).copied().unwrap_or(0);
                self.0
                    .kept
                    .borrow_mut()
                    .entry(k.into())
                    .or_default()
                    .insert(id.into(), (change, epoch));
                self.0.version.set(self.0.version.get() + 1);
            }
        }
    }
    fn load(&self, page: Value) -> Promise<()> {
        let k = key(&page);
        let epoch = self.bump(&k);
        let before = self.entry(&k);
        if before.as_ref().is_none_or(|e| e.status != ResourceStatus::Ready) {
            self.write(
                k.clone(),
                Some(Entry {
                    page: Rc::new(page.clone()),
                    status: ResourceStatus::Loading,
                    value: before.and_then(|e| e.value),
                }),
            );
        }
        let call =
            (self.0.spec.load)(self.0.env.doc.clone(), page.clone(), self.0.env.lifetime.clone());
        let (promise, resolver) = Promise::pending();
        self.0.running.borrow_mut().insert(k.clone(), promise.clone());
        let this = self.clone();
        let own = promise.clone();
        rt::spawn(async move {
            let result = call.reaction().await;
            let current = this.live() && this.0.epochs.borrow().get(&k) == Some(&epoch);
            let result = if !current {
                Ok(())
            } else {
                match result {
                    Ok(value) => {
                        let previous = this.get(&page);
                        let next = Rc::new(value);
                        this.write(
                            k.clone(),
                            Some(Entry {
                                page: Rc::new(page.clone()),
                                status: ResourceStatus::Ready,
                                value: Some(next.clone()),
                            }),
                        );
                        this.announce(PageMirrorChange {
                            page: page.clone(),
                            cause: "load",
                            event: None,
                            previous,
                            next: Some(next),
                        });
                        let released = {
                            let mut kept = this.0.kept.borrow_mut();
                            if let Some(entries) = kept.get_mut(&k) {
                                let before = entries.len();
                                entries.retain(|_, (_, e)| *e >= epoch);
                                before != entries.len()
                            } else {
                                false
                            }
                        };
                        if released {
                            this.0.version.set(this.0.version.get() + 1);
                            (this.0.env.notify)();
                        }
                        Ok(())
                    }
                    Err(error) => {
                        if error.is_plugin_error(Some("instance-closed")) {
                            Ok(())
                        } else {
                            this.write(
                                k.clone(),
                                Some(Entry {
                                    page: Rc::new(page.clone()),
                                    status: status_of_failure(&error),
                                    value: this.get(&page),
                                }),
                            );
                            Err(error)
                        }
                    }
                }
            };
            // `.finally(() => inFlight.delete(key))` (page-mirror.ts:194):
            // its reaction takes a turn, then the promise it returns is
            // adopted, which takes the TypeScript's two more.
            rt::tick().await;
            let mine = this.0.running.borrow().get(&k).is_some_and(|r| r.same(&own));
            if mine {
                this.0.running.borrow_mut().remove(&k);
            }
            rt::tick().await;
            rt::tick().await;
            resolver.settle(result);
        });
        promise
    }
    pub fn ensure_loaded(&self, page: Value) -> Promise<()> {
        if let Some(r) = self.0.running.borrow().get(&key(&page)) {
            return r.clone();
        }
        if self.get_status(&page) == ResourceStatus::Ready {
            return Promise::resolved(());
        }
        self.load(page)
    }
    pub fn refresh(&self, page: Option<Value>) -> Promise<()> {
        if let Some(page) = page {
            return self.load(page);
        }
        let loads: Vec<_> = self.loaded().into_iter().map(|p| self.load(p)).collect();
        Promise::spawn(async move {
            for load in loads {
                load.await?;
            }
            Ok(())
        })
    }
    fn event(&self, event: &Value) {
        if !self.live() {
            return;
        }
        self.fold_event(event);
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
    }
    fn fold_event(&self, event: &Value) {
        if is_resync(event) {
            for page in self.loaded() {
                self.load(page);
            }
            return;
        }
        if event["type"] == "pages.deleted" {
            if let Some(pages) = event["pages"].as_array() {
                for page in pages {
                    let k = key(page);
                    if let Some(before) = self.entry(&k) {
                        self.bump(&k);
                        self.0.kept.borrow_mut().remove(&k);
                        self.0.views.borrow_mut().remove(&k);
                        self.0.running.borrow_mut().remove(&k);
                        self.write(k, None);
                        if before.value.is_some() {
                            self.announce(PageMirrorChange {
                                page: page.clone(),
                                cause: "drop",
                                event: Some(event.clone()),
                                previous: before.value,
                                next: None,
                            });
                        }
                    }
                }
            }
            return;
        }
        let pages = match (self.0.spec.affected)(event) {
            Affected::None => return,
            Affected::All => self.loaded(),
            Affected::Pages(p) => p,
        };
        for page in pages {
            let k = key(&page);
            let Some(entry) = self.entry(&k) else {
                continue;
            };
            if entry.status == ResourceStatus::Idle {
                continue;
            }
            let result = match (&self.0.spec.fold, &entry.value) {
                (Some(fold), Some(value)) if entry.status == ResourceStatus::Ready => {
                    fold(value.clone(), event, &page)
                }
                _ => Ok(Fold::Reload),
            };
            let result = match result {
                Ok(r) => r,
                Err(error) => {
                    (self.0.env.report)(&error);
                    Fold::Reload
                }
            };
            match result {
                Fold::Value(next) => {
                    if entry.value.as_ref().is_some_and(|v| Rc::ptr_eq(v, &next)) {
                        continue;
                    }
                    self.bump(&k);
                    self.0.running.borrow_mut().remove(&k);
                    self.write(
                        k,
                        Some(Entry {
                            page: Rc::new(page.clone()),
                            status: ResourceStatus::Ready,
                            value: Some(next.clone()),
                        }),
                    );
                    self.announce(PageMirrorChange {
                        page,
                        cause: "event",
                        event: Some(event.clone()),
                        previous: entry.value,
                        next: Some(next),
                    });
                }
                _ => {
                    self.keep(&k, event);
                    self.load(page);
                }
            }
        }
    }
}
