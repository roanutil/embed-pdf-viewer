//! The form model: a detached snapshot of a document's field tree.
//!
//! G8. Wraps `EPDFForm_*`.
//!
//! `EPDFForm_LoadModel` walks the `/AcroForm` field tree and sweeps every
//! page's `/Annots` to attach widgets, which is why the result is a SNAPSHOT
//! rather than a live view: it is correct forever and stale after any
//! mutation. This layer owns the handle and closes it on drop; deciding when
//! a snapshot has gone stale is the engine's, because only the engine knows
//! what counts as a mutation.
//!
//! Field and widget positions are indices into this model, not addresses.
//! They are stable for as long as one model lives and mean nothing across
//! two, which is what [`FormModel::field_index_by_object_number`] and
//! [`FormModel::field_index_for_widget`] exist to bridge.

use std::marker::PhantomData;

use crate::{Document, PdfiumError};

/// A loaded form model, closed on drop.
#[derive(Debug)]
pub struct FormModel<'doc> {
    handle: pdfium_sys::EPDF_FORM_MODEL,
    document: &'doc Document,
    _not_send: PhantomData<*const ()>,
}

/// Which of a field's four JavaScript events to read.
///
/// `FPDF_ANNOT_AACTION_*` minus its offset: the fork's model numbers them
/// from zero while `fpdf_annot.h` names them 12 through 15.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldActionEvent {
    /// `/K`, on keystroke.
    Keystroke = 0,
    /// `/F`, on format.
    Format = 1,
    /// `/V`, on validate.
    Validate = 2,
    /// `/C`, on calculate.
    Calculate = 3,
}

/// Which serialization `export` produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormDataFormat {
    Fdf,
    Xfdf,
}

impl Document {
    /// Build a form model.
    ///
    /// `None` when PDFium refuses, which for a document with no `/AcroForm`
    /// it does not: an empty model is a model, and it reports a form kind of
    /// none and no fields.
    pub fn form_model(&self) -> Option<FormModel<'_>> {
        // SAFETY: the handle is live for the borrow this returns.
        let handle = unsafe { pdfium_sys::EPDFForm_LoadModel(self.as_raw()) };
        (!handle.is_null()).then_some(FormModel { handle, document: self, _not_send: PhantomData })
    }

    /// Serialize the document's form data.
    ///
    /// Off the DOCUMENT rather than off a model, because it reads the
    /// reconciled view straight out of the file: recovered fields are
    /// included and, on a layer, promoted values win. A model would be a
    /// second, older answer.
    pub fn export_form_data(&self, format: FormDataFormat) -> Result<Vec<u8>, PdfiumError> {
        let call = |buffer: *mut std::ffi::c_void, len: std::os::raw::c_ulong| {
            // SAFETY: the handle is live; a null path means "this document"
            // and zero flags is the default export.
            unsafe {
                match format {
                    FormDataFormat::Fdf => pdfium_sys::EPDFForm_ExportFDF(
                        self.as_raw(),
                        std::ptr::null(),
                        0,
                        buffer,
                        len,
                    ),
                    FormDataFormat::Xfdf => pdfium_sys::EPDFForm_ExportXFDF(
                        self.as_raw(),
                        std::ptr::null(),
                        0,
                        buffer,
                        len,
                    ),
                }
            }
        };

        let length = call(std::ptr::null_mut(), 0);
        if length == 0 {
            return Err(PdfiumError::InvalidArgument("form export produced nothing"));
        }
        let mut bytes: Vec<u8> = Vec::new();
        bytes.try_reserve_exact(length as usize).map_err(|_| PdfiumError::OutOfMemory)?;
        bytes.resize(length as usize, 0u8);

        let written = call(bytes.as_mut_ptr().cast(), length);
        if written != length {
            return Err(PdfiumError::InvalidArgument("form export changed length mid-call"));
        }
        Ok(bytes)
    }
}

impl Drop for FormModel<'_> {
    fn drop(&mut self) {
        // SAFETY: the handle came from EPDFForm_LoadModel and is closed once.
        unsafe { pdfium_sys::EPDFForm_CloseModel(self.handle) };
    }
}

