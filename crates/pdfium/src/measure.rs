//! Measurement viewports (`/VP`), their `/Measure` dictionaries and number
//! formats, over the fork's `epdf_measure.h`.
//!
//! Thin, one wrapper per native call, so that `epdf-engine` can transcribe
//! `packages/engine/services/src/features/measure/internal/measureCodec.ts`
//! call for call. What a value means (a fraction code's name, which
//! viewport is EmbedPDF's) is decided there, not here.
//!
//! Handles borrow the page they came from. The header's own rule still
//! applies on top of the borrow: a structural mutation (removing a
//! viewport, resetting a measure) makes every handle obtained before it
//! stale, and a stale handle fails closed. Reacquire after one.

use std::marker::PhantomData;
use std::os::raw::c_ulong;

use crate::{PageView, PdfiumError, PointF, RectF};

/// `EPDF_MEASURE_SUBTYPE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeasureSubtype {
    Unknown,
    /// Rectilinear: the only writable subtype.
    Rl,
    /// Geospatial: readable as "present, not ours".
    Geo,
}

/// `EPDF_MEASURE_AXIS`: which number-format array of a rectilinear measure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeasureAxis {
    X = 0,
    Y = 1,
    Distance = 2,
    Area = 3,
    Angle = 4,
    Slope = 5,
}

/// `EPDF_MEASURE_TEXT`: the text entries of a number format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeasureText {
    /// `/RT`
    ThousandsSeparator = 0,
    /// `/RD`
    DecimalSeparator = 1,
    /// `/PS`
    PrefixSpacing = 2,
    /// `/SS`
    SuffixSpacing = 3,
}

/// One entry of a page's `/VP` array.
#[derive(Debug)]
pub struct Viewport<'page> {
    handle: pdfium_sys::EPDF_VIEWPORT,
    _page: PhantomData<&'page ()>,
    _not_send: PhantomData<*const ()>,
}

/// A `/Measure` dictionary.
#[derive(Debug)]
pub struct Measure<'page> {
    handle: pdfium_sys::EPDF_MEASURE,
    _page: PhantomData<&'page ()>,
    _not_send: PhantomData<*const ()>,
}

/// One `/NumberFormat` dictionary inside a measure's format array.
#[derive(Debug)]
pub struct NumberFormat<'page> {
    handle: pdfium_sys::EPDF_NUMBERFORMAT,
    _page: PhantomData<&'page ()>,
    _not_send: PhantomData<*const ()>,
}

fn rect_in(rect: RectF) -> pdfium_sys::FS_RECTF {
    pdfium_sys::FS_RECTF { left: rect.left, top: rect.top, right: rect.right, bottom: rect.bottom }
}

/// A UTF-16 value, NUL-terminated as `FPDF_WIDESTRING` requires.
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// The two-call UTF-16 getters, read as `readUtf16String` reads them
/// (`packages/engine/services/src/runtime/memory/strings.ts`): `0` is
/// absent, `2` (a lone NUL) is an explicitly empty string. The two mean
/// different things for a number format's text entries.
fn read_text<F>(mut call: F) -> Result<Option<String>, PdfiumError>
where
    F: FnMut(*mut u16, c_ulong) -> c_ulong,
{
    let bytes = call(std::ptr::null_mut(), 0);
    if bytes == 0 {
        return Ok(None);
    }
    if bytes == 2 {
        return Ok(Some(String::new()));
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

impl PageView<'_> {
    /// Entries in the page's `/VP` array, in drawing order. A malformed
    /// entry keeps its index and reads as `None` from [`Self::viewport`].
    pub fn viewport_count(&self) -> u32 {
        // SAFETY: the page handle is live for the borrow.
        let count = unsafe { pdfium_sys::EPDFPage_CountViewports(self.as_raw()) };
        u32::try_from(count).unwrap_or(u32::MAX)
    }

    /// The viewport at `index`, or `None` past the end or for a malformed
    /// entry.
    pub fn viewport(&self, index: u32) -> Option<Viewport<'_>> {
        // SAFETY: the page handle is live for the borrow.
        let handle =
            unsafe { pdfium_sys::EPDFPage_GetViewport(self.as_raw(), c_ulong::from(index)) };
        (!handle.is_null()).then_some(Viewport {
            handle,
            _page: PhantomData,
            _not_send: PhantomData,
        })
    }

    /// Append a viewport over `bbox` (normalised by PDFium). Appending is
    /// deliberate: the last viewport containing a point wins (§12.9.1).
    pub fn add_viewport(&self, bbox: RectF) -> Option<Viewport<'_>> {
        let rect = rect_in(bbox);
        // SAFETY: the page handle is live and `rect` outlives the call.
        let handle = unsafe { pdfium_sys::EPDFPage_AddViewport(self.as_raw(), &rect) };
        (!handle.is_null()).then_some(Viewport {
            handle,
            _page: PhantomData,
            _not_send: PhantomData,
        })
    }

    /// Remove the viewport at `index`, and `/VP` with it when it empties.
    /// Every viewport handle taken from this page before the call is stale.
    pub fn remove_viewport(&self, index: u32) -> bool {
        // SAFETY: the page handle is live for the borrow.
        unsafe { pdfium_sys::EPDFPage_RemoveViewport(self.as_raw(), c_ulong::from(index)) != 0 }
    }
}

