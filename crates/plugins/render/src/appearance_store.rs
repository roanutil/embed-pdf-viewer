//! `appearance-store.ts`: the engine's pictures of a page's annotations or
//! form widgets, shared between the views and plugins that draw them. One
//! ask is one key (the page, the family, the scale and the owner's items at
//! their versions): asks with the same key share one fetch.
//!
//! Consumer aborts are refcounted, as in the raster store: a view that
//! leaves detaches without stopping the fetch for the others, and the fetch
//! stops only when the last one leaves before it resolved.
//!
//! Kept for undo: the last [`SETS_KEPT`] sets of each page and family, and
//! each item's last pictures ([`RecentPictures`]) a while after it left its
//! page.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use epdf_kernel::Error;
use epdf_kernel::rt::{self, Promise};
use epdf_kernel::signal::AbortSignal;
use indexmap::IndexMap;

/// How many of each page's latest sets are kept, per family.
pub const SETS_KEPT: usize = 4;

/// How many items' last pictures are kept, per family.
pub const ITEMS_KEPT: usize = 200;

struct Entry<P> {
    group: String,
    promise: RefCell<Option<Promise<Rc<Vec<P>>>>>,
    refs: Cell<i64>,
    abort: AbortSignal,
    resolved: Cell<bool>,
}

struct Inner<P> {
    entries: RefCell<HashMap<String, Rc<Entry<P>>>>,
    /// The keys of the resolved sets kept for each page and family, newest
    /// last.
    kept: RefCell<HashMap<String, Vec<String>>>,
}

/// `AppearanceStore<Picture>`. Cheap to clone; clones share the store.
pub struct AppearanceStore<P>(Rc<Inner<P>>);

impl<P> Clone for AppearanceStore<P> {
    fn clone(&self) -> Self {
        AppearanceStore(self.0.clone())
    }
}

impl<P: Clone + 'static> Default for AppearanceStore<P> {
    fn default() -> Self {
        AppearanceStore(Rc::new(Inner {
            entries: RefCell::new(HashMap::new()),
            kept: RefCell::new(HashMap::new()),
        }))
    }
}

