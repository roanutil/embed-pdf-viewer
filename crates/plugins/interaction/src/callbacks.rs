//! `Unregister`: the function a registry (`registerTool`, `registerHandler`)
//! returns.

use std::rc::Rc;

/// Callable any number of times; each call does what the TypeScript closure
/// does, which for `registerTool` is to delete by id again.
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
