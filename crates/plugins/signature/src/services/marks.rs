//! `services/marks.ts`: marks to the one-page appearance bytes the engine
//! draws into a field.

use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::rt::Promise;
use epdf_plugin_annotation::binary::resolve_binary_source;
use serde_json::Value;

use crate::contract::Mark;
use crate::controller::SignatureCapability;

impl SignatureCapability {
    /// `bytesOf(source)`: an `async` function awaiting
    /// `resolveBinarySource`.
    pub(crate) fn bytes_of(&self, source: &Value) -> Promise<Vec<u8>> {
        let resolved = resolve_binary_source(source);
        Promise::spawn(async move { Ok(resolved.await?.bytes) })
    }

    /// `markBytes(mark)`: an `async` function. An asset's bytes from the
    /// stamp plugin (`not-found` for one it doesn't hold), or the bytes the
    /// embedder brought.
    pub(crate) fn mark_bytes(&self, mark: &Mark) -> Promise<Vec<u8>> {
        match mark {
            Mark::Asset { asset_id } => {
                match self.stamp().and_then(|stamp| stamp.read_asset_bytes(asset_id)) {
                    Some(bytes) => Promise::resolved(bytes),
                    None => Promise::rejected(plugin_error(
                        code::NOT_FOUND,
                        "signature",
                        format!("unknown mark '{asset_id}'"),
                        None,
                        None,
                    )),
                }
            }
            Mark::Source { source } => {
                // `return bytesOf(source)`: an async function's promise,
                // adopted.
                let bytes = self.bytes_of(source);
                Promise::spawn(async move { crate::promise::adopt(bytes).await })
            }
        }
    }
}
