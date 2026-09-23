//! Owner-thread dispatch, queues and budgets.
//!
//! One owner OS thread per engine, inside Rust. The exported engine object is
//! thread-safe and submits jobs; handle types are `!Send + !Sync`. A Swift
//! actor, a coroutine dispatcher or a mutex does not guarantee affinity.
//!
//! Also: bounded queues for jobs, open documents, input bytes, raster and cache
//! bytes and search output, with admission checked before large allocations and
//! a reserved control route so saturation cannot block close or cancellation.
//! Every PDFium call is wrapped in `catch_unwind`; a panic after a potentially
//! mutating operation poisons the session unless an operation-specific
//! invariant says otherwise.
//!
//! Implementation reference: prototype `crates/epdf-ops/src/dispatch/`.
//!
//! The written contract, clause by clause with what implements each one and
//! what is still owed, is
//! `.agents/skills/embedpdf-conventions/references/engine-lifecycle.md`.
//! Nothing in this module is implemented yet; the queues, the budgets and the
//! reserved control route are all G3.
