//! Lets the experimental manual bridge (`crates/epdf-manual-bridge`) reach
//! a `ViewerHost`'s kernel thread without going through uniffi, and adds
//! the uniffi control export its benchmarks time. Not for shipping: only
//! the `manual-bridge` feature compiles this.

use std::sync::Weak;

use epdf_ffi::{KernelPost, ViewerError};
use epdf_manual_bridge::registry::{self, KernelJob, Registered};

use crate::ViewerHost;
use crate::host_table::HostTable;

/// Register a host that is being built. `host` is weak: the registry must
/// not keep a viewer alive, and a job for a viewer that is gone is dropped,
/// which answers the C caller `EPDF_ERROR_CLOSED`.
pub(crate) fn register(host: Weak<ViewerHost>, post: KernelPost) -> u64 {
    registry::register(Registered {
        post,
        run: Box::new(move |job: KernelJob| {
            if let Some(host) = host.upgrade() {
                host.fire(move |s| job(&s.kernel));
            }
        }),
        pixels: HostTable::get,
    })
}

pub(crate) fn unregister(token: u64) {
    registry::unregister(token);
}

#[uniffi::export]
impl ViewerHost {
    /// The token the manual bridge's `epdf_*_open` calls take. Experimental.
    pub fn experimental_kernel_token(&self) -> u64 {
        self.token
    }

    /// Posts an empty job to the kernel thread and waits for it: uniffi
    /// plus the hop, for the bridge benchmarks. Experimental.
    pub async fn bench_noop_hop(&self) -> Result<(), ViewerError> {
        self.call(|_| Ok(())).await
    }
}
