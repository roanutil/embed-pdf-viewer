//! The file attachment, and the embedded file it names.
//!
//! G7. Ports
//! `services/src/features/annotations/internal/read/readFileAttachmentAnnotation.ts`
//! and the read half of
//! `services/src/features/attachments/internal/attachmentPrimitives.ts`.
//!
//! An embedded file has exactly two homes in a PDF: the catalog's
//! `/EmbeddedFiles` name tree, and a file-attachment annotation's `/FS`. This
//! is the second. The first is G8's, and [`read_attachment_info`] is written
//! to serve both, because the projection is the same one and a second copy
//! would be a second thing to keep in step.
//!
//! No bytes. A listing that carried payloads would make opening a page as
//! expensive as downloading every file on it, so the DTO carries metadata and
//! the bytes leave through an explicit call.

use pdfium::{Annotation, Attachment};
use serde::{Deserialize, Serialize};

use super::dictionary_read;
use super::style::{Color, read_color, read_opacity};
use crate::error::EngineError;
use crate::features::annotation::vocabulary::{DEFAULT_FILE_ATTACHMENT_ICON, FileAttachmentIcon};

/// What an attachment marker with no `/C` is drawn as.
pub const DEFAULT_FILE_ATTACHMENT_COLOR: Color = Color { r: 255, g: 255, b: 0 };

/// The metadata projection of an embedded file.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentFileInfo {
    /// `/UF`. Empty rather than absent when unreadable: a filespec requires
    /// a name, so an empty one says the file is malformed while a missing
    /// field would say the reader did not look.
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// `/Params /Size`, the DECODED length.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<i32>,
    /// `/Params /CheckSum`, as lowercase hex.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
    /// `/Params /CreationDate`, as the PDF date string. Not converted to ISO
    /// 8601, unlike the annotation's own dates: this one is the file's, it is
    /// written by whatever produced the attachment, and the wire passes it
    /// through.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_date: Option<String>,
}

/// A file-attachment annotation's own fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileAttachmentFields {
    pub icon: FileAttachmentIcon,
    pub file: AttachmentFileInfo,
    pub color: Color,
    pub opacity: f64,
}

/// Read them.
pub fn read_file_attachment(
    annotation: &Annotation<'_>,
) -> Result<FileAttachmentFields, EngineError> {
    // A malformed annotation with no readable `/FS` still reads. It has a
    // rect, a colour and an icon, and reporting none of that because the
    // filespec is broken would lose more than it says; the explicit download
    // call is where the precise error belongs.
    let file = match annotation.file_attachment() {
        Some(attachment) => read_attachment_info(&attachment)?,
        None => AttachmentFileInfo::default(),
    };
    Ok(FileAttachmentFields {
        icon: dictionary_read("Name", annotation.name())?
            .and_then(|name| FileAttachmentIcon::from_pdf_name(&name))
            .unwrap_or(DEFAULT_FILE_ATTACHMENT_ICON),
        file,
        color: read_color(annotation, DEFAULT_FILE_ATTACHMENT_COLOR),
        opacity: read_opacity(annotation),
    })
}

/// The projection of one embedded file, from either of its two homes.
pub fn read_attachment_info(
    attachment: &Attachment<'_>,
) -> Result<AttachmentFileInfo, EngineError> {
    Ok(AttachmentFileInfo {
        name: dictionary_read("UF", attachment.name())?.unwrap_or_default(),
        mime_type: dictionary_read("Subtype", attachment.mime_type())?,
        description: dictionary_read("Desc", attachment.description())?,
        // A negative size is PDFium reporting something it could not make
        // sense of, and a negative length is not a fact about a file.
        size: dictionary_read("Size", attachment.integer_value("Size"))?.filter(|size| *size >= 0),
        checksum: normalize_checksum(dictionary_read(
            "CheckSum",
            attachment.string_value("CheckSum"),
        )?),
        creation_date: dictionary_read("CreationDate", attachment.string_value("CreationDate"))?,
    })
}

/// `/CheckSum` as lowercase hex, without the angle brackets a PDF hex string
/// carries.
///
/// A value that is not hex at all passes through UNCHANGED rather than being
/// rejected. It is somebody's checksum in some other form, and mangling it
/// would be worse than reporting it; a consumer comparing checksums will find
/// it does not match, which is the truth.
fn normalize_checksum(raw: Option<String>) -> Option<String> {
    let raw = raw.filter(|value| !value.is_empty())?;
    let inner =
        raw.strip_prefix('<').and_then(|rest| rest.strip_suffix('>')).unwrap_or(raw.as_str());
    if !inner.is_empty() && inner.chars().all(|c| c.is_ascii_hexdigit()) {
        return Some(inner.to_ascii_lowercase());
    }
    Some(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hex_checksum_loses_its_brackets_and_its_case() {
        assert_eq!(
            normalize_checksum(Some("<D41D8CD98F00B204E9800998ECF8427E>".into())),
            Some("d41d8cd98f00b204e9800998ecf8427e".into()),
        );
        // Already bare, which is how some producers write it.
        assert_eq!(normalize_checksum(Some("ABCD".into())), Some("abcd".into()));
    }

    #[test]
    fn anything_that_is_not_hex_survives_unchanged() {
        assert_eq!(
            normalize_checksum(Some("not-a-checksum".into())),
            Some("not-a-checksum".into())
        );
        // Brackets around non-hex are part of the value, not a wrapper.
        assert_eq!(normalize_checksum(Some("<zz>".into())), Some("<zz>".into()));
        // And an empty pair of brackets is not a checksum of nothing.
        assert_eq!(normalize_checksum(Some("<>".into())), Some("<>".into()));
        assert_eq!(normalize_checksum(Some(String::new())), None);
        assert_eq!(normalize_checksum(None), None);
    }
}
