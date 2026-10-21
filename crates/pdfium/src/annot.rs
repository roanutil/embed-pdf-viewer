//! Annotation handles and their accessors.
//!
//! G7. Wraps `FPDFAnnot_*` and the fork's `EPDFAnnot_*` extensions. This
//! layer decides nothing about what an annotation means: it owns the handle,
//! checks the lengths and hands back plain data.
//!
//! # Three ABIs, not one
//!
//! The getters here do not agree on how a buffer is measured, and the
//! difference is silent. `FPDFAnnot_GetStringValue` and
//! `EPDFAnnot_GetOverlayText` report a BYTE length including the terminator,
//! which is PDFium's house convention. `EPDFAnnot_GetIntent` reports a count
//! of UTF-16 CODE UNITS excluding the terminator, and then takes its `buflen`
//! in bytes anyway. The point-list getters report an ELEMENT count and take a
//! capacity in elements. Every one of those is handled at its own call site
//! rather than through one helper, because a helper would have to be told
//! which convention it was serving and that is the thing that gets forgotten.
//!
//! # The enum returns are unsigned and this layer hands back `i32`
//!
//! The fork typedefs `FPDF_BLENDMODE` and its siblings to `unsigned int`, and
//! the engine's decoders take `i32` because their vectors sweep past zero to
//! catch a cast that wraps a negative into a table's tail. The cast here is
//! therefore narrowing in principle. In practice every one of these enums is
//! under a hundred values, and a value that did overflow would land on some
//! negative, which every decoder already answers with its default. Nothing is
//! lost that was not already unreadable.

use std::ffi::CString;
use std::marker::PhantomData;

use crate::{Document, MatrixF, PageView, PdfiumError, PointF, QuadF, RectF};

/// Which appearance stream an accessor is asking about.
///
/// `FPDF_ANNOT_APPEARANCEMODE`. Normal is the one that renders; the other two
/// are the rollover and down states of a widget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppearanceMode {
    Normal = 0,
    Rollover = 1,
    Down = 2,
}

/// The `/DA` default appearance of a free-text annotation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DefaultAppearance {
    /// The raw `FPDF_STANDARD_FONT` code. Decoded by
    /// `epdf_engine::features::annotation::vocabulary::StandardFont`.
    pub font_code: i32,
    pub font_size: f32,
    pub color: (u8, u8, u8),
}

/// The effective border, after PDFium has resolved `/BS`, the legacy
/// `/Border` array and the ISO defaults.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BorderStyle {
    /// The raw `FPDF_ANNOT_BORDER_STYLE` code.
    pub style_code: i32,
    pub width: f32,
}

/// The `/RD` margins between `/Rect` and the geometry drawn inside it, in the
/// `[left, bottom, right, top]` order PDFium reports them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RectangleDifferences {
    pub left: f32,
    pub bottom: f32,
    pub right: f32,
    pub top: f32,
}

/// One annotation of a page, open for as long as this value lives.
///
/// Closing is `FPDFPage_CloseAnnot`, which releases the handle without
/// touching the document, so dropping one is always safe and never a write.
/// The lifetime ties it to the page it came from, which is what stops a
/// handle outliving the page whose annotation list owns its storage.
#[derive(Debug)]
pub struct Annotation<'page> {
    handle: pdfium_sys::FPDF_ANNOTATION,
    _page: PhantomData<&'page ()>,
    _not_send: PhantomData<*const ()>,
}

impl PageView<'_> {
    /// Opens the annotation at `index` in the page's `/Annots` order.
    ///
    /// `None` for an index past the end or an entry PDFium refuses, which a
    /// malformed `/Annots` can contain: an entry that is not a dictionary is
    /// a gap in the list rather than a failure of the list.
    pub fn annotation(&self, index: u32) -> Option<Annotation<'_>> {
        let index = i32::try_from(index).ok()?;
        // SAFETY: the page handle is live for the borrow.
        // SAFETY: the raw handle is used only for this call and not closed.
        let handle = unsafe { pdfium_sys::FPDFPage_GetAnnot(self.as_raw(), index) };
        if handle.is_null() {
            return None;
        }
        Some(Annotation { handle, _page: PhantomData, _not_send: PhantomData })
    }
}

impl Document {
    /// How many annotations a page's `/Annots` holds, read off the document
    /// without loading the page.
    ///
    /// The cheap path, and the one the raw reader uses. Loading a page parses
    /// its content stream, which a listing of what is on it does not need.
    ///
    /// A negative count is PDFium refusing the page rather than an empty
    /// page, so it is `None` and not zero: the two answers mean different
    /// things to a caller deciding whether to show an annotation layer.
    pub fn raw_annotation_count(&self, page_index: u32) -> Option<u32> {
        let page_index = i32::try_from(page_index).ok()?;
        // SAFETY: the handle is live and is not closed here.
        let count = unsafe { pdfium_sys::EPDFPage_GetAnnotCountRaw(self.as_raw(), page_index) };
        u32::try_from(count).ok()
    }

    /// One annotation off the document, without loading its page.
    ///
    /// The returned handle borrows the DOCUMENT, not a page, which is what
    /// makes the raw path possible: there is no page for it to outlive.
    pub fn raw_annotation(&self, page_index: u32, index: u32) -> Option<Annotation<'_>> {
        let page_index = i32::try_from(page_index).ok()?;
        let index = i32::try_from(index).ok()?;
        // SAFETY: the handle is live for the borrow this returns.
        let handle = unsafe { pdfium_sys::EPDFPage_GetAnnotRaw(self.as_raw(), page_index, index) };
        if handle.is_null() {
            return None;
        }
        Some(Annotation { handle, _page: PhantomData, _not_send: PhantomData })
    }
}

impl<'page> Annotation<'page> {
    /// Wrap a handle this crate just created.
    ///
    /// # Safety
    /// `handle` must be an `FPDF_ANNOTATION` the caller owns and has not
    /// closed, and `'page` must not outlive the page it belongs to. The
    /// lifetime is unbounded here, so the caller chooses it.
    pub(crate) unsafe fn from_raw_owned(
        handle: pdfium_sys::FPDF_ANNOTATION,
    ) -> Option<Annotation<'page>> {
        (!handle.is_null()).then_some(Annotation {
            handle,
            _page: PhantomData,
            _not_send: PhantomData,
        })
    }
}

impl Drop for Annotation<'_> {
    fn drop(&mut self) {
        // SAFETY: the handle came from FPDFPage_GetAnnot and is closed once.
        unsafe { pdfium_sys::FPDFPage_CloseAnnot(self.handle) };
    }
}

