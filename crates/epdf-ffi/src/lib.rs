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

pub use bound::{KernelBound, PromiseSlot};
pub use cancel::CancelToken;
pub use error::{PluginErrorInfo, ViewerError};
pub use geometry::{PageRef, Point, Rect, Size, TextQuad};
pub use kernel_thread::{Job, KernelPost, KernelThread};
pub use status::ResourceStatus;
pub use subscription::{Subscription, SubscriptionSlot};

uniffi::setup_scaffolding!();
