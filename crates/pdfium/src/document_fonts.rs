//! A document's fonts, the fonts a write would use, and the characters a
//! write drew with no font.
//!
//! The reads `engine/services/src/features/fonts/DocumentFontReader.ts`
//! makes (`readDocumentFaces`, `resolve`, `program`, the fingerprint), the
//! missing-character list `DocumentSession` keeps, and the two `/DA` calls
//! that name a document's own font program. Facts only, as the fork reports
//! them: which font a write may choose, and what a list shows, are the
//! engine's.
//!
//! The fork hands faces and resolutions back as detached snapshots. Each is
//! read whole into owned values here and closed before returning, so no
//! snapshot handle outlives the call that made it.

use std::marker::PhantomData;

use crate::font_program::{
    CodePointRange, FontLineMetrics, FontProgramFormat, read_ranges, read_utf8, read_utf16,
    zeroed_bytes,
};
use crate::{Annotation, Document, FontId, PdfiumError};

/// `EPDF_FONT_EMBEDDED_*`: how much of a font the PDF holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontEmbedding {
    None,
    Whole,
    /// The `/BaseFont` carries a subset tag.
    Subset,
}

impl FontEmbedding {
    /// `None` for -1 (out of range) and any code the fork does not define.
    pub fn from_code(code: i32) -> Option<Self> {
        match code {
            0 => Some(Self::None),
            1 => Some(Self::Whole),
            2 => Some(Self::Subset),
            _ => None,
        }
    }
}

/// `EPDF_FONT_SOURCE_*`: where a resolved run's font comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontSource {
    /// One of the standard 14.
    Standard,
    /// A font registered on this thread.
    Registered,
    /// A program the document holds.
    DocumentProgram,
    /// The box's `/DA` font as it is.
    DocumentFont,
}

impl FontSource {
    /// `None` for -1 (out of range) and any code the fork does not define.
    pub fn from_code(code: i32) -> Option<Self> {
        match code {
            0 => Some(Self::Standard),
            1 => Some(Self::Registered),
            2 => Some(Self::DocumentProgram),
            3 => Some(Self::DocumentFont),
            _ => None,
        }
    }
}

/// One face a document's fonts are: family, weight and italic, and the
/// programs behind it.
#[derive(Debug, Clone, PartialEq)]
pub struct DocumentFontFace {
    /// `None` when the fork reports none; empty for some Type 3 fonts.
    pub family_name: Option<String>,
    /// 100 to 900.
    pub weight: i32,
    pub italic: bool,
    /// One per program stream in the order found, plus one per kind for
    /// fonts without a program.
    pub programs: Vec<DocumentFontProgram>,
}

/// The facts about one program behind a face.
#[derive(Debug, Clone, PartialEq)]
pub struct DocumentFontProgram {
    /// The program stream's object number; 0 for a font without a program
    /// or an inline program stream.
    pub object_number: u32,
    /// `None` when the PDF only names the font.
    pub format: Option<FontProgramFormat>,
    pub embedding: Option<FontEmbedding>,
    /// The `/BaseFont` without its subset tag, as the fork reports it.
    pub postscript_name: Option<String>,
    /// What the OS/2 `fsType` grants, one of `EPDF_FONT_EMBEDDING_*`; -1
    /// when there is no OS/2 table to say.
    pub embedding_permission: i32,
    /// Whether the program loads, has glyphs and maps characters through a
    /// Unicode cmap.
    pub maps_unicode: bool,
    /// The SHA-256 of the decoded program.
    pub sha256: Option<[u8; 32]>,
    /// The SHA-256 of what a browser loads for it; `None` when a browser
    /// can't draw it.
    pub display_sha256: Option<[u8; 32]>,
    /// `None` when the program doesn't load.
    pub metrics: Option<FontLineMetrics>,
}

