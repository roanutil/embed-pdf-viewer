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

// Phase 0 contracts and the plumbing every operation family sits on.
pub mod access;
pub mod buffer;
pub mod dispatch;

pub mod features;
pub mod session;