impl<'page> Viewport<'page> {
    /// `/BBox`, as stored (not normalised), or `None` when it doesn't read.
    pub fn bbox(&self) -> Option<RectF> {
        let mut rect = pdfium_sys::FS_RECTF { left: 0.0, top: 0.0, right: 0.0, bottom: 0.0 };
        // SAFETY: the handle is live for the page borrow; `rect` is a local.
        let ok = unsafe { pdfium_sys::EPDFViewport_GetBBox(self.handle, &mut rect) } != 0;
        ok.then_some(RectF {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        })
    }

    pub fn set_bbox(&self, bbox: RectF) -> bool {
        let rect = rect_in(bbox);
        // SAFETY: as above.
        unsafe { pdfium_sys::EPDFViewport_SetBBox(self.handle, &rect) != 0 }
    }

    /// `/Name`; `Some("")` for an explicitly empty one.
    pub fn name(&self) -> Result<Option<String>, PdfiumError> {
        // SAFETY: the buffer and its length come from `read_text`.
        read_text(|buf, len| unsafe { pdfium_sys::EPDFViewport_GetName(self.handle, buf, len) })
    }

    pub fn set_name(&self, name: &str) -> bool {
        let name = wide(name);
        // SAFETY: `name` is NUL-terminated and outlives the call.
        unsafe { pdfium_sys::EPDFViewport_SetName(self.handle, name.as_ptr()) != 0 }
    }

    /// `/Measure`, or `None` when there is none.
    pub fn measure(&self) -> Option<Measure<'page>> {
        // SAFETY: the handle is live for the page borrow.
        let handle = unsafe { pdfium_sys::EPDFViewport_GetMeasure(self.handle) };
        Measure::wrap(handle)
    }

    /// `/Measure` reset to an EMPTY rectilinear measure, replacing any
    /// existing rectilinear one. `None` when the existing measure has
    /// another subtype (it is never overwritten).
    pub fn add_measure(&self) -> Option<Measure<'page>> {
        // SAFETY: the handle is live for the page borrow.
        let handle = unsafe { pdfium_sys::EPDFViewport_AddMeasure(self.handle) };
        Measure::wrap(handle)
    }
}

impl<'page> crate::Annotation<'page> {
    /// The annotation's `/Measure`, or `None` when it has none
    /// (`EPDFAnnot_GetMeasure`). Reading never changes the document.
    pub fn measure(&self) -> Option<Measure<'page>> {
        // SAFETY: the annotation handle is live for the page borrow.
        let handle = unsafe { pdfium_sys::EPDFAnnot_GetMeasure(self.as_raw()) };
        Measure::wrap(handle)
    }
}

impl<'page> Measure<'page> {
    fn wrap(handle: pdfium_sys::EPDF_MEASURE) -> Option<Self> {
        (!handle.is_null()).then_some(Measure {
            handle,
            _page: PhantomData,
            _not_send: PhantomData,
        })
    }

