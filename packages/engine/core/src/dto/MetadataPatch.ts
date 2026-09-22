/**
 * Three-state metadata patch.
 *
 * The shape is generated from the Rust definition in
 * `crates/epdf-engine/src/dto.rs`, which is canonical for internal wire DTOs
 * (`multiplatform-plan.md` Section 2). Regenerate with `cargo test -p
 * epdf-engine`; CI fails on a dirty diff. The zod schema in
 * `wire/schemas.ts` stays handwritten and is held against the Rust type by
 * `crates/epdf-engine/tests/schema_parity.rs`.
 *
 * This file stays as the import path so nothing downstream has to know where
 * the type is written.
 */
export type { MetadataPatch } from './generated/MetadataPatch';
