//! A `Set<() => void>` of callbacks, with the iteration rules JavaScript
//! gives `set.forEach`: a callback added during a pass is visited in that
//! pass, one removed during it is skipped, and adding the same function
//! twice keeps one entry. Unsubscribing answers `set.delete(cb)`: whether it
//! was still there.
//!
//! The hub's tool-change callbacks and selection's change and commit
//! callbacks are all this shape.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

pub type VoidCallback = Rc<dyn Fn()>;

#[derive(Default)]
struct Inner {
    entries: RefCell<Vec<Option<VoidCallback>>>,
    walking: Cell<u32>,
}

/// A callback set. Cheap to clone; clones share the set.
#[derive(Clone, Default)]
pub struct CallbackSet(Rc<Inner>);

fn same(a: &VoidCallback, b: &VoidCallback) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
}

impl CallbackSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// `set.add(cb)`, returning `() => set.delete(cb)`.
    pub fn add(&self, callback: VoidCallback) -> Unsubscriber {
        {
            let mut entries = self.0.entries.borrow_mut();
            if !entries.iter().flatten().any(|e| same(e, &callback)) {
                entries.push(Some(callback.clone()));
            }
        }
        let set = self.clone();
        Unsubscriber(Rc::new(move || set.delete(&callback)))
    }

    /// `set.delete(cb)`.
    pub fn delete(&self, callback: &VoidCallback) -> bool {
        let mut entries = self.0.entries.borrow_mut();
        let Some(slot) = entries.iter_mut().find(|e| e.as_ref().is_some_and(|e| same(e, callback)))
        else {
            return false;
        };
        *slot = None;
        if self.0.walking.get() == 0 {
            entries.retain(Option::is_some);
        }
        true
    }

    /// `set.forEach((cb) => cb())`.
    pub fn for_each(&self) {
        self.0.walking.set(self.0.walking.get() + 1);
        let mut index = 0;
        loop {
            let next = {
                let entries = self.0.entries.borrow();
                if index >= entries.len() {
                    break;
                }
                entries[index].clone()
            };
            index += 1;
            if let Some(callback) = next {
                callback();
            }
        }
        self.0.walking.set(self.0.walking.get() - 1);
        if self.0.walking.get() == 0 {
            self.0.entries.borrow_mut().retain(Option::is_some);
        }
    }

    /// Drop every callback: the Rust side's release of a closed document's
    /// subscribers (see the crate docs).
    pub fn clear(&self) {
        let mut entries = self.0.entries.borrow_mut();
        entries.iter_mut().for_each(|e| *e = None);
        if self.0.walking.get() == 0 {
            entries.clear();
        }
    }

    pub fn len(&self) -> usize {
        self.0.entries.borrow().iter().flatten().count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The function `onToolChange`, `onChange` and `onCommit` return. Calling it
/// answers whether the callback was still subscribed, as `Set.delete` does.
#[derive(Clone)]
pub struct Unsubscriber(Rc<dyn Fn() -> bool>);

impl Unsubscriber {
    pub fn call(&self) -> bool {
        (self.0)()
    }
}

/// The function a registry (`registerTool`, `registerHandler`) returns.
/// Callable any number of times; each call does what the TypeScript
/// closure does, which for `registerTool` is to delete by id again.
#[derive(Clone)]
pub struct Unregister(Rc<dyn Fn()>);

impl Unregister {
    pub fn new(run: impl Fn() + 'static) -> Self {
        Unregister(Rc::new(run))
    }

    pub fn call(&self) {
        (self.0)()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_callback_added_mid_pass_runs_and_a_removed_one_does_not() {
        let set = CallbackSet::new();
        let log = Rc::new(RefCell::new(Vec::new()));
        let late: VoidCallback = {
            let log = log.clone();
            Rc::new(move || log.borrow_mut().push("late"))
        };
        let removed: VoidCallback = {
            let log = log.clone();
            Rc::new(move || log.borrow_mut().push("removed"))
        };
        let first: VoidCallback = {
            let (log, set, late, removed) =
                (log.clone(), set.clone(), late.clone(), removed.clone());
            Rc::new(move || {
                log.borrow_mut().push("first");
                set.add(late.clone());
                set.delete(&removed);
            })
        };
        set.add(first);
        let off = set.add(removed.clone());
        set.for_each();
        assert_eq!(*log.borrow(), ["first", "late"]);
        assert!(!off.call());
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn the_same_callback_is_one_entry() {
        let set = CallbackSet::new();
        let cb: VoidCallback = Rc::new(|| {});
        let a = set.add(cb.clone());
        let b = set.add(cb);
        assert_eq!(set.len(), 1);
        assert!(!set.is_empty());
        assert!(a.call());
        assert!(!b.call());
        assert!(set.is_empty());
    }

    #[test]
    fn deleting_a_visited_callback_mid_pass_skips_nothing_after_it() {
        let set = CallbackSet::new();
        let log = Rc::new(RefCell::new(Vec::new()));
        let first: VoidCallback = {
            let log = log.clone();
            Rc::new(move || log.borrow_mut().push("first"))
        };
        let second: VoidCallback = {
            let (log, set, first) = (log.clone(), set.clone(), first.clone());
            Rc::new(move || {
                log.borrow_mut().push("second");
                set.delete(&first);
            })
        };
        let third: VoidCallback = {
            let log = log.clone();
            Rc::new(move || log.borrow_mut().push("third"))
        };
        set.add(first);
        set.add(second);
        set.add(third);
        set.for_each();
        assert_eq!(*log.borrow(), ["first", "second", "third"]);
    }

    #[test]
    fn clear_mid_pass_stops_the_pass() {
        let set = CallbackSet::new();
        let count = Rc::new(Cell::new(0));
        let (s, c) = (set.clone(), count.clone());
        set.add(Rc::new(move || {
            c.set(c.get() + 1);
            s.clear();
        }));
        let c2 = count.clone();
        set.add(Rc::new(move || c2.set(c2.get() + 10)));
        set.for_each();
        assert_eq!(count.get(), 1);
        assert!(set.is_empty());
    }
}
