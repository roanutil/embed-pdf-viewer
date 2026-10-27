//! `services/object-numbers.ts`: the object numbers a new field and its
//! widgets take: final names from the document's pool (`ctx.changes`), so a
//! field shows under the ref it keeps from its first frame. A few taken and
//! not used yet wait here for the next create; one the engine reclaims
//! leaves.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use epdf_kernel::Error;
use epdf_kernel::rt::Promise;

#[derive(Default)]
pub struct ObjectNumbers {
    held: Rc<RefCell<Vec<u32>>>,
    watching: Cell<bool>,
}

impl crate::controller::Core {
    fn watch_lost(&self) {
        if self.numbers.watching.get() {
            return;
        }
        let Some(doc) = self.doc() else { return };
        self.numbers.watching.set(true);
        let held = self.numbers.held.clone();
        let off = doc.on_lost_object_numbers(Rc::new(move |lost| {
            held.borrow_mut().retain(|n| !lost.numbers.contains(n));
        }));
        self.cx.listen(off);
    }

    /// `takeNow(count)`: `count` numbers now, or `None` when the pool has
    /// too few (it refills as it goes).
    pub(crate) fn take_numbers_now(&self, count: usize) -> Result<Option<Vec<u32>>, Error> {
        self.watch_lost();
        let changes = self.cx.changes()?;
        while self.numbers.held.borrow().len() < count {
            match changes.take_object_number()? {
                Some(number) => self.numbers.held.borrow_mut().push(number),
                None => return Ok(None),
            }
        }
        Ok(Some(self.numbers.held.borrow_mut().drain(..count).collect()))
    }

    /// `take(count)`: `count` numbers, once the pool has them.
    pub(crate) fn take_numbers(&self, count: usize) -> Promise<Vec<u32>> {
        let me = self.rc();
        Promise::spawn(async move {
            loop {
                if let Some(taken) = me.take_numbers_now(count)? {
                    return Ok(taken);
                }
                let missing = count - me.numbers.held.borrow().len();
                me.cx.changes()?.reserve_object_numbers(missing as u32).await?;
            }
        })
    }
}
