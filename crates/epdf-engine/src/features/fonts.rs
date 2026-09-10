//! G4. Font registry. Engine-level, not per-session.
//!
//! Replaces `services/src/features/fonts/FontRegistrar.ts` (187) and
//! `core/src/dto/FontSpec.ts`. Operations: `fonts.register`,
//! `fonts.addFallback`, `fonts.clearFallbacks`, `fonts.clear`.
//!
//! `FontRegistrar` takes a runtime and a `fontKey -> FontId` map with no
//! session, and retains file-access handles until `clear`
//! (`FontRegistrar.ts:51`). G4 moves the key-to-ID map, registration, fallback
//! order and retained handles together to one Rust owner per combined worker,
//! before startup fonts or documents load. `FontRegistrar` becomes a delegating
//! facade including `idFor`, with no second map.
