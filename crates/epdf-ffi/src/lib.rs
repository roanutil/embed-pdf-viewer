//! What every crate that exports the kernel or a plugin through uniffi
//! shares: the kernel's owner thread, the handle that reaches it, the error
//! every export returns, and the records more than one plugin uses.

pub mod annotation;
pub mod bound;
pub mod cancel;
pub mod error;
pub mod foreign;
pub mod form;
pub mod geometry;
pub mod input;
pub mod kernel_thread;
pub mod mirror;
pub mod page;
pub mod registration;
pub mod status;
pub mod subscription;
pub mod view;

pub use annotation::{
    AnnotationRef, CreationDraftAnchor, PageDestination, PageLinkTarget, PdfDestination,
    PdfLinkTarget,
};
pub use bound::{KernelBound, PromiseSlot, at_teardown};
pub use cancel::CancelToken;
pub use error::{PluginErrorInfo, ViewerError};
pub use foreign::{ForeignSettle, HostError, HostFailure};
pub use form::{FormFieldFamily, FormFieldRef};
pub use geometry::{PageRef, Point, Quad, Rect, Size};
pub use input::{
    Modifiers, PointerGesture, PointerKind, PointerPageHit, PointerPhase, PointerSample,
};
pub use kernel_thread::{Job, KernelPost, KernelThread};
pub use mirror::to_u32;
pub use page::PageInfo;
pub use registration::RegistrationHandle;
pub use status::ResourceStatus;
pub use subscription::{Subscription, SubscriptionSlot};
pub use view::{
    Camera, PageFrameInsets, StageAlign, StageAlignValue, StageAlignment, StageAlignmentValue,
    StageDirection, StageScrollMetrics, StageSizingMode, StageSpreadMode, ViewAnchor, ZoomMode,
    ZoomSpec,
};

/// Returns at once on the calling thread: the pure cost of entering Rust
/// through uniffi, for the bridge benchmarks. Experimental, never shipped.
#[cfg(feature = "bench")]
#[uniffi::export]
pub fn bench_noop() {}

uniffi::setup_scaffolding!();