impl Annotation<'_> {
    /// The raw `FPDF_ANNOTATION_SUBTYPE`.
    pub fn subtype_code(&self) -> i32 {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::FPDFAnnot_GetSubtype(self.handle) }
    }

    /// The raw `/F` word.
    ///
    /// PDFium answers `-1` when it cannot read the annotation at all, which
    /// widens to every bit set. That is deliberate and matches the
    /// TypeScript: an unreadable annotation reads as invisible and hidden,
    /// which is the safe direction for something a viewer might otherwise
    /// draw on top of a page.
    pub fn flags(&self) -> u32 {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::FPDFAnnot_GetFlags(self.handle) as u32 }
    }

    /// The annotation's PDF indirect object number, which is its identity
    /// across a reordering of `/Annots`.
    ///
    /// Zero means the file put a direct dictionary in `/Annots`. That is legal
    /// PDF and it means the annotation has no durable identity, so the engine
    /// has to synthesize one rather than treat zero as an object number.
    pub fn object_number(&self) -> u32 {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::EPDFAnnot_GetObjectNumber(self.handle) }
    }

    /// Whether the annotation dictionary has `key` at all.
    ///
    /// The distinction this preserves is between absent and empty: a `/T ()`
    /// is a title somebody set to nothing, and a missing `/T` is a title
    /// nobody set. A patch has to be able to tell them apart.
    pub fn has_key(&self, key: &str) -> Result<bool, PdfiumError> {
        let key = c_key(key)?;
        // SAFETY: the handle is live and the key outlives the call.
        Ok(unsafe { pdfium_sys::FPDFAnnot_HasKey(self.handle, key.as_ptr()) } != 0)
    }

    /// A UTF-16 string entry.
    ///
    /// `None` when the key is absent, `Some("")` when it is present and
    /// empty. The presence probe is separate from the read because the byte
    /// ABI cannot distinguish the two on its own: both answer two bytes.
    pub fn string_value(&self, key: &str) -> Result<Option<String>, PdfiumError> {
        if !self.has_key(key)? {
            return Ok(None);
        }
        let key = c_key(key)?;
        let text = read_utf16_bytes(|buffer, len| {
            // SAFETY: the handle and key are live, and `len` describes
            // `buffer` in bytes.
            unsafe { pdfium_sys::FPDFAnnot_GetStringValue(self.handle, key.as_ptr(), buffer, len) }
        })?;
        Ok(Some(text.unwrap_or_default()))
    }

    /// `/Rect`, un-normalized, in whatever corner order the file used.
    ///
    /// A rect PDFium refuses reads as four zeros rather than an error. An
    /// annotation with no usable rectangle still has a subtype, a title and
    /// possibly a reply thread, and dropping all of that to report a missing
    /// box would lose more than it says.
    pub fn rect(&self) -> RectF {
        let mut rect = pdfium_sys::FS_RECTF::default();
        // SAFETY: the handle is live and `rect` is a valid out-parameter.
        let ok = unsafe { pdfium_sys::FPDFAnnot_GetRect(self.handle, &raw mut rect) };
        if ok == 0 { RectF::default() } else { rect.into() }
    }

    /// One color entry, by `FPDFANNOT_COLORTYPE` selector.
    ///
    /// RGB only. Annotation transparency is `/CA` and is read through
    /// [`Annotation::opacity`], so there is one place it can come from.
    pub fn color(&self, color_type: i32) -> Option<(u8, u8, u8)> {
        let color_type = color_type as u32;
        let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
        // SAFETY: the handle is live and the three out-parameters are valid.
        let ok = unsafe {
            pdfium_sys::EPDFAnnot_GetColor(
                self.handle,
                color_type,
                &raw mut r,
                &raw mut g,
                &raw mut b,
            )
        };
        (ok != 0).then_some((r as u8, g as u8, b as u8))
    }

    /// `/CA`, as 0..=1.
    ///
    /// The fork reads `/CA` as stored, clamped to 0..=1, and 1 when absent:
    /// the same number `EPDFAnnot_GenerateAppearance` composites with, which
    /// a raw `/CA` read is not. It used to hand back a byte; it is an `f32`
    /// now, widened here the way the TypeScript's `readF32` widens it, so
    /// both sides produce the same JSON number.
    pub fn opacity(&self) -> Option<f64> {
        let mut opacity = 0f32;
        // SAFETY: the handle is live and `opacity` is a valid out-parameter.
        let ok = unsafe { pdfium_sys::EPDFAnnot_GetOpacity(self.handle, &raw mut opacity) };
        (ok != 0).then(|| f64::from(opacity))
    }

    /// The effective border style and width.
    pub fn border_style(&self) -> BorderStyle {
        let mut width = 0f32;
        // SAFETY: the handle is live and `width` is a valid out-parameter.
        let style_code =
            unsafe { pdfium_sys::EPDFAnnot_GetBorderStyle(self.handle, &raw mut width) };
        BorderStyle { style_code: style_code as i32, width }
    }

    /// The dash pattern of a dashed border, resolving `/BS /D` and then the
    /// legacy `/Border` array's fourth element.
    ///
    /// Empty when the effective border is not dashed or names no pattern.
    pub fn border_dash_pattern(&self) -> Result<Vec<f32>, PdfiumError> {
        // SAFETY: the handle is live.
        let count = unsafe { pdfium_sys::EPDFAnnot_GetBorderDashPatternCount(self.handle) };
        if count == 0 {
            return Ok(Vec::new());
        }
        let mut pattern = alloc_f32(count as usize)?;
        // SAFETY: `pattern` holds `count` floats, which is what the length
        // argument claims.
        let ok = unsafe {
            pdfium_sys::EPDFAnnot_GetBorderDashPattern(self.handle, pattern.as_mut_ptr(), count)
        };
        if ok == 0 {
            return Ok(Vec::new());
        }
        Ok(pattern)
    }

    /// `/BE /I`, the cloudy border intensity. `None` when there is no border
    /// effect.
    ///
    /// A `/BE` present with intensity zero comes back as `Some(0.0)` here.
    /// Normalizing that away is the engine's call, not this layer's: zero is
    /// what the file says.
    pub fn border_effect(&self) -> Option<f32> {
        let mut intensity = 0f32;
        // SAFETY: the handle is live and `intensity` is a valid out-parameter.
        let ok = unsafe { pdfium_sys::EPDFAnnot_GetBorderEffect(self.handle, &raw mut intensity) };
        (ok != 0).then_some(intensity)
    }

    /// `/RD`. `None` when the annotation has no entry.
    pub fn rectangle_differences(&self) -> Option<RectangleDifferences> {
        let (mut left, mut bottom, mut right, mut top) = (0f32, 0f32, 0f32, 0f32);
        // SAFETY: the handle is live and the four out-parameters are valid.
        let ok = unsafe {
            pdfium_sys::EPDFAnnot_GetRectangleDifferences(
                self.handle,
                &raw mut left,
                &raw mut bottom,
                &raw mut right,
                &raw mut top,
            )
        };
        (ok != 0).then_some(RectangleDifferences { left, bottom, right, top })
    }

    /// `/Vertices`, the point list of a polygon or polyline.
    pub fn vertices(&self) -> Result<Vec<PointF>, PdfiumError> {
        // SAFETY: a null buffer with zero capacity is the documented probe.
        let count =
            unsafe { pdfium_sys::FPDFAnnot_GetVertices(self.handle, std::ptr::null_mut(), 0) };
        read_points(count, |buffer, capacity| {
            // SAFETY: `buffer` holds `capacity` points.
            unsafe { pdfium_sys::FPDFAnnot_GetVertices(self.handle, buffer, capacity) }
        })
    }

    /// `/InkList`, one point list per pen stroke.
    ///
    /// Empty strokes are dropped rather than kept as empty arrays. A stroke
    /// with no points draws nothing and a consumer iterating strokes would
    /// have to skip it anyway.
    pub fn ink_list(&self) -> Result<Vec<Vec<PointF>>, PdfiumError> {
        // SAFETY: the handle is live.
        let paths = unsafe { pdfium_sys::FPDFAnnot_GetInkListCount(self.handle) };
        let mut out = Vec::new();
        for path in 0..paths {
            // SAFETY: a null buffer with zero capacity is the documented probe.
            let count = unsafe {
                pdfium_sys::FPDFAnnot_GetInkListPath(self.handle, path, std::ptr::null_mut(), 0)
            };
            let stroke = read_points(count, |buffer, capacity| {
                // SAFETY: `buffer` holds `capacity` points.
                unsafe { pdfium_sys::FPDFAnnot_GetInkListPath(self.handle, path, buffer, capacity) }
            })?;
            if !stroke.is_empty() {
                out.push(stroke);
            }
        }
        Ok(out)
    }

    /// `/L`, the two endpoints of a line annotation.
    pub fn line(&self) -> Option<(PointF, PointF)> {
        let mut start = pdfium_sys::FS_POINTF::default();
        let mut end = pdfium_sys::FS_POINTF::default();
        // SAFETY: the handle is live and both out-parameters are valid.
        let ok =
            unsafe { pdfium_sys::FPDFAnnot_GetLine(self.handle, &raw mut start, &raw mut end) };
        (ok != 0).then(|| (start.into(), end.into()))
    }

    /// `/LE`, as the two raw `FPDF_ANNOT_LINE_END` codes. `None` when the
    /// annotation has no entry.
    pub fn line_endings(&self) -> Option<(i32, i32)> {
        let (mut start, mut end) = (0u32, 0u32);
        // SAFETY: the handle is live and both out-parameters are valid.
        let ok = unsafe {
            pdfium_sys::EPDFAnnot_GetLineEndings(self.handle, &raw mut start, &raw mut end)
        };
        (ok != 0).then_some((start as i32, end as i32))
    }

    /// `/DA`, the free-text default appearance.
    pub fn default_appearance(&self) -> Option<DefaultAppearance> {
        let mut font_code = 0i32;
        let mut font_size = 0f32;
        let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
        // SAFETY: the handle is live and the five out-parameters are valid.
        let ok = unsafe {
            pdfium_sys::EPDFAnnot_GetDefaultAppearance(
                self.handle,
                &raw mut font_code,
                &raw mut font_size,
                &raw mut r,
                &raw mut g,
                &raw mut b,
            )
        };
        (ok != 0).then_some(DefaultAppearance {
            font_code,
            font_size,
            color: (r as u8, g as u8, b as u8),
        })
    }

    /// The rich text of a free text (`/RC` over `/DS` and `/DA`) or a rich
    /// text field, as the fork's UTF-8 JSON. When there is no `/RC` the fork
    /// synthesises the same shape from `/Contents`, so a free text always has
    /// one. `None` when the fork reports an error.
    pub fn rich_text_json(&self) -> Result<Option<String>, PdfiumError> {
        read_utf8(|buffer, len| {
            // SAFETY: the handle is live, and `len` describes `buffer` in bytes.
            unsafe { pdfium_sys::EPDFAnnot_GetRichTextJSON(self.handle, buffer.cast(), len) }
        })
    }

    /// `/Q`, the raw quadding code. An absent `/Q` answers zero, which is
    /// left, and is indistinguishable from an explicit zero. That is the
    /// PDF's own doing: zero is the default.
    pub fn text_alignment(&self) -> i32 {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::EPDFAnnot_GetTextAlignment(self.handle) as i32 }
    }

    /// `/OverlayText`, a redaction's label. `None` when absent, `Some("")`
    /// when present and empty.
    pub fn overlay_text(&self) -> Result<Option<String>, PdfiumError> {
        if !self.has_key("OverlayText")? {
            return Ok(None);
        }
        let text = read_utf16_bytes(|buffer, len| {
            // SAFETY: the handle is live and `len` describes `buffer` in bytes.
            unsafe { pdfium_sys::EPDFAnnot_GetOverlayText(self.handle, buffer, len) }
        })?;
        Ok(Some(text.unwrap_or_default()))
    }

    /// `/Repeat`. An absent key is `false`, the PDF default.
    pub fn overlay_text_repeat(&self) -> bool {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::EPDFAnnot_GetOverlayTextRepeat(self.handle) != 0 }
    }

    /// `/IT`, the intent name.
    ///
    /// The one getter here whose measure call counts UTF-16 code units rather
    /// than bytes, and excludes the terminator rather than including it, while
    /// still taking its capacity in bytes. Nothing about the call site would
    /// show that, which is why it does its own measuring instead of sharing
    /// [`read_utf16_bytes`].
    pub fn intent(&self) -> Result<Option<String>, PdfiumError> {
        // SAFETY: a null buffer with zero capacity is the documented probe.
        let units =
            unsafe { pdfium_sys::EPDFAnnot_GetIntent(self.handle, std::ptr::null_mut(), 0) };
        if units == 0 {
            return Ok(None);
        }
        let capacity = units as usize + 1;
        let mut buffer: Vec<u16> = Vec::new();
        buffer.try_reserve_exact(capacity).map_err(|_| PdfiumError::OutOfMemory)?;
        buffer.resize(capacity, 0u16);
        let bytes =
            std::os::raw::c_ulong::try_from(capacity * 2).map_err(|_| PdfiumError::OutOfMemory)?;

        // SAFETY: `buffer` holds `capacity` code units, which is `bytes`
        // bytes, and that is what the length argument claims.
        unsafe { pdfium_sys::EPDFAnnot_GetIntent(self.handle, buffer.as_mut_ptr(), bytes) };
        // Stop at the terminator rather than trusting the measured count: the
        // fill call reports its own length and the two have disagreed in the
        // past for a value that changed between the calls.
        let end = buffer.iter().position(|unit| *unit == 0).unwrap_or(buffer.len());
        Ok(Some(String::from_utf16_lossy(&buffer[..end])))
    }

    /// `/CL`, the callout leader line of a free-text callout.
    pub fn callout_line(&self) -> Result<Vec<PointF>, PdfiumError> {
        // SAFETY: the handle is live.
        let count = unsafe { pdfium_sys::EPDFAnnot_GetCalloutLineCount(self.handle) };
        read_points(count, |buffer, capacity| {
            // SAFETY: `buffer` holds `capacity` points.
            unsafe { pdfium_sys::EPDFAnnot_GetCalloutLine(self.handle, buffer, capacity) }
        })
    }

    /// `/QuadPoints`, the attachment quads of a text-markup annotation.
    ///
    /// A quad PDFium refuses is skipped rather than zero-filled: a zero quad
    /// would be a mark of no size at the page origin, which is worse than one
    /// mark fewer.
    pub fn quad_points(&self) -> Result<Vec<QuadF>, PdfiumError> {
        // SAFETY: the handle is live.
        let count = unsafe { pdfium_sys::FPDFAnnot_CountAttachmentPoints(self.handle) };
        if count == 0 {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        out.try_reserve_exact(count).map_err(|_| PdfiumError::OutOfMemory)?;
        for index in 0..count {
            let mut quad = pdfium_sys::FS_QUADPOINTSF::default();
            // SAFETY: the handle is live and `quad` is a valid out-parameter.
            let ok = unsafe {
                pdfium_sys::FPDFAnnot_GetAttachmentPoints(self.handle, index, &raw mut quad)
            };
            if ok != 0 {
                out.push(quad.into());
            }
        }
        Ok(out)
    }

    /// `/Name` as text, whatever its value: a standard icon or stamp name, or
    /// a custom identifier. `None` when absent. Which names mean something
    /// depends on the subtype, so the decode is the subtype's reader and not
    /// this layer's.
    pub fn name(&self) -> Result<Option<String>, PdfiumError> {
        read_utf8(|buffer, len| {
            // SAFETY: the handle is live, and `len` describes `buffer` in bytes.
            unsafe { pdfium_sys::EPDFAnnot_GetName(self.handle, buffer.cast(), len) }
        })
    }

    /// `/RT`, as the raw `FPDF_ANNOT_REPLY_TYPE` code.
    ///
    /// Answers `RT_REPLY` for an annotation with no `/IRT` at all, because it
    /// reports `/RT`'s own default in isolation. A caller has to have found
    /// an `/IRT` before this means anything.
    pub fn reply_type_code(&self) -> i32 {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::EPDFAnnot_GetReplyType(self.handle) as i32 }
    }

    /// The annotation another entry points at, by key, typically `IRT`.
    pub fn linked_annotation(&self, key: &str) -> Result<Option<Annotation<'_>>, PdfiumError> {
        let key = c_key(key)?;
        // SAFETY: the handle and key are live for the call.
        let handle = unsafe { pdfium_sys::FPDFAnnot_GetLinkedAnnot(self.handle, key.as_ptr()) };
        if handle.is_null() {
            return Ok(None);
        }
        Ok(Some(Annotation { handle, _page: PhantomData, _not_send: PhantomData }))
    }

    /// The `/Rotate` a stamp carries, in degrees. `None` when absent.
    pub fn rotate(&self) -> Option<f32> {
        let mut rotation = 0f32;
        // SAFETY: the handle is live and `rotation` is a valid out-parameter.
        let ok = unsafe { pdfium_sys::EPDFAnnot_GetRotate(self.handle, &raw mut rotation) };
        (ok != 0).then_some(rotation)
    }

    /// The raw `FPDF_BLENDMODE` of the normal appearance stream.
    pub fn blend_mode_code(&self) -> i32 {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::EPDFAnnot_GetBlendMode(self.handle) as i32 }
    }

    /// Which `/AP` sub-dictionaries the annotation has, as the
    /// `EPDFAnnot_GetAvailableAppearanceModes` bit set: 1 is `/N`, 2 is `/R`,
    /// 4 is `/D`.
    ///
    /// Zero means no `/AP` at all, which is the common case and not an
    /// error: most annotations are drawn by the viewer rather than baked.
    pub fn available_appearance_modes(&self) -> i32 {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::EPDFAnnot_GetAvailableAppearanceModes(self.handle) }
    }

    /// Ask PDFium to synthesize a form field's `/AP`.
    ///
    /// Widgets frequently ship without one, because the producer expected the
    /// viewer to draw the control. This WRITES to the in-memory document,
    /// which is why it is a method a caller reaches for deliberately rather
    /// than something a read does on its way past.
    pub fn generate_form_field_appearance(&self) -> bool {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::EPDFAnnot_GenerateFormFieldAP(self.handle) != 0 }
    }

    /// Whether the annotation has the named appearance stream at all.
    pub fn has_appearance_stream(&self, mode: AppearanceMode) -> bool {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::EPDFAnnot_HasAppearanceStream(self.handle, mode as i32) != 0 }
    }

    /// `/AS`, the appearance state the annotation shows, as the name's bytes.
    /// `None` when it names none.
    pub fn appearance_state(&self) -> Result<Option<String>, PdfiumError> {
        // SAFETY: a null buffer with zero length is the measure call.
        let bytes = unsafe {
            pdfium_sys::EPDFAnnot_GetAppearanceState(self.handle, std::ptr::null_mut(), 0)
        };
        if bytes == 0 {
            return Ok(None);
        }
        let text = read_utf8(|buffer, len| {
            // SAFETY: the handle is live and `len` describes `buffer`.
            unsafe { pdfium_sys::EPDFAnnot_GetAppearanceState(self.handle, buffer, len) }
        })?;
        Ok(Some(text.unwrap_or_default()))
    }

    /// The states `mode` stores, in the file's key order
    /// (`EPDFAnnot_GetAppearanceStateCount`, `EPDFAnnot_GetAppearanceStateName`):
    /// none when the mode is a single stream or missing. Each is the name
    /// [`crate::PageView::render_annotation_rgba_with_matrix`] takes as its
    /// `state`.
    pub fn appearance_state_names(&self, mode: AppearanceMode) -> Result<Vec<String>, PdfiumError> {
        // SAFETY: the handle is live.
        let count =
            unsafe { pdfium_sys::EPDFAnnot_GetAppearanceStateCount(self.handle, mode as i32) };
        let mut names = Vec::new();
        for index in 0..count.max(0) {
            let name = read_utf8(|buffer, len| {
                // SAFETY: the handle is live and `len` describes `buffer`.
                unsafe {
                    pdfium_sys::EPDFAnnot_GetAppearanceStateName(
                        self.handle,
                        mode as i32,
                        index,
                        buffer,
                        len,
                    )
                }
            })?;
            if let Some(name) = name {
                names.push(name);
            }
        }
        Ok(names)
    }

    /// Where `mode`'s appearance is drawn, in page space, without writing
    /// anything (`EPDFAnnot_GetDrawingRect`): a stored appearance fitted into
    /// `/Rect`, or, with none, the box the normal appearance PDFium draws in
    /// memory takes, which can reach past `/Rect`. `None` when nothing can be
    /// drawn without writing an appearance.
    pub fn drawing_rect(&self, mode: AppearanceMode) -> Option<RectF> {
        let mut rect = pdfium_sys::FS_RECTF::default();
        // SAFETY: the handle is live and `rect` is a valid out-parameter.
        let ok = unsafe {
            pdfium_sys::EPDFAnnot_GetDrawingRect(self.handle, mode as i32, &raw mut rect)
        };
        (ok != 0).then(|| rect.into())
    }

    /// `EPDFAnnot_ExportAppearance`, saved whole: the normal appearance as a
    /// one-page drawing document, with `degrees` (counterclockwise) about
    /// `unrotated_box` taken out. `Ok(None)` when there is no normal
    /// appearance, `Err(Save)` when the new document would not serialize.
    /// The source document is untouched.
    pub fn export_appearance(
        &self,
        degrees: f32,
        unrotated_box: Option<RectF>,
    ) -> Result<Option<Vec<u8>>, PdfiumError> {
        let unrotated_box = unrotated_box.map(|rect| pdfium_sys::FS_RECTF {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        });
        let box_ptr = unrotated_box.as_ref().map_or(std::ptr::null(), std::ptr::from_ref);
        // SAFETY: the handle is live, and `box_ptr` is null or points at a
        // local that outlives the call.
        let exported =
            unsafe { pdfium_sys::EPDFAnnot_ExportAppearance(self.handle, degrees, box_ptr) };
        if exported.is_null() {
            return Ok(None);
        }
        crate::save::save_scratch_document(exported).map(Some).ok_or(PdfiumError::Save)
    }

    /// `/MK /R`, the widget's turn in degrees counterclockwise, as the file
    /// holds it; 0 when absent.
    pub fn mk_rotation(&self) -> i32 {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::EPDFAnnot_GetMKRotation(self.handle) }
    }

    /// One `/MK` text entry by `EPDF_MK_TEXT` (`/CA` is 0): `None` when
    /// absent, `Some("")` when present and empty.
    pub fn mk_text(&self, which: i32) -> Result<Option<String>, PdfiumError> {
        // SAFETY: a null buffer with zero length is the measure call.
        let bytes =
            unsafe { pdfium_sys::EPDFAnnot_GetMKText(self.handle, which, std::ptr::null_mut(), 0) };
        if bytes == 0 {
            return Ok(None);
        }
        let text = read_utf16_bytes(|buffer, len| {
            // SAFETY: the handle is live and `len` describes `buffer` in bytes.
            unsafe { pdfium_sys::EPDFAnnot_GetMKText(self.handle, which, buffer, len) }
        })?;
        Ok(Some(text.unwrap_or_default()))
    }

    /// The face of the font `/DA` names
    /// (`EPDFAnnot_GetDefaultAppearanceFontFace`): its family, weight and
    /// italic, as the font's dictionary says. `None` when `/DA` names no font.
    pub fn default_appearance_font_face(&self) -> Result<Option<(String, i32, bool)>, PdfiumError> {
        let (mut weight, mut italic) = (0i32, 0i32);
        let family = read_utf16_bytes(|buffer, len| {
            // SAFETY: the handle is live, `len` describes `buffer` in bytes,
            // and the two out-parameters are valid.
            unsafe {
                pdfium_sys::EPDFAnnot_GetDefaultAppearanceFontFace(
                    self.handle,
                    buffer,
                    len,
                    &raw mut weight,
                    &raw mut italic,
                )
            }
        })?;
        Ok(family.filter(|family| !family.is_empty()).map(|family| (family, weight, italic != 0)))
    }

    /// One `/MK` appearance-characteristics colour, by `EPDF_MK_COLORTYPE`.
    ///
    /// A widget's border and background live in `/MK`, not in `/C`, which is
    /// why this is separate from [`Annotation::color`] rather than another
    /// selector on it: they read different dictionary entries.
    pub fn mk_color(&self, which: i32) -> Option<(u8, u8, u8)> {
        let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
        // SAFETY: the handle is live and the three out-parameters are valid.
        let ok = unsafe {
            pdfium_sys::EPDFAnnot_GetMKColor(
                self.handle,
                which as u32,
                &raw mut r,
                &raw mut g,
                &raw mut b,
            )
        };
        (ok != 0).then_some((r as u8, g as u8, b as u8))
    }

    /// The embedded file a file-attachment annotation's `/FS` points at.
    ///
    /// The handle is unretained: PDFium hands back a pointer into the
    /// document's own filespec and there is no close call, which is why
    /// [`Attachment`] borrows this annotation rather than owning anything.
    pub fn file_attachment(&self) -> Option<Attachment<'_>> {
        // SAFETY: the handle is live for the borrow this returns.
        let handle = unsafe { pdfium_sys::FPDFAnnot_GetFileAttachment(self.handle) };
        (!handle.is_null()).then_some(Attachment { handle, _annotation: PhantomData })
    }

    /// The raw handle, for the accessors that hang off `Document` instead.
    pub(crate) fn as_raw(&self) -> pdfium_sys::FPDF_ANNOTATION {
        self.handle
    }

    /// Whether the annotation carries an `/EMBD_Metadata` dictionary.
    ///
    /// Worth asking before every metadata read: an annotation from Acrobat,
    /// or one written before the dictionary existed, has none at all, and the
    /// per-key getters would each pay for discovering that.
    pub fn has_embed_metadata(&self) -> bool {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::EPDFAnnot_HasEmbedMetadata(self.handle) != 0 }
    }

    /// One `/EMBD_Metadata` string entry.
    ///
    /// A third string ABI, and the one that differs from
    /// [`Annotation::string_value`] in the way that matters: it reports zero
    /// for an absent key and two for a present empty one, so it distinguishes
    /// the two by itself and needs no separate presence probe.
    pub fn embed_metadata_string(&self, key: &str) -> Result<Option<String>, PdfiumError> {
        let key = c_key(key)?;
        // SAFETY: a null buffer with zero length is the documented measure
        // call; the handle and key are live.
        let bytes = unsafe {
            pdfium_sys::EPDFAnnot_GetEmbedMetadataString(
                self.handle,
                key.as_ptr(),
                std::ptr::null_mut(),
                0,
            )
        };
        if bytes == 0 {
            return Ok(None);
        }
        if bytes <= 2 {
            return Ok(Some(String::new()));
        }
        let units = (bytes as usize).div_ceil(2);
        let mut buffer: Vec<u16> = Vec::new();
        buffer.try_reserve_exact(units).map_err(|_| PdfiumError::OutOfMemory)?;
        buffer.resize(units, 0u16);

        // SAFETY: `buffer` holds `bytes` bytes, which is what the length
        // argument claims.
        let written = unsafe {
            pdfium_sys::EPDFAnnot_GetEmbedMetadataString(
                self.handle,
                key.as_ptr(),
                buffer.as_mut_ptr(),
                bytes,
            )
        };
        if written == 0 {
            return Ok(None);
        }
        let produced = ((written as usize) / 2).saturating_sub(1).min(buffer.len());
        Ok(Some(String::from_utf16_lossy(&buffer[..produced])))
    }

    /// One `/EMBD_Metadata` number entry.
    pub fn embed_metadata_number(&self, key: &str) -> Result<Option<f32>, PdfiumError> {
        let key = c_key(key)?;
        let mut value = 0f32;
        // SAFETY: the handle and key are live and `value` is a valid
        // out-parameter.
        let ok = unsafe {
            pdfium_sys::EPDFAnnot_GetEmbedMetadataNumber(self.handle, key.as_ptr(), &raw mut value)
        };
        Ok((ok != 0).then_some(value))
    }

    /// One `/EMBD_Metadata` rectangle entry, un-normalized.
    pub fn embed_metadata_rect(&self, key: &str) -> Result<Option<RectF>, PdfiumError> {
        let key = c_key(key)?;
        let mut rect = pdfium_sys::FS_RECTF::default();
        // SAFETY: the handle and key are live and `rect` is a valid
        // out-parameter.
        let ok = unsafe {
            pdfium_sys::EPDFAnnot_GetEmbedMetadataRect(self.handle, key.as_ptr(), &raw mut rect)
        };
        Ok((ok != 0).then(|| rect.into()))
    }

    /// The `/AP` stream's `/Matrix`. `None` when there is no such stream.
    pub fn appearance_matrix(&self, mode: AppearanceMode) -> Option<MatrixF> {
        let mut matrix = pdfium_sys::FS_MATRIX::default();
        // SAFETY: the handle is live and `matrix` is a valid out-parameter.
        let ok =
            unsafe { pdfium_sys::EPDFAnnot_GetAPMatrix(self.handle, mode as i32, &raw mut matrix) };
        (ok != 0).then(|| matrix.into())
    }

    /// The form the normal appearance is drawn in and the matrix that places
    /// it on the page (`EPDFAnnot_GetAppearanceTransform`): the form's
    /// `/BBox` and the whole transform a viewer applies, its `/Matrix` and
    /// the fit onto `/Rect` together. `None` when there is no normal
    /// appearance, or its box is empty.
    pub fn appearance_transform(&self) -> Option<(RectF, MatrixF)> {
        let mut rect = pdfium_sys::FS_RECTF::default();
        let mut matrix = pdfium_sys::FS_MATRIX::default();
        // SAFETY: the handle is live and both out-parameters are valid.
        let ok = unsafe {
            pdfium_sys::EPDFAnnot_GetAppearanceTransform(
                self.handle,
                &raw mut rect,
                &raw mut matrix,
            )
        };
        (ok != 0).then(|| (rect.into(), matrix.into()))
    }

    /// A boolean entry of the dictionary, such as a popup's `/Open`
    /// (`EPDFAnnot_GetBooleanValue`). `None` when it is absent or not a
    /// boolean.
    pub fn boolean_value(&self, key: &str) -> Result<Option<bool>, PdfiumError> {
        let key = c_key(key)?;
        let mut value = 0;
        // SAFETY: the handle and key are live and `value` is a valid
        // out-parameter.
        let ok = unsafe {
            pdfium_sys::EPDFAnnot_GetBooleanValue(self.handle, key.as_ptr(), &raw mut value)
        };
        Ok((ok != 0).then_some(value != 0))
    }

    /// One `/EMBD_Metadata` boolean entry
    /// (`EPDFAnnot_GetEmbedMetadataBoolean`). `None` when it is absent.
    pub fn embed_metadata_boolean(&self, key: &str) -> Result<Option<bool>, PdfiumError> {
        let key = c_key(key)?;
        let mut value = 0;
        // SAFETY: the handle and key are live and `value` is a valid
        // out-parameter.
        let ok = unsafe {
            pdfium_sys::EPDFAnnot_GetEmbedMetadataBoolean(self.handle, key.as_ptr(), &raw mut value)
        };
        Ok((ok != 0).then_some(value != 0))
    }

    /// A line's `/Cap`, `/CP` and `/CO` (`EPDFAnnot_GetLineCaption`). `None`
    /// when the annotation is not a line or the read fails.
    pub fn line_caption(&self) -> Option<LineCaption> {
        let mut enabled = 0;
        let mut position = pdfium_sys::EPDF_CAPTION_POSITION::default();
        let mut offset = pdfium_sys::EPDF_CAPTION_OFFSET::default();
        // SAFETY: the handle is live and the three out-parameters are valid.
        let ok = unsafe {
            pdfium_sys::EPDFAnnot_GetLineCaption(
                self.handle,
                &raw mut enabled,
                &raw mut position,
                &raw mut offset,
            )
        };
        (ok != 0).then_some(LineCaption {
            enabled: enabled != 0,
            position_code: position as i32,
            along: offset.along,
            perpendicular: offset.perpendicular,
        })
    }

    /// A polygon's or polyline's caption, as `/EMBD_Metadata` keeps it
    /// (`EPDFAnnot_GetShapeCaption`): whether it shows, and its manual
    /// center when it has one. `None` when the read fails.
    pub fn shape_caption(&self) -> Option<ShapeCaption> {
        let mut enabled = 0;
        let mut has_center = 0;
        let mut center = pdfium_sys::FS_POINTF::default();
        // SAFETY: the handle is live and the three out-parameters are valid.
        let ok = unsafe {
            pdfium_sys::EPDFAnnot_GetShapeCaption(
                self.handle,
                &raw mut enabled,
                &raw mut has_center,
                &raw mut center,
            )
        };
        (ok != 0).then(|| ShapeCaption {
            enabled: enabled != 0,
            center: (has_center != 0).then(|| center.into()),
        })
    }

    /// A line's `/LL`, `/LLE` and `/LLO` (`EPDFAnnot_GetLineLeader`):
    /// length, extension and offset. `None` when the read fails.
    pub fn line_leader(&self) -> Option<(f32, f32, f32)> {
        let (mut length, mut extension, mut offset) = (0f32, 0f32, 0f32);
        // SAFETY: the handle is live and the three out-parameters are valid.
        let ok = unsafe {
            pdfium_sys::EPDFAnnot_GetLineLeader(
                self.handle,
                &raw mut length,
                &raw mut extension,
                &raw mut offset,
            )
        };
        (ok != 0).then_some((length, extension, offset))
    }
}

