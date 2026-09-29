//! The sticky note.
//!
//! G7. Ports
//! `services/src/features/annotations/internal/read/readTextAnnotation.ts`.
//!
//! A `/Text` annotation is an icon with a conversation behind it. The
//! conversation is the base shell's `contents` and its reply edge; what is
//! here is the icon and the review state.
//!
//! `/State` and `/StateModel` are read FAITHFULLY: `None` exactly when the key
//! is absent. §12.5.6.3 says a state model present without an explicit state
//! implies one (`none` for review, `unmarked` for marked), and that defaulting
//! is not applied here. It belongs to whoever composes a thread, because it
//! depends on which annotation in the thread is being asked about, and a
//! reader that applied it would make a document say something it did not.

use pdfium::Annotation;
use serde::{Deserialize, Serialize};

use super::dictionary_read;
use super::style::{Color, read_color, read_opacity};
use crate::error::EngineError;
use crate::features::annotation::state::state_from_pdf;
use crate::features::annotation::state::state_model_from_pdf;
use crate::features::annotation::vocabulary::{DEFAULT_NOTE_ICON, NoteIcon};

/// What a note with no `/C` is drawn as.
pub const DEFAULT_NOTE_COLOR: Color = Color { r: 255, g: 255, b: 0 };

/// A text annotation's own fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextFields {
    pub icon: NoteIcon,
    pub color: Color,
    pub opacity: f64,
    /// `/State`, lowercased when it is one of the seven §12.5.6.3 names and
    /// verbatim when it is not. An open vocabulary, because Acrobat lets a
    /// workflow register its own.
    pub state: Option<String>,
    /// `/StateModel`, on the same terms.
    pub state_model: Option<String>,
}

/// Read them.
pub fn read_text(annotation: &Annotation<'_>) -> Result<TextFields, EngineError> {
    Ok(TextFields {
        // A `/Name` outside the note set reads as the default rather than as
        // an error: a file-attachment icon on a sticky note is a file being
        // wrong, not a document being unreadable.
        icon: dictionary_read("Name", annotation.name())?
            .and_then(|name| NoteIcon::from_pdf_name(&name))
            .unwrap_or(DEFAULT_NOTE_ICON),
        color: read_color(annotation, DEFAULT_NOTE_COLOR),
        opacity: read_opacity(annotation),
        state: dictionary_read("State", annotation.string_value("State"))?
            .map(|value| state_from_pdf(&value)),
        state_model: dictionary_read("StateModel", annotation.string_value("StateModel"))?
            .map(|value| state_model_from_pdf(&value)),
    })
}
