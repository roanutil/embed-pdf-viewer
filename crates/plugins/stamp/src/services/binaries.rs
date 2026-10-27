//! `services/binaries.ts`: the binaries resource. Asset bytes and cached
//! previews, the canonical library PDFs, and the ghost render caches. State
//! holds only the serializable descriptors; everything binary lives here,
//! next to the per-library mutation queue that orders whole-PDF rewrites.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;

use crate::contract::StampAssetPreview;
use crate::controller::StampCapability;

/// One asset's placement bytes (a one-page PDF) and its cached preview.
/// `bytes` is `None` when the engine's extraction answered no bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct AssetBinary {
    pub bytes: Option<Vec<u8>>,
    pub preview: Option<StampAssetPreview>,
}

/// Ghost renders of one asset (or one arm), by device pixel width.
pub type GhostCache = Rc<RefCell<HashMap<u64, Promise<Option<StampAssetPreview>>>>>;

/// A `Map` key for a width: `SameValueZero`, so `-0` is `0` and every `NaN`
/// is one key.
pub(crate) fn width_key(width: f64) -> u64 {
    if width.is_nan() {
        f64::NAN.to_bits()
    } else if width == 0.0 {
        0f64.to_bits()
    } else {
        width.to_bits()
    }
}

#[derive(Default)]
pub struct Binaries {
    /// Asset bytes and the cached preview, by asset id.
    pub binaries: RefCell<HashMap<String, AssetBinary>>,
    /// The durable source of truth for a canonical library: the rewritten
    /// whole PDF. `None` is a save that answered no bytes.
    pub library_binaries: RefCell<HashMap<String, Option<Vec<u8>>>>,
    /// Ghost renders, one cache per asset id.
    pub ghost_renders: RefCell<HashMap<String, GhostCache>>,
}

impl Binaries {
    /// `createBinaries`' cleanup.
    pub(crate) fn clear(&self) {
        self.binaries.borrow_mut().clear();
        self.ghost_renders.borrow_mut().clear();
        self.library_binaries.borrow_mut().clear();
    }
}

impl StampCapability {
    /// `mutateLibrary(libraryId, mutation, signal)`: whole-PDF rewrites are
    /// serialized per library (`ctx.serialQueue('library:<id>')`), or two
    /// concurrent appends could both start from the same bytes and lose one
    /// page. A mutation whose signal fired while it waited never starts.
    ///
    pub(crate) fn mutate_library<T: Clone + 'static>(
        &self,
        library_id: &str,
        mutation: impl FnOnce() -> Promise<T> + 'static,
        signal: Option<AbortSignal>,
    ) -> Promise<T> {
        let queue = self.0.cx.serial_queue(&format!("library:{library_id}"));
        queue.run_with(move || Ok(mutation()), signal)
    }
}