/// A line's caption (`/Cap`, `/CP`, `/CO`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineCaption {
    pub enabled: bool,
    /// The raw `EPDF_CAPTION_POSITION`: 0 inline, 1 top.
    pub position_code: i32,
    /// `/CO`, along the line and across it.
    pub along: f32,
    pub perpendicular: f32,
}

/// A polygon's or polyline's caption.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapeCaption {
    pub enabled: bool,
    /// The label's manual center, in the file's coordinates.
    pub center: Option<PointF>,
}

impl Document {
    /// `EPDFLayer_GetBirthName`: the page and the position in the base
    /// page's `/Annots` an annotation object was born at, when this layer
    /// promoted it from an inline annotation of the base. `None` for any
    /// other object, and for a document that is not a layer.
    pub fn layer_birth_name(&self, object_number: u32) -> Option<(u32, u32)> {
        let (mut page, mut birth): (std::os::raw::c_ulong, std::os::raw::c_ulong) = (0, 0);
        // SAFETY: the handle is live and both out-parameters are valid.
        let ok = unsafe {
            pdfium_sys::EPDFLayer_GetBirthName(
                self.as_raw(),
                std::os::raw::c_ulong::from(object_number),
                &raw mut page,
                &raw mut birth,
            )
        };
        if ok == 0 {
            return None;
        }
        Some((u32::try_from(page).ok()?, u32::try_from(birth).ok()?))
    }

