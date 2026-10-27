//! `services/siblings.ts`: the sibling planes. The form plugin is required;
//! the stamp and annotation plugins are optional and read when needed.

use epdf_plugin_annotation::{ANNOTATION, AnnotationCapability};
use epdf_plugin_form::FormCapability;
use epdf_plugin_stamp::{STAMP, StampCapability};

use crate::controller::SignatureCapability;

impl SignatureCapability {
    /// `form`: `ctx.get(FormToken)`, resolved once when the controller is
    /// built.
    pub(crate) fn form(&self) -> &FormCapability {
        &self.0.form
    }

    /// `stamp()`: `ctx.tryGet(StampToken)`.
    pub(crate) fn stamp(&self) -> Option<StampCapability> {
        self.cx().try_get(&STAMP).ok().flatten()
    }

    /// `annotation()`: `ctx.tryGet(AnnotationToken)`.
    pub(crate) fn annotation(&self) -> Option<AnnotationCapability> {
        self.cx().try_get(&ANNOTATION).ok().flatten()
    }
}