impl<P: Clone + 'static> AppearanceStore<P> {
    /// `acquire(group, key, fetch, signal?)`: `group` names the page and
    /// family, `key` the whole ask.
    pub fn acquire(
        &self,
        group: &str,
        key: &str,
        fetch: impl FnOnce(&AbortSignal) -> Promise<Vec<P>>,
        signal: Option<&AbortSignal>,
    ) -> Promise<Rc<Vec<P>>> {
        let existing = self.0.entries.borrow().get(key).cloned();
        if let Some(existing) = existing {
            if existing.resolved.get() {
                self.keep(group, key);
            }
            return self.attach(key, &existing, signal);
        }

        let entry = Rc::new(Entry {
            group: group.to_string(),
            promise: RefCell::new(None),
            refs: Cell::new(0),
            abort: AbortSignal::new(),
            resolved: Cell::new(false),
        });
        let task = fetch(&entry.abort);
        let (store, settled) = (self.clone(), entry.clone());
        let (group, owned_key) = (group.to_string(), key.to_string());
        let promise = Promise::spawn(async move {
            match task.await {
                Ok(pictures) => {
                    settled.resolved.set(true);
                    store.keep(&group, &owned_key);
                    Ok(Rc::new(pictures))
                }
                Err(error) => {
                    // A failed fetch isn't kept: the next ask tries again.
                    store.drop_entry(&owned_key, &settled);
                    Err(error)
                }
            }
        });
        // Each consumer sees the rejection through its own attachment.
        promise.ignore();
        *entry.promise.borrow_mut() = Some(promise);
        self.0.entries.borrow_mut().insert(key.to_string(), entry.clone());
        self.attach(key, &entry, signal)
    }

    /// Keep `key` as the group's newest set, letting go of the oldest beyond
    /// the few kept.
    fn keep(&self, group: &str, key: &str) {
        let gone: Vec<String> = {
            let mut kept = self.0.kept.borrow_mut();
            let list = kept.entry(group.to_string()).or_default();
            list.retain(|each| each != key);
            list.push(key.to_string());
            let excess = list.len().saturating_sub(SETS_KEPT);
            list.drain(..excess).collect()
        };
        for old in gone {
            let mut entries = self.0.entries.borrow_mut();
            if entries.get(&old).is_some_and(|entry| entry.refs.get() <= 0) {
                entries.remove(&old);
            }
        }
    }

    fn is_kept(&self, group: &str, key: &str) -> bool {
        self.0.kept.borrow().get(group).is_some_and(|list| list.iter().any(|k| k == key))
    }

    fn attach(
        &self,
        key: &str,
        entry: &Rc<Entry<P>>,
        signal: Option<&AbortSignal>,
    ) -> Promise<Rc<Vec<P>>> {
        if let Some(signal) = signal
            && let Some(reason) = signal.reason()
        {
            return Promise::rejected(reason);
        }
        entry.refs.set(entry.refs.get() + 1);
        let shared = entry.promise.borrow().clone().expect("entry promise is set before attach");
        let Some(signal) = signal else {
            let (store, entry, key) = (self.clone(), entry.clone(), key.to_string());
            let awaited = shared.clone();
            rt::spawn(async move {
                let _ = awaited.await;
                store.release(&key, &entry);
            });
            return shared;
        };
        let (consumer, resolver) = Promise::pending();
        let settled = Rc::new(Cell::new(false));
        let on_abort = {
            let (settled, entry, store) = (settled.clone(), entry.clone(), self.clone());
            let (key, resolver) = (key.to_string(), resolver.clone());
            move |reason: &Error| {
                if settled.replace(true) {
                    return;
                }
                entry.refs.set(entry.refs.get() - 1);
                if entry.refs.get() <= 0 && !entry.resolved.get() {
                    entry.abort.abort(reason.clone());
                    store.drop_entry(&key, &entry);
                }
                resolver.reject(reason.clone());
            }
        };
        let listener = signal.on_abort(Rc::new(on_abort));
        let (store, entry, key, signal) =
            (self.clone(), entry.clone(), key.to_string(), signal.clone());
        rt::spawn(async move {
            let result = shared.await;
            if settled.replace(true) {
                return;
            }
            store.release(&key, &entry);
            signal.remove(listener);
            resolver.settle(result);
        });
        consumer
    }

    /// A consumer let go: a set no longer kept goes with the last one.
    fn release(&self, key: &str, entry: &Rc<Entry<P>>) {
        entry.refs.set(entry.refs.get() - 1);
        if entry.refs.get() <= 0 && entry.resolved.get() && !self.is_kept(&entry.group, key) {
            self.drop_entry(key, entry);
        }
    }

    fn drop_entry(&self, key: &str, entry: &Rc<Entry<P>>) {
        let mut entries = self.0.entries.borrow_mut();
        if entries.get(key).is_some_and(|e| Rc::ptr_eq(e, entry)) {
            entries.remove(key);
        }
    }

    /// Entries held, in flight or resolved (diagnostics and tests).
    pub fn size(&self) -> usize {
        self.0.entries.borrow().len()
    }
}

/// `RecentPictures<Picture>`: the last pictures of each item (all its
/// states), the newest [`ITEMS_KEPT`] items per family, kept after the item
/// left its page.
pub struct RecentPictures<P> {
    families: RefCell<HashMap<String, IndexMap<String, Rc<Vec<P>>>>>,
}

impl<P> Default for RecentPictures<P> {
    fn default() -> Self {
        RecentPictures { families: RefCell::new(HashMap::new()) }
    }
}

impl<P: Clone> RecentPictures<P> {
    /// A fetch's pictures, grouped by item: each replaces what its item had.
    pub fn remember(&self, family: &str, pictures: &[P], key_of: impl Fn(&P) -> String) {
        let mut by_item: IndexMap<String, Vec<P>> = IndexMap::new();
        for picture in pictures {
            by_item.entry(key_of(picture)).or_default().push(picture.clone());
        }
        let mut families = self.families.borrow_mut();
        let items = families.entry(family.to_string()).or_default();
        for (key, item_pictures) in by_item {
            items.shift_remove(&key);
            items.insert(key, Rc::new(item_pictures));
        }
        while items.len() > ITEMS_KEPT {
            items.shift_remove_index(0);
        }
    }

    /// `of(family, key)`: an item's last pictures, empty for one never drawn.
    pub fn of(&self, family: &str, key: &str) -> Rc<Vec<P>> {
        self.families
            .borrow()
            .get(family)
            .and_then(|items| items.get(key).cloned())
            .unwrap_or_default()
    }
}