    /// `EPDFLayer_IsPagePromoted`: whether this layer promoted the page's
    /// inline annotations, so they resolve through their births rather than
    /// by position. `false` for a document that is not a layer.
    pub fn layer_is_page_promoted(&self, page_object_number: u32) -> bool {
        // SAFETY: the handle is live.
        unsafe {
            pdfium_sys::EPDFLayer_IsPagePromoted(
                self.as_raw(),
                std::os::raw::c_ulong::from(page_object_number),
            ) != 0
        }
    }

    /// `EPDFLayer_GetBirthObjectNumber`: the object the inline annotation
    /// born at `birth_index` of the page became when this layer promoted the
    /// page. `None` when the page is not promoted or had no inline
    /// annotation there.
    pub fn layer_birth_object_number(
        &self,
        page_object_number: u32,
        birth_index: u32,
    ) -> Option<u32> {
        // SAFETY: the handle is live.
        let number = unsafe {
            pdfium_sys::EPDFLayer_GetBirthObjectNumber(
                self.as_raw(),
                std::os::raw::c_ulong::from(page_object_number),
                std::os::raw::c_ulong::from(birth_index),
            )
        };
        u32::try_from(number).ok().filter(|number| *number > 0)
    }

    /// `EPDFPage_PromoteInlineAnnotsRaw`: make every inline annotation of a
    /// page's `/Annots` an object, without loading the page. On a layer the
    /// first promotion of a page the base has records its births. Answers
    /// how many moved, or `None` when the runtime refused.
    pub fn promote_inline_annotations_raw(&self, page_index: u32) -> Option<u32> {
        let page_index = i32::try_from(page_index).ok()?;
        // SAFETY: the handle is live.
        let moved =
            unsafe { pdfium_sys::EPDFPage_PromoteInlineAnnotsRaw(self.as_raw(), page_index) };
        u32::try_from(moved).ok()
    }
}

