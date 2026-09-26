//! `packages/plugin/annotation` in Rust.
// Callback-shaped types are the port of JavaScript closures, as in the kernel.
#![allow(clippy::type_complexity)]

pub mod binary;
pub mod comments;
pub mod contract;
pub mod controller;
pub mod create_input;
pub mod host_contract;
pub mod model;
pub mod promise;
pub mod repository;
pub mod rich_text;
pub mod text_item;
pub mod tools;
pub mod value;
pub mod write;

#[cfg(feature = "harness")]
pub mod harness;

pub use contract::*;
