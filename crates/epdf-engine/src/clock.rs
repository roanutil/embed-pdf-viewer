//! G1. Injected time source.
//!
//! Replaces `services/src/shared/pdf-date.ts` plus every `Date.now()` call.
//! Injected rather than ambient so the conformance corpus is deterministic.
