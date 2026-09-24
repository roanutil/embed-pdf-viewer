//! What a binary payload actually is.
//!
//! G11. Ports `core/src/resource/binaryMetadata.ts`.
//!
//! The DECLARED type is never trusted. A caller's `mimeType` is a label it
//! chose; what PDFium has to be handed depends on what the bytes ARE, and
//! handing JPEG bytes to the PNG decoder produces a corrupt image object
//! rather than an error. So the format is sniffed from the magic bytes on
//! every engine and on the server, and the declaration is used only where it
//! is genuinely advisory, such as an attachment's `/Subtype`.
//!
//! The raster formats also give up their intrinsic size here, because the
//! fit policy needs an aspect ratio before the image object exists.

/// A format this build can place in a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryFormat {
    /// The pixel dimensions come from the IHDR chunk.
    Png { width: u32, height: u32 },
    /// The pixel dimensions come from the frame header.
    Jpeg { width: u32, height: u32 },
    /// Identified and not measured: a PDF's size is a page box rather than a
    /// pixel count, and the stamp path takes the page whole.
    Pdf,
}

impl BinaryFormat {
    /// The intrinsic size, for the formats that have one.
    pub fn dimensions(self) -> Option<(u32, u32)> {
        match self {
            Self::Png { width, height } | Self::Jpeg { width, height } => Some((width, height)),
            Self::Pdf => None,
        }
    }
}

/// Identify a payload, or `None` when it is not one of the three.
pub fn sniff(bytes: &[u8]) -> Option<BinaryFormat> {
    match bytes {
        [0x89, b'P', b'N', b'G', ..] => parse_png(bytes),
        [0xFF, 0xD8, 0xFF, ..] => parse_jpeg(bytes),
        [b'%', b'P', b'D', b'F', ..] => Some(BinaryFormat::Pdf),
        _ => None,
    }
}

/// The IHDR chunk is always first and always at a fixed offset, so the width
/// and height are the two big-endian words at 16 and 20.
fn parse_png(bytes: &[u8]) -> Option<BinaryFormat> {
    let width = u32::from_be_bytes(bytes.get(16..20)?.try_into().ok()?);
    let height = u32::from_be_bytes(bytes.get(20..24)?.try_into().ok()?);
    // A zero dimension is a corrupt header rather than an empty image: there
    // is no such thing as a PNG with no pixels, and the fit policy would
    // divide by it.
    (width != 0 && height != 0).then_some(BinaryFormat::Png { width, height })
}

/// JPEG has no fixed offset: the size lives in whichever start-of-frame
/// marker the encoder used, behind a chain of segments of declared length.
fn parse_jpeg(bytes: &[u8]) -> Option<BinaryFormat> {
    let mut offset = 2usize;
    while offset + 4 < bytes.len() {
        if bytes[offset] != 0xFF {
            return None;
        }
        let marker = bytes[offset + 1];
        // SOF0 through SOF3: baseline, extended, progressive, lossless. The
        // later SOF markers are arithmetic-coded variants PDFium does not
        // take, so finding one is as good as finding nothing.
        if (0xC0..=0xC3).contains(&marker) {
            let height = u16::from_be_bytes(bytes.get(offset + 5..offset + 7)?.try_into().ok()?);
            let width = u16::from_be_bytes(bytes.get(offset + 7..offset + 9)?.try_into().ok()?);
            return (width != 0 && height != 0).then_some(BinaryFormat::Jpeg {
                width: u32::from(width),
                height: u32::from(height),
            });
        }
        // `D8` and `D9` are the start and end of the image and carry no
        // length; a run of `FF` is padding between segments; everything else
        // declares its own length in the two bytes after the marker.
        offset += match marker {
            0xD8 | 0xD9 => 2,
            0xFF => 1,
            _ => {
                let length = usize::from(u16::from_be_bytes(
                    bytes.get(offset + 2..offset + 4)?.try_into().ok()?,
                ));
                // A segment that declares no length would leave the offset
                // where it was and spin forever on a malformed file.
                if length < 2 {
                    return None;
                }
                2 + length
            }
        };
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PNG header: the signature, then an IHDR chunk whose first two words
    /// are the size. Nothing after byte 24 is read.
    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        bytes.extend_from_slice(&[0, 0, 0, 13]);
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes
    }

    #[test]
    fn a_png_reports_the_size_in_its_header() {
        assert_eq!(sniff(&png(640, 480)), Some(BinaryFormat::Png { width: 640, height: 480 }));
    }

    #[test]
    fn a_zero_dimension_is_a_corrupt_header_and_not_an_empty_image() {
        assert_eq!(sniff(&png(0, 480)), None);
        assert_eq!(sniff(&png(640, 0)), None);
    }

    #[test]
    fn a_truncated_png_header_is_not_a_png() {
        let mut bytes = png(1, 1);
        bytes.truncate(20);
        assert_eq!(sniff(&bytes), None);
    }

    #[test]
    fn a_jpeg_is_found_behind_its_segment_chain() {
        // SOI, then an APP0 segment of declared length, then SOF0 carrying
        // the size. Walking the chain is the whole of the parse.
        let mut bytes = vec![0xFF, 0xD8];
        bytes.extend_from_slice(&[0xFF, 0xE0, 0x00, 0x08, 1, 2, 3, 4, 5, 6]);
        bytes.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08]);
        bytes.extend_from_slice(&200u16.to_be_bytes());
        bytes.extend_from_slice(&300u16.to_be_bytes());
        bytes.extend_from_slice(&[0; 8]);
        assert_eq!(sniff(&bytes), Some(BinaryFormat::Jpeg { width: 300, height: 200 }));
    }

    #[test]
    fn a_segment_with_no_length_does_not_spin() {
        // A declared length below two would leave the offset where it was.
        // The loop has to end rather than walk the same byte forever.
        let mut bytes = vec![0xFF, 0xD8];
        bytes.extend_from_slice(&[0xFF, 0xE0, 0x00, 0x00]);
        bytes.extend_from_slice(&[0; 32]);
        assert_eq!(sniff(&bytes), None);
    }

    #[test]
    fn a_pdf_is_identified_and_not_measured() {
        assert_eq!(sniff(b"%PDF-1.7\nrest"), Some(BinaryFormat::Pdf));
        assert_eq!(BinaryFormat::Pdf.dimensions(), None);
    }

    #[test]
    fn anything_else_is_nothing() {
        assert_eq!(sniff(b""), None);
        assert_eq!(sniff(b"GIF89a"), None);
        assert_eq!(sniff(&[0x89, b'P', b'N']), None);
    }
}
