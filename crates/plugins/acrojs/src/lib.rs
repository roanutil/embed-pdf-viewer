//! `packages/core/acrojs` in Rust: the host side of Acrobat JavaScript.
//!
//! What ports is the bridge a host runs scripts through: the script realm and
//! its transaction mutex ([`host`]), the field view ([`input`]), color
//! conversion ([`color`]), JavaScript out of an action tree ([`actions`]), and
//! the JSON contract and constants ([`types`]). The actions plugin uses all of
//! it, and the form plugin will.
//!
//! What does not port is `prelude.ts`, the JavaScript that runs inside the
//! sandbox. Rust ships its text as data, [`PRELUDE_SOURCE`], for a host to
//! evaluate in whatever sandbox it supplies. The sandbox itself is the
//! [`host::ScriptSandbox`] trait: JSON in, JSON out.

#![allow(clippy::type_complexity)]

pub mod actions;
pub mod color;
pub mod host;
pub mod input;
pub mod js;
pub mod types;

#[cfg(feature = "harness")]
pub mod harness;

pub use actions::{java_script_program_from_action_tree, java_script_sources_from_action_tree};
pub use color::{
    Rgb, css_to_script_color, is_script_color_array, script_color_to_css, script_color_to_rgb,
};
pub use host::{
    IdentityOverrides, ScriptHost, ScriptHostOptions, ScriptSandbox, ScriptSandboxFactory,
    ScriptTransaction, ScriptWorldInput, create_script_host, resolve_script_identity, seed_from,
};
pub use input::script_fields_from_snapshot;
pub use types::*;

/// `PRELUDE_SOURCE`: the self-contained script that installs the curated
/// Acrobat object model (`__acrojsBoot`, `__acrojsRun`) into a sandbox realm.
///
/// `tooling/plugin-conformance/src/suites/acrojs.ts` writes this file from
/// `packages/core/acrojs/src/prelude.ts`, and its `--check` fails when the
/// file is stale.
pub const PRELUDE_SOURCE: &str = include_str!("../prelude.js");
