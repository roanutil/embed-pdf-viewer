//! The ported engine: everything that decides what a PDF means.
//!
//! This is the destination for the 16,620 source lines in
//! `packages/engine/services` plus the hand-ported half of
//! `packages/engine/core` (see multiplatform-port-order.md section 2). It knows
//! nothing about `uniffi`, the browser, UIKit or Android types; host adapters
//! live in `epdf-uniffi`, `epdf-wasm` and `epdf-node`.
//!
//! Module names follow the port groups. Each module carries the group number,
//! the TypeScript files it replaces and the `WorkerRequest` kinds it owns.
//!
//! # What is implemented
//!
//! G1's shared kernel, the Phase 0 buffer and coordinate contracts, the
//! ownership-mode abstraction in [`access`], G2 [`features::geometry`]
//! (`pages.geometry`), which reads through either ownership mode, and G3's
//! scope vocabulary in [`auth::scope`], which is a deliberate duplicate of the
//! TypeScript resolver rather than a port and is pinned to it by
//! `tests/scope_conformance.rs`.
//!
//! Everything else is a stub carrying its group's design notes: the session
//! ([`session`]), owner-thread dispatch ([`dispatch`]), and the remaining
//! operation families under [`features`]. They are documented, not written.

// G1: shared kernel. Pure, no PDFium.
pub mod cancel;
pub mod clock;
pub mod dto;
pub mod error;
pub mod geom;
pub mod identity;
pub mod ids;
pub mod mutation;
pub mod revision;

// G3: authorization vocabulary. Pure, no PDFium.
pub mod auth;

// Phase 0 contracts and the plumbing every operation family sits on.
pub mod access;
pub mod buffer;
pub mod dispatch;
pub mod host;

pub mod features;
pub mod session;
pub mod spike;
