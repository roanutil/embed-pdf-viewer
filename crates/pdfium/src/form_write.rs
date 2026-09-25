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

impl Document {
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
