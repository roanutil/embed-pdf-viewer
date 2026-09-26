//! `services/binaries.ts`: the binary sidecar of the serializable store
//! (asset bytes and cached previews, the canonical library PDFs, the ghost
//! render caches) and the per-library mutation queue.

use epdf_kernel::rt::{self, Promise};

use crate::contract::StampAssetPreview;
use crate::controller::StampCapability;
use crate::promise::{adopt, finally};

/// One asset's placement bytes and cached preview. `bytes` is `None` when
/// the engine's extraction answered no bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct AssetBinary {
    pub bytes: Option<Vec<u8>>,
    pub preview: Option<StampAssetPreview>,
}

impl StampCapability {
    /// `mutateLibrary(libraryId, mutation)`: whole-PDF rewrites run one at a
    /// time per library.
    pub(crate) fn mutate_library<T: Clone + 'static>(
        &self,
        library_id: &str,
        mutation: impl FnOnce() -> Promise<T> + 'static,
    ) -> Promise<T> {
        let previous = self.0.library_mutation_tails.borrow().get(library_id).cloned();
        // `(previous ?? Promise.resolve()).then(mutation)`
        let result = Promise::spawn(async move {
            match previous {
                Some(previous) => {
                    let _ = previous.await;
                }
                None => rt::tick().await,
            }
            adopt(mutation()).await
        });
        let tail = {
            let result = result.clone();
            Promise::spawn(async move {
                let _ = result.await;
                Ok(())
            })
        };
        self.0.library_mutation_tails.borrow_mut().insert(library_id.to_string(), tail.clone());
        let inner = self.0.clone();
        let key = library_id.to_string();
        finally(result, move || {
            let current = inner.library_mutation_tails.borrow().get(&key).cloned();
            if current.is_some_and(|current| current.same(&tail)) {
                inner.library_mutation_tails.borrow_mut().remove(&key);
            }
        })
    }
}
