//! What every crate that exports the kernel or a plugin through uniffi
//! shares: the kernel's owner thread, the handle that reaches it, the error
//! every export returns, and the records more than one plugin uses.

pub mod bound;
pub mod cancel;
pub mod error;
pub mod geometry;
pub mod kernel_thread;
pub mod status;
pub mod subscription;

pub use bound::{KernelBound, PromiseSlot, at_teardown};
pub use cancel::CancelToken;
pub use error::{PluginErrorInfo, ViewerError};
pub use geometry::{PageRef, Point, Rect, Size, TextQuad};
pub use kernel_thread::{Job, KernelPost, KernelThread};
pub use status::ResourceStatus;
pub use subscription::{Subscription, SubscriptionSlot};

/// Returns at once on the calling thread: the pure cost of entering Rust
/// through uniffi, for the bridge benchmarks. Experimental, never shipped.
#[cfg(feature = "bench")]
#[uniffi::export]
pub fn bench_noop() {}

uniffi::setup_scaffolding!();
