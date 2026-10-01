//! The two control entry points: pure FFI cost, and FFI plus the hop to
//! the kernel thread.

use std::ffi::c_void;

use crate::{Completion, EpdfComplete, registry};

/// Returns at once on the calling thread.
#[unsafe(no_mangle)]
pub extern "C" fn epdf_bench_noop() {}

/// Posts an empty job to the kernel thread and answers from it. `result`
/// is null.
///
/// # Safety
/// `done` is called once with `ctx`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn epdf_bench_noop_hop(kernel: u64, ctx: *mut c_void, done: EpdfComplete) {
    let completion = Completion::new(ctx, done);
    let Some(registered) = registry::lookup(kernel) else { return };
    (registered.post)(Box::new(move || completion.ok_ptr(std::ptr::null())));
}
