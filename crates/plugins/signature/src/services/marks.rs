//! `services/marks.ts`: marks to the one-page appearance bytes the engine
//! draws into a field.

use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::rt::Promise;
use epdf_plugin_annotation::binary::resolve_binary_source;
use serde_json::Value;

use crate::contract::Mark;
use crate::controller::SignatureCapability;
use crate::promise::adopt;

impl SignatureCapability {
    /// `bytesOf(source)`: an `async` function over `resolveBinarySource`.
    pub(crate) fn bytes_of(&self, source: &Value) -> Promise<Vec<u8>> {
        let resolved = resolve_binary_source(source);
        Promise::spawn(async move { Ok(resolved.await?.bytes) })
    }

    /// `markBytes(mark)`: an asset's bytes from the stamp plugin, or the
    /// bytes the embedder brought.
    pub(crate) fn mark_bytes(&self, mark: &Mark) -> Promise<Vec<u8>> {
        match mark {
            Mark::Asset { asset_id } => {
                let bytes = match self.stamp() {
                    Ok(stamp) => stamp.and_then(|s| s.read_asset_bytes(asset_id)),
                    Err(error) => return Promise::rejected(error),
                };
                match bytes {
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
                let bytes = self.bytes_of(source);
                Promise::spawn(async move { adopt(bytes).await })
            }
        }
    }
}
