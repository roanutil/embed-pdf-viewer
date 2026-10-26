//! The `fields` mirror holds JSON (`FieldIndex::to_mirror`): the snapshot
//! and the picture versions. Every read wants the index with its records,
//! so each mirror value is decoded once and kept beside it while it is
//! recent. An entry holds its value, so a value's address names it for as
//! long as the entry lives.

use std::cell::RefCell;
use std::rc::Rc;

use serde_json::Value;

use crate::model::FieldIndex;

/// How many recent values keep their index.
const KEPT: usize = 32;

#[derive(Default)]
pub struct IndexCache {
    entries: RefCell<Vec<(Rc<Value>, Rc<FieldIndex>)>>,
}

impl IndexCache {
    fn keep(&self, value: Rc<Value>, index: Rc<FieldIndex>) {
        let mut entries = self.entries.borrow_mut();
        entries.push((value, index));
        if entries.len() > KEPT {
            entries.remove(0);
        }
    }

    /// The index a mirror value holds.
    pub fn index(&self, value: &Rc<Value>) -> Rc<FieldIndex> {
        if let Some((_, index)) = self.entries.borrow().iter().find(|(v, _)| Rc::ptr_eq(v, value)) {
            return index.clone();
        }
        let index = Rc::new(FieldIndex::from_mirror(value));
        self.keep(value.clone(), index.clone());
        index
    }

    /// The mirror value of an index: the one it was read from, or a new one.
    pub fn value(&self, index: &Rc<FieldIndex>) -> Rc<Value> {
        if let Some((value, _)) = self.entries.borrow().iter().find(|(_, i)| Rc::ptr_eq(i, index)) {
            return value.clone();
        }
        let value = Rc::new(index.to_mirror());
        self.keep(value.clone(), index.clone());
        value
    }
}
