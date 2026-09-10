//! `DocumentAccess` / `PageAccess`: what every reader is written against.
//!
//! The generalization of the `FormModelSource { Borrowed | Owned }` pattern in
//! multiplatform-port-hazards.md 3.4. Stage A supplies the borrowed
//! implementation over a validated pointer lent by the TypeScript session;
//! Stage B (G10) supplies the owned one over `&Session`. Without this the read
//! groups get written once against a raw pointer and again at G10.
//!
//! The cost: the trait is designed before the Stage B session exists, so expect
//! to widen it once.
