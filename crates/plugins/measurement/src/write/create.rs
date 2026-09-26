//! `write/create.ts`: a measurement annotation with the page's scale,
//! through the annotation plugin.

use epdf_annotation_core::AnnotationRef;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::rt::Promise;
use epdf_plugin_annotation::CreateAnnotationInput;

use crate::contract::*;
use crate::controller::MeasurementCapability;

impl MeasurementCapability {
    /// Create a measurement annotation with the page's scale. Rejects
    /// `not-ready` before the scale is known. The promise is the annotation
    /// plugin's own.
    pub fn create_measurement(&self, input: CreateMeasurementInput) -> Promise<AnnotationRef> {
        let s = self.s();
        let refuse = |code: &str, message: &str| {
            Promise::rejected(plugin_error(code, "measurement", message, None, None))
        };
        if !s.annotation.can_create() {
            return refuse(code::PERMISSION_DENIED, "measuring needs annotation create authority");
        }
        if !s.scale_of(&input.page).ready {
            return refuse(code::NOT_READY, "the page scale is not known yet");
        }
        let tool = input.tool.clone().unwrap_or_else(|| input.kind.as_str().to_string());
        let base = |subtype: &str| CreateAnnotationInput {
            page: input.page,
            subtype: subtype.to_string(),
            tool: Some(tool.clone()),
            props: None,
            flags: None,
            select: None,
            bounds: None,
            rotation: None,
            from: None,
            to: None,
            vertices: None,
            strokes: None,
            quads: None,
            callout: None,
        };
        match input.kind {
            MeasurementKind::Distance => {
                let [from, to] = input.points[..] else {
                    return refuse(code::INVALID_INPUT, "a distance needs exactly two points");
                };
                s.annotation.create(CreateAnnotationInput {
                    from: Some(from),
                    to: Some(to),
                    ..base("line")
                })
            }
            MeasurementKind::Perimeter => s.annotation.create(CreateAnnotationInput {
                vertices: Some(input.points.clone()),
                ..base("polyline")
            }),
            MeasurementKind::Area => s.annotation.create(CreateAnnotationInput {
                vertices: Some(input.points.clone()),
                ..base("polygon")
            }),
        }
    }
}
