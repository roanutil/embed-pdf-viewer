use super::*;

fn cancellable_operation<T: Clone + 'static>(
    run: impl FnOnce(epdf_kernel::signal::AbortSignal) -> Result<epdf_kernel::Promise<T>, Error>,
) -> Result<epdf_kernel::Promise<T>, Error> {
    let signal = epdf_kernel::signal::AbortSignal::new();
    let task = run(signal.clone())?;
    Ok(epdf_kernel::cancellable::cancellable("form", Some(signal.clone()), task)
        .with_abort_member(move |reason| signal.abort(Error::abort(reason))))
}

/// The form owner's settings-panel control vocabulary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, uniffi::Enum)]
#[serde(rename_all = "camelCase")]
pub enum FormSettingControl {
    Text,
    Flag,
    Number,
    Options,
    Choice,
    Choices,
}

#[derive(Clone, Debug, PartialEq, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct FormSettingProperty {
    pub key: String,
    pub control: FormSettingControl,
    pub label: String,
    pub label_key: String,
    pub min: Option<f64>,
    pub free: Option<bool>,
    pub options: Option<Vec<String>>,
}

/// Family-specific descriptors and their open engine values.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormFieldPanelSettings {
    pub properties: Vec<FormSettingProperty>,
    pub values_json: String,
}

fn object_payload(source: &str, kind: &str) -> Result<Value, Error> {
    let value: Value = serde_json::from_str(source).map_err(|error| {
        plugin_error(
            code::INVALID_INPUT,
            "form",
            format!("{kind} is not JSON: {error}"),
            None,
            None,
        )
    })?;
    if !value.is_object() {
        return Err(plugin_error(
            code::INVALID_INPUT,
            "form",
            format!("{kind} must be an object"),
            None,
            None,
        ));
    }
    Ok(value)
}

/// A widget's annotation row, including its typed identity and page-space bounds.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormWidgetRecord {
    pub annotation_ref: AnnotationRef,
    pub page: PageRef,
    pub bounds: Rect,
    pub fields_json: String,
}

fn widget_out(value: &Value) -> Result<FormWidgetRecord, Error> {
    Ok(FormWidgetRecord {
        annotation_ref: AnnotationRef::from_json(&value["ref"])?,
        page: page_of(&value["page"])?,
        bounds: rect_of(&value["rect"]),
        fields_json: value.to_string(),
    })
}

/// A position in the calculation order names a neighbour, never a transient index.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum FormCalculationPosition {
    Start,
    End,
    Before { field: FormFieldRef },
    After { field: FormFieldRef },
}

impl FormCalculationPosition {
    fn to_json(&self) -> Value {
        match self {
            Self::Start => json!("start"),
            Self::End => json!("end"),
            Self::Before { field } => json!({"before": field_ref_json(field)}),
            Self::After { field } => json!({"after": field_ref_json(field)}),
        }
    }
}

#[uniffi::export]
impl FormHandle {
    pub async fn field_settings(
        &self,
        field_ref: FormFieldRef,
    ) -> Result<Option<FormFieldPanelSettings>, ViewerError> {
        self.cap
            .call(move |capability| {
                open(capability)?
                    .get_field_settings(&field_ref_json(&field_ref))
                    .map(|settings| {
                        Ok(FormFieldPanelSettings {
                            properties: decode(settings["properties"].clone())?,
                            values_json: settings["values"].to_string(),
                        })
                    })
                    .transpose()
            })
            .await
    }

