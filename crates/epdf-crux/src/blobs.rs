//! Bulk bytes, kept out of Crux's serialization.
//!
//! Crux's generated shells decode a `Vec<u8>` one element at a time: in a
//! release Swift build a 7.7 MB page raster took 240 ms to decode, against a
//! memcpy for the same bytes through uniffi's `Data`. So documents, rasters
//! and every other payload go through this store instead, and Crux messages
//! carry a [`Blob`] naming one.
//!
//! The shell puts the document in with `CoreFfi::put_blob` and sends the id;
//! the middleware swaps ids for bytes before calling the engine and bytes
//! for ids after. The app never holds bytes, which also keeps them out of
//! its model and out of every `view()`.
//!
//! Every blob is taken exactly once. One the shell never takes stays until
//! the core is dropped, which is the leak this design accepts.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use facet::Facet;
use serde::{Deserialize, Serialize};

/// A handle to bytes in the [`BlobStore`].
#[derive(Facet, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Blob {
    pub id: u64,
}

#[derive(Debug, Default)]
pub struct BlobStore {
    next: AtomicU64,
    blobs: Mutex<HashMap<u64, Vec<u8>>>,
}

impl BlobStore {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn put(&self, bytes: Vec<u8>) -> Blob {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        self.blobs.lock().expect("blobs").insert(id, bytes);
        Blob { id }
    }

    /// The bytes, removed. `None` for an id that was never issued or has
    /// already been taken.
    pub fn take(&self, blob: Blob) -> Option<Vec<u8>> {
        self.blobs.lock().expect("blobs").remove(&blob.id)
    }

    pub fn len(&self) -> usize {
        self.blobs.lock().expect("blobs").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
