//! G3. Layer scopes and permission enforcement.
//!
//! Replaces `core/src/auth/scope/*` (904) and `core/src/dto/LayerScopes.ts` (42).
//!
//! A deliberate duplicate: web keeps the TypeScript copy in `engine/main` until
//! Stage C, because native permission enforcement has no other owner. Gate:
//! shared vectors agree with the TypeScript resolver.
