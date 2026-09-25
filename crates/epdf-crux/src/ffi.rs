//! The FFI a shell calls: Crux's three functions and one callback.
//!
//! Exported with the uniffi the workspace already pins rather than with
//! BoltFFI, which Crux's own examples moved to in 0.19. Crux only needs bytes
//! in and bytes out, so the choice is ours; staying on uniffi keeps one FFI
//! generator in the repository.
//!
//! Every byte array here is bincode, and the types it decodes to are the ones
//! `src/bin/codegen.rs` generates. The shell never writes a wire envelope.

use std::sync::Arc;

use crux_core::Core;
use crux_core::bridge::EffectId;
use crux_core::middleware::{
    BincodeFfiFormat, Bridge, HandleEffectLayer, Layer as _, MapEffectLayer,
};

use crate::ShellEffect;
use crate::app::PdfViewer;
use crate::blobs::{Blob, BlobStore};
use crate::middleware::EngineMiddleware;

type CoreBridge = Bridge<
    MapEffectLayer<HandleEffectLayer<Core<PdfViewer>, EngineMiddleware>, ShellEffect>,
    BincodeFfiFormat,
>;

/// Implemented by the shell.
#[uniffi::export(with_foreign)]
pub trait CruxShell: Send + Sync {
    /// Effects that became ready after `update` or `resolve` returned: every
    /// engine answer arrives this way. Called on the PDFium owner thread, so
    /// a UI shell hops to its main thread before touching state.
    fn process_effects(&self, bytes: Vec<u8>);
}

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum CoreError {
    #[error("{detail}")]
    Bridge { detail: String },
}

#[derive(uniffi::Object)]
pub struct CoreFfi {
    core: CoreBridge,
    blobs: Arc<BlobStore>,
}

#[uniffi::export]
impl CoreFfi {
    #[uniffi::constructor]
    pub fn new(shell: Arc<dyn CruxShell>) -> Arc<Self> {
        let blobs = BlobStore::new();
        let core = Core::<PdfViewer>::new()
            .handle_effects_using(EngineMiddleware::new(Arc::clone(&blobs)))
            .map_effect::<ShellEffect>()
            .bridge::<BincodeFfiFormat>(move |effects| match effects {
                Ok(bytes) => shell.process_effects(bytes),
                // Nothing to hand the shell but the failure, and the callback
                // has no error channel. Serialization of our own types is the
                // only thing that can fail here.
                Err(e) => panic!("effect serialization failed: {e}"),
            });
        Arc::new(Self { core, blobs })
    }

    /// Stores bytes for an event to name, and returns the id to name them by.
    /// This, not the event, is how a document gets in: see `blobs.rs`.
    pub fn put_blob(&self, bytes: Vec<u8>) -> u64 {
        self.blobs.put(bytes).id
    }

    /// Removes and returns the bytes an effect named, or `None` for an id
    /// that was never issued or was already taken.
    pub fn take_blob(&self, id: u64) -> Option<Vec<u8>> {
        self.blobs.take(Blob { id })
    }

    pub fn update(&self, event: Vec<u8>) -> Result<Vec<u8>, CoreError> {
        let mut effects = Vec::new();
        self.core.update(&event, &mut effects).map_err(bridge_error)?;
        Ok(effects)
    }

    pub fn resolve(&self, id: u32, output: Vec<u8>) -> Result<Vec<u8>, CoreError> {
        let mut effects = Vec::new();
        self.core.resolve(EffectId(id), &output, &mut effects).map_err(bridge_error)?;
        Ok(effects)
    }

    pub fn view(&self) -> Result<Vec<u8>, CoreError> {
        let mut view = Vec::new();
        self.core.view(&mut view).map_err(bridge_error)?;
        Ok(view)
    }
}

fn bridge_error(error: impl std::fmt::Display) -> CoreError {
    CoreError::Bridge { detail: error.to_string() }
}
