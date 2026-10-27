//! `persistence.ts`: keeping custom libraries, over the plugin's public
//! API, in any environment. The plugin knows when a library changes
//! (`onLibraryChanged`) and what its canonical bytes are (`exportLibrary`,
//! a complete PDF). Where those bytes live is the embedder's decision:
//! [`StampLibraryStore`] is the port. These helpers are the proof that the
//! two capability calls are all a store needs.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use epdf_kernel::bytes::Bytes;
use epdf_kernel::rt::{self, Promise};
use epdf_kernel::{Error, Unsubscribe};
use serde_json::Value;

use crate::contract::{ImportLibraryOptions, OperationOptions, StampLibraryChangeReason};
use crate::controller::StampCapability;

/// One stored library.
#[derive(Clone, Debug, PartialEq)]
pub struct StoredLibrary {
    pub id: String,
    pub bytes: Vec<u8>,
}

/// `StampLibraryStore`: the persistence port, bytes by library id.
pub trait StampLibraryStore {
    fn list(&self) -> Promise<Vec<StoredLibrary>>;
    fn put(&self, id: &str, bytes: Vec<u8>) -> Promise<()>;
    fn delete(&self, id: &str) -> Promise<()>;
}

/// `memoryStampStore()`: an in-memory store, for tests and hosts without
/// storage. Rows list in insertion order, as a `Map` iterates; a put of a
/// stored id keeps its place.
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
        Promise::resolved(self.rows.borrow().clone())
    }

    fn put(&self, id: &str, bytes: Vec<u8>) -> Promise<()> {
        let mut rows = self.rows.borrow_mut();
        match rows.iter_mut().find(|row| row.id == id) {
            Some(row) => row.bytes = bytes,
            None => rows.push(StoredLibrary { id: id.to_string(), bytes }),
        }
        Promise::resolved(())
    }

    fn delete(&self, id: &str) -> Promise<()> {
        self.rows.borrow_mut().retain(|row| row.id != id);
        Promise::resolved(())
    }
}

fn warn(message: String, error: &Error) {
    crate::console::warn(vec![Value::String(message), error.to_canonical()]);
}

/// `restoreStampLibraries(stamp, store)`: bring every stored library back.
/// Each PDF is imported as it is (its title, registry and PieceInfo id come
/// from the file; the store's key is only a hint); one that fails is
/// reported and skipped. Call once at boot, before seeding defaults.
/// Answers the restored ids.
pub fn restore_stamp_libraries(
    stamp: &StampCapability,
    store: Rc<dyn StampLibraryStore>,
) -> Promise<Vec<String>> {
    let stamp = stamp.clone();
    let listing = store.list();
    Promise::spawn(async move {
        let mut restored = Vec::new();
        for row in listing.await? {
            let source = Bytes(row.bytes).to_json();
            match stamp.import_library(source, ImportLibraryOptions::default()).await {
                Ok(library) => restored.push(library.id),
                Err(error) => warn(
                    format!("[stamp] stored library '{}' could not be restored:", row.id),
                    &error,
                ),
            }
        }
        Ok(restored)
    })
}

/// `persistStampLibraries`' options: `except` names libraries never to
/// persist (the bundled default set, typically).
#[derive(Clone, Debug, Default)]
pub struct PersistOptions {
    pub except: Vec<String>,
}

/// What a library's line does next in the store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Save,
    Delete,
}

/// One library's line: the store step waiting or running, and the one to
/// follow it.
struct Line {
    step: Step,
    started: bool,
    next: Option<Step>,
}

struct Lines {
    stamp: StampCapability,
    store: Rc<dyn StampLibraryStore>,
    lines: RefCell<HashMap<String, Rc<RefCell<Line>>>>,
}

impl Lines {
    /// `run(libraryId, step)`: not started yet, the line does the latest
    /// wish; running, the latest wish follows it. A new line starts once
    /// the current burst of changes is over, so the burst is one step.
    fn run(self: &Rc<Self>, library_id: &str, step: Step) {
        if let Some(line) = self.lines.borrow().get(library_id) {
            let mut line = line.borrow_mut();
            if line.started {
                line.next = Some(step);
            } else {
                line.step = step;
            }
            return;
        }
        let fresh = Rc::new(RefCell::new(Line { step, started: false, next: None }));
        self.lines.borrow_mut().insert(library_id.to_string(), fresh.clone());
        let (this, id) = (self.clone(), library_id.to_string());
        // `void Promise.resolve().then(async () => ...)`
        rt::spawn(async move {
            rt::tick().await;
            fresh.borrow_mut().started = true;
            let step = fresh.borrow().step;
            let outcome = match step {
                Step::Save => {
                    match this.stamp.export_library(&id, OperationOptions::default()).await {
                        Ok(bytes) => this.store.put(&id, bytes).await,
                        Err(error) => Err(error),
                    }
                }
                Step::Delete => this.store.delete(&id).await,
            };
            if let Err(error) = outcome {
                let doing = match fresh.borrow().step {
                    Step::Save => "persisting",
                    Step::Delete => "deleting stored",
                };
                warn(format!("[stamp] {doing} library '{id}' failed:"), &error);
            }
            this.lines.borrow_mut().remove(&id);
            let next = fresh.borrow().next;
            if let Some(next) = next {
                this.run(&id, next);
            }
        });
    }
}

/// `persistStampLibraries(stamp, store, options)`: keep the store in sync
/// from now on. Every canonical change saves the library's PDF, a removal
/// deletes it. Each library has one line of store work, so saves never
/// overlap and a delete never lands under a late save. Returns the
/// unsubscribe; work already asked for still finishes.
pub fn persist_stamp_libraries(
    stamp: &StampCapability,
    store: Rc<dyn StampLibraryStore>,
    options: PersistOptions,
) -> Unsubscribe {
    let lines = Rc::new(Lines { stamp: stamp.clone(), store, lines: RefCell::default() });
    let except = options.except;
    stamp.on_library_changed(move |change| {
        if except.contains(&change.library_id) {
            return Ok(());
        }
        let step = match change.reason {
            StampLibraryChangeReason::Removed => Step::Delete,
            _ => Step::Save,
        };
        lines.run(&change.library_id, step);
        Ok(())
    })
}
