//! `standard/programs.ts`: the standard faces as programs a renderer can
//! load, each built-in face in an OpenType font, decoded the first time it's
//! asked for (about 20 KB each).

use std::sync::OnceLock;

use serde::Serialize;

use crate::base64::decode_base64;
use crate::{FontStyle, StandardFont, face};

/// A font program a renderer can mount: its bytes, the CSS family it goes by,
/// and its style. Serializes to its data members, as the TypeScript object
/// minus `load`.
#[derive(Debug, Serialize)]
pub struct FontProgram {
    /// SHA-256 of the bytes, hex.
    pub hash: &'static str,
    /// The CSS family to mount it under ([`font_program_family`]).
    pub family: String,
    pub weight: u32,
    pub style: FontStyle,
    #[serde(skip)]
    font: StandardFont,
}

impl FontProgram {
    /// The bytes, the same slice every time. The TypeScript's `load()` is
    /// asynchronous because it imports the program's module on demand, and
    /// retries a failed import; here the program is in the binary, so the
    /// first call decodes it and none can fail.
    pub fn load(&self) -> &'static [u8] {
        static BYTES: [OnceLock<Vec<u8>>; 14] = [const { OnceLock::new() }; 14];
        BYTES[self.font as usize].get_or_init(|| decode_base64(face(self.font).program))
    }
}

/// The CSS family a mounted program goes by: named by its hash, it can't
/// shadow any other font. Takes the first 16 UTF-16 code units, as
/// `String#slice` does.
pub fn font_program_family(hash: &str) -> String {
    let mut units = 0;
    let prefix: String = hash
        .chars()
        .take_while(|c| {
            units += c.len_utf16();
            units <= 16
        })
        .collect();
    format!("epdf-{prefix}")
}

/// The face's program, the same value every time.
pub fn standard_font_program(font: StandardFont) -> &'static FontProgram {
    static PROGRAMS: [OnceLock<FontProgram>; 14] = [const { OnceLock::new() }; 14];
    PROGRAMS[font as usize].get_or_init(|| {
        let hash = face(font).program_hash;
        let standard = font.face();
        FontProgram {
            hash,
            family: font_program_family(hash),
            weight: standard.weight,
            style: if standard.italic { FontStyle::Italic } else { FontStyle::Normal },
            font,
        }
    })
}