/// One run of text to resolve: the face it asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontResolveRun {
    pub text: String,
    /// A family name, or a registered font's.
    pub family: String,
    /// 100 to 900.
    pub weight: i32,
    pub italic: bool,
    /// The object number of the document program a write would name for
    /// the run, or 0 to resolve the family.
    pub program: i32,
}

/// The font one run would be drawn with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedFontRun {
    pub source: Option<FontSource>,
    /// The registered font, for [`FontSource::Registered`].
    pub registered_font: Option<FontId>,
    /// The program stream's object number; 0 when there is none.
    pub program_object_number: u32,
    pub display_sha256: Option<[u8; 32]>,
    /// The font's `/BaseFont`.
    pub base_font_name: Option<String>,
    /// The face the run is drawn in.
    pub family_name: Option<String>,
    pub weight: i32,
    pub italic: bool,
    /// Whether the face isn't the one the run asked for.
    pub degraded: bool,
    /// The fallback fonts for the run's style, in the order they are tried.
    pub fallbacks: Vec<FontId>,
}

/// What [`Document::resolve_fonts`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontResolution {
    /// One per run, in the order given.
    pub runs: Vec<ResolvedFontRun>,
    /// The characters (graphemes) no font has.
    pub missing_characters: Vec<String>,
}

/// An `EPDF_FONT_FACES` snapshot, closed on drop.
struct Faces<'doc> {
    handle: pdfium_sys::EPDF_FONT_FACES,
    _document: PhantomData<&'doc Document>,
}

impl Drop for Faces<'_> {
    fn drop(&mut self) {
        // SAFETY: the snapshot came from EPDFFontFaces_Load and is closed once.
        unsafe { pdfium_sys::EPDFFontFaces_Close(self.handle) };
    }
}

/// An `EPDF_FONT_RESOLUTION` snapshot, closed on drop.
struct Resolution<'doc> {
    handle: pdfium_sys::EPDF_FONT_RESOLUTION,
    _document: PhantomData<&'doc Document>,
}

impl Drop for Resolution<'_> {
    fn drop(&mut self) {
        // SAFETY: the snapshot came from EPDFDoc_ResolveFonts and is closed once.
        unsafe { pdfium_sys::EPDFFontResolution_Close(self.handle) };
    }
}

/// A UTF-16 value, NUL-terminated as `FPDF_WIDESTRING` requires.
fn wide(value: &str) -> Vec<u16> {
    let mut units: Vec<u16> = value.encode_utf16().collect();
    units.push(0);
    units
}

/// Read a 32-byte hash through a getter that reports whether it wrote one.
fn hash_of(read: impl FnOnce(*mut u8) -> i32) -> Option<[u8; 32]> {
    let mut hash = [0u8; 32];
    (read(hash.as_mut_ptr()) != 0).then_some(hash)
}