impl FormModel<'_> {
    /// The raw `EPDF_FORMKIND_*`: 0 none, 1 acroform, 2 xfa.
    pub fn form_kind(&self) -> i32 {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::EPDFForm_GetFormKind(self.handle) }
    }

    /// `/NeedAppearances`.
    pub fn need_appearances(&self) -> bool {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::EPDFForm_GetNeedAppearances(self.handle) != 0 }
    }

    pub fn field_count(&self) -> u32 {
        // SAFETY: the handle is live.
        u32::try_from(unsafe { pdfium_sys::EPDFForm_CountFields(self.handle) }).unwrap_or(0)
    }

    /// How many entries `/CO` has.
    pub fn calculation_order_count(&self) -> u32 {
        // SAFETY: the handle is live.
        u32::try_from(unsafe { pdfium_sys::EPDFForm_CountCalculationOrder(self.handle) })
            .unwrap_or(0)
    }

    /// The field index at one `/CO` position.
    ///
    /// Raw, including a negative, because a `/CO` entry naming a field the
    /// model does not have is a fact about the document that the engine
    /// reports rather than hides.
    pub fn calculation_order_field(&self, order_index: u32) -> i32 {
        let Ok(order_index) = i32::try_from(order_index) else {
            return -1;
        };
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::EPDFForm_GetCalculationOrderFieldIndex(self.handle, order_index) }
    }

    /// The field index for a widget's object number, or `None` when the
    /// widget belongs to no field.
    pub fn field_index_for_widget(&self, widget_object_number: u32) -> Option<u32> {
        // SAFETY: the handle is live.
        let index = unsafe {
            pdfium_sys::EPDFForm_GetFieldIndexForWidget(self.handle, widget_object_number)
        };
        u32::try_from(index).ok()
    }

    /// The field index for a field's own object number.
    pub fn field_index_by_object_number(&self, field_object_number: u32) -> Option<u32> {
        // SAFETY: the handle is live.
        let index =
            unsafe { pdfium_sys::EPDFForm_GetFieldIndexByObjNum(self.handle, field_object_number) };
        u32::try_from(index).ok()
    }

    /// One field, by index.
    ///
    /// `None` past the end. The borrow keeps the model alive, so a field
    /// cannot outlive the snapshot its indices refer to.
    pub fn field(&self, index: u32) -> Option<FormField<'_>> {
        (index < self.field_count()).then_some(FormField { model: self, index: index as i32 })
    }

    /// The action model for one of a field's events.
    pub fn field_action_model(
        &self,
        field_index: u32,
        event: FieldActionEvent,
    ) -> Option<crate::ActionModel<'_>> {
        let Ok(field_index) = i32::try_from(field_index) else {
            return None;
        };
        // SAFETY: the handle is live for the borrow this returns, and the
        // action model borrows the DOCUMENT, which outlives this model.
        let handle = unsafe {
            pdfium_sys::EPDFForm_GetFieldActionModel(self.handle, field_index, event as i32)
        };
        crate::action::action_model_from_raw(handle, self.document)
    }
}

/// One field of a loaded model.
#[derive(Debug)]
pub struct FormField<'model> {
    model: &'model FormModel<'model>,
    index: i32,
}

