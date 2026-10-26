//! `services/siblings.ts`: the sibling plugins, the annotation plugin (host
//! lens) and the interaction hub.

use epdf_kernel::{ControllerContext, Error};
use epdf_plugin_annotation::{ANNOTATION, AnnotationCapability};
use epdf_plugin_interaction::{INTERACTION, InteractionCapability};

pub struct MeasurementSiblings {
    pub annotation: AnnotationCapability,
    pub interaction: InteractionCapability,
}

/// `resolveSiblings(ctx)`.
pub fn resolve_siblings(cx: &ControllerContext) -> Result<MeasurementSiblings, Error> {
    Ok(MeasurementSiblings { annotation: cx.get(&ANNOTATION)?, interaction: cx.get(&INTERACTION)? })
}
