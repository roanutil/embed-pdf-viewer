//! The Phase 0 buffer contract.
//!
//! Every pixel buffer that crosses the engine boundary is described by one
//! [`BufferLayout`]: version, dimensions, stride, pixel format, alpha mode,
//! colour space. Constructing a layout validates every length and
//! multiplication before anything is allocated, and a buffer's length is
//! always exactly what its layout says, so nothing downstream frees by guessed
//! capacity.
//!
//! Two rules live outside this type because they belong to the host adapters:
//! the release operation is an explicit call, never an implicit drop across
//! the FFI boundary, and a WASM host rebuilds its heap views after memory
//! growth, because a `Uint8Array` over the old buffer is detached the moment
//! the heap grows.

use serde::{Deserialize, Serialize};

use crate::error::{EngineError, EngineErrorCode};

/// The layout version, carried so a host that was built against an older
/// engine can refuse a buffer rather than misread one.
pub const BUFFER_LAYOUT_VERSION: u16 = 1;

/// Byte order of the channels in the buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PixelFormat {
    /// PDFium's `FPDFBitmap_BGRA`, four bytes per pixel.
    Bgra8888,
}

impl PixelFormat {
    pub fn bytes_per_pixel(self) -> u32 {
        match self {
            Self::Bgra8888 => 4,
        }
    }
}

/// How the alpha channel relates to the colour channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AlphaMode {
    /// Colour channels are not multiplied by alpha. What PDFium writes.
    Unpremultiplied,
    Premultiplied,
    /// Alpha bytes are present but always `0xFF`, which is what a page
    /// rendered onto a white ground produces.
    Opaque,
}

/// The colour space the channel values are in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ColorSpace {
    /// PDFium renders to device RGB with no colour management. Naming it
    /// sRGB would be a claim about the values that nothing enforces.
    DeviceRgb,
}

/// The full description of one pixel buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BufferLayout {
    pub version: u16,
    pub width: u32,
    pub height: u32,
    /// Bytes per row, which may exceed `width * bytes_per_pixel` when a
    /// producer pads rows.
    pub stride: u32,
    pub format: PixelFormat,
    pub alpha: AlphaMode,
    pub color_space: ColorSpace,
}

impl BufferLayout {
    /// Validates a layout, refusing anything whose byte length is not
    /// representable.
    ///
    /// This is the admission check: it runs before the allocation it
    /// describes, so an unrepresentable request is an error rather than an
    /// abort or a wrapped multiplication.
    pub fn new(
        width: u32,
        height: u32,
        stride: u32,
        format: PixelFormat,
        alpha: AlphaMode,
        color_space: ColorSpace,
    ) -> Result<Self, EngineError> {
        let invalid = |what: &str| {
            EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("invalid buffer layout: {what}"),
            )
        };
        if width == 0 || height == 0 {
            return Err(invalid("zero width or height"));
        }
        let minimum_stride =
            width.checked_mul(format.bytes_per_pixel()).ok_or_else(|| invalid("width overflows"))?;
        if stride < minimum_stride {
            return Err(invalid("stride is narrower than one row of pixels"));
        }
        let bytes = stride
            .checked_mul(height)
            .ok_or_else(|| invalid("stride * height overflows"))?;
        if usize::try_from(bytes).map_err(|_| invalid("byte length exceeds usize"))?
            > isize::MAX as usize
        {
            return Err(invalid("byte length exceeds isize::MAX"));
        }
        Ok(Self {
            version: BUFFER_LAYOUT_VERSION,
            width,
            height,
            stride,
            format,
            alpha,
            color_space,
        })
    }

    /// A tightly packed layout: `stride == width * bytes_per_pixel`.
    pub fn packed(
        width: u32,
        height: u32,
        format: PixelFormat,
        alpha: AlphaMode,
        color_space: ColorSpace,
    ) -> Result<Self, EngineError> {
        let stride = width.checked_mul(format.bytes_per_pixel()).ok_or_else(|| {
            EngineError::with_message(
                EngineErrorCode::InvalidArg,
                "invalid buffer layout: width overflows",
            )
        })?;
        Self::new(width, height, stride, format, alpha, color_space)
    }

    /// Exactly how many bytes a conforming buffer holds.
    pub fn byte_len(&self) -> usize {
        // Checked in `new`, which is the only constructor.
        self.stride as usize * self.height as usize
    }
}

/// Pixels plus the layout that describes them, held together so neither can be
/// passed without the other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PixelBuffer {
    layout: BufferLayout,
    bytes: Vec<u8>,
}

