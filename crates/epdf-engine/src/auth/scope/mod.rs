//! Scope vocabulary, parser, resolver and PDF bit decoder.
//!
//! G3. Replaces `packages/engine/core/src/auth/scope/*` for native, where
//! permission enforcement has no other owner.
//!
//! This is a deliberate duplicate and not a port: web keeps the TypeScript
//! resolver in `engine/core` and its enforcement callers in `engine/main`, and
//! `multiplatform-port-order.md` G3 says no deletion at Stage C is implied.
//! Two copies of an authorization rule drift, and drift here is privilege
//! escalation rather than a wrong pixel, so the two are pinned together by
//! `tooling/engine-conformance/vectors/scope/` and checked by
//! `tests/scope_conformance.rs`. Change one side and the other fails.
//!
//! `builders.ts` has no counterpart here. It mints scope arrays for SDK
//! callers, which is a cloud concern; the engine only ever consumes them.

pub mod parser;
pub mod pdf_bits;
pub mod resolver;
pub mod types;

pub use parser::{InvalidScope, parse_scope, validate_scope_array};
pub use pdf_bits::decode_pdf_bits;
pub use resolver::{
    Capabilities, check_any_capability, check_capability, check_collab, check_set_group,
    expand_raw_scope, expanded_capabilities, filter_matches, parse_all,
};
pub use types::{
    CollabAction, CollabActionPattern, CollabEntity, CollabFilter, CollabTarget, DocCapability,
    IdentityClaims, ParsedScope, PdfBits,
};
