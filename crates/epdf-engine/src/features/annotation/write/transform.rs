//! The rotation keys a writer puts into `/EMBD_Metadata`.
//!
//! G11. Ports
//! `services/src/features/annotations/internal/write/writeAnnotationTransformMetadata.ts`,
//! the write side of [`super::super::read::transform`].
//!
//! # Two families, and why the split is not cosmetic
//!
//! A BOX kind (square, circle, free text, stamp, caret) writes the PAIR:
//! `/Rotation` and `/UnrotatedRect`. With both present the fork's appearance
//! generator emits an `/AP /Matrix` that turns a box-sized appearance about
//! the box centre, and `/Rect` is the enclosing axis-aligned box. That is the
//! rotation any other reader sees correctly, because it is carried by the
//! appearance stream rather than by a key only this project knows.
//!
//! A VERTEX kind (line, polyline, polygon, ink) writes `/Rotation` ALONE, and
//! it is advisory: the points are already rotated, so they are the visual.
//! The generator ignores a `/Rotation` with no `/UnrotatedRect`, which is
//! exactly the behaviour that makes the lone key safe to record. It exists so
//! a UI can draw an oriented selection box and offer a reset.
//!
//! So a `/UnrotatedRect` on a vertex kind is not merely redundant, it is a
//! state that would drive the generator into rotating an appearance whose
//! points were rotated once already. The vertex writer clears it whenever it
//! touches the rotation, rather than trusting that nothing put one there.
//!
//! # The tri-state
//!
//! Both fields follow the engine-wide law: `Keep` touches nothing, `Clear`
//! removes that one key, a value sets it. Zero is a `Clear` rather than a
//! value, because zero degrees is not a rotation, and a file that had no
//! rotation key should not gain one by being asked to rotate by nothing.
//!
//! Clearing is always per key. The dictionary also holds the identity fields
//! (`UserID`, `GroupID`, `CreatedBy`, `UpdatedBy`), and dropping the whole
//! dictionary to remove a rotation would take an audit trail with it.

use crate::dto::Patch;
use crate::error::{EngineError, EngineErrorCode};
use crate::geom::PdfRect;

const KEY_ROTATION: &str = "Rotation";
const KEY_UNROTATED_RECT: &str = "UnrotatedRect";

/// The `/EMBD_Metadata /SchemaVersion` this build writes.
///
/// Seeded when a transform key is about to create the dictionary, so that the
/// marker every reader looks for is present from the first write rather than
/// from whichever later write happened to add it.
const EMBED_METADATA_SCHEMA_VERSION: f32 = 1.0;

/// What a rotatable draft or patch carries.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AnnotationTransform {
    /// Degrees, PDF convention. `Clear` and zero both remove the key.
    pub rotation: Patch<f64>,
    /// The logical box, box kinds only.
    pub unrotated_rect: Patch<PdfRect>,
}

/// Write a BOX kind's pair.
///
/// `has_existing_box` answers whether the annotation already carries an
/// `/UnrotatedRect`, and it is a closure rather than a bool because a patch
/// that supplies its own box never needs to ask: reading the key costs a
/// dictionary lookup that the common case does not owe.
pub fn write_box_transform(
    annotation: &pdfium::Annotation<'_>,
    transform: &AnnotationTransform,
    has_existing_box: impl FnOnce() -> Result<bool, EngineError>,
) -> Result<(), EngineError> {
    if let Some(rotation) = transform.rotation.as_set().copied() {
        if rotation == 0.0 {
            clear(annotation, KEY_ROTATION)?;
        } else {
            // A rotation with no box anywhere is unsatisfiable: the
            // generator needs the pair, and writing half of it produces an
            // annotation that renders unrotated while claiming an angle.
            // Refusing beats writing a state nothing can act on.
            let has_box = match &transform.unrotated_rect {
                Patch::Set(_) => true,
                Patch::Clear => false,
                Patch::Keep => has_existing_box()?,
            };
            if !has_box {
                return Err(EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    "rotation requires unrotatedRect: supply the pair together, or rely on one already on the annotation",
                ));
            }
            seed_schema_version(annotation)?;
            number(annotation, KEY_ROTATION, rotation)?;
        }
    } else if transform.rotation == Patch::Clear {
        clear(annotation, KEY_ROTATION)?;
    }

    match &transform.unrotated_rect {
        Patch::Keep => {}
        Patch::Clear => clear(annotation, KEY_UNROTATED_RECT)?,
        Patch::Set(rect) => {
            seed_schema_version(annotation)?;
            let native = pdfium::RectF {
                left: rect.left as f32,
                top: rect.top as f32,
                right: rect.right as f32,
                bottom: rect.bottom as f32,
            };
            report(
                annotation.set_embed_metadata_rect(KEY_UNROTATED_RECT, native),
                KEY_UNROTATED_RECT,
            )?;
        }
    }
    Ok(())
}

/// Write a VERTEX kind's advisory scalar.
///
/// `Keep` returns without touching the document, which is what lets a patch
/// that moves a polygon leave its recorded angle alone. Anything else clears
/// the `/UnrotatedRect`: see the module note on why that key must never
/// survive on this family.
pub fn write_vertex_transform(
    annotation: &pdfium::Annotation<'_>,
    rotation: Patch<f64>,
) -> Result<(), EngineError> {
    let Some(rotation) = rotation.as_set().copied() else {
        if rotation == Patch::Clear {
            clear(annotation, KEY_ROTATION)?;
            clear(annotation, KEY_UNROTATED_RECT)?;
        }
        return Ok(());
    };
    if rotation == 0.0 {
        clear(annotation, KEY_ROTATION)?;
    } else {
        seed_schema_version(annotation)?;
        number(annotation, KEY_ROTATION, rotation)?;
    }
    // Inert on the create path: a new annotation has no dictionary, so a
    // build without this line passes every create vector. It runs for the
    // update path, where a kind that was a box before this patch can still
    // be carrying the box key that would drive the generator.
    clear(annotation, KEY_UNROTATED_RECT)
}

fn seed_schema_version(annotation: &pdfium::Annotation<'_>) -> Result<(), EngineError> {
    if annotation.has_embed_metadata() {
        return Ok(());
    }
    number(annotation, "SchemaVersion", f64::from(EMBED_METADATA_SCHEMA_VERSION))
}

fn number(annotation: &pdfium::Annotation<'_>, key: &str, value: f64) -> Result<(), EngineError> {
    report(annotation.set_embed_metadata_number(key, value as f32), key)
}

fn clear(annotation: &pdfium::Annotation<'_>, key: &str) -> Result<(), EngineError> {
    // A clear of a key that is not there is not a failure: the dictionary
    // may not exist at all, which is the ordinary state of a new annotation.
    // Only the call itself failing is reported.
    annotation.clear_embed_metadata_key(key).map(|_| ()).map_err(|e| {
        EngineError::with_message(
            EngineErrorCode::RuntimeUnavailable,
            format!("clearing /EMBD_Metadata /{key} failed: {e}"),
        )
    })
}

fn report(result: Result<bool, pdfium::PdfiumError>, key: &str) -> Result<(), EngineError> {
    match result {
        Ok(true) => Ok(()),
        Ok(false) => Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            format!("PDFium refused /EMBD_Metadata /{key}"),
        )),
        Err(e) => Err(EngineError::with_message(
            EngineErrorCode::RuntimeUnavailable,
            format!("writing /EMBD_Metadata /{key} failed: {e}"),
        )),
    }
}
