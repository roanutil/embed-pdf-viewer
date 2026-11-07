//! `fonts.ts` and `paint.ts`: the fonts a layout draws with, as a browser
//! draws them, and which one draws each grapheme, as the engine picks it
//! (`MatchGrapheme`). The box's font if it has every character of the
//! grapheme, else the first fallback, in the order the engine tries them for
//! the box's style, that has them all, else the last-resort font, which draws
//! a box. A grapheme that draws nothing stays with the box's font.
//!
//! The TypeScript's layout is generic over its font type (`TextFont`, or the
//! viewer's `PaintFont`, which adds the CSS and program a browser draws it
//! with); every caller lays out in paint fonts, so here there is one type,
//! [`PaintFont`], held by `Rc`, and two fonts are the same font when they are
//! the same `Rc` (the TypeScript compares them by identity).

use std::cell::OnceCell;
use std::rc::Rc;

use epdf_annotation_core::SceneFontProgram;
use epdf_annotation_core::json::Object;
use epdf_fonts::{
    FontStyle, StandardFamily, StandardFont, StandardFontMetrics, font_program_family,
    standard_font_css, standard_font_metrics, standard_font_program,
};
use serde::Serialize;
use serde_json::Value;

use super::bidi::mirror_of;
use super::graphemes::{code_points_of, is_invisible_grapheme, needs_glyph};

include!(concat!(env!("OUT_DIR"), "/last_resort.rs"));

/// What the engine lays text out with in the last-resort program, in
/// thousandths of an em: its box's advance, and its ascent and descent
/// (`LAST_RESORT_METRICS`).
pub const LAST_RESORT_ADVANCE: f64 = 700.0;
pub const LAST_RESORT_ASCENT: f64 = 800.0;
pub const LAST_RESORT_DESCENT: f64 = -200.0;

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

    /// `load()`: the program's bytes.
    pub fn load(&self) -> Vec<u8> {
        match &self.bytes {
            ProgramBytes::Standard(font) => standard_font_program(*font).load().to_vec(),
            ProgramBytes::LastResort => LAST_RESORT_PROGRAM.to_vec(),
            ProgramBytes::Given(load) => load(),
        }
    }
}

fn style_name(style: FontStyle) -> &'static str {
    match style {
        FontStyle::Normal => "normal",
        FontStyle::Italic => "italic",
    }
}

impl SceneFontProgram for MountableFontProgram {
    fn members(&self) -> Object {
        match serde_json::to_value(self) {
            Ok(Value::Object(members)) => members,
            _ => Object::new(),
        }
    }
    fn family(&self) -> &str {
        &self.family
    }
    fn weight(&self) -> f64 {
        f64::from(self.weight)
    }
    fn style(&self) -> &str {
        style_name(self.style)
    }
    fn load(&self) -> &[u8] {
        match &self.bytes {
            ProgramBytes::Standard(font) => standard_font_program(*font).load(),
            ProgramBytes::LastResort => LAST_RESORT_PROGRAM,
            // A scene's program is a standard face or the last resort; a
            // host's program is read through `MountableFontProgram::load`.
            ProgramBytes::Given(_) => &[],
        }
    }
}

/// A program's measure: how wide a text is in it, in thousandths of the
/// font size, shaped right to left when the flag says.
pub type MeasureText = Rc<dyn Fn(&[u16], bool) -> f64>;

/// How a font answers `covers` and `width`.
#[derive(Clone)]
enum Measure {
    Standard(&'static StandardFontMetrics),
    LastResort,
    Program {
        /// `(start, count)`, sorted by start.
        covers: Vec<(u32, u32)>,
        measure: MeasureText,
    },
}

/// `PaintFont`: a font as the layout reads it (`TextFont`), with the CSS and
/// program a browser draws it with. Serializes to its data members.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaintFont {
    /// What the caller names it by: a standard font's name, or its program's
    /// display hash.
    pub key: String,
    /// How far it reaches above the baseline, in thousandths of the font size.
    pub ascent: f64,
    /// How far below, in thousandths of the font size, as a negative number.
    pub descent: f64,
    /// One of the 14 standard fonts: one glyph per character through its
    /// encoding, no shaping.
    pub standard: bool,
    /// The CSS family list that draws it, its program's family first.
    pub font_family: String,
    pub font_weight: u32,
    pub font_style: FontStyle,
    /// The program it draws with.
    pub program: Rc<MountableFontProgram>,
    #[serde(skip)]
    measure: Measure,
}

