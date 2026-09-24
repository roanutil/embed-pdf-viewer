//! Creating an annotation and writing its dictionary.
//!
//! G11. Wraps the `*_Set*` half of `fpdf_annot.h` and the fork's creation
//! helper.
//!
//! Everything here WRITES to the open document, which is what separates it
//! from [`crate::annot`]. It is a separate module for that reason alone: a
//! reader reaching for one of these by accident is a bug that a file boundary
//! makes visible.
//!
//! # Creation is through the fork's helper on purpose
//!
//! `EPDFPage_CreateAnnot` creates an INDIRECT object, so a new annotation is
//! born with an object number and therefore with a durable identity. PDFium's
//! own `FPDFPage_CreateAnnot` may produce a direct dictionary, which would be
//! an annotation nobody can address across a reordering. The engine checks
//! the object number afterwards rather than trusting either.

use std::ffi::CString;

use crate::{Annotation, PageView, PdfiumError, QuadF, RectF};

fn c_key(key: &str) -> Result<CString, PdfiumError> {
    CString::new(key).map_err(|_| PdfiumError::InvalidArgument("annotation key contains NUL"))
}

/// A UTF-16 value, NUL-terminated as `FPDF_WIDESTRING` requires.
fn wide(value: &str) -> Vec<u16> {
    let mut units: Vec<u16> = value.encode_utf16().collect();
    units.push(0);
    units
}

impl PageView<'_> {
    /// Create an annotation of `subtype_code` and append it to `/Annots`.
    ///
    /// Appends, always. PDFium has no insert-at-position helper, which is
    /// what makes a create non-index-shifting and therefore
    /// non-invalidating: the new annotation lands past everything that was
    /// already there.
    pub fn create_annotation(&self, subtype_code: i32) -> Option<Annotation<'_>> {
        // SAFETY: the page handle is live for the borrow this returns.
        let handle = unsafe { pdfium_sys::EPDFPage_CreateAnnot(self.as_raw(), subtype_code) };
        // SAFETY: a non-null result is an annotation this page now owns.
        unsafe { Annotation::from_raw_owned(handle) }
    }

    /// The index of an annotation within this page's `/Annots`.
    ///
    /// Negative when PDFium cannot find it, which for an annotation this
    /// page just created would mean the create did not attach it.
    pub fn annotation_index(&self, annotation: &Annotation<'_>) -> i32 {
        // SAFETY: both handles are live.
        unsafe { pdfium_sys::FPDFPage_GetAnnotIndex(self.as_raw(), annotation.as_raw()) }
    }

    /// The annotation with this PDF object number, or `None`.
    ///
    /// A probe, and the reason deletes have one: removing by a reference
    /// that names nothing should say so rather than return a bare false
    /// that the caller has to guess the meaning of.
    pub fn annotation_by_object_number(&self, object_number: u32) -> Option<Annotation<'_>> {
        // SAFETY: the page handle is live for the borrow this returns.
        let handle =
            unsafe { pdfium_sys::EPDFPage_GetAnnotByObjectNumber(self.as_raw(), object_number) };
        // SAFETY: a non-null result is a handle this caller now owns.
        unsafe { Annotation::from_raw_owned(handle) }
    }

    /// The annotation with this `/NM`, or `None`.
    pub fn annotation_by_name(&self, nm: &str) -> Option<Annotation<'_>> {
        let nm = wide(nm);
        // SAFETY: the page handle is live and the name outlives the call.
        let handle = unsafe { pdfium_sys::EPDFPage_GetAnnotByName(self.as_raw(), nm.as_ptr()) };
        // SAFETY: as above.
        unsafe { Annotation::from_raw_owned(handle) }
    }

    /// Remove the annotation with this object number.
    pub fn remove_annotation_by_object_number(&self, object_number: u32) -> bool {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::EPDFPage_RemoveAnnotByObjectNumber(self.as_raw(), object_number) != 0 }
    }

    /// Remove the annotation with this `/NM`.
    pub fn remove_annotation_by_name(&self, nm: &str) -> bool {
        let nm = wide(nm);
        // SAFETY: the handle is live and the name outlives the call.
        unsafe { pdfium_sys::EPDFPage_RemoveAnnotByName(self.as_raw(), nm.as_ptr()) != 0 }
    }

    /// Remove the annotation at this index in `/Annots`.
    ///
    /// The FORK's helper, which also frees the indirect object when the
    /// annotation has one. PDFium's own `FPDFPage_RemoveAnnot` unlinks the
    /// entry and leaves the object behind, which grows the file by the size
    /// of everything anyone ever deleted.
    pub fn remove_annotation_at(&self, index: u32) -> bool {
        let Ok(index) = i32::try_from(index) else {
            return false;
        };
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::EPDFPage_RemoveAnnot(self.as_raw(), index) != 0 }
    }

    /// Move a set of annotations, as one contiguous block, to `to_index`.
    ///
    /// `from_indices` are positions in the CURRENT `/Annots`, and
    /// `to_index` is a position in the POST-REMOVAL one: the block is
    /// detached first and inserted afterwards, so the two index spaces are
    /// not the same and a caller that range-checked against the current
    /// count would be off by the size of its own block.
    ///
    /// Atomic. A false return means the helper rejected the request and
    /// changed nothing, which is what lets a caller undo its bookkeeping
    /// rather than having to work out how far it got.
    pub fn move_annotations(&self, from_indices: &[i32], to_index: u32) -> bool {
        let Ok(to_index) = i32::try_from(to_index) else {
            return false;
        };
        let Ok(len) = i32::try_from(from_indices.len()) else {
            return false;
        };
        // SAFETY: the handle is live and the slice outlives the call.
        unsafe {
            pdfium_sys::EPDFPage_MoveAnnots(self.as_raw(), from_indices.as_ptr(), len, to_index)
                != 0
        }
    }

    /// Rebuild the page's content stream.
    ///
    /// Called after an appearance is generated. Without it the new stream is
    /// in the document and the page does not reference it, so the annotation
    /// is there and invisible.
    pub fn generate_content(&self) -> bool {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::FPDFPage_GenerateContent(self.as_raw()) != 0 }
    }
}

