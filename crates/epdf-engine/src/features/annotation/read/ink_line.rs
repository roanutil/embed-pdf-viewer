//! Line, and the ink that is not quite one.
//!
//! G7. Ports
//! `services/src/features/annotations/internal/read/{readLineAnnotation,readInkAnnotation}.ts`.
//!
//! They are together because they are the two families with no shared parent:
//! a line is a filled kind with two endpoints, ink is a stroke-only kind with
//! a list of pen paths, and each is one subtype.

use pdfium::Annotation;
use serde::{Deserialize, Serialize};

use super::style::{
    FilledStyleFields, GeometryStyleFields, read_filled_style, read_geometry_style,
};
use super::transform::read_rotation;
use super::{dictionary_read, points_to_wire};
use crate::error::EngineError;
use crate::features::annotation::intent::InkIntent;
use crate::features::annotation::vocabulary::{LineEnding, LineEndings};
use crate::geom::{InkList, LinePoints, PdfPoint};

/// What a line with no `/L` reports.
///
/// A degenerate line at the origin rather than an absent one, because the
/// field is not optional on the wire: `/L` is what a line annotation IS, and
/// a file that omitted it has produced something no consumer can draw either
/// way. The zero at least has a shape.
const ZERO_LINE: LinePoints =
    LinePoints { start: PdfPoint { x: 0.0, y: 0.0 }, end: PdfPoint { x: 0.0, y: 0.0 } };

/// A line annotation's own fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineFields {
    #[serde(flatten)]
    pub style: FilledStyleFields,
    pub line_points: LinePoints,
    pub line_endings: LineEndings,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f64>,
}

/// An ink annotation's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InkFields {
    #[serde(flatten)]
    pub style: GeometryStyleFields,
    /// `/IT`. `None` for ordinary pen ink; `Some` only for the Acrobat
    /// freehand highlighter.
    pub intent: Option<InkIntent>,
    pub ink_list: InkList,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f64>,
}

/// Read a line.
pub fn read_line(annotation: &Annotation<'_>) -> Result<LineFields, EngineError> {
    Ok(LineFields {
        style: read_filled_style(annotation)?,
        line_points: annotation.line().map_or(ZERO_LINE, |(start, end)| LinePoints {
            start: PdfPoint { x: f64::from(start.x), y: f64::from(start.y) },
            end: PdfPoint { x: f64::from(end.x), y: f64::from(end.y) },
        }),
        line_endings: read_line_endings(annotation),
        rotation: read_rotation(annotation)?,
    })
}

/// Read ink.
pub fn read_ink(annotation: &Annotation<'_>) -> Result<InkFields, EngineError> {
    let intent = dictionary_read("IT", annotation.intent())?;
    Ok(InkFields {
        style: read_geometry_style(annotation)?,
        intent: InkIntent::from_name(intent.as_deref()),
        ink_list: dictionary_read("InkList", annotation.ink_list())?
            .into_iter()
            .map(points_to_wire)
            .collect(),
        rotation: read_rotation(annotation)?,
    })
}

/// `/LE`, with an absent entry reading as a well-formed pair of nothings.
///
/// Both endings rather than an `Option<LineEndings>`, so a consumer drawing a
/// line never has to decide what an absent pair means. §12.5.6.7's default is
/// `None` at both ends, which is what this reports.
pub fn read_line_endings(annotation: &Annotation<'_>) -> LineEndings {
    annotation.line_endings().map_or(
        LineEndings { start: LineEnding::None, end: LineEnding::None },
        |(start, end)| LineEndings {
            start: LineEnding::from_code(start),
            end: LineEnding::from_code(end),
        },
    )
}