impl std::fmt::Debug for PaintFont {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PaintFont").field("key", &self.key).finish_non_exhaustive()
    }
}

impl PaintFont {
    /// Whether it has a glyph for `code_point`.
    pub fn covers(&self, code_point: u32) -> bool {
        match &self.measure {
            Measure::Standard(metrics) => {
                code_point <= 0xffff && metrics.advance(code_point).is_some()
            }
            Measure::LastResort => code_point >= 0x20 && !(0xd800..=0xdfff).contains(&code_point),
            Measure::Program { covers, .. } => {
                let (mut low, mut high) = (0isize, covers.len() as isize - 1);
                while low <= high {
                    let middle = (low + high) >> 1;
                    let (start, count) = covers[middle as usize];
                    if code_point < start {
                        high = middle - 1;
                    } else if f64::from(code_point) >= f64::from(start) + f64::from(count) {
                        low = middle + 1;
                    } else {
                        return true;
                    }
                }
                false
            }
        }
    }

    /// How wide `text` is in it, in thousandths of the font size: shaped as
    /// one run, right to left when `rtl`.
    pub fn width(&self, text: &[u16], rtl: bool) -> f64 {
        match &self.measure {
            Measure::Standard(metrics) => code_points_of(text)
                .into_iter()
                .map(|cp| {
                    f64::from(metrics.advance(if rtl { mirror_of(cp) } else { cp }).unwrap_or(0))
                })
                .sum(),
            Measure::LastResort => code_points_of(text)
                .into_iter()
                .filter(|&cp| cp != 0x200d && !is_variation_selector(cp))
                .map(|_| LAST_RESORT_ADVANCE)
                .sum(),
            Measure::Program { measure, .. } => measure(text, rtl),
        }
    }
}

fn is_variation_selector(code_point: u32) -> bool {
    (0xfe00..=0xfe0f).contains(&code_point) || (0xe0100..=0xe01ef).contains(&code_point)
}

/// `PaintFonts`: the fonts a box's text is drawn with.
#[derive(Clone, Debug)]
pub struct PaintFonts {
    /// The box's own font.
    pub face: Rc<PaintFont>,
    /// The fallbacks, in the order the engine tries them for the box's style.
    pub fallbacks: Vec<Rc<PaintFont>>,
    /// The font that draws a box for a character no font has; `None` when
    /// there is none.
    pub last_resort: Option<Rc<PaintFont>>,
}

/// `GraphemeMatch`.
pub struct GraphemeMatch {
    pub font: Rc<PaintFont>,
    /// `false` when no font has the grapheme: it is drawn as a box, or not at
    /// all.
    pub covered: bool,
}

/// Whether `font` has every character of `grapheme` that needs a glyph.
pub fn covers_grapheme(font: &PaintFont, grapheme: &[u16]) -> bool {
    code_points_of(grapheme).into_iter().all(|cp| !needs_glyph(cp) || font.covers(cp))
}

/// The font that draws `grapheme`.
pub fn match_grapheme(grapheme: &[u16], fonts: &PaintFonts) -> GraphemeMatch {
    if covers_grapheme(&fonts.face, grapheme) {
        return GraphemeMatch { font: fonts.face.clone(), covered: true };
    }
    if let Some(fallback) = fonts.fallbacks.iter().find(|font| covers_grapheme(font, grapheme)) {
        return GraphemeMatch { font: fallback.clone(), covered: true };
    }
    let last_resort =
        if is_invisible_grapheme(grapheme) { None } else { fonts.last_resort.clone() };
    GraphemeMatch { font: last_resort.unwrap_or_else(|| fonts.face.clone()), covered: false }
}

thread_local! {
    static STANDARD: [OnceCell<Rc<PaintFont>>; 14] = const { [const { OnceCell::new() }; 14] };
    static LAST_RESORT: OnceCell<Rc<PaintFont>> = const { OnceCell::new() };
}

