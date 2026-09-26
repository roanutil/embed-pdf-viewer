//! `persistence.ts`: keeping custom libraries, over the plugin's public
//! API. The plugin knows when a library changes (`onLibraryChanged`) and
//! what its canonical bytes are (`exportLibrary`); where those bytes live is
//! the embedder's [`StampLibraryStore`].

use std::cell::RefCell;
use std::rc::Rc;

use epdf_kernel::bytes::Bytes;
use epdf_kernel::rt::{self, Promise, TimerId};
use epdf_kernel::{Error, Unsubscribe};

use crate::contract::{StampLibraryChange, StampLibraryChangeReason};
use crate::controller::StampCapability;

/// One stored library.
#[derive(Clone, Debug, PartialEq)]
pub struct StoredLibrary {
    pub id: String,
    pub bytes: Vec<u8>,
}

/// `StampLibraryStore`: bytes by library id.
pub trait StampLibraryStore {
    fn list(&self) -> Promise<Vec<StoredLibrary>>;
    fn put(&self, id: &str, bytes: Vec<u8>) -> Promise<()>;
    fn delete(&self, id: &str) -> Promise<()>;
}

/// `memoryStampStore()`: an in-memory store, for tests and hosts without
/// storage. Rows list in insertion order, as a `Map` iterates.
#[derive(Clone, Default)]
pub struct MemoryStampStore {
    rows: Rc<RefCell<Vec<StoredLibrary>>>,
}

/// `memoryStampStore()`.
pub fn memory_stamp_store() -> MemoryStampStore {
    MemoryStampStore::default()
}

impl StampLibraryStore for MemoryStampStore {
    fn list(&self) -> Promise<Vec<StoredLibrary>> {
        let rows = self.rows.clone();
        Promise::spawn(async move { Ok(rows.borrow().clone()) })
    }

    fn put(&self, id: &str, bytes: Vec<u8>) -> Promise<()> {
        let (rows, id) = (self.rows.clone(), id.to_string());
        Promise::spawn(async move {
            let mut rows = rows.borrow_mut();
            match rows.iter_mut().find(|r| r.id == id) {
                Some(row) => row.bytes = bytes,
                None => rows.push(StoredLibrary { id, bytes }),
            }
            Ok(())
        })
    }

    fn delete(&self, id: &str) -> Promise<()> {
        let (rows, id) = (self.rows.clone(), id.to_string());
        Promise::spawn(async move {
            rows.borrow_mut().retain(|r| r.id != id);
            Ok(())
        })
    }
}

/// The `console.warn` the helpers report to.
pub type Warn = Option<Rc<dyn Fn(&str, &Error)>>;

fn warn(sink: &Warn, message: &str, error: &Error) {
    if let Some(sink) = sink {
        sink(message, error);
    }
}

/// `restoreStampLibraries(stamp, store)`: import every stored library as it
/// is; one that fails is reported and skipped. Answers the restored ids.
pub fn restore_stamp_libraries(
    stamp: &StampCapability,
    store: Rc<dyn StampLibraryStore>,
    report: Warn,
) -> Promise<Vec<String>> {
    let stamp = stamp.clone();
    Promise::spawn(async move {
        let mut restored = Vec::new();
        for row in store.list().await? {
            let source = Bytes(row.bytes).to_json();
            match stamp.import_library(source, Default::default()).await {
                Ok(id) => restored.push(id),
                Err(error) => warn(
                    &report,
                    &format!("[stamp] stored library '{}' could not be restored:", row.id),
                    &error,
                ),
            }
        }
        Ok(restored)
    })
}

/// `persistStampLibraries(stamp, store, {except, debounceMs})`'s options.
#[derive(Clone, Debug, Default)]
pub struct PersistOptions {
    pub except: Vec<String>,
    /// Default 250.
    pub debounce_ms: Option<f64>,
}

struct Persisting {
    stamp: StampCapability,
    store: Rc<dyn StampLibraryStore>,
    report: Warn,
    /// Pending writes in insertion order, as a `Map` keeps them.
    timers: RefCell<Vec<(String, TimerId)>>,
}

impl Persisting {
    /// `write(libraryId)`: export, then store; a failure is reported.
    fn write(self: &Rc<Self>, library_id: &str) {
        self.timers.borrow_mut().retain(|(id, _)| id != library_id);
        let exporting = self.stamp.export_library(library_id);
        let (this, id) = (self.clone(), library_id.to_string());
        rt::spawn(async move {
            // `.then((bytes) => store.put(id, bytes)).catch(warn)`
            let outcome = match exporting.await {
                Ok(bytes) => {
                    let putting = this.store.put(&id, bytes);
                    // The `then` promise adopts the put's promise.
                    rt::tick().await;
                    putting.await
                }
                Err(error) => Err(error),
            };
            if let Err(error) = outcome {
                // The `catch` reaction.
                rt::tick().await;
                warn(&this.report, &format!("[stamp] persisting library '{id}' failed:"), &error);
            }
        });
    }
}

/// `persistStampLibraries(stamp, store, opts)`: keep the store in sync from
/// now on. Each change writes the library's PDF once changes stop for the
/// debounce; a removal deletes the stored copy. Answers the stop function,
/// which unsubscribes and flushes pending writes.
pub fn persist_stamp_libraries(
    stamp: &StampCapability,
    store: Rc<dyn StampLibraryStore>,
    opts: PersistOptions,
    report: Warn,
) -> Rc<dyn Fn()> {
    let persisting = Rc::new(Persisting {
        stamp: stamp.clone(),
        store,
        report,
        timers: RefCell::new(Vec::new()),
    });
    let debounce = opts.debounce_ms.unwrap_or(250.0);
    let except = opts.except;
    let listening = persisting.clone();
    let off: Unsubscribe = stamp.on_library_changed(move |change: &StampLibraryChange| {
        let id = change.library_id.clone();
        if except.contains(&id) {
            return Ok(());
        }
        let pending = listening.timers.borrow().iter().find(|(k, _)| *k == id).map(|(_, t)| *t);
        if let Some(pending) = pending {
            rt::clear_timer(pending);
        }
        if change.reason == StampLibraryChangeReason::Removed {
            listening.timers.borrow_mut().retain(|(k, _)| *k != id);
            let deleting = listening.store.delete(&id);
            let (report, id) = (listening.report.clone(), id.clone());
            rt::spawn(async move {
                if let Err(error) = deleting.await {
                    warn(
                        &report,
                        &format!("[stamp] deleting stored library '{id}' failed:"),
                        &error,
                    );
                }
            });
            return Ok(());
        }
        let writer = listening.clone();
        let key = id.clone();
        let timer = rt::set_timeout(debounce, move || writer.write(&key));
        let mut timers = listening.timers.borrow_mut();
        match timers.iter_mut().find(|(k, _)| *k == id) {
            Some(slot) => slot.1 = timer,
            None => timers.push((id, timer)),
        }
        Ok(())
    });
    Rc::new(move || {
        off.call();
        let pending: Vec<(String, TimerId)> = persisting.timers.borrow().clone();
        for (library_id, timer) in pending {
            rt::clear_timer(timer);
            persisting.write(&library_id);
        }
    })
}
