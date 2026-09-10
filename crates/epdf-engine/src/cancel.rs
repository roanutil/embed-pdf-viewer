//! G1. Cancellation tokens.
//!
//! Replaces `services/src/shared/abort.ts`. A token, not an `AbortSignal`.
//!
//! Cancellation is one of the three Phase 0 contracts: four cases, with the web
//! mechanism chosen per operation family. A reserved control route keeps queue
//! saturation from blocking cancellation or close.