fn c_key(key: &str) -> Result<CString, PdfiumError> {
    CString::new(key).map_err(|_| PdfiumError::InvalidArgument("annotation key contains NUL"))
}

fn alloc_f32(len: usize) -> Result<Vec<f32>, PdfiumError> {
    let mut out: Vec<f32> = Vec::new();
    out.try_reserve_exact(len).map_err(|_| PdfiumError::OutOfMemory)?;
    out.resize(len, 0.0);
    Ok(out)
}

/// The element-counted point getters: probe for a count, fill a buffer of
/// exactly that many `FS_POINTF`, and trust the fill call's own count over
/// the probe's.
fn read_points<F>(count: std::os::raw::c_ulong, mut fill: F) -> Result<Vec<PointF>, PdfiumError>
where
    F: FnMut(*mut pdfium_sys::FS_POINTF, std::os::raw::c_ulong) -> std::os::raw::c_ulong,
{
    if count == 0 {
        return Ok(Vec::new());
    }
    let mut buffer: Vec<pdfium_sys::FS_POINTF> = Vec::new();
    buffer.try_reserve_exact(count as usize).map_err(|_| PdfiumError::OutOfMemory)?;
    buffer.resize(count as usize, pdfium_sys::FS_POINTF::default());

    let written = fill(buffer.as_mut_ptr(), count);
    if written == 0 {
        return Ok(Vec::new());
    }
    let produced = (written as usize).min(buffer.len());
    Ok(buffer[..produced].iter().copied().map(PointF::from).collect())
}

