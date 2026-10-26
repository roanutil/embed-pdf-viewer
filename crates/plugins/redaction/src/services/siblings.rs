//! `services/siblings.ts`: the sibling plugins, annotation (required, host
//! lens) and selection and search (optional, looked up on each use).

use epdf_kernel::{ControllerContext, Error};
use epdf_plugin_annotation::{ANNOTATION, AnnotationCapability};
use epdf_plugin_search::{SEARCH, SearchCapability};
use epdf_plugin_selection::{SELECTION, SelectionCapability};

pub struct RedactionSiblings {
    cx: ControllerContext,
    pub annotation: AnnotationCapability,
}

impl RedactionSiblings {
    /// `ctx.tryGet(SelectionToken)`.
    pub fn selection(&self) -> Result<Option<SelectionCapability>, Error> {
        self.cx.try_get(&SELECTION)
    }

    /// `ctx.tryGet(SearchToken)`.
    pub fn search(&self) -> Result<Option<SearchCapability>, Error> {
        self.cx.try_get(&SEARCH)
    }
}

/// `resolveSiblings(ctx)`.
pub fn resolve_siblings(cx: &ControllerContext) -> Result<RedactionSiblings, Error> {
    Ok(RedactionSiblings { annotation: cx.get(&ANNOTATION)?, cx: cx.clone() })
}
