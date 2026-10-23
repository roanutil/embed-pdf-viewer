//! `packages/core/fonts` in Rust: the 14 standard PDF fonts as the engine
//! draws them. PDFium draws a standard font that a document doesn't embed with
//! one of its own built-in faces, on every platform; this crate holds those
//! faces as OpenType programs a renderer can load, and the numbers the engine
//! lays text out with in them.
//!
//! The data is the TypeScript package's generated output, read at build time
//! (`build.rs`), so both languages carry the same faces. Pure: no PDFium, no
//! I/O, builds for wasm32.
//!
//! | TypeScript | Rust |
//! |---|---|
//! | `standardFontMetrics(font)` | [`standard_font_metrics`] |
//! | `standardFontProgram(font)` | [`standard_font_program`] |
//! | `fontProgramFamily(hash)` | [`font_program_family`] |
//! | `standardFontCss(font)` | [`standard_font_css`] |
//! | `StandardFont`, `STANDARD_FONTS` (`engine-core`) | [`StandardFont`], [`StandardFont::ALL`] |
//! | `STANDARD_FACES` (`engine-core`) | [`StandardFont::face`] |

mod base64;
mod css;
mod font;
mod metrics;
mod programs;

#[cfg(feature = "harness")]
pub mod harness;

pub use css::{StandardFontCss, standard_font_css};
pub use font::{FontStyle, StandardFace, StandardFamily, StandardFont, UnknownStandardFont};
pub use metrics::{StandardFontMetrics, standard_font_metrics};
pub use programs::{FontProgram, font_program_family, standard_font_program};

/// One standard face's generated layout numbers and program
/// (`faces.generated.ts`, `programs/<font>.generated.ts`).
struct FaceData {
    /// In 1/1000 em, as the engine's text layout reads them: the face's font box.
    ascent: i32,
    descent: i32,
    /// SHA-256 of the face's display program, hex.
    program_hash: &'static str,
    /// The characters the face's encoding can write, in code order.
    code_points: &'static [u32],
    /// Each one's advance in 1/1000 em, in the same order.
    advances: &'static [u32],
    /// The OpenType program, base64.
    program: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/faces.rs"));

fn face(font: StandardFont) -> &'static FaceData {
    &FACES[font as usize]
}
