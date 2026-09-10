//! G1. Wire DTOs, the `ts-rs` source of truth.
//!
//! Replaces `core/src/dto/*` (1,636 lines). Generated into
//! `packages/engine/core/src/dto/`; CI fails on a dirty diff. The zod HTTP
//! schemas in `wire/schemas.ts` and the 20 annotation schema files stay
//! handwritten, with bidirectional parity tests against these types.
//!
//! Patch types are three-state. `MetadataPatch` (`wire/schemas.ts:105`) treats
//! absent as "leave the field", `null` as "clear it" and a value as "set it",
//! which `serde`'s `Option<T>` flattens to two states unless the type says
//! otherwise. So patches get an explicit `Keep | Clear | Set(T)` field type with
//! its own serialization, generated to TypeScript as `?: T | null`.
