//! G3. Document security and permission state.
//!
//! Replaces `services/src/features/security/` and the pure half of
//! `core/src/engine/document-security-state.ts`.
//!
//! Split by what touches PDFium. [`state`] is the derivation: probe shapes,
//! the all-allowed rule, the advisory and the wire state, all pure and all
//! pinned to the TypeScript by conformance vectors. The reader that calls
//! PDFium to fill a probe is separate, so the rules can be checked without a
//! document and without a fixture for every combination of bits.

pub mod reader;
pub mod state;
