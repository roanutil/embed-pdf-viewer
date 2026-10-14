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

use std::collections::BTreeMap;

use crate::error::{EngineError, EngineErrorCode};
use crate::features::fonts::FontRegistry;

/// One binary payload that arrived with a mutation.
///
/// The bytes are borrowed rather than owned: a stamp image or an attachment
/// can be megabytes, and a writer reads it once and hands it straight to
/// PDFium, so there is nothing for a copy to buy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteResource<'a> {
    pub bytes: &'a [u8],
    /// What the caller says the bytes are. Advisory: a file attachment takes
    /// any bytes at all and records this as the stream's `/Subtype`.
    pub mime_type: Option<&'a str>,
}

/// The payloads accompanying one mutation, keyed as the caller keyed them.
pub type WriteResourceMap<'a> = BTreeMap<String, WriteResource<'a>>;

/// The capabilities threaded into a per-subtype writer.
#[derive(Debug, Default, Clone, Copy)]
pub struct AnnotationWriteContext<'a> {
    /// This thread's fonts, for a `/DA` that names a registered family.
    pub fonts: Option<&'a FontRegistry>,
    /// The binaries this mutation brought with it. A draft names one by key
    /// rather than carrying it, because a draft is JSON and a PNG is not.
    pub resources: Option<&'a WriteResourceMap<'a>>,
}

impl<'a> AnnotationWriteContext<'a> {
    /// A context with nothing in it, for a caller that creates only kinds
    /// needing nothing.
    pub fn empty() -> Self {
        Self::default()
    }

    /// With this thread's font registry.
    pub fn with_fonts(fonts: &'a FontRegistry) -> Self {
        Self { fonts: Some(fonts), resources: None }
    }

    /// With the binaries this mutation carried.
    pub fn with_resources(mut self, resources: &'a WriteResourceMap<'a>) -> Self {
        self.resources = Some(resources);
        self
    }

    /// Look up one payload.
    ///
    /// A key with nothing behind it is the caller's mistake rather than a
    /// missing file: the draft and the payloads travel together, so a draft
    /// naming a key that did not arrive means the two were assembled apart.
    pub fn resource(&self, key: &str) -> Result<WriteResource<'a>, EngineError> {
        self.resources.and_then(|map| map.get(key)).copied().ok_or_else(|| {
            EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("the draft names resource '{key}' but no such payload accompanied it"),
            )
        })
    }

    /// What a registered key resolved to, or `None` when it is not one.
    /// `describeRegisteredFont` in `annotationWriteContext.ts`.
    pub fn describe_font(&self, key: &str) -> Option<pdfium::FontIdentity> {
        self.fonts.and_then(|fonts| fonts.describe(key))
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
