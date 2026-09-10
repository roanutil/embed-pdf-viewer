//! G2. Page geometry. The first operation ported, and the first to flip.
//!
//! Replaces `services/src/features/geometry/PageGeometryReader.ts` (333) and
//! `core/src/dto/PageGeometrySnapshot.ts` (127). Operation: `pages.geometry`.
//!
//! The internal representation is a flat numeric buffer, which is what the
//! shared vectors pin. The public result is not: `PageGeometrySnapshot` is a
//! discriminated union of upright and rotated runs, each holding glyph objects
//! with an optional `tightBox` or `tightQuad` and an optional `fontSize`, and
//! `PageGeometryReader.ts:86` materializes all of it on every read. So the
//! packed buffer is an internal encoding with a decode on the far side.
//!
//! Gates: `runPageGeometryOrientationConformance`, a glyph-level differential
//! harness against `PageGeometryReader`, no per-glyph host call, viewer flows
//! F3 and F4, and a speed floor set from the Phase 0 baseline. The committed
//! 2.6x is a per-item-vs-coarse ratio from a different arm, not the threshold.
