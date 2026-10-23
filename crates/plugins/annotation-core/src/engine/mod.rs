//! The pure part of `@embedpdf/engine-core/runtime` the annotation core
//! calls: the kind tables, the predictor that says what a create or an
//! update writes, the appearance verdict, and the measurement labels. The
//! engine crate (`epdf-engine`) has its own copies on PDFium's side; these
//! build for any target, `wasm32` included.

pub mod appearance;
pub mod facts;
pub mod geom;
pub mod kind_facts;
pub mod measure;
pub mod resolve;
pub mod shape_for_rect;
pub mod space;
pub mod text;