impl FormField<'_> {
    /// The field dictionary's object number. Zero for a field the document
    /// stored as a direct object, which has no durable address.
    pub fn object_number(&self) -> u32 {
        // SAFETY: the model handle is live and the index is in range.
        unsafe { pdfium_sys::EPDFForm_GetFieldObjNum(self.model.handle, self.index) }
    }

    /// The fully qualified name.
    pub fn name(&self) -> Result<Option<String>, PdfiumError> {
        read_utf16(|buffer, len| {
            // SAFETY: as above, and `len` describes `buffer` in bytes.
            unsafe { pdfium_sys::EPDFForm_GetFieldName(self.model.handle, self.index, buffer, len) }
        })
    }

    /// `/TU`, the alternate name a viewer shows as a tooltip.
    pub fn alternate_name(&self) -> Result<Option<String>, PdfiumError> {
        read_utf16(|buffer, len| {
            // SAFETY: as above.
            unsafe {
                pdfium_sys::EPDFForm_GetFieldAlternateName(
                    self.model.handle,
                    self.index,
                    buffer,
                    len,
                )
            }
        })
    }

    /// `/TM`, the name an export uses instead of the field's own.
    pub fn mapping_name(&self) -> Result<Option<String>, PdfiumError> {
        read_utf16(|buffer, len| {
            // SAFETY: as above.
            unsafe {
                pdfium_sys::EPDFForm_GetFieldMappingName(self.model.handle, self.index, buffer, len)
            }
        })
    }

    /// The raw `/Ff` word.
    pub fn flags(&self) -> u32 {
        // SAFETY: as above.
        unsafe { pdfium_sys::EPDFForm_GetFieldFlags(self.model.handle, self.index) }
    }

    /// The raw `EPDF_FORMFIELD_FAMILY_*` code.
    pub fn family_code(&self) -> i32 {
        // SAFETY: as above.
        unsafe { pdfium_sys::EPDFForm_GetFieldFamily(self.model.handle, self.index) }
    }

    /// Where the field came from: 0 the `/AcroForm` tree, 1 recovered from a
    /// page's widgets when the tree did not list it.
    pub fn origin_code(&self) -> i32 {
        // SAFETY: as above.
        unsafe { pdfium_sys::EPDFForm_GetFieldOrigin(self.model.handle, self.index) }
    }

    /// `/MaxLen`, or a non-positive value when the field has none.
    pub fn max_length(&self) -> i32 {
        // SAFETY: as above.
        unsafe { pdfium_sys::EPDFForm_GetFieldMaxLen(self.model.handle, self.index) }
    }

    /// The shape of `/V`: 0 absent, 1 scalar, 2 array, 3 something else.
    pub fn value_kind(&self) -> i32 {
        // SAFETY: as above.
        unsafe { pdfium_sys::EPDFForm_GetFieldValueKind(self.model.handle, self.index) }
    }

    /// The same for `/DV`.
    pub fn default_value_kind(&self) -> i32 {
        // SAFETY: as above.
        unsafe { pdfium_sys::EPDFForm_GetFieldDefaultValueKind(self.model.handle, self.index) }
    }

    pub fn value_count(&self) -> u32 {
        // SAFETY: as above.
        u32::try_from(unsafe {
            pdfium_sys::EPDFForm_CountFieldValues(self.model.handle, self.index)
        })
        .unwrap_or(0)
    }

    pub fn default_value_count(&self) -> u32 {
        // SAFETY: as above.
        u32::try_from(unsafe {
            pdfium_sys::EPDFForm_CountFieldDefaultValues(self.model.handle, self.index)
        })
        .unwrap_or(0)
    }

    pub fn value_at(&self, value_index: u32) -> Result<Option<String>, PdfiumError> {
        let Ok(value_index) = i32::try_from(value_index) else {
            return Ok(None);
        };
        read_utf16(|buffer, len| {
            // SAFETY: as above.
            unsafe {
                pdfium_sys::EPDFForm_GetFieldValueAt(
                    self.model.handle,
                    self.index,
                    value_index,
                    buffer,
                    len,
                )
            }
        })
    }

    pub fn default_value_at(&self, value_index: u32) -> Result<Option<String>, PdfiumError> {
        let Ok(value_index) = i32::try_from(value_index) else {
            return Ok(None);
        };
        read_utf16(|buffer, len| {
            // SAFETY: as above.
            unsafe {
                pdfium_sys::EPDFForm_GetFieldDefaultValueAt(
                    self.model.handle,
                    self.index,
                    value_index,
                    buffer,
                    len,
                )
            }
        })
    }

    pub fn widget_count(&self) -> u32 {
        // SAFETY: as above.
        u32::try_from(unsafe {
            pdfium_sys::EPDFForm_CountFieldWidgets(self.model.handle, self.index)
        })
        .unwrap_or(0)
    }

    pub fn widget_object_number(&self, widget_index: u32) -> u32 {
        let Ok(widget_index) = i32::try_from(widget_index) else {
            return 0;
        };
        // SAFETY: as above.
        unsafe {
            pdfium_sys::EPDFForm_GetFieldWidgetObjNum(self.model.handle, self.index, widget_index)
        }
    }

    pub fn widget_page_object_number(&self, widget_index: u32) -> u32 {
        let Ok(widget_index) = i32::try_from(widget_index) else {
            return 0;
        };
        // SAFETY: as above.
        unsafe {
            pdfium_sys::EPDFForm_GetFieldWidgetPageObjNum(
                self.model.handle,
                self.index,
                widget_index,
            )
        }
    }

    /// The widget's `/AP /N` on-state name.
    ///
    /// UTF-8, not UTF-16, and it is the one string in this module that is: an
    /// on-state is a PDF NAME (`/Yes`, `/Off`, `/Choice1`) rather than a text
    /// string, and names are byte strings.
    pub fn widget_on_state(&self, widget_index: u32) -> Result<Option<String>, PdfiumError> {
        let Ok(widget_index) = i32::try_from(widget_index) else {
            return Ok(None);
        };
        read_utf8(|buffer, len| {
            // SAFETY: as above.
            unsafe {
                pdfium_sys::EPDFForm_GetFieldWidgetOnState(
                    self.model.handle,
                    self.index,
                    widget_index,
                    buffer,
                    len,
                )
            }
        })
    }

    pub fn widget_export_value(&self, widget_index: u32) -> Result<Option<String>, PdfiumError> {
        let Ok(widget_index) = i32::try_from(widget_index) else {
            return Ok(None);
        };
        read_utf16(|buffer, len| {
            // SAFETY: as above.
            unsafe {
                pdfium_sys::EPDFForm_GetFieldWidgetExportValue(
                    self.model.handle,
                    self.index,
                    widget_index,
                    buffer,
                    len,
                )
            }
        })
    }

    pub fn widget_is_checked(&self, widget_index: u32) -> bool {
        let Ok(widget_index) = i32::try_from(widget_index) else {
            return false;
        };
        // SAFETY: as above.
        unsafe {
            pdfium_sys::EPDFForm_IsFieldWidgetChecked(self.model.handle, self.index, widget_index)
                != 0
        }
    }

    pub fn option_count(&self) -> u32 {
        // SAFETY: as above.
        u32::try_from(unsafe {
            pdfium_sys::EPDFForm_CountFieldOptions(self.model.handle, self.index)
        })
        .unwrap_or(0)
    }

    pub fn option_label(&self, option_index: u32) -> Result<Option<String>, PdfiumError> {
        let Ok(option_index) = i32::try_from(option_index) else {
            return Ok(None);
        };
        read_utf16(|buffer, len| {
            // SAFETY: as above.
            unsafe {
                pdfium_sys::EPDFForm_GetFieldOptionLabel(
                    self.model.handle,
                    self.index,
                    option_index,
                    buffer,
                    len,
                )
            }
        })
    }

    pub fn option_value(&self, option_index: u32) -> Result<Option<String>, PdfiumError> {
        let Ok(option_index) = i32::try_from(option_index) else {
            return Ok(None);
        };
        read_utf16(|buffer, len| {
            // SAFETY: as above.
            unsafe {
                pdfium_sys::EPDFForm_GetFieldOptionValue(
                    self.model.handle,
                    self.index,
                    option_index,
                    buffer,
                    len,
                )
            }
        })
    }

    pub fn option_is_selected(&self, option_index: u32) -> bool {
        let Ok(option_index) = i32::try_from(option_index) else {
            return false;
        };
        // SAFETY: as above.
        unsafe {
            pdfium_sys::EPDFForm_IsFieldOptionSelected(self.model.handle, self.index, option_index)
                != 0
        }
    }

    /// `/TI`, the option a list box shows in its top row, as the file holds
    /// it: possibly past the end of the options, or negative.
    pub fn top_index(&self) -> i32 {
        // SAFETY: as above.
        unsafe { pdfium_sys::EPDFForm_GetFieldTopIndex(self.model.handle, self.index) }
    }

    /// Whether the field dictionary carries an `/EMBD_Metadata` dictionary.
    pub fn has_embed_metadata(&self) -> bool {
        // SAFETY: as above.
        unsafe { pdfium_sys::EPDFForm_HasFieldEmbedMetadata(self.model.handle, self.index) != 0 }
    }

    /// One `/EMBD_Metadata` string entry of the field: `None` when absent,
    /// `Some("")` when present and empty.
    pub fn embed_metadata_string(&self, key: &str) -> Result<Option<String>, PdfiumError> {
        let key = std::ffi::CString::new(key)
            .map_err(|_| PdfiumError::InvalidArgument("metadata key contains NUL"))?;
        // SAFETY: a null buffer with zero length is the measure call; the
        // model handle and key are live.
        let bytes = unsafe {
            pdfium_sys::EPDFForm_GetFieldEmbedMetadataString(
                self.model.handle,
                self.index,
                key.as_ptr(),
                std::ptr::null_mut(),
                0,
            )
        };
        if bytes == 0 {
            return Ok(None);
        }
        let text = read_utf16(|buffer, len| {
            // SAFETY: as above, and `len` describes `buffer` in bytes.
            unsafe {
                pdfium_sys::EPDFForm_GetFieldEmbedMetadataString(
                    self.model.handle,
                    self.index,
                    key.as_ptr(),
                    buffer,
                    len,
                )
            }
        })?;
        Ok(Some(text.unwrap_or_default()))
    }
}

/// Measure then fill, for a UTF-16 accessor whose length is in bytes and
/// counts the terminator.
fn read_utf16<F>(mut call: F) -> Result<Option<String>, PdfiumError>
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

/// The same for a UTF-8 accessor.
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
