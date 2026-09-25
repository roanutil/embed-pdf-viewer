//! The two `engine-core` resource helpers the stamp tool calls:
//! `resolveBinarySource` (`resource/BinarySource.ts`) and
//! `sniffBinaryMetadata` (`resource/binaryMetadata.ts`).
//!
//! A `BinarySource` reaches the plugin as canonical JSON: bytes are
//! `{"$bytes": base64}`, a payload is `{data, mimeType?, name?}`. A `Blob`
//! has no spelling there, so only its TypeScript error paths survive.

use epdf_kernel::Error;
use epdf_kernel::bytes::Bytes;
use epdf_kernel::rt::Promise;
use serde_json::Value;

/// `WireResource`: bytes detached and ready to ship.
#[derive(Clone, Debug, PartialEq)]
pub struct WireResource {
    pub bytes: Vec<u8>,
    pub mime_type: Option<String>,
    pub name: Option<String>,
}

fn type_error(message: &str) -> Error {
    Error::named("TypeError", message)
}

/// The `TypeError` V8 throws reading `x.arrayBuffer` off a non-`Blob`.
fn not_a_blob(value: Option<&Value>) -> Error {
    match value {
        None => type_error("Cannot read properties of undefined (reading 'arrayBuffer')"),
        Some(Value::Null) => type_error("Cannot read properties of null (reading 'arrayBuffer')"),
        Some(v) if epdf_kernel::is_undefined(v) => {
            type_error("Cannot read properties of undefined (reading 'arrayBuffer')")
        }
        Some(_) => type_error("data.arrayBuffer is not a function"),
    }
}

fn resolve_now(source: &Value) -> Result<WireResource, Error> {
    if let Some(bytes) = Bytes::from_json(source) {
        return Ok(WireResource { bytes: bytes.0, mime_type: None, name: None });
    }
    match source {
        Value::Null => {
            return Err(type_error(
                "Cannot destructure property 'data' of 'source' as it is null.",
            ));
        }
        v if epdf_kernel::is_undefined(v) => {
            return Err(type_error(
                "Cannot destructure property 'data' of 'source' as it is undefined.",
            ));
        }
        _ => {}
    }
    let data = source.get("data");
    let Some(inner) = data.and_then(Bytes::from_json) else {
        return Err(not_a_blob(data.filter(|_| source.is_object())));
    };
    let non_empty = |k: &str| {
        source.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string)
    };
    let name = source.get("name").and_then(Value::as_str).map(str::to_string);
    Ok(WireResource { bytes: inner.0, mime_type: non_empty("mimeType"), name })
}

/// `resolveBinarySource(source)`: an `async` function with no await on the
/// paths a vector can express, so its promise has settled on return.
pub fn resolve_binary_source(source: &Value) -> Promise<WireResource> {
    match resolve_now(source) {
        Ok(resource) => Promise::resolved(resource),
        Err(error) => Promise::rejected(error),
    }
}

/// `BinaryMetadata`: the sniffed format, with intrinsic pixel dimensions
/// for the raster formats.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BinaryMetadata {
    pub mime_type: &'static str,
    /// `(width, height)`; `None` for PDF.
    pub size: Option<(f64, f64)>,
}

fn be32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn be16(b: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([b[at], b[at + 1]])
}

fn parse_png(b: &[u8]) -> Option<BinaryMetadata> {
    if b.len() < 24 {
        return None;
    }
    let (width, height) = (be32(b, 16), be32(b, 20));
    if width == 0 || height == 0 {
        return None;
    }
    Some(BinaryMetadata { mime_type: "image/png", size: Some((width as f64, height as f64)) })
}

fn parse_jpeg(b: &[u8]) -> Option<BinaryMetadata> {
    let mut offset = 2usize;
    while offset + 4 < b.len() {
        if b[offset] != 0xff {
            return None;
        }
        let marker = b[offset + 1];
        if (0xc0..=0xc3).contains(&marker) {
            if offset + 9 > b.len() {
                return None;
            }
            let height = be16(b, offset + 5);
            let width = be16(b, offset + 7);
            if width == 0 || height == 0 {
                return None;
            }
            return Some(BinaryMetadata {
                mime_type: "image/jpeg",
                size: Some((width as f64, height as f64)),
            });
        }
        if marker == 0xd8 || marker == 0xd9 {
            offset += 2;
        } else if marker == 0xff {
            offset += 1;
        } else {
            let seg = ((b[offset + 2] as usize) << 8) | b[offset + 3] as usize;
            offset += 2 + seg;
        }
    }
    None
}

/// `sniffBinaryMetadata(buffer)`: format by magic bytes, `None` for
/// unsupported or corrupt data.
pub fn sniff_binary_metadata(b: &[u8]) -> Option<BinaryMetadata> {
    if b.len() < 4 {
        return None;
    }
    if b[..4] == [0x89, 0x50, 0x4e, 0x47] {
        return parse_png(b);
    }
    if b[..3] == [0xff, 0xd8, 0xff] {
        return parse_jpeg(b);
    }
    if b[..4] == [0x25, 0x50, 0x44, 0x46] {
        return Some(BinaryMetadata { mime_type: "application/pdf", size: None });
    }
    None
}
