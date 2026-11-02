//! A document's fonts as the fork finds them, read without loading a page.
//!
//! Ports `services/src/features/fonts/DocumentFontReader.ts`: the faces a
//! document's dictionaries name and what each program is, the engine's
//! fonts, the fonts a write would draw text with, and programs by hash.
//! Facts only: what a font allows is decided above the worker, from these.
//! The `document.fonts.list`, `document.fonts.resolve` and
//! `document.fonts.program` kinds answer with them.

use serde::Serialize;

use super::list::{
    DocumentFaceFacts, Embedded, FontListFacts, FontProgramFacts, ProgramPermission,
    document_fonts_of, document_program_for_name,
};
use super::{FontFormat, FontRegistry, ProgramInfo, hex, thousandths};
use crate::access::OwnedDocument;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::annotation::rich_text::standard_face;
use crate::features::annotation::vocabulary::StandardFont;
use crate::identity::AnnotationRef;

/// `FontResolveRun`: a run of text and the font it asks for, by the name
/// `fontFamily` takes or by its face.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontResolveRun {
    Named { text: String, font_family: String },
    Face { text: String, family: String, weight: i32, italic: bool },
}

impl FontResolveRun {
    fn text(&self) -> &str {
        match self {
            Self::Named { text, .. } | Self::Face { text, .. } => text,
        }
    }

    /// One run off the wire. A run that has `fontFamily` names its font by
    /// it, whatever else it has (`'fontFamily' in run`). `weight` takes
    /// `ToInt32` of the number, as the `Int32Array` the TypeScript writes it
    /// into does.
    pub fn from_wire(value: &serde_json::Value) -> Result<Self, EngineError> {
        let refused = |what: &str| {
            EngineError::with_message(
                EngineErrorCode::WireFormat,
                format!("a 'document.fonts.resolve' run's {what}"),
            )
        };
        let object = value.as_object().ok_or_else(|| refused("shape: not an object"))?;
        let string = |name: &str| {
            object
                .get(name)
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| refused(&format!("'{name}' is not a string")))
        };
        let text = string("text")?;
        if object.contains_key("fontFamily") {
            return Ok(Self::Named { text, font_family: string("fontFamily")? });
        }
        let weight = object
            .get("weight")
            .and_then(serde_json::Value::as_f64)
            .map(crate::js_number::to_int32)
            .ok_or_else(|| refused("'weight' is not a number"))?;
        let italic = object
            .get("italic")
            .and_then(serde_json::Value::as_bool)
            .ok_or_else(|| refused("'italic' is not a boolean"))?;
        Ok(Self::Face { text, family: string("family")?, weight, italic })
    }
}

/// `ResolvedRunFacts['source']`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunSource {
    Standard,
    Engine,
    DocumentProgram,
    DocumentFont,
}

/// `ResolvedRunFacts`: what the fork's resolver says about one run.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedRunFacts {
    pub source: RunSource,
    pub engine_font: Option<String>,
    pub object_number: u32,
    pub display_sha256: Option<String>,
    pub base_font_name: Option<String>,
    pub family_name: String,
    pub weight: i32,
    pub italic: bool,
    pub degraded: bool,
    pub fallbacks: Vec<String>,
}

/// What `DocumentFontReader.resolve` answers.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FontResolutionFacts {
    pub runs: Vec<ResolvedRunFacts>,
    pub missing_characters: Vec<String>,
}

/// `readDocumentFaces`: the faces a document's fonts are, with the facts
/// about each program (the fork's walk). `Unknown` when the fork answers no
/// snapshot.
pub fn read_document_faces(
    document: &pdfium::Document,
) -> Result<Vec<DocumentFaceFacts>, EngineError> {
    let faces = document.font_faces().map_err(super::engine_error)?.ok_or_else(|| {
        EngineError::with_message(EngineErrorCode::Unknown, "EPDFFontFaces_Load failed")
    })?;
    Ok(faces
        .into_iter()
        .map(|face| DocumentFaceFacts {
            family_name: face.family_name.unwrap_or_default(),
            weight: face.weight,
            italic: face.italic,
            programs: face.programs.into_iter().map(program_facts).collect(),
        })
        .collect())
}

