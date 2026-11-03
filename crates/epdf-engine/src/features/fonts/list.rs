//! What a document's fonts allow, as far as the worker reads it.
//!
//! Ports the parts of `engine/core/src/fonts/fontList.ts` that
//! `DocumentFontReader.ts` calls: the facts a font list is made from, which
//! programs new text may be written with (`canChooseProgram`), a document
//! program's name (`documentFontNameOf`), the document's fonts one per name
//! (`documentFontsOf`) and the program a `fontFamily` names
//! (`documentProgramForName`). The list itself (`fontListOf`) is built above
//! the worker, from these facts.

use serde::Serialize;

use super::{EngineFontFacts, FontFormat};
use crate::features::annotation::rich_text::{family_key, standard_family};

/// `FontProgramFacts['embedded']`: how much of a program the PDF holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Embedded {
    Whole,
    Subset,
    None,
}

/// `FontProgramFacts['permission']`: what the OS/2 `fsType` grants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProgramPermission {
    Installable,
    Editable,
    PreviewAndPrint,
    Restricted,
    BitmapOnly,
}

impl ProgramPermission {
    /// `PERMISSIONS` in `DocumentFontReader.ts`: `EPDF_FONT_EMBEDDING_*`,
    /// `None` for -1 (no OS/2 table) and any code it doesn't name.
    pub fn from_code(code: i32) -> Option<Self> {
        Some(match code {
            0 => Self::Installable,
            1 => Self::Editable,
            2 => Self::PreviewAndPrint,
            3 => Self::Restricted,
            4 => Self::BitmapOnly,
            _ => return None,
        })
    }
}

/// `FontProgramFacts`: a font program as the engine found it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FontProgramFacts {
    /// 0 when there is no program, or it is inline.
    pub object_number: u32,
    pub format: Option<FontFormat>,
    pub embedded: Embedded,
    pub post_script_name: Option<String>,
    pub permission: Option<ProgramPermission>,
    pub maps_unicode: bool,
    pub sha256: Option<String>,
    pub display_sha256: Option<String>,
    /// Thousandths of the font size; 0 when it doesn't load.
    pub ascent: f64,
    pub descent: f64,
}

/// `DocumentFaceFacts`: a face the document's fonts are.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentFaceFacts {
    pub family_name: String,
    pub weight: i32,
    pub italic: bool,
    pub programs: Vec<FontProgramFacts>,
}

/// `FontListFacts`: everything a font list is made from.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FontListFacts {
    pub faces: Vec<DocumentFaceFacts>,
    pub engine_fonts: Vec<EngineFontFacts>,
    /// Keys of the engine's fallback fonts, by tier, each in the order added.
    pub fallbacks: Vec<String>,
}

/// `DocumentFace`: a face as a font dictionary describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentFace {
    pub family_name: String,
    pub weight: i32,
    pub italic: bool,
}

/// `DocumentFontFacts`: every program the PDF draws under one name.
#[derive(Debug, Clone, PartialEq)]
pub struct DocumentFontFacts {
    pub name: String,
    /// How its first program's font dictionary describes it.
    pub face: DocumentFace,
    /// The program new text uses first, then the rest.
    pub programs: Vec<FontProgramFacts>,
}

/// `documentProgramForName`'s answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentProgramChoice {
    pub object_number: u32,
    pub choosable: bool,
}

fn is_sfnt(format: Option<FontFormat>) -> bool {
    matches!(format, Some(FontFormat::Truetype | FontFormat::Opentype))
}

fn allows_editing(permission: Option<ProgramPermission>) -> bool {
    matches!(permission, Some(ProgramPermission::Installable | ProgramPermission::Editable))
}

/// `canChooseProgram`: a whole TrueType or OpenType program that maps
/// characters through a Unicode cmap, whose licence allows editing.
pub fn can_choose_program(program: &FontProgramFacts) -> bool {
    program.embedded == Embedded::Whole
        && is_sfnt(program.format)
        && program.maps_unicode
        && allows_editing(program.permission)
}