    /// The engine's FormFieldDraft, including family options and action/script data.
    /// Cancellation ends the wait; admitted changes remain kernel-owned.
    pub async fn create_draft(
        &self,
        draft_json: String,
        cancel: Arc<CancelToken>,
    ) -> Result<FormField, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |capability| {
                    let draft = object_payload(&draft_json, "a form draft")?;
                    cancellable_operation(
                        |signal| Ok(open(capability)?.create(draft, Some(signal))),
                    )
                },
                |result| field_result(&result),
            )
            .await?
    }

    /// The engine's FormFieldPatch: omission preserves a value; null clears it.
    pub async fn update_patch(
        &self,
        field_ref: FormFieldRef,
        patch_json: String,
        cancel: Arc<CancelToken>,
    ) -> Result<FormField, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |capability| {
                    let patch = object_payload(&patch_json, "a form patch")?;
                    cancellable_operation(|signal| {
                        Ok(open(capability)?.update(
                            field_ref_json(&field_ref),
                            patch,
                            Some(signal),
                        ))
                    })
                },
                |result| field_result(&result),
            )
            .await?
    }

    /// The engine's WidgetPlacement, including appearance and action data.
    pub async fn add_widget_placement(
        &self,
        field_ref: FormFieldRef,
        placement_json: String,
        cancel: Arc<CancelToken>,
    ) -> Result<FormWidgetAdded, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |capability| {
                    let placement = object_payload(&placement_json, "a widget placement")?;
                    cancellable_operation(|signal| {
                        Ok(open(capability)?.add_widget(
                            field_ref_json(&field_ref),
                            placement,
                            Some(signal),
                        ))
                    })
                },
                |result| {
                    Ok::<_, ViewerError>(FormWidgetAdded {
                        field: field_result(&result)?,
                        widget_json: result["widget"].to_string(),
                    })
                },
            )
            .await?
    }

    pub async fn widget(
        &self,
        widget: AnnotationRef,
    ) -> Result<Option<FormWidgetRecord>, ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?
                    .get_widget(&annotation_ref_json(&widget))
                    .as_deref()
                    .map(widget_out)
                    .transpose()
            })
            .await
    }

    /// Every widget annotation of this field in its own order.
    pub async fn field_widgets(
        &self,
        field_ref: FormFieldRef,
    ) -> Result<Vec<FormWidgetRecord>, ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?
                    .get_widgets(&field_ref_json(&field_ref))
                    .iter()
                    .map(|w| widget_out(w))
                    .collect()
            })
            .await
    }

    pub async fn selected_field(&self) -> Result<Option<FormField>, ViewerError> {
        self.cap
            .call(|c| {
                open(c)?.get_selected_field().as_deref().map(FormField::from_json).transpose()
            })
            .await
    }

    /// Reorder calculated fields. Cancellation ends the wait while an admitted change remains kernel-owned.
    pub async fn reorder_calculations(
        &self,
        fields: Vec<FormFieldRef>,
        position: FormCalculationPosition,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<Option<FormFieldRef>>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let fields = fields.iter().map(field_ref_json).collect();
                    cancellable_operation(|signal| {
                        Ok(open(c)?.reorder_calculations(
                            Value::Array(fields),
                            position.to_json(),
                            Some(signal),
                        ))
                    })
                },
                |result| {
                    let refs = result["calculationOrder"]
                        .as_array()
                        .ok_or_else(|| invalid_state("a calculation order is not a list".into()))?;
                    refs.iter()
                        .map(|r| if r.is_null() { Ok(None) } else { field_ref_of(r).map(Some) })
                        .collect::<Result<_, Error>>()
                        .map_err(ViewerError::from)
                },
            )
            .await?
    }

    /// The engine's open FormWidgetPatch payload. Cancellation ends the wait;
    /// the kernel still reconciles a change it has already admitted.
    pub async fn update_widget(
        &self,
        widget: AnnotationRef,
        patch_json: String,
        cancel: Arc<CancelToken>,
    ) -> Result<FormWidgetRecord, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let patch: Value = serde_json::from_str(&patch_json).map_err(|e| {
                        plugin_error(
                            code::INVALID_INPUT,
                            "form",
                            format!("a widget patch is not JSON: {e}"),
                            None,
                            None,
                        )
                    })?;
                    if !patch.is_object() {
                        return Err(plugin_error(
                            code::INVALID_INPUT,
                            "form",
                            "a widget patch must be an object",
                            None,
                            None,
                        ));
                    }
                    cancellable_operation(|signal| {
                        Ok(open(c)?.update_widget(
                            annotation_ref_json(&widget),
                            patch,
                            Some(signal),
                        ))
                    })
                },
                |result| widget_out(&result["widget"]).map_err(ViewerError::from),
            )
            .await?
    }

    /// Delete the widget from its page and field. A detached widget has no remaining field.
    /// Cancellation ends the wait while an admitted change remains kernel-owned.
    pub async fn delete_widget(
        &self,
        widget: AnnotationRef,
        cancel: Arc<CancelToken>,
    ) -> Result<Option<FormField>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    cancellable_operation(|signal| {
                        Ok(open(c)?.delete_widget(annotation_ref_json(&widget), Some(signal)))
                    })
                },
                |result| {
                    if result["field"].is_null() {
                        Ok(None)
                    } else {
                        FormField::from_json(&result["field"]).map(Some).map_err(ViewerError::from)
                    }
                },
            )
            .await?
    }
}