/// `programFacts`: one program's facts, named as the wire names them.
fn program_facts(program: pdfium::DocumentFontProgram) -> FontProgramFacts {
    let (ascent, descent) = thousandths(program.metrics);
    FontProgramFacts {
        object_number: program.object_number,
        format: FontFormat::of_program(program.format),
        embedded: match program.embedding {
            Some(pdfium::FontEmbedding::Whole) => Embedded::Whole,
            Some(pdfium::FontEmbedding::Subset) => Embedded::Subset,
            _ => Embedded::None,
        },
        post_script_name: program.postscript_name.filter(|name| !name.is_empty()),
        permission: ProgramPermission::from_code(program.embedding_permission),
        maps_unicode: program.maps_unicode,
        sha256: program.sha256.map(|hash| hex(&hash)),
        display_sha256: program.display_sha256.map(|hash| hex(&hash)),
        ascent,
        descent,
    }
}

/// `DocumentFontReader.facts`: everything the document's font list is made
/// from.
pub fn facts(
    document: &pdfium::Document,
    fonts: &FontRegistry,
) -> Result<FontListFacts, EngineError> {
    Ok(FontListFacts {
        faces: read_document_faces(document)?,
        engine_fonts: fonts.engine_font_facts(),
        fallbacks: fonts.fallback_order(),
    })
}

/// The face a run asks for (`faceOfRun`).
struct RunFace {
    family: String,
    weight: i32,
    italic: bool,
    program: i32,
}

/// `faceOfRun`: the face a run asks for, found as a write finds it: a
/// standard name's, an engine font's, a document font's (with the program a
/// write would name, when new text can be written in it), or a family.
fn face_of_run(run: &FontResolveRun, facts: &FontListFacts, fonts: &FontRegistry) -> RunFace {
    let name = match run {
        FontResolveRun::Face { family, weight, italic, .. } => {
            return RunFace {
                family: family.clone(),
                weight: *weight,
                italic: *italic,
                program: 0,
            };
        }
        FontResolveRun::Named { font_family, .. } => font_family.as_str(),
    };
    if let Some(standard) = StandardFont::from_family(name) {
        let (family, weight, italic) = standard_face(standard);
        let weight = i32::try_from(weight).unwrap_or(400);
        return RunFace { family: family.to_owned(), weight, italic, program: 0 };
    }
    if let Some(identity) = fonts.describe(name) {
        return RunFace {
            family: identity.family_name,
            weight: identity.weight,
            italic: identity.italic,
            program: 0,
        };
    }
    if let Some(font) = document_fonts_of(&facts.faces).into_iter().find(|font| font.name == name) {
        let program = document_program_for_name(&facts.faces, name)
            .filter(|choice| choice.choosable)
            .map_or(0, |choice| i32::try_from(choice.object_number).unwrap_or(0));
        return RunFace {
            family: font.face.family_name,
            weight: font.face.weight,
            italic: font.face.italic,
            program,
        };
    }
    // `faceForFontName` with no describer: the name as a family.
    RunFace { family: name.to_owned(), weight: 400, italic: false, program: 0 }
}

