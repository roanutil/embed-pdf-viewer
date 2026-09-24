//! Serializing an open session.
//!
//! G10. Ports `services/src/features/save/DocumentSaver.ts` and
//! `internal/pdfSaveMode.ts`. Operations: `document.saveBuffer`,
//! `document.saveFile`, `document.saveLayerBuffer`.
//!
//! # The two outputs are not interchangeable
//!
//! A STANDALONE save is a PDF. A LAYER ARTIFACT is this project's storage
//! format for a delta and is not a PDF at all, so handing one to a reader
//! produces a file that will not open, and handing the other to the
//! persistence layer produces a base that has lost its history.
//!
//! Only a layer session can produce an artifact, and that is checked HERE
//! rather than in the wrapper: PDFium cannot tell a layer handle from a base
//! one, but the session knows how it was opened.
//!
//! # Saving is a read
//!
//! No revision bumps, no registry refresh, no events. A save observes the
//! document and writes bytes elsewhere; nothing about the open session is
//! different afterwards, which is what lets it run beside any other read.

use pdfium::{LayerOutput, LayerSaveError, PdfSaveMode};
use serde::{Deserialize, Serialize};

use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};
use crate::session::document::{DocumentSession, OpenKind};

/// How much of the file a standalone save rewrites.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SaveMode {
    /// Append a revision, leaving the original bytes intact. Preserves
    /// anything this build did not understand, because the untouched objects
    /// are literally the original ones.
    Incremental,
    /// Rewrite the whole file. Smaller to keep and slower to produce, and the
    /// history goes with it.
    #[default]
    Full,
}

impl SaveMode {
    fn native(self) -> PdfSaveMode {
        match self {
            Self::Incremental => PdfSaveMode::Incremental,
            Self::Full => PdfSaveMode::Full,
        }
    }
}

/// Which layer output to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LayerArtifactKind {
    /// Base plus delta, as one artifact.
    Artifact,
    /// The delta alone, for a caller that already holds the base bytes.
    Delta,
}

impl LayerArtifactKind {
    fn native(self) -> LayerOutput {
        match self {
            Self::Artifact => LayerOutput::Artifact,
            Self::Delta => LayerOutput::Delta,
        }
    }
}

/// Serialize the session as a standalone PDF.
///
/// Works for any session kind. For a layer session an incremental save copies
/// the base bytes through and appends the delta as an ordinary revision, so
/// the result is a file any reader opens.
pub fn save_to_buffer(
    session: &DocumentSession,
    mode: SaveMode,
    cancel: &CancelToken,
) -> Result<Vec<u8>, EngineError> {
    cancel.check()?;
    let document = session.document()?.document();
    // Cancellation is cooperative up to the write and not inside it: the
    // save is one synchronous call that cannot be interrupted, so a token set
    // during it is observed on the far side.
    let bytes = document.save_to_buffer(mode.native()).map_err(|e| {
        EngineError::with_message(
            EngineErrorCode::DocOpenFailed,
            format!("failed to save document: {e}"),
        )
    })?;
    cancel.check()?;
    Ok(bytes)
}

/// Serialize a layer session's artifact or its delta.
pub fn save_layer_to_buffer(
    session: &DocumentSession,
    output: LayerArtifactKind,
    cancel: &CancelToken,
) -> Result<Vec<u8>, EngineError> {
    cancel.check()?;
    if session.kind() != Some(OpenKind::Layer) {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "document session is not a layer",
        ));
    }
    let document = session.document()?.document();
    let bytes = document.save_layer_to_buffer(output.native()).map_err(layer_error)?;
    cancel.check()?;
    Ok(bytes)
}

/// Each layer-save failure means something different to a caller.
fn layer_error(error: LayerSaveError) -> EngineError {
    let code = match error {
        // Not a runtime failure: the delta has outgrown the format and has to
        // be flattened into a new base. That is an operation the caller can
        // perform, which is why it is not `Unknown`.
        LayerSaveError::AppendOnlyOffsetTooLarge => EngineErrorCode::DocOpenFailed,
        LayerSaveError::SaveFailed | LayerSaveError::Unknown(_) => EngineErrorCode::DocOpenFailed,
    };
    EngineError::with_message(code, format!("failed to save layer artifact: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wire_names_are_the_typescript_ones() {
        // `PdfSaveMode` crosses the wire, so these two strings are a
        // contract rather than a spelling.
        assert_eq!(
            serde_json::to_value(SaveMode::Incremental).unwrap(),
            serde_json::json!("incremental"),
        );
        assert_eq!(serde_json::to_value(SaveMode::Full).unwrap(), serde_json::json!("full"));
        assert_eq!(SaveMode::default(), SaveMode::Full);
    }
}
