//! `layout/`: the engine's text layout, where its lines break, and the runs
//! the generator writes from it.

pub(crate) mod breaks;
mod edit;
mod variable_text;

pub(crate) use edit::text_runs;
pub use variable_text::{
    Alignment, LaidLine, LaidWord, PdfRect, TextLayout, VariableTextOptions, layout_variable_text,
};
