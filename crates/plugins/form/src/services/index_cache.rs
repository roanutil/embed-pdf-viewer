//! The `fields` mirror's values. The TypeScript mirror holds the
//! `FieldIndex` object itself; the kernel's mirror holds JSON and copies
//! each value it writes, so a value here is a small token,
//! `{ "$fieldIndex": n }`, naming an index this cache keeps. Folds,
//! predictions and reads then pass the index by pointer, and no write
//! copies the form (M21).
//!
//! The cache keeps the [`KEPT`] indexes read or written last, and always
//! the confirmed one (`pin`). A value without a token is the empty index:
//! the mirror's initial value.

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
            // Only a value older than every kept one can miss, and the mirror
            // holds none that old: its confirmed value is pinned and its view
            // is replayed from it.
            debug_assert!(false, "[form] the fields mirror named an index it no longer keeps");
            return self.empty.clone();
        };
        entries.insert(id, index.clone());
        index
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

    /// Keep the index a value names for as long as it is the confirmed one.
    pub fn pin(&self, value: &Value) {
        self.pinned.set(token_of(value));
    }
}
