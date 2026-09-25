//! Writing a form field's value.
//!
//! G12. The write half of [`crate::form`], and the one place where a write
//! reports back which ANNOTATIONS it changed.
//!
//! # Why every value write hands back a widget list
//!
//! A field and its widgets are different objects. Setting a text field's
//! value rewrites one dictionary and re-bakes an appearance on each widget
//! that displays it, and a radio group's widgets change in pairs: the one
//! turning on and the one turning off. A caller holding rasters keyed by
//! annotation cannot work out that set from the value alone, so the fork
//! reports it and this layer passes it through.
//!
//! The buffer is fixed and the reported count is not clamped to it. PDFium
//! writes what fits and says how many there were, so a count larger than the
//! capacity means the list was truncated rather than that the call failed.

use std::ffi::CString;

use crate::{Document, PdfiumError};

/// How many changed widgets one write can report.
///
/// Far above any real form: a field with more than this many widgets would
/// be a single control repeated on 256 pages.
const CHANGED_WIDGETS_CAPACITY: usize = 256;

/// What a value write changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedWidgets {
    /// The object numbers PDFium reported, up to the capacity.
    pub object_numbers: Vec<u32>,
    /// How many it said there were, which may exceed what it wrote.
    pub total: usize,
}

impl ChangedWidgets {
    /// Whether the list is the whole truth.
    pub fn is_complete(&self) -> bool {
        self.total == self.object_numbers.len()
    }
}

/// What an import applied and what it did not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FormImportCounts {
    /// Fields the payload named.
    pub fields_total: u32,
    /// Fields it named that the document has and that took the value.
    pub fields_applied: u32,
    /// Fields it named that the document does not have, or that refused.
    pub fields_skipped: u32,
    pub widgets_changed: u32,
}

/// What a repair pass found and fixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FormRepairReport {
    /// The document had no `/AcroForm` and now has one.
    pub acroform_created: bool,
    /// Fields found by sweeping the pages and linked into `/Fields`.
    pub fields_linked: u32,
    /// Widgets attached to the field they name through `/Parent`.
    pub widgets_linked: u32,
    /// Fields a sweep found and could not attach to anything.
    pub fields_unrepairable: u32,
    pub appearances_baked: u32,
    /// `/NeedAppearances` was set and has been cleared, which it can be
    /// once every widget has a stream of its own.
    pub need_appearances_cleared: bool,
}

impl Document {
    /// Apply an FDF payload.
    pub fn import_fdf(&self, bytes: &[u8]) -> Option<FormImportCounts> {
        // SAFETY: the handle is live and the buffer outlives the call.
        self.import_with(|result| unsafe {
            pdfium_sys::EPDFForm_ImportFDF(
                self.as_raw(),
                bytes.as_ptr().cast(),
                bytes.len() as std::os::raw::c_ulong,
                result,
            )
        })
    }

    /// Apply an XFDF payload.
    pub fn import_xfdf(&self, bytes: &[u8]) -> Option<FormImportCounts> {
        // SAFETY: as above.
        self.import_with(|result| unsafe {
            pdfium_sys::EPDFForm_ImportXFDF(
                self.as_raw(),
                bytes.as_ptr().cast(),
                bytes.len() as std::os::raw::c_ulong,
                result,
            )
        })
    }

    fn import_with(
        &self,
        call: impl FnOnce(*mut pdfium_sys::EPDF_FORM_IMPORT_RESULT) -> i32,
    ) -> Option<FormImportCounts> {
        let mut result = pdfium_sys::EPDF_FORM_IMPORT_RESULT::default();
        if call(&raw mut result) == 0 {
            return None;
        }
        Some(FormImportCounts {
            fields_total: result.fields_total,
            fields_applied: result.fields_applied,
            fields_skipped: result.fields_skipped,
            widgets_changed: result.widgets_changed,
        })
    }

    /// Relink a form whose `/AcroForm` and pages disagree.
    ///
    /// `bake_appearances` also draws a stream for every widget, which is
    /// what lets `/NeedAppearances` be cleared: the flag asks a viewer to
    /// draw the widgets itself, and once they all have streams it is a
    /// request nobody needs to honour.
    pub fn repair_form(&self, bake_appearances: bool) -> Option<FormRepairReport> {
        /// `EPDF_FORM_REPAIR_BAKE_APPEARANCES`.
        const BAKE_APPEARANCES: u32 = 0x1;

        let mut report = pdfium_sys::EPDF_FORM_REPAIR_REPORT::default();
        let flags = if bake_appearances { BAKE_APPEARANCES } else { 0 };
        // SAFETY: the handle is live and `report` is a valid out-parameter.
        let ok = unsafe { pdfium_sys::EPDFForm_Repair(self.as_raw(), flags, &raw mut report) };
        if ok == 0 {
            return None;
        }
        Some(FormRepairReport {
            acroform_created: report.acroform_created != 0,
            fields_linked: report.fields_linked,
            widgets_linked: report.widgets_linked,
            fields_unrepairable: report.fields_unrepairable,
            appearances_baked: report.appearances_baked,
            need_appearances_cleared: report.need_appearances_cleared != 0,
        })
    }

