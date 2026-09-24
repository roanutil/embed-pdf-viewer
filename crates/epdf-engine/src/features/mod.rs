//! Operation families, one module per port group.
//!
//! Reads before writes: G2 and G4 through G9 are reads that run under Stage A
//! with a session TypeScript still owns. G11 onward are writes and need the
//! Stage B session (G10).

pub mod actions;
pub mod annotation;
pub mod assembly;
pub mod extract;
pub mod files;
pub mod fonts;
pub mod forms;
pub mod geometry;
pub mod metadata;
pub mod pages;
pub mod redaction;
pub mod render;
pub mod save;
pub mod search;
pub mod security;
pub mod server;
pub mod structure;
pub mod text;
