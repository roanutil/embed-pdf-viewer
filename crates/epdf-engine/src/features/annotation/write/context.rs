//! What a writer needs that is not on the draft.
//!
//! G11. Ports
//! `services/src/features/annotations/internal/write/annotationWriteContext.ts`.
//!
//! Two of the write families reach outside the annotation they are writing. A
//! free text can name a font this thread registered, and the id that names it
//! is a per-thread counter that must be resolved at the moment of use. A stamp
//! carries an image, which arrives as a separate binary payload rather than
//! inside the JSON draft.
//!
//! Every member is optional end to end. A writer that does not need one never
//! looks, and a host with no font registry wired can still create everything
//! that does not name a registered font.

use crate::error::{EngineError, EngineErrorCode};
use crate::features::fonts::FontRegistry;

/// The capabilities threaded into a per-subtype writer.
#[derive(Debug, Default, Clone, Copy)]
pub struct AnnotationWriteContext<'a> {
    /// This thread's fonts, for a `/DA` that names a registered family.
    pub fonts: Option<&'a FontRegistry>,
}

impl<'a> AnnotationWriteContext<'a> {
    /// A context with nothing in it, for a caller that creates only kinds
    /// needing nothing.
    pub fn empty() -> Self {
        Self::default()
    }

    /// With this thread's font registry.
    pub fn with_fonts(fonts: &'a FontRegistry) -> Self {
        Self { fonts: Some(fonts) }
    }

    /// Resolve a registered font key to this thread's id.
    ///
    /// A key with no registry behind it is a programming error rather than a
    /// missing font, and it is reported as one: silently falling back to
    /// Helvetica would embed the wrong glyphs into a document that asked for
    /// a specific face, and nothing downstream could tell that had happened.
    pub fn resolve_font(&self, key: &str) -> Result<pdfium::FontId, EngineError> {
        let Some(fonts) = self.fonts else {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!(
                    "fontFamily '{key}' is not a standard font and this host has no font registry"
                ),
            ));
        };
        fonts.id_for(key)
    }
}
