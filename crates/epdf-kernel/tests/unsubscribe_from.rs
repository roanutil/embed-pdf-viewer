//! A plugin remover (`Rc<dyn Fn()>`) converts into an `Unsubscribe`, which
//! runs it once.

use std::cell::Cell;
use std::rc::Rc;

use epdf_kernel::Unsubscribe;

#[test]
fn a_remover_converts_and_runs_once() {
    let calls = Rc::new(Cell::new(0));
    let c = calls.clone();
    let remove: Rc<dyn Fn()> = Rc::new(move || c.set(c.get() + 1));
    let off: Unsubscribe = remove.into();
    off.call();
    off.call();
    assert_eq!(calls.get(), 1);
}