/// `isDisplayable`: it has a display program, and its licence, when it
/// states one, allows editing.
fn is_displayable(program: &FontProgramFacts) -> bool {
    program.display_sha256.is_some()
        && (program.permission.is_none() || allows_editing(program.permission))
}

/// `programRank`: one it can author with, then one a box keeps, then the
/// rest embedded, then the ones not embedded.
fn program_rank(program: &FontProgramFacts) -> u8 {
    if can_choose_program(program) {
        0
    } else if is_displayable(program) {
        1
    } else if program.embedded != Embedded::None {
        2
    } else {
        3
    }
}

/// `WEIGHT_WORDS`: a weight's word in a style suffix; 400 has none.
fn weight_word(weight: i32) -> Option<&'static str> {
    Some(match weight {
        100 => "Thin",
        200 => "ExtraLight",
        300 => "Light",
        400 => "",
        500 => "Medium",
        600 => "SemiBold",
        700 => "Bold",
        800 => "ExtraBold",
        900 => "Black",
        _ => return None,
    })
}

/// `documentFontNameOf`: its PostScript name, else its family with a style
/// suffix in PDF's form (`Inter`, `Inter,Medium`, `Inter,BoldItalic`,
/// `Inter,450`).
pub fn document_font_name_of(face: &DocumentFace, post_script_name: Option<&str>) -> String {
    if let Some(name) = post_script_name.filter(|name| !name.is_empty()) {
        return name.to_owned();
    }
    let weight = weight_word(face.weight).map_or_else(|| face.weight.to_string(), str::to_owned);
    let style = format!("{weight}{}", if face.italic { "Italic" } else { "" });
    if style.is_empty() {
        face.family_name.clone()
    } else {
        format!("{},{style}", face.family_name)
    }
}

/// Whether two programs found under one name are the same program
/// (`documentFontsOf`'s `known`).
fn same_program(program: &FontProgramFacts, other: &FontProgramFacts) -> bool {
    if program.object_number > 0 {
        other.object_number == program.object_number
            && other.display_sha256 == program.display_sha256
    } else {
        other.object_number == 0
            && other.embedded == program.embedded
            && other.format == program.format
            && other.sha256 == program.sha256
    }
}

/// `documentFontsOf`: the document's fonts, one per name, in the order
/// first found, each described by the face of the program new text uses
/// first. A program with neither a PostScript name nor a family isn't one.
pub fn document_fonts_of(faces: &[DocumentFaceFacts]) -> Vec<DocumentFontFacts> {
    let mut named: Vec<(String, Vec<(DocumentFace, &FontProgramFacts)>)> = Vec::new();
    for face in faces {
        let described = DocumentFace {
            family_name: face.family_name.clone(),
            weight: face.weight,
            italic: face.italic,
        };
        for program in &face.programs {
            let post_script_name = program.post_script_name.as_deref().filter(|n| !n.is_empty());
            if post_script_name.is_none() && face.family_name.is_empty() {
                continue;
            }
            let name = document_font_name_of(&described, post_script_name);
            let at = match named.iter().position(|(n, _)| *n == name) {
                Some(at) => at,
                None => {
                    named.push((name, Vec::new()));
                    named.len() - 1
                }
            };
            let entries = &mut named[at].1;
            if !entries.iter().any(|(_, other)| same_program(program, other)) {
                entries.push((described.clone(), program));
            }
        }
    }
    named
        .into_iter()
        .map(|(name, mut entries)| {
            // `Array.prototype.sort` is stable, as `sort_by_key` is.
            entries.sort_by_key(|(_, program)| program_rank(program));
            DocumentFontFacts {
                name,
                face: entries[0].0.clone(),
                programs: entries.into_iter().map(|(_, program)| program.clone()).collect(),
            }
        })
        .collect()
}

/// `isStandardStandIn`: a document font that is a standard font the PDF only
/// names.
fn is_standard_stand_in(font: &DocumentFontFacts) -> bool {
    standard_family(&family_key(&font.face.family_name)).is_some()
        && font.programs.iter().all(|program| program.embedded == Embedded::None)
}