/// Measure then fill, for a UTF-8 accessor.
///
/// The reported length counts the terminator, and a length of one or less is
/// an absent value rather than an empty one.
fn read_utf8<F>(mut call: F) -> Result<Option<String>, PdfiumError>
where
    F: FnMut(*mut std::ffi::c_void, std::os::raw::c_ulong) -> std::os::raw::c_ulong,
{
    let bytes = call(std::ptr::null_mut(), 0);
    if bytes <= 1 {
        return Ok(None);
    }
    let mut buffer: Vec<u8> = Vec::new();
    buffer.try_reserve_exact(bytes as usize).map_err(|_| PdfiumError::OutOfMemory)?;
    buffer.resize(bytes as usize, 0u8);

    let written = call(buffer.as_mut_ptr().cast(), bytes);
    if written == 0 {
        return Ok(None);
    }
    let produced = (written as usize).saturating_sub(1).min(buffer.len());
    Ok(Some(String::from_utf8_lossy(&buffer[..produced]).into_owned()))
}

/// The byte-counted UTF-16 getters: the reported length counts the
/// terminator, so two bytes or fewer is nothing.
fn read_utf16_bytes<F>(mut call: F) -> Result<Option<String>, PdfiumError>
where
    F: FnMut(*mut u16, std::os::raw::c_ulong) -> std::os::raw::c_ulong,
{
    let bytes = call(std::ptr::null_mut(), 0);
    if bytes <= 2 {
        return Ok(None);
    }
    let units = (bytes as usize).div_ceil(2);
    let mut buffer: Vec<u16> = Vec::new();
    buffer.try_reserve_exact(units).map_err(|_| PdfiumError::OutOfMemory)?;
    buffer.resize(units, 0u16);

    let written = call(buffer.as_mut_ptr(), bytes);
    if written == 0 {
        return Ok(None);
    }
    let produced = ((written as usize) / 2).saturating_sub(1).min(buffer.len());
    Ok(Some(String::from_utf16_lossy(&buffer[..produced])))
}