    /// `/V` on a text field.
    pub fn set_field_text_value(
        &self,
        field_object_number: u32,
        value: &str,
    ) -> Option<ChangedWidgets> {
        let value: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: the handle is live and the string outlives the call.
        self.with_changed_widgets(|buffer, capacity, count| unsafe {
            pdfium_sys::EPDFForm_SetTextValue(
                self.as_raw(),
                field_object_number,
                value.as_ptr(),
                buffer,
                capacity,
                count,
            )
        })
    }

    /// `/V` on a checkbox or a radio group.
    ///
    /// An EMPTY state clears the group, which is the C API's null and is why
    /// this takes a `&str` rather than an `Option`: "off" is a state a file
    /// can name, and the empty string is the absence of one.
    pub fn set_field_toggle(
        &self,
        field_object_number: u32,
        on_state: &str,
    ) -> Result<Option<ChangedWidgets>, PdfiumError> {
        let state =
            CString::new(on_state).map_err(|_| PdfiumError::InvalidArgument("toggle state"))?;
        // SAFETY: the handle is live and the string outlives the call.
        Ok(self.with_changed_widgets(|buffer, capacity, count| unsafe {
            pdfium_sys::EPDFForm_SetToggle(
                self.as_raw(),
                field_object_number,
                state.as_ptr(),
                buffer,
                capacity,
                count,
            )
        }))
    }

    /// `/V` on a combo box or a list box.
    ///
    /// A list takes several; a combo box takes one. Which is legal is the
    /// field's business and PDFium's, not this layer's.
    pub fn set_field_choice_values(
        &self,
        field_object_number: u32,
        values: &[String],
    ) -> Option<ChangedWidgets> {
        let encoded: Vec<Vec<u16>> = values
            .iter()
            .map(|value| value.encode_utf16().chain(std::iter::once(0)).collect())
            .collect();
        let pointers: Vec<*const u16> = encoded.iter().map(|value| value.as_ptr()).collect();
        // SAFETY: the handle is live, and both the strings and the pointer
        // array outlive the call.
        self.with_changed_widgets(|buffer, capacity, count| unsafe {
            pdfium_sys::EPDFForm_SetChoiceValues(
                self.as_raw(),
                field_object_number,
                pointers.as_ptr().cast(),
                pointers.len() as std::os::raw::c_ulong,
                buffer,
                capacity,
                count,
            )
        })
    }

    /// Put a field back to its `/DV`.
    ///
    /// Not the same as writing an empty value: a field with a default goes
    /// back to it, and one without goes empty. The distinction belongs to
    /// PDFium because `/DV` is where it lives.
    pub fn reset_field(&self, field_object_number: u32) -> Option<ChangedWidgets> {
        // SAFETY: the handle is live.
        self.with_changed_widgets(|buffer, capacity, count| unsafe {
            pdfium_sys::EPDFForm_ResetField(
                self.as_raw(),
                field_object_number,
                buffer,
                capacity,
                count,
            )
        })
    }

    /// Run a write with the changed-widgets out-parameters wired up.
    ///
    /// `None` is PDFium refusing the write, which it does for a value the
    /// field cannot hold: an unknown toggle state, a string past `/MaxLen`,
    /// a choice that is not one of the options.
    fn with_changed_widgets(
        &self,
        call: impl FnOnce(*mut u32, std::os::raw::c_ulong, *mut std::os::raw::c_ulong) -> i32,
    ) -> Option<ChangedWidgets> {
        let mut buffer = [0u32; CHANGED_WIDGETS_CAPACITY];
        let mut count: std::os::raw::c_ulong = 0;
        let ok = call(
            buffer.as_mut_ptr(),
            CHANGED_WIDGETS_CAPACITY as std::os::raw::c_ulong,
            &raw mut count,
        );
        if ok == 0 {
            return None;
        }
        let total = count as usize;
        Some(ChangedWidgets {
            object_numbers: buffer[..total.min(CHANGED_WIDGETS_CAPACITY)].to_vec(),
            total,
        })
    }
}
