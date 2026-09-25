//! `raster-store.ts`: per-key singleflight plus a cost-budgeted LRU over
//! encoded page images, shared by full-page renders and tiles.
//!
//! Consumer aborts are refcounted: a caller abandoning a fetch detaches
//! without killing it for others, and the engine call aborts only when the
//! last in-flight consumer leaves. Resolved entries stay (evictable by LRU),
//! so an abort after resolution does nothing.
//!
//! The browser objects a handle can lead to (an object URL, a decoded
//! bitmap) are not here: the store only reads a handle's `source`, `width`
//! and `height` to cost it, and hands the rest back to the caller as the
//! engine's JSON.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use epdf_kernel::Error;
use epdf_kernel::bytes::Bytes;
use epdf_kernel::rt::{self, Promise};
use epdf_kernel::signal::AbortSignal;
use indexmap::IndexMap;
use serde::{Serialize, Serializer};
use serde_json::Value;

/// A `PageImageHandle` as the engine answered it: the result JSON (`source`
/// as `{kind: 'bytes', bytes}` or `{kind: 'url', url}`, `width`, `height`,
/// and whatever else the engine put there). Cheap to clone.
#[derive(Clone, Debug, PartialEq)]
pub struct PageImageHandle(Rc<Value>);

impl PageImageHandle {
    pub fn new(value: Value) -> Self {
        PageImageHandle(Rc::new(value))
    }

    pub fn json(&self) -> &Value {
        &self.0
    }

    /// The encoded bytes, when the handle carries them (local engines).
    pub fn bytes(&self) -> Option<Bytes> {
        let source = self.0.get("source")?;
        if source.get("kind").and_then(Value::as_str) != Some("bytes") {
            return None;
        }
        Bytes::from_json(source.get("bytes")?)
    }

    /// Approximate memory cost: encoded bytes when known, the pixel count as
    /// a decoded-order estimate for URL handles, 1 when nothing is known.
    pub fn cost(&self) -> f64 {
        let source = self.0.get("source");
        if source.and_then(|s| s.get("kind")).and_then(Value::as_str) == Some("bytes") {
            return self.bytes().map_or(f64::NAN, |b| b.0.len() as f64);
        }
        match (self.0.get("width"), self.0.get("height")) {
            (Some(width), Some(height)) => js_number(width) * js_number(height),
            _ => 1.0,
        }
    }
}

/// `Number(value)` for what a handle's `width` and `height` can hold.
fn js_number(value: &Value) -> f64 {
    match value {
        Value::Null => 0.0,
        Value::Bool(b) => f64::from(u8::from(*b)),
        Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        _ => f64::NAN,
    }
}

impl Serialize for PageImageHandle {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(s)
    }
}

struct Entry {
    promise: RefCell<Option<Promise<PageImageHandle>>>,
    handle: RefCell<Option<PageImageHandle>>,
    /// Approximate bytes, set at resolution.
    cost: Cell<Option<f64>>,
    refs: Cell<i64>,
    abort: AbortSignal,
    used: Cell<u64>,
}

struct Inner {
    entries: RefCell<IndexMap<String, Rc<Entry>>>,
    tick: Cell<u64>,
    total: Cell<f64>,
    max_cost: f64,
}

/// `RasterStore`. Cheap to clone; clones share the store.
#[derive(Clone)]
pub struct RasterStore(Rc<Inner>);

impl Default for RasterStore {
    fn default() -> Self {
        RasterStore::new(128.0 * 1024.0 * 1024.0)
    }
}

impl RasterStore {
    pub fn new(max_cost: f64) -> Self {
        RasterStore(Rc::new(Inner {
            entries: RefCell::new(IndexMap::new()),
            tick: Cell::new(0),
            total: Cell::new(0.0),
            max_cost,
        }))
    }

    fn next_tick(&self) -> u64 {
        let tick = self.0.tick.get() + 1;
        self.0.tick.set(tick);
        tick
    }

    fn get(&self, key: &str) -> Option<Rc<Entry>> {
        self.0.entries.borrow().get(key).cloned()
    }

