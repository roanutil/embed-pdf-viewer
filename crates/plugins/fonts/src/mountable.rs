//! `MountableFontProgram` (`standard/programs.ts`) and the last-resort program
//! (`last-resort.ts`): a font program as a browser mounts it, whether it is a
//! standard face, the last resort or a program the host has.

use std::rc::Rc;

use serde::Serialize;

use crate::{FontStyle, StandardFont, font_program_family, standard_font_program};

include!(concat!(env!("OUT_DIR"), "/last_resort.rs"));

/// `LAST_RESORT_METRICS`: what the engine lays text out with in the
/// last-resort program, in thousandths of an em.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LastResortMetrics {
    /// Its box's advance.
    pub advance: f64,
    pub ascent: f64,
    pub descent: f64,
}

/// `LAST_RESORT_METRICS`.
pub const LAST_RESORT_METRICS: LastResortMetrics =
    LastResortMetrics { advance: 700.0, ascent: 800.0, descent: -200.0 };

/// `lastResortFontBytes()`: the last-resort program, a box for every
/// character. About a kilobyte, decoded when the crate built.
pub fn last_resort_font_bytes() -> &'static [u8] {
    LAST_RESORT_PROGRAM
}

/// Where a program's bytes come from.
#[derive(Clone)]
enum ProgramBytes {
    Standard(StandardFont),
    LastResort,
    Given(Rc<dyn Fn() -> Vec<u8>>),
}

/// `MountableFontProgram`: a font program a browser mounts under its own
/// family, in its own weight and style. Serializes to its data members, as the
/// TypeScript object minus `load`.
#[derive(Clone, Serialize)]
pub struct MountableFontProgram {
    /// The program's display hash.
    pub hash: String,
    pub family: String,
    pub weight: u32,
    pub style: FontStyle,
    #[serde(skip)]
    bytes: ProgramBytes,
}

impl std::fmt::Debug for MountableFontProgram {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MountableFontProgram")
            .field("hash", &self.hash)
            .field("family", &self.family)
            .field("weight", &self.weight)
            .field("style", &self.style)
            .finish()
    }
}

impl MountableFontProgram {
    /// A program the host has, its bytes from `load`.
    pub fn new(
        hash: impl Into<String>,
        weight: u32,
        style: FontStyle,
        load: Rc<dyn Fn() -> Vec<u8>>,
    ) -> MountableFontProgram {
        let hash = hash.into();
        MountableFontProgram {
            family: font_program_family(&hash),
            hash,
            weight,
            style,
            bytes: ProgramBytes::Given(load),
        }
    }

    /// `standardFontProgram(font)` as a mountable program.
    pub fn standard(font: StandardFont) -> MountableFontProgram {
        let program = standard_font_program(font);
        MountableFontProgram {
            hash: program.hash.to_string(),
            family: program.family.clone(),
            weight: program.weight,
            style: program.style,
            bytes: ProgramBytes::Standard(font),
        }
    }

    /// `load()`: the program's bytes.
    pub fn load(&self) -> Vec<u8> {
        match &self.bytes {
            ProgramBytes::Given(load) => load(),
            _ => self.built_in_bytes().unwrap_or_default().to_vec(),
        }
    }

    /// The bytes of a program built into the binary (a standard face or the
    /// last resort), borrowed; `None` for a program the host has, read
    /// through [`MountableFontProgram::load`].
    pub fn built_in_bytes(&self) -> Option<&'static [u8]> {
        match &self.bytes {
            ProgramBytes::Standard(font) => Some(standard_font_program(*font).load()),
            ProgramBytes::LastResort => Some(LAST_RESORT_PROGRAM),
            ProgramBytes::Given(_) => None,
        }
    }
}

/// `lastResortFontProgram()`: the last-resort program, mounted under the
/// family its hash names.
pub fn last_resort_font_program() -> MountableFontProgram {
    MountableFontProgram {
        hash: LAST_RESORT_HASH.to_string(),
        family: font_program_family(LAST_RESORT_HASH),
        weight: 400,
        style: FontStyle::Normal,
        bytes: ProgramBytes::LastResort,
    }
}