/// `DocumentFontReader.resolve`: the fonts a write would draw each run in,
/// and the characters no font has. `annotation` is the box being edited,
/// opened off the document without loading its page.
pub fn resolve(
    owned: &OwnedDocument,
    fonts: &FontRegistry,
    annotation: Option<&AnnotationRef>,
    runs: &[FontResolveRun],
    facts: &FontListFacts,
) -> Result<FontResolutionFacts, EngineError> {
    let document = owned.document();
    let asked: Vec<pdfium::FontResolveRun> = runs
        .iter()
        .map(|run| {
            let face = face_of_run(run, facts, fonts);
            pdfium::FontResolveRun {
                text: run.text().to_owned(),
                family: face.family,
                weight: face.weight,
                italic: face.italic,
                program: face.program,
            }
        })
        .collect();
    let opened = match annotation {
        Some(reference) => {
            let page_index = owned.page_index(reference.page())?;
            let index = crate::features::annotation::write::resolve::resolve_annotation_index_raw(
                document, page_index, reference,
            )?;
            let found = document.raw_annotation(page_index, index).ok_or_else(|| {
                EngineError::with_message(EngineErrorCode::NotFound, "annotation not found")
            })?;
            Some(found)
        }
        None => None,
    };
    let resolution =
        document.resolve_fonts(opened.as_ref(), &asked).map_err(super::engine_error)?.ok_or_else(
            || EngineError::with_message(EngineErrorCode::Unknown, "EPDFDoc_ResolveFonts failed"),
        )?;
    let runs = resolution
        .runs
        .into_iter()
        .map(|run| ResolvedRunFacts {
            source: match run.source {
                Some(pdfium::FontSource::Registered) => RunSource::Engine,
                Some(pdfium::FontSource::DocumentProgram) => RunSource::DocumentProgram,
                Some(pdfium::FontSource::DocumentFont) => RunSource::DocumentFont,
                _ => RunSource::Standard,
            },
            engine_font: run.registered_font.and_then(|id| fonts.key_for_id(id)),
            object_number: run.program_object_number,
            display_sha256: run.display_sha256.map(|hash| hex(&hash)),
            base_font_name: run.base_font_name.filter(|name| !name.is_empty()),
            family_name: run.family_name.unwrap_or_default(),
            weight: run.weight,
            italic: run.italic,
            degraded: run.degraded,
            fallbacks: run.fallbacks.into_iter().filter_map(|id| fonts.key_for_id(id)).collect(),
        })
        .collect();
    Ok(FontResolutionFacts { runs, missing_characters: resolution.missing_characters })
}

/// `DocumentFontReader.program`: a program the document's fonts name, by
/// its display hash: an engine font's or a document program's. Its coverage
/// and metrics, and with `data` the bytes a browser loads. `NotFound` for a
/// hash neither names.
pub fn program(
    document: &pdfium::Document,
    fonts: &FontRegistry,
    display_hash: &str,
    data: bool,
) -> Result<(ProgramInfo, Option<Vec<u8>>), EngineError> {
    if let Some(info) = fonts.program_info(display_hash)? {
        let bytes = if data { fonts.program_bytes(display_hash)? } else { None };
        return Ok((info, bytes));
    }
    let faces = read_document_faces(document)?;
    let found = faces.iter().flat_map(|face| &face.programs).find(|candidate| {
        candidate.display_sha256.as_deref() == Some(display_hash) && candidate.object_number > 0
    });
    let (Some(program), Some(hash)) = (found, bytes_of_hex(display_hash)) else {
        return Err(EngineError::with_message(
            EngineErrorCode::NotFound,
            format!("no font program {display_hash}"),
        ));
    };
    // By the hash: a stream two font dictionaries read differently is two
    // programs.
    let covers = document
        .font_display_program_coverage(&hash)
        .map_err(super::engine_error)?
        .into_iter()
        .map(Into::into)
        .collect();
    let bytes = if data {
        Some(
            document
                .font_display_program_data(&hash)
                .map_err(super::engine_error)?
                .unwrap_or_default(),
        )
    } else {
        None
    };
    Ok((ProgramInfo { covers, ascent: program.ascent, descent: program.descent }, bytes))
}

/// `bytesOfHex`: the 32 bytes a SHA-256's hex names. Only a display hash
/// the reader wrote reaches it, so anything else is `None`.
fn bytes_of_hex(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 || !hex.is_ascii() {
        return None;
    }
    let mut out = [0u8; 32];
    for (at, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[at * 2..at * 2 + 2], 16).ok()?;
    }
    Some(out)
}