/// `documentProgramForName`: the program new text uses first in the
/// document font of that name. `None` when no document font has the name.
pub fn document_program_for_name(
    faces: &[DocumentFaceFacts],
    name: &str,
) -> Option<DocumentProgramChoice> {
    let font = document_fonts_of(faces)
        .into_iter()
        .find(|candidate| candidate.name == name && !is_standard_stand_in(candidate))?;
    let Some(program) = font.programs.first() else {
        return Some(DocumentProgramChoice { object_number: 0, choosable: false });
    };
    Some(DocumentProgramChoice {
        object_number: program.object_number,
        choosable: can_choose_program(program) && program.object_number > 0,
    })
}

/// `FontRestriction`: why a program can't be chosen or go to a browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FontRestriction {
    NotEmbedded,
    UnsupportedFormat,
    Subset,
    NoUnicodeMap,
    PreviewAndPrint,
}

/// `restrictionsOfProgram`: every reason a document program can't be
/// chosen, or can't go to a browser.
fn restrictions_of_program(program: &FontProgramFacts) -> Vec<FontRestriction> {
    let mut restrictions = Vec::new();
    if program.embedded == Embedded::None {
        restrictions.push(if program.format == Some(FontFormat::Type3) {
            FontRestriction::UnsupportedFormat
        } else {
            FontRestriction::NotEmbedded
        });
        return restrictions;
    }
    if program.embedded == Embedded::Subset {
        restrictions.push(FontRestriction::Subset);
    }
    if !is_sfnt(program.format) {
        restrictions.push(FontRestriction::UnsupportedFormat);
    } else if !program.maps_unicode {
        restrictions.push(FontRestriction::NoUnicodeMap);
    }
    if program.permission.is_some() && !allows_editing(program.permission) {
        restrictions.push(FontRestriction::PreviewAndPrint);
    }
    restrictions
}

/// `FontProgram` as a document font's list entry names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListedProgram {
    pub display_hash: Option<String>,
    pub source_hash: Option<String>,
    pub embedded: Embedded,
    pub format: Option<FontFormat>,
    pub embedding_permission: Option<ProgramPermission>,
    pub can_download: bool,
}

/// `documentProgramOf`.
fn listed_program(program: &FontProgramFacts) -> ListedProgram {
    let displayable = is_displayable(program);
    let none = program.embedded == Embedded::None;
    ListedProgram {
        display_hash: if displayable { program.display_sha256.clone() } else { None },
        source_hash: if none { None } else { program.sha256.clone() },
        embedded: program.embedded,
        format: if none { None } else { program.format },
        embedding_permission: match program.permission {
            Some(ProgramPermission::Restricted | ProgramPermission::BitmapOnly) => {
                Some(ProgramPermission::PreviewAndPrint)
            }
            other => other,
        },
        can_download: displayable,
    }
}

/// A `Font` of `source: 'document'`, as `documentFontOf` lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListedDocumentFont {
    pub name: String,
    pub family_name: String,
    pub weight: i32,
    pub italic: bool,
    pub post_script_name: Option<String>,
    pub can_choose: bool,
    pub restrictions: Vec<FontRestriction>,
    pub programs: Vec<ListedProgram>,
}