/// Why an extraction produced nothing.
///
/// `EPDFAttachmentExtractStatus`. Separate from [`PdfiumError`] because each
/// of these means something specific about the DOCUMENT rather than about the
/// runtime, and the engine maps them to different error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentExtractError {
    /// No `/EF`: the filespec names a file the document does not embed.
    NoFileStream,
    /// The stream is there and its filters will not decode.
    DecodeFailed,
    /// Larger than the caller's ceiling.
    SizeLimitExceeded,
    /// The destination refused the bytes.
    WriteFailed,
    /// A status this build does not know, kept rather than collapsed.
    Unknown(u32),
}

impl AttachmentExtractError {
    fn from_status(status: u32) -> Self {
        match status {
            1 => Self::NoFileStream,
            2 => Self::DecodeFailed,
            3 => Self::SizeLimitExceeded,
            4 => Self::WriteFailed,
            other => Self::Unknown(other),
        }
    }
}

impl std::fmt::Display for AttachmentExtractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoFileStream => write!(f, "the filespec has no embedded file stream (/EF)"),
            Self::DecodeFailed => write!(f, "the embedded file stream failed to decode"),
            Self::SizeLimitExceeded => {
                write!(f, "the embedded file exceeds the decoded-size limit")
            }
            Self::WriteFailed => write!(f, "writing the extracted file failed"),
            Self::Unknown(status) => write!(f, "extraction failed with status {status}"),
        }
    }
}

impl Document {
    /// Add an embedded file to the catalog's `/EmbeddedFiles` name tree.
    ///
    /// The name becomes the tree KEY, which is the entry's identity. PDFium
    /// refuses a duplicate, and the attachment is empty until
    /// [`Attachment::set_file`] writes bytes into it.
    pub fn add_attachment(&self, name: &str) -> Option<Attachment<'_>> {
        let name: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: the handle is live and the name outlives the call.
        let handle = unsafe { pdfium_sys::FPDFDoc_AddAttachment(self.as_raw(), name.as_ptr()) };
        // SAFETY: the handle is PDFium's and is not owned by this wrapper.
        unsafe { Attachment::from_raw(handle) }
    }

    /// Remove one, by its position in the tree.
    ///
    /// By INDEX, because that is what PDFium takes. The tree is sorted by
    /// key, so a removal shifts the entries after it: a caller holding
    /// several indices has to resolve each against a fresh read.
    pub fn delete_attachment(&self, index: u32) -> bool {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::FPDFDoc_DeleteAttachment(self.as_raw(), index as i32) != 0 }
    }

    /// How many entries the catalog's `/EmbeddedFiles` name tree holds.
    pub fn attachment_count(&self) -> u32 {
        // SAFETY: the handle is live.
        let count = unsafe { pdfium_sys::FPDFDoc_GetAttachmentCount(self.as_raw()) };
        u32::try_from(count).unwrap_or(0)
    }

    /// One entry of the name tree, by position.
    ///
    /// Position is DISPLAY order, not an address: creating or removing an
    /// entry shifts every index after it. The durable address is the tree
    /// key, which [`Document::attachment_key`] reads and
    /// [`Document::attachment_index_by_key`] resolves.
    pub fn attachment(&self, index: u32) -> Option<Attachment<'_>> {
        let index = i32::try_from(index).ok()?;
        // SAFETY: the handle is live for the borrow this returns.
        let handle = unsafe { pdfium_sys::FPDFDoc_GetAttachment(self.as_raw(), index) };
        (!handle.is_null()).then_some(Attachment { handle, _annotation: PhantomData })
    }

    /// The name-tree KEY at `index`, which is the durable address.
    ///
    /// Usually equal to the file's display name, and not reliably so: keys
    /// are unique within the tree by construction while two `/UF` values can
    /// collide, so a foreign document can have both files called
    /// `report.pdf` under two different keys.
    pub fn attachment_key(&self, index: u32) -> Result<Option<String>, PdfiumError> {
        let Ok(index) = i32::try_from(index) else {
            return Ok(None);
        };
        read_utf16_bytes(|buffer, len| {
            // SAFETY: the handle is live and `len` describes `buffer` in
            // bytes.
            unsafe { pdfium_sys::EPDFDoc_GetAttachmentKey(self.as_raw(), index, buffer, len) }
        })
    }

    /// The current index of the entry with this key, or `None`.
    pub fn attachment_index_by_key(&self, key: &str) -> Option<u32> {
        let mut units: Vec<u16> = key.encode_utf16().collect();
        units.push(0);
        // SAFETY: the handle is live and the key is NUL-terminated and
        // outlives the call.
        let index =
            unsafe { pdfium_sys::EPDFDoc_GetAttachmentIndexByKey(self.as_raw(), units.as_ptr()) };
        u32::try_from(index).ok()
    }
}

