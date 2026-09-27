//! `services/siblings.ts`: the sibling planes. The form plugin is
//! required; the stamp and annotation plugins are read when needed.

use epdf_kernel::{Controller, Error};
use epdf_plugin_annotation::{ANNOTATION, AnnotationCapability};
use epdf_plugin_form::{FORM, FormCapability};
use epdf_plugin_stamp::{STAMP, StampCapability};

use crate::controller::SignatureCapability;
use crate::model::{SignatureAction, SignatureState};

/// `ctx.get(FormToken)`, resolved once when the controller is built.
pub(crate) fn resolve_form(
    cx: &Controller<SignatureState, SignatureAction>,
) -> Result<FormCapability, Error> {
    cx.get(&FORM)
}

impl SignatureCapability {
    pub(crate) fn form(&self) -> &FormCapability {
        &self.0.form
    }

    /// `stamp()`: `ctx.tryGet(StampToken)`.
    pub(crate) fn stamp(&self) -> Result<Option<StampCapability>, Error> {
        self.cx().try_get(&STAMP)
    }

    /// `annotation()`: `ctx.get(AnnotationToken)`.
    pub(crate) fn annotation(&self) -> Result<AnnotationCapability, Error> {
        self.cx().get(&ANNOTATION)
    }
}