    /// `/Subtype`; an absent one reads as `Rl` (the ISO default).
    pub fn subtype(&self) -> MeasureSubtype {
        // SAFETY: the handle is live for the page borrow.
        match unsafe { pdfium_sys::EPDFMeasure_GetSubtype(self.handle) } {
            1 => MeasureSubtype::Rl,
            2 => MeasureSubtype::Geo,
            _ => MeasureSubtype::Unknown,
        }
    }

    /// `/R`, the scale-ratio text.
    pub fn ratio(&self) -> Result<Option<String>, PdfiumError> {
        // SAFETY: the buffer and its length come from `read_text`.
        read_text(|buf, len| unsafe { pdfium_sys::EPDFMeasure_GetRatio(self.handle, buf, len) })
    }

    pub fn set_ratio(&self, ratio: &str) -> bool {
        let ratio = wide(ratio);
        // SAFETY: `ratio` is NUL-terminated and outlives the call.
        unsafe { pdfium_sys::EPDFMeasure_SetRatio(self.handle, ratio.as_ptr()) != 0 }
    }

    /// `/O`, or `None` when absent.
    pub fn origin(&self) -> Option<PointF> {
        let mut point = pdfium_sys::FS_POINTF { x: 0.0, y: 0.0 };
        // SAFETY: the handle is live; `point` is a local.
        let ok = unsafe { pdfium_sys::EPDFMeasure_GetOrigin(self.handle, &mut point) } != 0;
        ok.then_some(PointF { x: point.x, y: point.y })
    }

    pub fn set_origin(&self, origin: PointF) -> bool {
        let point = pdfium_sys::FS_POINTF { x: origin.x, y: origin.y };
        // SAFETY: `point` outlives the call.
        unsafe { pdfium_sys::EPDFMeasure_SetOrigin(self.handle, &point) != 0 }
    }

    /// `/CYX`, or `None` when absent.
    pub fn cyx(&self) -> Option<f32> {
        let mut cyx = 0f32;
        // SAFETY: the handle is live; `cyx` is a local.
        let ok = unsafe { pdfium_sys::EPDFMeasure_GetCYX(self.handle, &mut cyx) } != 0;
        ok.then_some(cyx)
    }

    pub fn set_cyx(&self, cyx: f32) -> bool {
        // SAFETY: `cyx` outlives the call.
        unsafe { pdfium_sys::EPDFMeasure_SetCYX(self.handle, &cyx) != 0 }
    }

    pub fn format_count(&self, axis: MeasureAxis) -> u32 {
        // SAFETY: the handle is live for the page borrow.
        let count = unsafe { pdfium_sys::EPDFMeasure_CountFormats(self.handle, axis as u32) };
        u32::try_from(count).unwrap_or(u32::MAX)
    }

    pub fn format(&self, axis: MeasureAxis, index: u32) -> Option<NumberFormat<'page>> {
        // SAFETY: the handle is live for the page borrow.
        let handle = unsafe {
            pdfium_sys::EPDFMeasure_GetFormat(self.handle, axis as u32, c_ulong::from(index))
        };
        NumberFormat::wrap(handle)
    }

    /// Append a format with its two required entries, `/U` and `/C`.
    pub fn add_format(
        &self,
        axis: MeasureAxis,
        unit: &str,
        conversion: f32,
    ) -> Option<NumberFormat<'page>> {
        let unit = wide(unit);
        // SAFETY: `unit` is NUL-terminated and outlives the call.
        let handle = unsafe {
            pdfium_sys::EPDFMeasure_AddFormat(self.handle, axis as u32, unit.as_ptr(), conversion)
        };
        NumberFormat::wrap(handle)
    }
}

