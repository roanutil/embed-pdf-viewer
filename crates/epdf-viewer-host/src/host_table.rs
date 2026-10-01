//! Bytes that must not cross as base64: a raster's pixels, a PDF being
//! opened. Kernel-thread local, like the runtime.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

type Table = (u64, HashMap<u64, Arc<Vec<u8>>>);

thread_local! {
    static TABLE: RefCell<Table> = RefCell::new((0, HashMap::new()));
}

pub struct HostTable;

impl HostTable {
    pub fn put(bytes: Vec<u8>) -> u64 {
        TABLE.with(|t| {
            let mut t = t.borrow_mut();
            t.0 += 1;
            let id = t.0;
            t.1.insert(id, Arc::new(bytes));
            id
        })
    }

    pub fn get(id: u64) -> Option<Arc<Vec<u8>>> {
        TABLE.with(|t| t.borrow().1.get(&id).cloned())
    }

    pub fn take(id: u64) -> Option<Vec<u8>> {
        TABLE
            .with(|t| t.borrow_mut().1.remove(&id))
            .map(|a| Arc::try_unwrap(a).unwrap_or_else(|a| (*a).clone()))
    }

    pub fn release(id: u64) {
        TABLE.with(|t| t.borrow_mut().1.remove(&id));
    }
}
