//! The Stage B bridge spike. Throwaway, and labelled as such on purpose.
//!
//! `multiplatform-plan.md` Section 6 asks Phase 0 to prove the Stage B bridge
//! "as a throwaway spike over `annotations.create` through the existing
//! `AnnotationMutator` against a Rust-owned document, with failure-after-write,
//! form model invalidation, and close-under-lease exercised". This is that
//! spike. It is deleted when G10 lands, and the plan draws the line itself: the
//! Phase 0 spike is a throwaway, G3w is not.
//!
//! # What it is for
//!
//! Stage A, which is what everything else in this crate implements today, has
//! TypeScript owning the session and Rust borrowing a page pointer for one
//! synchronous call. Stage B inverts that. Rust owns the document and
//! TypeScript leases a handle from it for one scoped operation, which means
//! the dangerous direction: a host that can hold a pointer into Rust-owned
//! state across a call Rust does not control, and mutate through it.
//!
//! No Rust borrow can span that call. The compiler cannot help. So the
//! protocol is a runtime state machine, and this module exists to find out
//! whether the shape in the plan survives contact before `DocumentSession`
//! moves behind it.
//!
//! # The protocol
//!
//! Three separate synchronous entry points, as Section 6 specifies, not one
//! call with a closure: the host's work happens between them, in another
//! language, and may not come back.
//!
//! 1. [`SpikeSession::checkout`] hands out a token and a copy of the document
//!    pointer.
//! 2. [`SpikeSession::return_lease`] takes the token back, with what happened.
//! 3. [`SpikeSession::finalize`] advances the mutation sequence and invalidates
//!    what the write invalidated.
//!
//! While a lease is out, or an outcome is unfinalized, every other document
//! operation is rejected. A second checkout is rejected. `close` stops
//! accepting new work immediately and completes only once the lease is back
//! and the outcome finalized.
//!
//! # Removal criterion
//!
//! Section 6 requires one in writing. This module goes when G10 lands a real
//! session with the same three entry points and the same rejections, proven by
//! tests that replace `tests/stage_b_lease_spike.rs` rather than by these ones
//! being deleted.

pub mod lease;