/// A standard font as the engine draws it with its built-in face: its
/// advances and line metrics, one glyph per character its encoding writes, a
/// right-to-left run's characters mirrored. The same `Rc` every time.
pub fn standard_paint_font(font: StandardFont) -> Rc<PaintFont> {
    STANDARD.with(|cache| {
        cache[font as usize]
            .get_or_init(|| {
                let metrics = standard_font_metrics(font);
                let css = standard_font_css(font);
                let program = standard_font_program(font);
                Rc::new(PaintFont {
                    key: font.as_str().to_string(),
                    ascent: f64::from(metrics.ascent),
                    descent: f64::from(metrics.descent),
                    standard: true,
                    font_family: css.font_family,
                    font_weight: css.font_weight,
                    font_style: css.font_style,
                    program: Rc::new(MountableFontProgram {
                        hash: program.hash.to_string(),
                        family: program.family.clone(),
                        weight: program.weight,
                        style: program.style,
                        bytes: ProgramBytes::Standard(font),
                    }),
                    measure: Measure::Standard(metrics),
                })
            })
            .clone()
    })
}

/// A font program the engine draws with, as a browser draws it: the
/// characters it covers (`(start, count)` ranges) and its line metrics as the
/// engine reads them (`doc.fonts.getProgram`), and its widths from `measure`,
/// the browser's own shaping of the mounted program, in thousandths of the
/// font size.
pub fn program_paint_font(
    covers: &[(u32, u32)],
    ascent: f64,
    descent: f64,
    program: MountableFontProgram,
    measure: MeasureText,
) -> Rc<PaintFont> {
    let mut sorted = covers.to_vec();
    sorted.sort_by_key(|&(start, _)| start);
    Rc::new(PaintFont {
        key: program.hash.clone(),
        ascent,
        descent,
        standard: false,
        font_family: format!("\"{}\"", program.family),
        font_weight: program.weight,
        font_style: program.style,
        program: Rc::new(program),
        measure: Measure::Program { covers: sorted, measure },
    })
}

/// The last-resort font: one box for every character from U+0020 up, each as
/// wide as the box the engine draws (joiners and variation selectors draw
/// nothing). The same `Rc` every time.
pub fn last_resort_paint_font() -> Rc<PaintFont> {
    LAST_RESORT.with(|cache| {
        cache
            .get_or_init(|| {
                let family = font_program_family(LAST_RESORT_HASH);
                Rc::new(PaintFont {
                    key: LAST_RESORT_HASH.to_string(),
                    ascent: LAST_RESORT_ASCENT,
                    descent: LAST_RESORT_DESCENT,
                    standard: false,
                    font_family: format!("\"{family}\""),
                    font_weight: 400,
                    font_style: FontStyle::Normal,
                    program: Rc::new(MountableFontProgram {
                        hash: LAST_RESORT_HASH.to_string(),
                        family,
                        weight: 400,
                        style: FontStyle::Normal,
                        bytes: ProgramBytes::LastResort,
                    }),
                    measure: Measure::LastResort,
                })
            })
            .clone()
    })
}

/// `standardFontStack(font)`: the platform's look-alikes of a standard face,
/// as a CSS family list (`STACKS`).
pub fn standard_font_stack(font: StandardFont) -> &'static str {
    match font.face().family {
        StandardFamily::Helvetica => "Helvetica, Arial, sans-serif",
        StandardFamily::Times => "\"Times New Roman\", Times, serif",
        StandardFamily::Courier => "\"Courier New\", Courier, monospace",
        StandardFamily::Symbol => "Symbol, serif",
        StandardFamily::ZapfDingbats => "\"Zapf Dingbats\", ZapfDingbats, serif",
    }
}

/// `standardFontOf(name)`: the standard font a name is, Helvetica for any
/// other.
pub fn standard_font_of(name: &str) -> StandardFont {
    StandardFont::from_name(name).unwrap_or(StandardFont::Helvetica)
}

/// The fonts a box draws with when the viewer gives none: its standard font
/// (Helvetica for any other), no fallbacks, and the last resort.
pub fn default_paint_fonts(font_family: Option<&str>) -> PaintFonts {
    PaintFonts {
        face: standard_paint_font(standard_font_of(font_family.unwrap_or("helvetica"))),
        fallbacks: Vec::new(),
        last_resort: Some(last_resort_paint_font()),
    }
}
