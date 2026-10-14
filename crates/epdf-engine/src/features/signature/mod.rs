//! Digital signatures: `packages/engine/services/src/features/signature/`
//! and the pure half of `packages/engine/core/src/signature/`.
//!
//! The wire kinds are `signatures.list`, `signatures.contents`,
//! `signatures.digest`, `signatures.revisionBytes` and `document.version`.
//! Each function keeps its TypeScript namesake's boundaries, snake-cased, so
//! the two can be read side by side.

pub mod protection;
pub mod reader;
pub mod types;
