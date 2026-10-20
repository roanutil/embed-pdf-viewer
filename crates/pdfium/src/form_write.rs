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

/// Which kind of control `create_field` makes.
///
/// `EPDF_FORMFIELD_FAMILY_*`. Unknown is missing deliberately: the fork's
/// creator does not make it, and a family this layer could name but not
/// create would be a promise the call cannot keep. At this pin the creator
/// makes push buttons too (their caption is the widgets' `/MK /CA`), and
/// signatures, created unsigned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreatableFieldFamily {
    Pushbutton = 1,
    Checkbox = 2,
    Radio = 3,
    Text = 4,
    Combobox = 5,
    Listbox = 6,
    Signature = 7,
}

/// What a field deletion detached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetachedWidgets {
    pub object_numbers: Vec<u32>,
    pub total: usize,
}

/// What a value writer writes, in the shapes `EPDFForm_GetFieldValueKind`
/// reads.
///
/// `EPDF_FORM_VALUE_NONE`, `_SCALAR` and `_ARRAY`. The writers decide
/// nothing: no cut to `/MaxLen`, no sibling widgets, no option order. The
/// caller works out what the field holds and writes exactly that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldValueWrite<'a> {
    /// The field holds no value of its own. The key is removed or, when a
    /// parent field holds one, an empty value shadows it.
    None,
    /// One value: a text string, or a toggle's state name as UTF-8.
    Scalar(&'a str),
    /// Any number of values: a multi-select list box only.
    Array(&'a [String]),
}

impl FieldValueWrite<'_> {
    /// `EPDF_FORM_VALUE_*`.
    fn kind(self) -> i32 {
        match self {
            Self::None => pdfium_sys::EPDF_FORM_VALUE_NONE as i32,
            Self::Scalar(_) => pdfium_sys::EPDF_FORM_VALUE_SCALAR as i32,
            Self::Array(_) => pdfium_sys::EPDF_FORM_VALUE_ARRAY as i32,
        }
    }

    /// The values as NUL-terminated UTF-16, one per entry.
    fn encode(self) -> Vec<Vec<u16>> {
        let encode = |value: &str| value.encode_utf16().chain(std::iter::once(0)).collect();
        match self {
            Self::None => Vec::new(),
            Self::Scalar(value) => vec![encode(value)],
            Self::Array(values) => values.iter().map(|value| encode(value)).collect(),
        }
    }
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

    /// Make a field, and return its object number.
    ///
    /// Zero is a refusal: a name that collides with a sibling, or one a
    /// field cannot have. The call makes the FIELD only; its widgets are
    /// born through the annotation plane and adopted with
    /// [`Document::attach_widget`].
    pub fn create_field(&self, family: CreatableFieldFamily, full_name: &str) -> Option<u32> {
        let name: Vec<u16> = full_name.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: the handle is live and the name outlives the call.
        let object_number = unsafe {
            // Zero is "the next free object number"; a chosen number needs a
            // layer transaction, which this layer does not open.
            pdfium_sys::EPDFForm_CreateField(self.as_raw(), family as i32, name.as_ptr(), 0)
        };
        (object_number > 0).then_some(object_number)
    }

    /// Adopt an existing widget annotation into a field.
    ///
    /// `on_state` names the appearance a toggle turns on with, and is empty
    /// for the families that have none.
    pub fn attach_widget(
        &self,
        field_object_number: u32,
        widget_object_number: u32,
        on_state: &str,
    ) -> Result<bool, PdfiumError> {
        let state = CString::new(on_state).map_err(|_| PdfiumError::InvalidArgument("on state"))?;
        // SAFETY: the handle is live and the string outlives the call.
        Ok(unsafe {
            pdfium_sys::EPDFForm_AttachWidget(
                self.as_raw(),
                field_object_number,
                widget_object_number,
                state.as_ptr(),
                // Zero: a merged field split by the adoption gives its old
                // widget the next free object number.
                0,
            ) != 0
        })
    }

    /// Release a widget from its field, leaving it an inert annotation.
    pub fn detach_widget(&self, field_object_number: u32, widget_object_number: u32) -> bool {
        // SAFETY: the handle is live.
        unsafe {
            pdfium_sys::EPDFForm_DetachWidget(
                self.as_raw(),
                field_object_number,
                widget_object_number,
            ) != 0
        }
    }

    /// Delete a terminal field, detaching its widgets.
    ///
    /// The widgets survive as inert annotations, which is why their object
    /// numbers come back: removing them from their pages is the annotation
    /// plane's job, and a caller that did not know which they were could
    /// not ask for it.
    pub fn delete_field(&self, field_object_number: u32) -> Option<DetachedWidgets> {
        let mut buffer = [0u32; CHANGED_WIDGETS_CAPACITY];
        let mut count: std::os::raw::c_ulong = 0;
        // SAFETY: the handle is live and both out-parameters are valid.
        let ok = unsafe {
            pdfium_sys::EPDFForm_DeleteField(
                self.as_raw(),
                field_object_number,
                buffer.as_mut_ptr(),
                CHANGED_WIDGETS_CAPACITY as std::os::raw::c_ulong,
                &raw mut count,
            )
        };
        if ok == 0 {
            return None;
        }
        let total = count as usize;
        Some(DetachedWidgets {
            object_numbers: buffer[..total.min(CHANGED_WIDGETS_CAPACITY)].to_vec(),
            total,
        })
    }

    /// `/T`, the partial name.
    pub fn set_field_name(&self, field_object_number: u32, name: &str) -> bool {
        self.wide_field_setter(pdfium_sys::EPDFForm_SetFieldName, field_object_number, name)
    }

    /// `/TU`, the accessible label.
    pub fn set_field_alternate_name(&self, field_object_number: u32, name: &str) -> bool {
        self.wide_field_setter(
            pdfium_sys::EPDFForm_SetFieldAlternateName,
            field_object_number,
            name,
        )
    }

    /// `/TM`, the export name.
    pub fn set_field_mapping_name(&self, field_object_number: u32, name: &str) -> bool {
        self.wide_field_setter(pdfium_sys::EPDFForm_SetFieldMappingName, field_object_number, name)
    }

    fn wide_field_setter(
        &self,
        setter: unsafe extern "C" fn(
            pdfium_sys::FPDF_DOCUMENT,
            u32,
            pdfium_sys::FPDF_WIDESTRING,
        ) -> i32,
        field_object_number: u32,
        value: &str,
    ) -> bool {
        let value: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: the handle is live and the string outlives the call.
        unsafe { setter(self.as_raw(), field_object_number, value.as_ptr()) != 0 }
    }

    /// `/Ff`, as bits to set and bits to clear.
    ///
    /// Two masks rather than a word, because a field's flags include bits
    /// this build does not model and writing a whole word would drop them.
    pub fn set_field_flags(&self, field_object_number: u32, set: u32, clear: u32) -> bool {
        // SAFETY: the handle is live.
        unsafe {
            pdfium_sys::EPDFForm_SetFieldFlags(self.as_raw(), field_object_number, set, clear) != 0
        }
    }

    /// `/MaxLen`.
    pub fn set_field_max_length(&self, field_object_number: u32, max_length: i32) -> bool {
        // SAFETY: the handle is live.
        unsafe {
            pdfium_sys::EPDFForm_SetFieldMaxLen(self.as_raw(), field_object_number, max_length) != 0
        }
    }

    /// `/Opt`, the label and export value of each choice.
    pub fn set_field_options(
        &self,
        field_object_number: u32,
        labels: &[String],
        exports: &[String],
    ) -> bool {
        let encode = |values: &[String]| -> Vec<Vec<u16>> {
            values
                .iter()
                .map(|value| value.encode_utf16().chain(std::iter::once(0)).collect())
                .collect()
        };
        let labels = encode(labels);
        let exports = encode(exports);
        let label_pointers: Vec<*const u16> = labels.iter().map(|v| v.as_ptr()).collect();
        let export_pointers: Vec<*const u16> = exports.iter().map(|v| v.as_ptr()).collect();
        // SAFETY: the handle is live, both arrays are as long as the count,
        // and every string outlives the call.
        unsafe {
            pdfium_sys::EPDFForm_SetFieldOptions(
                self.as_raw(),
                field_object_number,
                label_pointers.as_ptr().cast(),
                export_pointers.as_ptr().cast(),
                label_pointers.len().min(export_pointers.len()) as std::os::raw::c_ulong,
            ) != 0
        }
    }

    /// `/V`, written as given.
    ///
    /// Drops `/RV` and copies `/V` onto the field's twin controls in other
    /// planes. Draws nothing: [`Document::redraw_field_widgets`] makes a text
    /// or choice field's new picture, and [`Document::set_field_widgets_checked`]
    /// is a toggle's. Refused for a field that is not terminal, or an
    /// [`FieldValueWrite::Array`] on anything but a list box.
    pub fn set_field_value(&self, field_object_number: u32, value: FieldValueWrite<'_>) -> bool {
        self.write_field_value(pdfium_sys::EPDFForm_SetFieldValue, field_object_number, value)
    }

    /// `/DV`, written the way [`Document::set_field_value`] writes `/V`.
    ///
    /// [`FieldValueWrite::None`] is how a default is removed now; the
    /// dedicated remove call is gone from the fork.
    pub fn set_field_default_value(
        &self,
        field_object_number: u32,
        value: FieldValueWrite<'_>,
    ) -> bool {
        self.write_field_value(
            pdfium_sys::EPDFForm_SetFieldDefaultValue,
            field_object_number,
            value,
        )
    }

    fn write_field_value(
        &self,
        writer: unsafe extern "C" fn(
            pdfium_sys::FPDF_DOCUMENT,
            u32,
            i32,
            *const pdfium_sys::FPDF_WIDESTRING,
            std::os::raw::c_ulong,
        ) -> i32,
        field_object_number: u32,
        value: FieldValueWrite<'_>,
    ) -> bool {
        let encoded = value.encode();
        let pointers: Vec<pdfium_sys::FPDF_WIDESTRING> =
            encoded.iter().map(|value| value.as_ptr()).collect();
        let values = if pointers.is_empty() { std::ptr::null() } else { pointers.as_ptr() };
        // SAFETY: the handle is live, the pointer array is as long as the
        // count, and both it and every string outlive the call.
        unsafe {
            writer(
                self.as_raw(),
                field_object_number,
                value.kind(),
                values,
                pointers.len() as std::os::raw::c_ulong,
            ) != 0
        }
    }

    /// `/I` on a combo box or list box: the selected options' positions in
    /// `/Opt`, as given. Empty says none.
    pub fn set_field_selected_indices(&self, field_object_number: u32, indices: &[i32]) -> bool {
        let pointer = if indices.is_empty() { std::ptr::null() } else { indices.as_ptr() };
        // SAFETY: the handle is live and the slice outlives the call.
        unsafe {
            pdfium_sys::EPDFForm_SetFieldSelectedIndices(
                self.as_raw(),
                field_object_number,
                pointer,
                indices.len() as std::os::raw::c_ulong,
            ) != 0
        }
    }

    /// Turn a checkbox's or radio group's widgets on or off, one entry per
    /// widget in the model's widget order.
    ///
    /// The visual half of a toggle write: a widget on gets its own on-state
    /// as `/AS`, a widget off gets `/Off`. `/V` is
    /// [`Document::set_field_value`]'s to write. Refused when the field is
    /// not a toggle or the slice is not its widget count.
    pub fn set_field_widgets_checked(
        &self,
        field_object_number: u32,
        checked: &[bool],
    ) -> Option<ChangedWidgets> {
        let checked: Vec<pdfium_sys::FPDF_BOOL> = checked.iter().map(|&on| i32::from(on)).collect();
        let pointer = if checked.is_empty() { std::ptr::null() } else { checked.as_ptr() };
        // SAFETY: the handle is live and the array outlives the call.
        self.with_changed_widgets(|buffer, capacity, count| unsafe {
            pdfium_sys::EPDFForm_SetFieldWidgetsChecked(
                self.as_raw(),
                field_object_number,
                pointer,
                checked.len() as std::os::raw::c_ulong,
                buffer,
                capacity,
                count,
            )
        })
    }

    /// Draw a new `/AP` for every widget of a text field, combo box or list
    /// box from what the field holds, and report them all as changed.
    pub fn redraw_field_widgets(&self, field_object_number: u32) -> Option<ChangedWidgets> {
        // SAFETY: the handle is live.
        self.with_changed_widgets(|buffer, capacity, count| unsafe {
            pdfium_sys::EPDFForm_RedrawFieldWidgets(
                self.as_raw(),
                field_object_number,
                buffer,
                capacity,
                count,
            )
        })
    }

    /// `Field.display` on every widget of a terminal field.
    ///
    /// Acrobat's four-valued property rather than the PDF flags underneath
    /// it: the fork changes only the Invisible, Hidden, Print and NoView
    /// bits and leaves every unrelated flag alone, which is what makes this
    /// safe to drive from a form script that knows nothing about `/F`.
    pub fn set_field_display(
        &self,
        field_object_number: u32,
        display: FieldDisplay,
    ) -> Option<ChangedWidgets> {
        // SAFETY: the handle is live.
        self.with_changed_widgets(|buffer, capacity, count| unsafe {
            pdfium_sys::EPDFForm_SetFieldDisplay(
                self.as_raw(),
                field_object_number,
                display as i32,
                buffer,
                capacity,
                count,
            )
        })
    }

    /// Regenerate every text and combo widget's `/AP` from this text.
    ///
    /// The field's `/V` is UNCHANGED. This is the sink for an `/AA /F`
    /// format script, whose whole job is to make a value look different
    /// without making it a different value: a number stored as `1234.5` and
    /// displayed as `$1,234.50` is one field, not two.
    pub fn set_field_appearance_text(
        &self,
        field_object_number: u32,
        text: &str,
    ) -> Option<ChangedWidgets> {
        let text: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: the handle is live and the string outlives the call.
        self.with_changed_widgets(|buffer, capacity, count| unsafe {
            pdfium_sys::EPDFForm_SetFieldAppearanceText(
                self.as_raw(),
                field_object_number,
                text.as_ptr(),
                buffer,
                capacity,
                count,
            )
        })
    }

    /// Run a write with the changed-widgets out-parameters wired up.
    ///
    /// `None` is PDFium refusing the write: a field of the wrong family, or
    /// an argument that does not fit it.
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

/// Acrobat's `Field.display`.
///
/// Four values rather than the two annotation flag bits they touch, because
/// that is the vocabulary a form script speaks: `EPDF_FORM_DISPLAY_*` in
/// `epdf_form.h`, and `FormFieldDisplay` in `core/src/forms/effects.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldDisplay {
    Visible = 0,
    Hidden = 1,
    NoPrint = 2,
    NoView = 3,
}