/// An embedded file, read through the filespec that names it.
///
/// Unretained, like everything `FPDF_ATTACHMENT` addresses: there is no close
/// call, and the lifetime ties it to whatever produced it so it cannot outlive
/// the document its bytes live in.
#[derive(Debug)]
pub struct Attachment<'owner> {
    handle: pdfium_sys::FPDF_ATTACHMENT,
    _annotation: PhantomData<&'owner ()>,
}

impl<'owner> Attachment<'owner> {
    /// Wrap a handle PDFium produced.
    ///
    /// # Safety
    /// `handle` must be a live `FPDF_ATTACHMENT` belonging to something that
    /// outlives `'owner`.
    pub(crate) unsafe fn from_raw(handle: pdfium_sys::FPDF_ATTACHMENT) -> Option<Self> {
        (!handle.is_null()).then_some(Self { handle, _annotation: PhantomData })
    }

    /// Write the embedded file's bytes.
    ///
    /// PDFium derives `/Params` `Size`, `CheckSum` and `CreationDate` from
    /// what is written here, which is why an empty attachment is a real call
    /// with a null pointer rather than no call at all: skipping it would
    /// leave a filespec naming a stream that does not exist.
    pub fn set_file(&self, document: &Document, bytes: &[u8]) -> bool {
        let (ptr, len) = if bytes.is_empty() {
            (std::ptr::null(), 0)
        } else {
            (bytes.as_ptr().cast::<std::ffi::c_void>(), bytes.len() as std::os::raw::c_ulong)
        };
        // SAFETY: both handles are live and the buffer outlives the call.
        unsafe { pdfium_sys::FPDFAttachment_SetFile(self.handle, document.as_raw(), ptr, len) != 0 }
    }

    /// `/Subtype` of the stream, which is the declared MIME type.
    pub fn set_mime_type(&self, mime: &str) -> Result<bool, PdfiumError> {
        let mime = CString::new(mime).map_err(|_| PdfiumError::InvalidArgument("mime type"))?;
        // SAFETY: the handle is live and the string outlives the call.
        Ok(unsafe { pdfium_sys::EPDFAttachment_SetSubtype(self.handle, mime.as_ptr()) != 0 })
    }

    /// `/UF` and `/F`, the file name. Every other entry is kept. PDFium
    /// refuses an empty name.
    pub fn set_name(&self, name: &str) -> bool {
        let name: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: the handle is live and the string outlives the call.
        unsafe { pdfium_sys::EPDFAttachment_SetName(self.handle, name.as_ptr()) != 0 }
    }

    /// One `/Params` string entry, written as a text string.
    pub fn set_string_value(&self, key: &str, value: &str) -> Result<bool, PdfiumError> {
        let key = c_key(key)?;
        let value: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: the handle is live and both strings outlive the call.
        Ok(unsafe {
            pdfium_sys::FPDFAttachment_SetStringValue(self.handle, key.as_ptr(), value.as_ptr())
                != 0
        })
    }

    /// `/Desc`.
    pub fn set_description(&self, description: &str) -> bool {
        let description: Vec<u16> = description.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: the handle is live and the string outlives the call.
        unsafe { pdfium_sys::EPDFAttachment_SetDescription(self.handle, description.as_ptr()) != 0 }
    }

    /// `/UF`, the display name.
    pub fn name(&self) -> Result<Option<String>, PdfiumError> {
        read_utf16_bytes(|buffer, len| {
            // SAFETY: the handle is live and `len` describes `buffer`.
            unsafe { pdfium_sys::FPDFAttachment_GetName(self.handle, buffer, len) }
        })
    }

    /// `/Subtype` of the embedded file stream, which is its MIME type.
    pub fn mime_type(&self) -> Result<Option<String>, PdfiumError> {
        read_utf16_bytes(|buffer, len| {
            // SAFETY: as above.
            unsafe { pdfium_sys::FPDFAttachment_GetSubtype(self.handle, buffer, len) }
        })
    }

    /// `/Desc`.
    pub fn description(&self) -> Result<Option<String>, PdfiumError> {
        read_utf16_bytes(|buffer, len| {
            // SAFETY: as above.
            unsafe { pdfium_sys::EPDFAttachment_GetDescription(self.handle, buffer, len) }
        })
    }

    /// One `/Params` string entry, `None` when the key is absent.
    pub fn string_value(&self, key: &str) -> Result<Option<String>, PdfiumError> {
        let key = c_key(key)?;
        // SAFETY: the handle and key are live.
        let present = unsafe { pdfium_sys::FPDFAttachment_HasKey(self.handle, key.as_ptr()) };
        if present == 0 {
            return Ok(None);
        }
        read_utf16_bytes(|buffer, len| {
            // SAFETY: as above, and `len` describes `buffer`.
            unsafe {
                pdfium_sys::FPDFAttachment_GetStringValue(self.handle, key.as_ptr(), buffer, len)
            }
        })
    }

    /// Decode the embedded file into a fresh buffer.
    ///
    /// `max_decoded_bytes` of zero is unbudgeted, which is the fork's
    /// convention and not this layer's choice.
    ///
    /// The decode happens ONCE, inside PDFium, into a buffer it owns and this
    /// frees. A zero-byte embedded file is a valid result and not a failure:
    /// an empty file is a file.
    pub fn extract(&self, max_decoded_bytes: u64) -> Result<Vec<u8>, AttachmentExtractError> {
        let mut buffer: *mut std::ffi::c_void = std::ptr::null_mut();
        let mut size = 0u32;
        // The fork's status enum is unsigned, and `u32::MAX` is a value it
        // never issues, so an untouched out-parameter is distinguishable from
        // every real status.
        let mut status = u32::MAX;

        // SAFETY: the handle is live and all three out-parameters are valid.
        let ok = unsafe {
            pdfium_sys::EPDFAttachment_ExtractFileToOwnedBuffer(
                self.handle,
                max_decoded_bytes,
                &raw mut buffer,
                &raw mut size,
                &raw mut status,
            )
        };
        if ok == 0 {
            return Err(AttachmentExtractError::from_status(status));
        }
        if size == 0 || buffer.is_null() {
            // Still free it: a zero-size success can hand back a buffer.
            if !buffer.is_null() {
                // SAFETY: the buffer came from the call above and is freed
                // once.
                unsafe { pdfium_sys::EPDF_FreeBuffer(buffer) };
            }
            return Ok(Vec::new());
        }

        // SAFETY: PDFium reported `size` readable bytes at `buffer`.
        let bytes =
            unsafe { std::slice::from_raw_parts(buffer.cast::<u8>(), size as usize) }.to_vec();
        // SAFETY: as above, and nothing else holds the pointer.
        unsafe { pdfium_sys::EPDF_FreeBuffer(buffer) };
        Ok(bytes)
    }

    /// One `/Params` integer entry.
    pub fn integer_value(&self, key: &str) -> Result<Option<i32>, PdfiumError> {
        let key = c_key(key)?;
        let mut value = 0i32;
        // SAFETY: the handle and key are live and `value` is a valid
        // out-parameter.
        let ok = unsafe {
            pdfium_sys::EPDFAttachment_GetIntegerValue(self.handle, key.as_ptr(), &raw mut value)
        };
        Ok((ok != 0).then_some(value))
    }
}