impl NumberFormat<'_> {
    fn wrap(handle: pdfium_sys::EPDF_NUMBERFORMAT) -> Option<Self> {
        (!handle.is_null()).then_some(NumberFormat {
            handle,
            _page: PhantomData,
            _not_send: PhantomData,
        })
    }

    /// `/U`.
    pub fn unit(&self) -> Result<Option<String>, PdfiumError> {
        // SAFETY: the buffer and its length come from `read_text`.
        read_text(|buf, len| unsafe { pdfium_sys::EPDFNumberFormat_GetUnit(self.handle, buf, len) })
    }

    /// `/C`, or `None` when it doesn't read.
    pub fn conversion(&self) -> Option<f32> {
        let mut value = 0f32;
        // SAFETY: the handle is live; `value` is a local.
        let ok = unsafe { pdfium_sys::EPDFNumberFormat_GetConversion(self.handle, &mut value) };
        (ok != 0).then_some(value)
    }

    /// `/F` as its `EPDF_MEASURE_FRACTION` code, or `None` when absent.
    pub fn fraction(&self) -> Option<u32> {
        let mut value: pdfium_sys::EPDF_MEASURE_FRACTION = 0;
        // SAFETY: as above.
        let ok = unsafe { pdfium_sys::EPDFNumberFormat_GetFraction(self.handle, &mut value) };
        (ok != 0).then_some(value)
    }

    pub fn set_fraction(&self, code: u32) -> bool {
        // SAFETY: the handle is live for the page borrow.
        unsafe { pdfium_sys::EPDFNumberFormat_SetFraction(self.handle, code) != 0 }
    }

    /// `/D`, or `None` when absent.
    pub fn precision(&self) -> Option<i32> {
        let mut value = 0i32;
        // SAFETY: as above.
        let ok = unsafe { pdfium_sys::EPDFNumberFormat_GetPrecision(self.handle, &mut value) };
        (ok != 0).then_some(value)
    }

    /// Refused for a value below 1.
    pub fn set_precision(&self, precision: i32) -> bool {
        // SAFETY: the handle is live for the page borrow.
        unsafe { pdfium_sys::EPDFNumberFormat_SetPrecision(self.handle, precision) != 0 }
    }

    /// `/FD`, or `None` when absent.
    pub fn fixed_denominator(&self) -> Option<bool> {
        let mut value: pdfium_sys::FPDF_BOOL = 0;
        // SAFETY: as above.
        let ok =
            unsafe { pdfium_sys::EPDFNumberFormat_GetFixedDenominator(self.handle, &mut value) };
        (ok != 0).then_some(value != 0)
    }

    pub fn set_fixed_denominator(&self, fixed: bool) -> bool {
        // SAFETY: the handle is live for the page borrow.
        unsafe {
            pdfium_sys::EPDFNumberFormat_SetFixedDenominator(self.handle, i32::from(fixed)) != 0
        }
    }

    /// `/O` as its `EPDF_MEASURE_LABEL_POSITION` code (0 suffix, 1 prefix),
    /// or `None` when absent.
    pub fn label_position(&self) -> Option<u32> {
        let mut value: pdfium_sys::EPDF_MEASURE_LABEL_POSITION = 0;
        // SAFETY: as above.
        let ok = unsafe { pdfium_sys::EPDFNumberFormat_GetLabelPosition(self.handle, &mut value) };
        (ok != 0).then_some(value)
    }

    pub fn set_label_position(&self, code: u32) -> bool {
        // SAFETY: the handle is live for the page borrow.
        unsafe { pdfium_sys::EPDFNumberFormat_SetLabelPosition(self.handle, code) != 0 }
    }

    /// One of `/RT /RD /PS /SS`: `None` when absent, `Some("")` when
    /// explicitly empty.
    pub fn text(&self, kind: MeasureText) -> Result<Option<String>, PdfiumError> {
        // SAFETY: the buffer and its length come from `read_text`.
        read_text(|buf, len| unsafe {
            pdfium_sys::EPDFNumberFormat_GetText(self.handle, kind as u32, buf, len)
        })
    }

    pub fn set_text(&self, kind: MeasureText, text: &str) -> bool {
        let text = wide(text);
        // SAFETY: `text` is NUL-terminated and outlives the call.
        unsafe {
            pdfium_sys::EPDFNumberFormat_SetText(self.handle, kind as u32, text.as_ptr()) != 0
        }
    }
}