/// The document fonts `fontListOf` lists (`listedDocumentFontsOf` in
/// `DocumentFontReader.ts`): every document font but a standard font's
/// stand-in and the last resort a box drew with. What a write that changes
/// the list changes.
pub fn listed_document_fonts(faces: &[DocumentFaceFacts]) -> Vec<ListedDocumentFont> {
    let is_last_resort = |font: &DocumentFontFacts| {
        font.programs
            .iter()
            .any(|program| program.sha256.as_deref() == Some(super::LAST_RESORT_HASH))
    };
    document_fonts_of(faces)
        .into_iter()
        .filter(|font| !is_standard_stand_in(font) && !is_last_resort(font))
        .map(|font| {
            let first = font.programs.first();
            ListedDocumentFont {
                name: font.name.clone(),
                family_name: font.face.family_name.clone(),
                weight: font.face.weight,
                italic: font.face.italic,
                post_script_name: first.and_then(|p| p.post_script_name.clone()),
                can_choose: first.is_some_and(can_choose_program),
                restrictions: first
                    .map_or_else(|| vec![FontRestriction::NotEmbedded], restrictions_of_program),
                programs: font.programs.iter().map(listed_program).collect(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program(object_number: u32, post_script_name: Option<&str>) -> FontProgramFacts {
        FontProgramFacts {
            object_number,
            format: Some(FontFormat::Truetype),
            embedded: Embedded::Whole,
            post_script_name: post_script_name.map(str::to_owned),
            permission: Some(ProgramPermission::Installable),
            maps_unicode: true,
            sha256: Some("aa".into()),
            display_sha256: Some("aa".into()),
            ascent: 900.0,
            descent: -200.0,
        }
    }

    fn face(
        family: &str,
        weight: i32,
        italic: bool,
        programs: Vec<FontProgramFacts>,
    ) -> DocumentFaceFacts {
        DocumentFaceFacts { family_name: family.into(), weight, italic, programs }
    }

    #[test]
    fn a_program_is_named_by_its_postscript_name_else_its_face() {
        let face = |weight, italic| DocumentFace { family_name: "Inter".into(), weight, italic };
        assert_eq!(document_font_name_of(&face(400, false), Some("Inter-Medium")), "Inter-Medium");
        assert_eq!(document_font_name_of(&face(400, false), None), "Inter");
        assert_eq!(document_font_name_of(&face(500, false), None), "Inter,Medium");
        assert_eq!(document_font_name_of(&face(700, true), None), "Inter,BoldItalic");
        assert_eq!(document_font_name_of(&face(400, true), None), "Inter,Italic");
        assert_eq!(document_font_name_of(&face(450, false), None), "Inter,450");
    }

    #[test]
    fn one_name_is_one_font_and_a_choosable_program_comes_first() {
        let mut subset = program(7, Some("Inter"));
        subset.embedded = Embedded::Subset;
        let fonts = document_fonts_of(&[
            face("Inter", 400, false, vec![subset.clone(), program(7, Some("Inter"))]),
            // The same program under a second face is one program.
            face("Inter", 700, false, vec![program(9, Some("Inter")), subset.clone()]),
            // No PostScript name and no family: no name, so no font.
            face("", 400, false, vec![program(11, None)]),
        ]);
        assert_eq!(fonts.len(), 1, "{fonts:?}");
        let font = &fonts[0];
        assert_eq!(font.name, "Inter");
        // Object 7 read the same way twice is one program, kept as first
        // found (the subset); the choosable 9 ranks first, and the face is
        // its face.
        let numbers: Vec<_> = font.programs.iter().map(|p| p.object_number).collect();
        assert_eq!(numbers, [9, 7]);
        assert_eq!(font.programs[1].embedded, Embedded::Subset);
        assert_eq!(font.face.weight, 700);
    }

    #[test]
    fn a_standard_font_the_pdf_only_names_is_not_a_document_program() {
        let mut named = program(0, Some("Helvetica"));
        named.embedded = Embedded::None;
        named.format = None;
        let faces = [face("Helvetica", 400, false, vec![named])];
        assert_eq!(document_program_for_name(&faces, "Helvetica"), None);

        let faces = [face("Roboto", 400, false, vec![program(5, Some("Roboto-Regular"))])];
        assert_eq!(
            document_program_for_name(&faces, "Roboto-Regular"),
            Some(DocumentProgramChoice { object_number: 5, choosable: true })
        );
        let mut print_only = program(5, Some("Roboto-Regular"));
        print_only.permission = Some(ProgramPermission::PreviewAndPrint);
        let faces = [face("Roboto", 400, false, vec![print_only])];
        assert_eq!(
            document_program_for_name(&faces, "Roboto-Regular"),
            Some(DocumentProgramChoice { object_number: 5, choosable: false })
        );
    }
}
