//! `packages/core/annotation` in Rust: the pure annotation core. Records
//! hold the engine's annotation; kinds and shape families read it; `update`
//! turns a message into the next session, the records it changed and the
//! engine work to do; the view selectors and `scene` say what to draw.
//! No DOM, engine or framework.
//!
//! Modules mirror the TypeScript files and names (`geomHit` is
//! [`geometry::geom_hit`]). The engine's annotation, drafts and patches are
//! JSON objects kept in the order the TypeScript builds them (`json.rs`);
//! everything worked out from them is typed, and each typed object
//! remembers its member order so it prints as the TypeScript object does
//! (`object.rs`). The pure part of `@embedpdf/engine-core/runtime` the core
//! calls (the kind tables, the write predictor, the appearance verdict and
//! the measurement labels) is ported in [`engine`], so this crate builds for
//! any target, `wasm32` included. JavaScript numerics go through [`js`].

pub mod object;

pub mod anchor;
pub mod appearance;
pub mod cloudy;
pub mod editable;
pub mod endings;
pub mod engine;
pub mod error;
pub mod flags;
pub mod font_css;
pub mod frame;
pub mod geometry;
pub mod group;
pub mod helvetica;
pub mod hit;
pub mod icons;
pub mod ink;
pub mod js;
pub mod json;
pub mod kinds;
pub mod links;
pub mod measurement;
pub mod measurement_shape;
pub mod painted;
pub mod placement;
pub mod plane;
pub mod props;
pub mod record;
pub mod rect;
pub mod richtext;
pub mod scene;
pub mod selection;
pub mod shapes;
pub mod snap;
pub mod types;
pub mod update;
pub mod view;

#[cfg(feature = "harness")]
pub mod harness;

pub use error::{CoreError, CoreResult};
pub use object::{Json, JsonError, Nullable};
pub use types::*;
pub use update::{new_records_at_most, update};