impl Document {
    /// The faces the document's fonts are, with the facts about each
    /// program, found without loading a page.
    ///
    /// `Ok(None)` when the fork returns no snapshot, which it does only for a
    /// null document.
    pub fn font_faces(&self) -> Result<Option<Vec<DocumentFontFace>>, PdfiumError> {
        // SAFETY: the handle is live for the call.
        let handle = unsafe { pdfium_sys::EPDFFontFaces_Load(self.as_raw()) };
        if handle.is_null() {
            return Ok(None);
        }
        let faces = Faces { handle, _document: PhantomData };
        let h = faces.handle;

        // SAFETY (every getter below): `h` is a live snapshot, and the
        // indices stay inside the counts it reported.
        let count = unsafe { pdfium_sys::EPDFFontFaces_Count(h) }.max(0);
        let mut result = Vec::with_capacity(count as usize);
        for face in 0..count {
            let program_count = unsafe { pdfium_sys::EPDFFontFaces_CountPrograms(h, face) }.max(0);
            let mut programs = Vec::with_capacity(program_count as usize);
            for program in 0..program_count {
                programs.push(DocumentFontProgram {
                    object_number: unsafe {
                        pdfium_sys::EPDFFontFaces_GetProgramObjNum(h, face, program)
                    } as u32,
                    format: FontProgramFormat::from_code(unsafe {
                        pdfium_sys::EPDFFontFaces_GetProgramFormat(h, face, program)
                    }),
                    embedding: FontEmbedding::from_code(unsafe {
                        pdfium_sys::EPDFFontFaces_GetProgramEmbedded(h, face, program)
                    }),
                    postscript_name: read_utf8(|buffer, length| unsafe {
                        pdfium_sys::EPDFFontFaces_GetProgramPostScriptName(
                            h, face, program, buffer, length,
                        )
                    })?,
                    embedding_permission: unsafe {
                        pdfium_sys::EPDFFontFaces_GetProgramEmbeddingPermission(h, face, program)
                    },
                    maps_unicode: unsafe {
                        pdfium_sys::EPDFFontFaces_ProgramMapsUnicode(h, face, program)
                    } != 0,
                    sha256: hash_of(|out| unsafe {
                        pdfium_sys::EPDFFontFaces_GetProgramHash(h, face, program, out)
                    }),
                    display_sha256: hash_of(|out| unsafe {
                        pdfium_sys::EPDFFontFaces_GetProgramDisplayHash(h, face, program, out)
                    }),
                    metrics: {
                        let (mut ascent, mut descent) = (0f32, 0f32);
                        let ok = unsafe {
                            pdfium_sys::EPDFFontFaces_GetProgramMetrics(
                                h,
                                face,
                                program,
                                &mut ascent,
                                &mut descent,
                            )
                        };
                        (ok != 0).then_some(FontLineMetrics { ascent, descent })
                    },
                });
            }
            result.push(DocumentFontFace {
                family_name: read_utf16(|buffer, length| unsafe {
                    pdfium_sys::EPDFFontFaces_GetFamily(h, face, buffer, length)
                })?,
                weight: unsafe { pdfium_sys::EPDFFontFaces_GetWeight(h, face) },
                italic: unsafe { pdfium_sys::EPDFFontFaces_IsItalic(h, face) } != 0,
                programs,
            });
        }
        Ok(Some(result))
    }

    /// The SHA-256 of every fact [`Document::font_faces`] lists: equal
    /// fingerprints, the same faces. `None` for a null document.
    pub fn fonts_fingerprint(&self) -> Option<[u8; 32]> {
        // SAFETY: the handle is live and `out` is the 32 bytes written.
        hash_of(|out| unsafe { pdfium_sys::EPDFDoc_GetFontsFingerprint(self.as_raw(), out) })
    }

    /// What a browser loads for the program with this display hash. `None`
    /// when no program the document's fonts use has it.
    pub fn font_display_program_data(
        &self,
        display_sha256: &[u8; 32],
    ) -> Result<Option<Vec<u8>>, PdfiumError> {
        let hash = display_sha256.as_ptr();
        // SAFETY: the handle is live, `hash` is 32 bytes, and a null buffer
        // with zero length asks for the length only.
        let length = unsafe {
            pdfium_sys::EPDFDoc_GetFontDisplayProgramData(
                self.as_raw(),
                hash,
                std::ptr::null_mut(),
                0,
            )
        };
        if length == 0 {
            return Ok(None);
        }
        let mut buffer = zeroed_bytes(length)?;
        // SAFETY: `buffer` holds `length` bytes and lives for the call.
        let written = unsafe {
            pdfium_sys::EPDFDoc_GetFontDisplayProgramData(
                self.as_raw(),
                hash,
                buffer.as_mut_ptr().cast(),
                length,
            )
        };
        // The fork copies nothing into a short buffer.
        if written != length {
            return Ok(None);
        }
        Ok(Some(buffer))
    }