impl Annotation<'_> {
    /// `/Rect`.
    pub fn set_rect(&self, rect: RectF) -> bool {
        let rect = pdfium_sys::FS_RECTF {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        };
        // SAFETY: the handle is live and `rect` outlives the call.
        unsafe { pdfium_sys::EPDFAnnot_SetRect(self.as_raw(), &raw const rect) != 0 }
    }

    /// One colour entry, by `FPDFANNOT_COLORTYPE` selector.
    pub fn set_color(&self, color_type: i32, r: u8, g: u8, b: u8) -> bool {
        // SAFETY: the handle is live.
        unsafe {
            pdfium_sys::EPDFAnnot_SetColor(
                self.as_raw(),
                color_type as u32,
                u32::from(r),
                u32::from(g),
                u32::from(b),
            ) != 0
        }
    }

    /// `/CA`, from a 0..=1 opacity.
    ///
    /// The rounding is here rather than at the call site because the native
    /// entry point takes a byte and every caller would otherwise repeat it,
    /// which is how two callers end up rounding differently.
    pub fn set_opacity(&self, opacity: f64) -> bool {
        let alpha = (opacity * 255.0).round().clamp(0.0, 255.0) as u32;
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::EPDFAnnot_SetOpacity(self.as_raw(), alpha) != 0 }
    }

    /// The raw `/F` word.
    pub fn set_flags(&self, flags: u32) -> bool {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::FPDFAnnot_SetFlags(self.as_raw(), flags as i32) != 0 }
    }

    /// Write a UTF-16 string entry.
    ///
    /// Setting a value is not how an entry is REMOVED: a null value here
    /// writes an empty string. [`Annotation::remove_key`] is removal, and the
    /// difference is what keeps a cleared field reading back as absent rather
    /// than as empty.
    pub fn set_string_value(&self, key: &str, value: &str) -> Result<bool, PdfiumError> {
        let key = c_key(key)?;
        let value = wide(value);
        // SAFETY: the handle is live and both strings outlive the call.
        Ok(unsafe {
            pdfium_sys::FPDFAnnot_SetStringValue(self.as_raw(), key.as_ptr(), value.as_ptr()) != 0
        })
    }

    /// Remove an entry from the dictionary.
    pub fn remove_key(&self, key: &str) -> Result<bool, PdfiumError> {
        let key = c_key(key)?;
        // SAFETY: the handle is live and the key outlives the call.
        Ok(unsafe { pdfium_sys::EPDFAnnot_RemoveKey(self.as_raw(), key.as_ptr()) != 0 })
    }

    /// `/IT`.
    pub fn set_intent(&self, intent: &str) -> Result<bool, PdfiumError> {
        let intent = c_key(intent)?;
        // SAFETY: the handle is live and the name outlives the call.
        Ok(unsafe { pdfium_sys::EPDFAnnot_SetIntent(self.as_raw(), intent.as_ptr()) != 0 })
    }

    /// `/DA`, built from one of the fourteen standard fonts.
    ///
    /// The colour here is the `/DA` colour, which a free text uses for its
    /// border and its default text and a redaction for its overlay label. It
    /// is not `/C`.
    pub fn set_default_appearance(
        &self,
        font_code: i32,
        font_size: f32,
        color: (u8, u8, u8),
    ) -> bool {
        // SAFETY: the handle is live.
        unsafe {
            pdfium_sys::EPDFAnnot_SetDefaultAppearance(
                self.as_raw(),
                font_code,
                font_size,
                u32::from(color.0),
                u32::from(color.1),
                u32::from(color.2),
            ) != 0
        }
    }

    /// The same from a font registered on this thread.
    ///
    /// The id is a per-thread counter, which is why it is a [`crate::FontId`]
    /// rather than a number: it names a different font on another thread and
    /// nothing at all on one that has registered nothing.
    pub fn set_default_appearance_registered_font(
        &self,
        font: crate::FontId,
        font_size: f32,
        color: (u8, u8, u8),
    ) -> bool {
        // SAFETY: the handle is live.
        unsafe {
            pdfium_sys::EPDFAnnot_SetDefaultAppearanceRegisteredFont(
                self.as_raw(),
                font.raw(),
                font_size,
                u32::from(color.0),
                u32::from(color.1),
                u32::from(color.2),
            ) != 0
        }
    }

    /// `/Q`, as an `FPDF_TEXT_ALIGNMENT` code.
    pub fn set_text_alignment(&self, alignment: i32) -> bool {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::EPDFAnnot_SetTextAlignment(self.as_raw(), alignment as u32) != 0 }
    }

    /// `/CL`, a free-text callout's leader line.
    ///
    /// Two points draw a straight leader and three a knee-jointed one. The
    /// count is passed through rather than checked: which counts are legal is
    /// §12.5.6.6's business, and the engine's.
    pub fn set_callout_line(&self, points: &[crate::PointF]) -> bool {
        let native: Vec<pdfium_sys::FS_POINTF> =
            points.iter().map(|p| pdfium_sys::FS_POINTF { x: p.x, y: p.y }).collect();
        // SAFETY: the handle is live and the buffer outlives the call.
        unsafe {
            pdfium_sys::EPDFAnnot_SetCalloutLine(
                self.as_raw(),
                native.as_ptr(),
                native.len() as std::os::raw::c_ulong,
            ) != 0
        }
    }

    /// `/Name`, as an `FPDF_ANNOT_NAME` code.
    ///
    /// Which block of that enum a code belongs to depends on the subtype,
    /// exactly as on the read side: this layer passes the integer through and
    /// the engine's vocabulary decides what it means.
    pub fn set_name(&self, name: i32) -> bool {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::EPDFAnnot_SetName(self.as_raw(), name) != 0 }
    }

    /// Append one quad to `/QuadPoints`.
    pub fn append_quad_point(&self, quad: QuadF) -> bool {
        let quad = pdfium_sys::FS_QUADPOINTSF {
            x1: quad.x1,
            y1: quad.y1,
            x2: quad.x2,
            y2: quad.y2,
            x3: quad.x3,
            y3: quad.y3,
            x4: quad.x4,
            y4: quad.y4,
        };
        // SAFETY: the handle is live and `quad` outlives the call.
        unsafe { pdfium_sys::FPDFAnnot_AppendAttachmentPoints(self.as_raw(), &raw const quad) != 0 }
    }

    /// Overwrite the quad at `index`.
    ///
    /// PDFium can grow `/QuadPoints` by appending and cannot shrink it, so a
    /// caller replacing a list has to refuse a shorter one rather than
    /// leaving stale quads behind.
    pub fn set_quad_point(&self, index: usize, quad: QuadF) -> bool {
        let quad = pdfium_sys::FS_QUADPOINTSF {
            x1: quad.x1,
            y1: quad.y1,
            x2: quad.x2,
            y2: quad.y2,
            x3: quad.x3,
            y3: quad.y3,
            x4: quad.x4,
            y4: quad.y4,
        };
        // SAFETY: as above.
        unsafe {
            pdfium_sys::FPDFAnnot_SetAttachmentPoints(self.as_raw(), index, &raw const quad) != 0
        }
    }

    /// `/BS`: the style code and the width.
    pub fn set_border_style(&self, style_code: i32, width: f32) -> bool {
        // SAFETY: the handle is live.
        unsafe {
            pdfium_sys::EPDFAnnot_SetBorderStyle(self.as_raw(), style_code as u32, width) != 0
        }
    }

    /// `/BS /D`, the dash pattern.
    pub fn set_border_dash_pattern(&self, dashes: &[f32]) -> bool {
        // SAFETY: the handle is live and the slice outlives the call.
        unsafe {
            pdfium_sys::EPDFAnnot_SetBorderDashPattern(
                self.as_raw(),
                dashes.as_ptr(),
                dashes.len() as std::os::raw::c_ulong,
            ) != 0
        }
    }

    /// `/BE /I`, the cloudy border intensity.
    pub fn set_border_effect(&self, intensity: f32) -> bool {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::EPDFAnnot_SetBorderEffect(self.as_raw(), intensity) != 0 }
    }

    /// Remove `/BE`.
    pub fn clear_border_effect(&self) -> bool {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::EPDFAnnot_ClearBorderEffect(self.as_raw()) != 0 }
    }

    /// `/RD`, in the `[left, bottom, right, top]` order PDFium takes.
    pub fn set_rectangle_differences(&self, left: f32, bottom: f32, right: f32, top: f32) -> bool {
        // SAFETY: the handle is live.
        unsafe {
            pdfium_sys::EPDFAnnot_SetRectangleDifferences(self.as_raw(), left, bottom, right, top)
                != 0
        }
    }

    /// Remove `/RD`.
    pub fn clear_rectangle_differences(&self) -> bool {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::EPDFAnnot_ClearRectangleDifferences(self.as_raw()) != 0 }
    }

    /// Remove one colour entry.
    pub fn clear_color(&self, color_type: i32) -> bool {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::EPDFAnnot_ClearColor(self.as_raw(), color_type as u32) != 0 }
    }

    /// `/Vertices`, the point list of a polygon or polyline.
    pub fn set_vertices(&self, points: &[crate::PointF]) -> bool {
        let native: Vec<pdfium_sys::FS_POINTF> =
            points.iter().map(|p| pdfium_sys::FS_POINTF { x: p.x, y: p.y }).collect();
        // SAFETY: the handle is live and the buffer outlives the call.
        unsafe {
            pdfium_sys::EPDFAnnot_SetVertices(
                self.as_raw(),
                native.as_ptr(),
                native.len() as std::os::raw::c_ulong,
            ) != 0
        }
    }

    /// `/L`, the two endpoints of a line.
    pub fn set_line(&self, start: crate::PointF, end: crate::PointF) -> bool {
        let start = pdfium_sys::FS_POINTF { x: start.x, y: start.y };
        let end = pdfium_sys::FS_POINTF { x: end.x, y: end.y };
        // SAFETY: the handle is live and both points outlive the call.
        unsafe {
            pdfium_sys::EPDFAnnot_SetLine(self.as_raw(), &raw const start, &raw const end) != 0
        }
    }

    /// `/LE`, as the two raw `FPDF_ANNOT_LINE_END` codes.
    pub fn set_line_endings(&self, start: i32, end: i32) -> bool {
        // SAFETY: the handle is live.
        unsafe {
            pdfium_sys::EPDFAnnot_SetLineEndings(self.as_raw(), start as u32, end as u32) != 0
        }
    }

    /// Append one pen stroke to `/InkList`.
    ///
    /// Returns the stroke's index, or a negative value when PDFium refused.
    /// There is no way to replace a stroke, so a writer changing an ink list
    /// removes the whole thing first.
    pub fn add_ink_stroke(&self, points: &[crate::PointF]) -> i32 {
        let native: Vec<pdfium_sys::FS_POINTF> =
            points.iter().map(|p| pdfium_sys::FS_POINTF { x: p.x, y: p.y }).collect();
        // SAFETY: the handle is live and the buffer outlives the call.
        unsafe { pdfium_sys::FPDFAnnot_AddInkStroke(self.as_raw(), native.as_ptr(), native.len()) }
    }

    /// Remove `/InkList` entirely.
    pub fn remove_ink_list(&self) -> bool {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::FPDFAnnot_RemoveInkList(self.as_raw()) != 0 }
    }

    /// One `/EMBD_Metadata` number entry.
    pub fn set_embed_metadata_number(&self, key: &str, value: f32) -> Result<bool, PdfiumError> {
        let key = c_key(key)?;
        // SAFETY: the handle and key are live.
        Ok(unsafe {
            pdfium_sys::EPDFAnnot_SetEmbedMetadataNumber(self.as_raw(), key.as_ptr(), value) != 0
        })
    }

    /// One `/EMBD_Metadata` rect entry, in the `FS_RECTF` field order.
    pub fn set_embed_metadata_rect(
        &self,
        key: &str,
        rect: crate::RectF,
    ) -> Result<bool, PdfiumError> {
        let key = c_key(key)?;
        let native = pdfium_sys::FS_RECTF {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        };
        // SAFETY: the handle and key are live, and `native` outlives the call.
        Ok(unsafe {
            pdfium_sys::EPDFAnnot_SetEmbedMetadataRect(
                self.as_raw(),
                key.as_ptr(),
                &raw const native,
            ) != 0
        })
    }

    /// Remove one `/EMBD_Metadata` entry.
    pub fn clear_embed_metadata_key(&self, key: &str) -> Result<bool, PdfiumError> {
        let key = c_key(key)?;
        // SAFETY: as above.
        Ok(unsafe { pdfium_sys::EPDFAnnot_ClearEmbedMetadataKey(self.as_raw(), key.as_ptr()) != 0 })
    }

    /// Bake the `/AP /N` appearance stream from the dictionary.
    ///
    /// `blend` is a `FPDF_BLENDMODE`; `None` uses PDFium's default entry
    /// point, which is not the same as passing normal: the two take different
    /// code paths in the fork and the engine keeps the distinction.
    pub fn generate_appearance(&self, blend: Option<i32>) -> bool {
        // SAFETY: the handle is live.
        unsafe {
            match blend {
                None => pdfium_sys::EPDFAnnot_GenerateAppearance(self.as_raw()) != 0,
                Some(blend) => {
                    pdfium_sys::EPDFAnnot_GenerateAppearanceWithBlend(self.as_raw(), blend as u32)
                        != 0
                }
            }
        }
    }
}
