//! G14. Metadata writes.
//!
//! Replaces `services/src/features/metadata/{MetadataMutator,internal/write/*}.ts`
//! (193) and `core/src/mutation/MetadataUpdateResult.ts`. Operation:
//! `metadata.update`.
//!
//! This is the operation the Phase 0 DTO codegen proof drives: `MetadataPatch`
//! is three-state, and the test pushes absent, `null` and a value through an
//! actual `metadata.update` on each host and reads the metadata back. Reads
//! live in `features::structure` (G6).