    /// The characters a browser draws with the program with this display
    /// hash. Empty when it maps none or no program has the hash.
    pub fn font_display_program_coverage(
        &self,
        display_sha256: &[u8; 32],
    ) -> Result<Vec<CodePointRange>, PdfiumError> {
        read_ranges(|out, capacity| {
            // SAFETY: the handle is live, the hash is 32 bytes, and `out`
            // holds `capacity` pairs or is null with zero.
            unsafe {
                pdfium_sys::EPDFDoc_GetFontDisplayProgramCoverage(
                    self.as_raw(),
                    display_sha256.as_ptr(),
                    out,
                    capacity,
                )
            }
        })
    }

    /// Which font each run would be drawn with, found the way a write finds
    /// it, without writing.
    ///
    /// `annotation` is the box being edited, whose own font counts first, or
    /// `None` for a new one. `Ok(None)` when the fork returns no snapshot.
    pub fn resolve_fonts(
        &self,
        annotation: Option<&Annotation<'_>>,
        runs: &[FontResolveRun],
    ) -> Result<Option<FontResolution>, PdfiumError> {
        let run_count = i32::try_from(runs.len())
            .map_err(|_| PdfiumError::InvalidArgument("too many runs to resolve"))?;
        let texts: Vec<Vec<u16>> = runs.iter().map(|run| wide(&run.text)).collect();
        let families: Vec<Vec<u16>> = runs.iter().map(|run| wide(&run.family)).collect();
        let text_table: Vec<pdfium_sys::FPDF_WIDESTRING> =
            texts.iter().map(|text| text.as_ptr()).collect();
        let family_table: Vec<pdfium_sys::FPDF_WIDESTRING> =
            families.iter().map(|family| family.as_ptr()).collect();
        let weights: Vec<i32> = runs.iter().map(|run| run.weight).collect();
        let italics: Vec<pdfium_sys::FPDF_BOOL> =
            runs.iter().map(|run| i32::from(run.italic)).collect();
        // Always passed, as the TypeScript passes it, zeros and all.
        let programs: Vec<i32> = runs.iter().map(|run| run.program).collect();
        let annot = annotation.map_or(std::ptr::null_mut(), Annotation::as_raw);

        // SAFETY: the handle is live; every table holds `run_count` entries
        // and every string it points at is NUL-terminated and outlives the
        // call; the fork copies what it keeps into the snapshot.
        let handle = unsafe {
            pdfium_sys::EPDFDoc_ResolveFonts(
                self.as_raw(),
                annot,
                run_count,
                text_table.as_ptr(),
                family_table.as_ptr(),
                weights.as_ptr(),
                italics.as_ptr(),
                programs.as_ptr(),
            )
        };
        if handle.is_null() {
            return Ok(None);
        }
        let resolution = Resolution { handle, _document: PhantomData };
        let h = resolution.handle;

        // SAFETY (every getter below): `h` is a live snapshot, and the
        // indices stay inside the counts it reported.
        let mut resolved = Vec::with_capacity(runs.len());
        for run in 0..run_count {
            let fallback_count = unsafe {
                pdfium_sys::EPDFFontResolution_GetFallbacks(h, run, std::ptr::null_mut(), 0)
            };
            let mut fallback_ids =
                vec![0u32; usize::try_from(fallback_count).map_err(|_| PdfiumError::OutOfMemory)?];
            if fallback_count > 0 {
                unsafe {
                    pdfium_sys::EPDFFontResolution_GetFallbacks(
                        h,
                        run,
                        fallback_ids.as_mut_ptr(),
                        fallback_count,
                    )
                };
            }
            resolved.push(ResolvedFontRun {
                source: FontSource::from_code(unsafe {
                    pdfium_sys::EPDFFontResolution_GetSource(h, run)
                }),
                registered_font: FontId::from_raw(unsafe {
                    pdfium_sys::EPDFFontResolution_GetRegisteredFont(h, run)
                }),
                program_object_number: unsafe {
                    pdfium_sys::EPDFFontResolution_GetProgramObjNum(h, run)
                } as u32,
                display_sha256: hash_of(|out| unsafe {
                    pdfium_sys::EPDFFontResolution_GetProgramDisplayHash(h, run, out)
                }),
                base_font_name: read_utf8(|buffer, length| unsafe {
                    pdfium_sys::EPDFFontResolution_GetBaseFontName(h, run, buffer, length)
                })?,
                family_name: read_utf16(|buffer, length| unsafe {
                    pdfium_sys::EPDFFontResolution_GetFamily(h, run, buffer, length)
                })?,
                weight: unsafe { pdfium_sys::EPDFFontResolution_GetWeight(h, run) },
                italic: unsafe { pdfium_sys::EPDFFontResolution_IsItalic(h, run) } != 0,
                degraded: unsafe { pdfium_sys::EPDFFontResolution_IsDegraded(h, run) } != 0,
                fallbacks: fallback_ids.into_iter().filter_map(FontId::from_raw).collect(),
            });
        }

        let missing_count =
            unsafe { pdfium_sys::EPDFFontResolution_CountMissingCharacters(h) }.max(0);
        let mut missing_characters = Vec::new();
        for index in 0..missing_count {
            let character = read_utf16(|buffer, length| unsafe {
                pdfium_sys::EPDFFontResolution_GetMissingCharacter(h, index, buffer, length)
            })?;
            // The TypeScript keeps only the non-empty ones.
            if let Some(character) = character.filter(|c| !c.is_empty()) {
                missing_characters.push(character);
            }
        }
        Ok(Some(FontResolution { runs: resolved, missing_characters }))
    }

