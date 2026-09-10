//! G6. Document structure reads: metadata, page list, actions, destinations.
//!
//! Replaces `services/src/features/metadata/{MetadataReader,internal/*}.ts`
//! (151), `services/src/features/pages/PagesReader.ts` (214),
//! `services/src/features/actions/{ActionModelReader,DocumentActionsReader}.ts`
//! (522), `services/src/features/destinations/*` (130), and the structure DTOs
//! (507).
//!
//! Operations: `metadata.read`, `pages.list`, `actions.read`. Gate includes page
//! identity staying stable between lazy per-page lookup and full enumeration
//! (`DocumentSession.ts:41-44`).
