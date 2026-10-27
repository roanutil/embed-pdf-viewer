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
    /// The last index [`IndexCache::to_text`] wrote, held so its address
    /// can't be reused, with its text.
    text: RefCell<Option<(Rc<FieldIndex>, Rc<str>)>>,
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
        self.shown(value).to_json()
    }

    /// [`to_json`](Self::to_json) as text, for a host that reads the store
    /// as text (`epdf-web`'s `state_json` and `slice_text`). The text of
    /// the last index shown is kept, so the store cell's rewrites that keep
    /// the form (a status change) and every refetch reuse it: one
    /// serialisation per confirmed form (M38).
    pub fn to_text(&self, value: &Value) -> Rc<str> {
        let index = self.shown(value);
        if let Some((shown, text)) = &*self.text.borrow()
            && Rc::ptr_eq(shown, &index)
        {
            return text.clone();
        }
        let text: Rc<str> = index.to_text().into();
        *self.text.borrow_mut() = Some((index, text.clone()));
        text
    }

    fn shown(&self, value: &Value) -> Rc<FieldIndex> {
        match token_of(value) {
            None => self.empty.clone(),
            Some(id) => {
                let kept = self.entries.borrow().get(&id).cloned();
                kept.unwrap_or_else(|| self.missed(id))
            }
        }
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

    /// The text of an index is written once and kept while the cell shows
    /// it, whichever token names it, and is the JSON's text byte for byte.
    #[test]
    fn the_text_of_a_shown_index_is_written_once() {
        let cache = IndexCache::default();
        let index = Rc::new(FieldIndex {
            by_key: [("obj:2".into(), 1.0), ("obj:1".into(), 0.0)].into(),
            ..Default::default()
        });
        let token = cache.value(&index);
        let before = crate::model::index_serialisations();
        let text = cache.to_text(&token);
        assert_eq!(&*text, cache.to_json(&token).to_string().as_str());
        let after = crate::model::index_serialisations();
        assert_eq!(after, before + 2, "one text, one value");
        assert!(Rc::ptr_eq(&text, &cache.to_text(&token)));
        assert!(Rc::ptr_eq(&text, &cache.to_text(&cache.value(&index))), "another token");
        assert_eq!(crate::model::index_serialisations(), after);
        let other = Rc::new(FieldIndex::default());
        assert_ne!(cache.to_text(&cache.value(&other)), text);
    }
}