    /// Forget the characters earlier appearances drew with no font: a write
    /// starts.
    pub fn clear_missing_characters(&self) {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::EPDFDoc_ClearMissingCharacters(self.as_raw()) };
    }

    /// The characters (graphemes) appearances drew with no font since the
    /// last clear, first seen first. The fork keeps at most 256.
    pub fn missing_characters(&self) -> Result<Vec<String>, PdfiumError> {
        // SAFETY: the handle is live.
        let count = unsafe { pdfium_sys::EPDFDoc_CountMissingCharacters(self.as_raw()) }.max(0);
        let mut characters = Vec::new();
        for index in 0..count {
            let character = read_utf16(|buffer, length| unsafe {
                // SAFETY: the handle is live and the index is in range.
                pdfium_sys::EPDFDoc_GetMissingCharacter(self.as_raw(), index, buffer, length)
            })?;
            // The TypeScript keeps only the non-empty ones.
            if let Some(character) = character.filter(|c| !c.is_empty()) {
                characters.push(character);
            }
        }
        Ok(characters)
    }
}

impl Annotation<'_> {
    /// The PostScript name of the font `/DA` names: the `/BaseFont` of the
    /// font dictionary `/DR` holds for it (a Type0 font's descendant's),
    /// without its subset tag, or the program's for an alias set by
    /// [`Annotation::set_default_appearance_document_font`]. `None` when `/DA`
    /// names no font dictionary or it has no `/BaseFont`.
    pub fn default_appearance_font_postscript_name(&self) -> Result<Option<String>, PdfiumError> {
        read_utf8(|buffer, length| {
            // SAFETY: the handle is live; `buffer` holds `length` bytes or is
            // null with zero.
            unsafe {
                pdfium_sys::EPDFAnnot_GetDefaultAppearanceFontPostScriptName(
                    self.as_raw(),
                    buffer,
                    length,
                )
            }
        })
    }

    /// Set `/DA` to a font program the document already holds, by its
    /// stream's object number. False for a program new text can't be written
    /// with: not TrueType or OpenType, no Unicode cmap, or a licence that
    /// doesn't allow editing.
    pub fn set_default_appearance_document_font(
        &self,
        program_object_number: u32,
        font_size: f32,
        color: (u8, u8, u8),
    ) -> bool {
        // SAFETY: the handle is live.
        unsafe {
            pdfium_sys::EPDFAnnot_SetDefaultAppearanceDocumentFont(
                self.as_raw(),
                program_object_number.into(),
                font_size,
                u32::from(color.0),
                u32::from(color.1),
                u32::from(color.2),
            ) != 0
        }
    }
}