    /// `acquire(key, fetch, signal?)`. `fetch` runs synchronously when the
    /// key is new and answers the engine call in flight, whose value becomes
    /// the handle; an `Err` from it is the TypeScript's synchronous throw and
    /// leaves nothing behind.
    pub fn acquire(
        &self,
        key: &str,
        fetch: impl FnOnce(&AbortSignal) -> Result<Promise<Value>, Error>,
        signal: Option<&AbortSignal>,
    ) -> Result<Promise<PageImageHandle>, Error> {
        if let Some(existing) = self.get(key) {
            existing.used.set(self.next_tick());
            return Ok(self.attach(key, &existing, signal));
        }

        let entry = Rc::new(Entry {
            promise: RefCell::new(None),
            handle: RefCell::new(None),
            cost: Cell::new(None),
            refs: Cell::new(0),
            abort: AbortSignal::new(),
            used: Cell::new(self.next_tick()),
        });
        let task = fetch(&entry.abort)?;
        let store = self.clone();
        let settled = entry.clone();
        let owned_key = key.to_string();
        let promise = Promise::spawn(async move {
            match task.await {
                Ok(value) => {
                    let handle = PageImageHandle::new(value);
                    *settled.handle.borrow_mut() = Some(handle.clone());
                    let cost = handle.cost();
                    settled.cost.set(Some(cost));
                    store.0.total.set(store.0.total.get() + cost);
                    // Cost is known only now: the budget check is at resolution.
                    store.evict();
                    Ok(handle)
                }
                Err(error) => {
                    // A failed fetch must not be sticky.
                    store.drop_entry(&owned_key, &settled);
                    Err(error)
                }
            }
        });
        // The shared promise's rejection is observed by each consumer
        // through its own attachment.
        promise.ignore();
        *entry.promise.borrow_mut() = Some(promise);
        self.0.entries.borrow_mut().insert(key.to_string(), entry.clone());
        self.evict();
        Ok(self.attach(key, &entry, signal))
    }

    /// The resolved handle, if present: a synchronous peek for paint-time
    /// reuse. Freshens the entry for the LRU.
    pub fn peek(&self, key: &str) -> Option<PageImageHandle> {
        let entry = self.get(key)?;
        let handle = entry.handle.borrow().clone();
        if handle.is_some() {
            entry.used.set(self.next_tick());
        }
        handle
    }

    /// Entries held, in flight or resolved.
    pub fn size(&self) -> usize {
        self.0.entries.borrow().len()
    }

    /// Total cost (~bytes) of resolved entries: the budgeted quantity.
    pub fn cost_used(&self) -> f64 {
        self.0.total.get()
    }

    fn attach(
        &self,
        key: &str,
        entry: &Rc<Entry>,
        signal: Option<&AbortSignal>,
    ) -> Promise<PageImageHandle> {
        if let Some(signal) = signal
            && let Some(reason) = signal.reason()
        {
            return Promise::rejected(reason);
        }
        // Every consumer holds a ref while the fetch is in flight, signal-less
        // ones included.
        entry.refs.set(entry.refs.get() + 1);
        let shared = entry.promise.borrow().clone().expect("entry promise is set before attach");
        let Some(signal) = signal else {
            let release = entry.clone();
            let awaited = shared.clone();
            rt::spawn(async move {
                let _ = awaited.await;
                release.refs.set(release.refs.get() - 1);
            });
            return shared;
        };
        let (consumer, resolver) = Promise::pending();
        let settled = Rc::new(Cell::new(false));
        let on_abort = {
            let settled = settled.clone();
            let entry = entry.clone();
            let store = self.clone();
            let key = key.to_string();
            let resolver = resolver.clone();
            move |reason: &Error| {
                if settled.get() {
                    return;
                }
                settled.set(true);
                entry.refs.set(entry.refs.get() - 1);
                // The last live consumer leaving an unresolved fetch kills it.
                if entry.refs.get() <= 0 && entry.handle.borrow().is_none() {
                    entry.abort.abort(reason.clone());
                    store.drop_entry(&key, &entry);
                }
                resolver.reject(reason.clone());
            }
        };
        let listener = signal.on_abort(Rc::new(on_abort));
        let entry = entry.clone();
        let signal = signal.clone();
        rt::spawn(async move {
            let result = shared.await;
            if settled.get() {
                return;
            }
            settled.set(true);
            entry.refs.set(entry.refs.get() - 1);
            signal.remove(listener);
            resolver.settle(result);
        });
        consumer
    }

    /// Remove an entry, keeping the cost ledger honest.
    fn drop_entry(&self, key: &str, entry: &Rc<Entry>) {
        let mut entries = self.0.entries.borrow_mut();
        if !entries.get(key).is_some_and(|e| Rc::ptr_eq(e, entry)) {
            return;
        }
        entries.shift_remove(key);
        if let Some(cost) = entry.cost.get() {
            self.0.total.set(self.0.total.get() - cost);
        }
    }

    fn evict(&self) {
        if self.0.total.get() <= self.0.max_cost {
            return;
        }
        // Oldest first over evictable entries: resolved and consumer-free.
        let mut candidates: Vec<(String, Rc<Entry>)> = self
            .0
            .entries
            .borrow()
            .iter()
            .filter(|(_, e)| e.refs.get() <= 0 && e.handle.borrow().is_some())
            .map(|(k, e)| (k.clone(), e.clone()))
            .collect();
        candidates.sort_by_key(|(_, e)| e.used.get());
        for (key, entry) in candidates {
            if self.0.total.get() <= self.0.max_cost {
                return;
            }
            self.drop_entry(&key, &entry);
        }
    }
}