impl PixelBuffer {
    /// Pairs bytes with a layout, refusing a length that disagrees.
    pub fn new(layout: BufferLayout, bytes: Vec<u8>) -> Result<Self, EngineError> {
        if bytes.len() != layout.byte_len() {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!(
                    "buffer is {} bytes, layout describes {}",
                    bytes.len(),
                    layout.byte_len()
                ),
            ));
        }
        Ok(Self { layout, bytes })
    }

    pub fn layout(&self) -> BufferLayout {
        self.layout
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Hands the allocation to the caller. The release operation is theirs
    /// from here, which is the explicit half of the contract.
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    /// One row, or `None` when `row` is past the bottom.
    pub fn row(&self, row: u32) -> Option<&[u8]> {
        if row >= self.layout.height {
            return None;
        }
        let start = row as usize * self.layout.stride as usize;
        Some(&self.bytes[start..start + self.layout.stride as usize])
    }
}

impl From<pdfium::Bitmap> for PixelBuffer {
    /// A rendered bitmap already satisfies the contract: `pdfium` validated
    /// the layout before allocating and filled exactly that many bytes.
    fn from(bitmap: pdfium::Bitmap) -> Self {
        let layout = BufferLayout {
            version: BUFFER_LAYOUT_VERSION,
            width: bitmap.width,
            height: bitmap.height,
            stride: bitmap.stride,
            format: PixelFormat::Bgra8888,
            // The renderer fills the page with opaque white first, so every
            // alpha byte is 0xFF.
            alpha: AlphaMode::Opaque,
            color_space: ColorSpace::DeviceRgb,
        };
        Self { layout, bytes: bitmap.bgra }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packed(width: u32, height: u32) -> Result<BufferLayout, EngineError> {
        BufferLayout::packed(
            width,
            height,
            PixelFormat::Bgra8888,
            AlphaMode::Opaque,
            ColorSpace::DeviceRgb,
        )
    }

    #[test]
    fn a_packed_layout_reports_its_exact_byte_length() {
        let layout = packed(612, 792).unwrap();
        assert_eq!(layout.stride, 612 * 4);
        assert_eq!(layout.byte_len(), 612 * 4 * 792);
        assert_eq!(layout.version, BUFFER_LAYOUT_VERSION);
    }

    #[test]
    fn overflowing_layouts_are_refused_before_any_allocation() {
        for (width, height) in [(0, 1), (1, 0), (u32::MAX, 1), (1 << 30, 1), (1 << 20, 1 << 20)] {
            let err = packed(width, height).unwrap_err();
            assert!(err.is(EngineErrorCode::InvalidArg), "{width}x{height} was accepted");
        }
    }

    #[test]
    fn a_stride_narrower_than_a_row_is_refused() {
        let err = BufferLayout::new(
            10,
            10,
            39, // one byte short of 10 * 4
            PixelFormat::Bgra8888,
            AlphaMode::Opaque,
            ColorSpace::DeviceRgb,
        )
        .unwrap_err();
        assert!(err.is(EngineErrorCode::InvalidArg));
        // Padded rows are legal, so a wider stride is accepted.
        let padded = BufferLayout::new(
            10,
            10,
            48,
            PixelFormat::Bgra8888,
            AlphaMode::Opaque,
            ColorSpace::DeviceRgb,
        )
        .unwrap();
        assert_eq!(padded.byte_len(), 480);
    }

    #[test]
    fn a_buffer_whose_length_disagrees_with_its_layout_is_refused() {
        let layout = packed(4, 2).unwrap();
        assert!(PixelBuffer::new(layout, vec![0; 32]).is_ok());
        assert!(PixelBuffer::new(layout, vec![0; 31]).is_err());
        assert!(PixelBuffer::new(layout, vec![0; 33]).is_err());
    }

    #[test]
    fn rows_are_addressed_by_stride_and_stop_at_the_bottom() {
        let layout = BufferLayout::new(
            2,
            3,
            12, // 8 bytes of pixels plus 4 of padding
            PixelFormat::Bgra8888,
            AlphaMode::Opaque,
            ColorSpace::DeviceRgb,
        )
        .unwrap();
        let mut bytes = vec![0u8; layout.byte_len()];
        bytes[12] = 0xAB; // first byte of row 1
        let buffer = PixelBuffer::new(layout, bytes).unwrap();

        assert_eq!(buffer.row(1).unwrap()[0], 0xAB);
        assert_eq!(buffer.row(0).unwrap().len(), 12);
        assert!(buffer.row(3).is_none());
    }
}
