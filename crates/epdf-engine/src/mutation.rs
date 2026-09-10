//! G1. Mutation metadata and the refetch policy readers depend on.
//!
//! Replaces `core/src/mutation/{MutationMeta,RefetchReason,PageStructureCache}.ts`,
//! and later the per-family result and impact types
//! (`AnnotationListMutationMeta`, `AnnotationMutationImpactPolicy`,
//! `AnnotationMutationResults` in G11; `FormMutationResults` in G12; the page
//! result types in G13; `AttachmentMutationResults` in G14).
