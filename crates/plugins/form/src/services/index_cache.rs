//! The `fields` mirror's values. The TypeScript mirror holds the
//! `FieldIndex` object itself; the kernel's mirror holds JSON and copies
//! each value it writes, so a value here is a small token,
//! `{ "$fieldIndex": n }`, naming an index this cache keeps. Folds,
//! predictions and reads then pass the index by pointer, and no write
//! copies the form (M21).
//!
//! The cache keeps the [`KEPT`] indexes read or written last, and always
//! the confirmed one (`pin`). A value without a token is the empty index:
//! the mirror's initial value. The store shows the index itself, as the
//! TypeScript's mirror cell does: [`IndexCache::to_json`] is the fields
//! mirror's `to_json`, which the kernel calls only when the store is dumped.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use indexmap::IndexMap;
use serde_json::{Value, json};

use crate::model::FieldIndex;

/// How many recent indexes the cache keeps beside the pinned one.
const KEPT: usize = 256;

/// The token's member.
const TOKEN: &str = "$fieldIndex";

#[derive(Default)]
pub struct IndexCache {
    next: Cell<u64>,
    /// Least recently used first.
    entries: RefCell<IndexMap<u64, Rc<FieldIndex>>>,
    pinned: Cell<Option<u64>>,
    empty: Rc<FieldIndex>,
}

fn token_of(value: &Value) -> Option<u64> {
    value.get(TOKEN).and_then(Value::as_u64)
}

impl IndexCache {
    /// The mirror's initial value: the empty index.
    pub fn empty_value() -> Value {
        json!({})
    }

    /// The index a mirror value names.
    pub fn index(&self, value: &Value) -> Rc<FieldIndex> {
        let Some(id) = token_of(value) else { return self.empty.clone() };
        let mut entries = self.entries.borrow_mut();
        let Some(index) = entries.shift_remove(&id) else {
            drop(entries);
            return self.missed(id);
        };
        entries.insert(id, index.clone());
        index
    }

    /// A token older than every kept index. The mirror holds none that old:
    /// its confirmed value is pinned and its view is replayed from it. The
    /// TypeScript's mirror holds the `FieldIndex` itself and can't miss, so
    /// the nearest reading is the confirmed form: say so, and show it
    /// rather than none (the empty form only when nothing is confirmed).
    fn missed(&self, id: u64) -> Rc<FieldIndex> {
        let fallback = self.pinned.get().and_then(|p| self.entries.borrow().get(&p).cloned());
        crate::console::error(
            "[form] the fields mirror named a form it no longer keeps:",
            json!({ "token": id, "fallback": if fallback.is_some() { "confirmed" } else { "empty" } }),
        );
        fallback.unwrap_or_else(|| self.empty.clone())
    }

    /// The mirror value naming an index: the token it already has, or a new
    /// one.
    pub fn value(&self, index: &Rc<FieldIndex>) -> Rc<Value> {
        if Rc::ptr_eq(index, &self.empty) {
            return Rc::new(Self::empty_value());
        }
        let mut entries = self.entries.borrow_mut();
        let found = entries.iter().rev().find(|(_, i)| Rc::ptr_eq(i, index)).map(|(id, _)| *id);
        let id = match found {
            Some(id) => {
                let kept = entries.shift_remove(&id).expect("found above");
                entries.insert(id, kept);
                id
            }
            None => {
                let id = self.next.get();
                self.next.set(id + 1);
                entries.insert(id, index.clone());
                id
            }
        };
        while entries.len() > KEPT {
            let pinned = self.pinned.get();
            let Some(oldest) = entries.keys().copied().find(|id| Some(*id) != pinned) else {
                break;
            };
            entries.shift_remove(&oldest);
        }
        Rc::new(json!({ TOKEN: id }))
    }

    /// The mirror cell as the TypeScript's store shows it: the whole
    /// `FieldIndex` a value names (`FieldIndex::to_json`), read without
    /// touching the recency order. A miss reads as [`IndexCache::index`]'s
    /// does.
    pub fn to_json(&self, value: &Value) -> Value {
        let index = match token_of(value) {
            None => self.empty.clone(),
            Some(id) => {
                let kept = self.entries.borrow().get(&id).cloned();
                kept.unwrap_or_else(|| self.missed(id))
            }
        };
        index.to_json()
    }

    /// Keep the index a value names for as long as it is the confirmed one.
    pub fn pin(&self, value: &Value) {
        self.pinned.set(token_of(value));
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    #[test]
    fn a_miss_is_logged_and_shows_the_confirmed_form() {
        let logged: Rc<RefCell<Vec<Vec<Value>>>> = Rc::default();
        let sink = logged.clone();
        let previous = crate::console::set_sink(Some(Rc::new(move |_, args| {
            sink.borrow_mut().push(args);
        })));
        let cache = IndexCache::default();
        let confirmed =
            Rc::new(FieldIndex { by_key: [("obj:1".into(), 0.0)].into(), ..Default::default() });
        let value = cache.value(&confirmed);
        cache.pin(&value);
        let missed = cache.index(&json!({ TOKEN: 999 }));
        // The store dump reads a miss the same way.
        let shown = cache.to_json(&json!({ TOKEN: 999 }));
        crate::console::set_sink(previous);
        assert!(Rc::ptr_eq(&missed, &confirmed));
        assert_eq!(shown, confirmed.to_json());
        assert_ne!(shown, FieldIndex::default().to_json());
        let logged = logged.borrow();
        assert_eq!(logged.len(), 2);
        for line in logged.iter() {
            assert_eq!(line[0], json!("[form] the fields mirror named a form it no longer keeps:"));
            assert_eq!(line[1], json!({ "token": 999, "fallback": "confirmed" }));
        }
    }
}
