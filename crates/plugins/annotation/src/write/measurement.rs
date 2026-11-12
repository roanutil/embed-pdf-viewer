//! `write/measurement.ts`: the measurement seam. The per-page viewports
//! (scale regions) the measurement plugin supplies for dimension drafts, and
//! recalibration of a page's dimensions to a new scale.

use std::cell::RefCell;
use std::collections::HashMap;

use epdf_annotation_core::engine::measure::{
    DimensionInput, Readout, is_dimension, measurement_readout,
};
use epdf_annotation_core::flags::{annot_contents_editable, annot_transformable};
use epdf_kernel::Error;
use epdf_kernel::mirror::ResourceStatus;
use epdf_kernel::rt::Promise;
use serde_json::{Map, Value, json};

use crate::controller::Core;
use crate::js::{key_or_none, page_object_number};
use crate::services::staging::StoreChange;
use crate::sync::records::{by_key, dto_of};

#[derive(Default)]
pub struct MeasurementState {
    /// Each page's viewports (or none) and fallback scale.
    viewports: RefCell<HashMap<String, (Option<Vec<Value>>, Value)>>,
}

/// `serializeError(error)`: an engine error as its wire form; anything
/// else as `Unknown` (`Aborted` for an `AbortError`).
fn serialize_error(error: &Error) -> Value {
    let mut out = Map::new();
    if error.name == "EngineError" {
        out.insert("code".into(), error.member("code").cloned().unwrap_or(Value::Null));
        out.insert("message".into(), json!(error.message));
        if let Some(details) = error.member("details").filter(|d| !epdf_kernel::is_undefined(d)) {
            out.insert("details".into(), details.clone());
        }
    } else {
        out.insert(
            "code".into(),
            json!(if error.name == "AbortError" { "Aborted" } else { "Unknown" }),
        );
        out.insert("message".into(), json!(error.message));
    }
    Value::Object(out)
}

fn page_key(page: f64) -> String {
    epdf_annotation_core::js::number_to_string(page)
}

impl Core {
    /// The measurement area's cleanup.
    pub(crate) fn wire_measurement(&self) {
        let me = self.weak();
        self.cx.cleanup_fn(move || {
            if let Some(core) = me.upgrade() {
                core.measurement.viewports.borrow_mut().clear();
            }
        });
    }

    /// `viewportsOf(page)`.
    pub(crate) fn viewports_of(&self, page: f64) -> Option<(Option<Vec<Value>>, Value)> {
        self.measurement.viewports.borrow().get(&page_key(page)).cloned()
    }

    /// `setPageViewports(page, viewports, fallback)`.
    pub fn set_page_viewports(&self, page: &Value, viewports: Option<Vec<Value>>, fallback: Value) {
        self.measurement
            .viewports
            .borrow_mut()
            .insert(page_key(page_object_number(page)), (viewports, fallback));
    }

    /// `whenRemeasurable()`: resolves once the document's measurements can be
    /// re-measured: the annotations loaded (a failed load retried) and every
    /// pending load settled.
    pub fn when_remeasurable(&self) -> Promise<Value> {
        let me = self.weak();
        Promise::spawn(async move {
            let core = me.upgrade().ok_or_else(|| Error::new("closed"))?;
            if core.records.get_status() != ResourceStatus::Ready {
                let refresh = core.records.refresh();
                let _ = refresh.await;
            }
            core.records.settled().await?;
            Ok(epdf_kernel::undefined())
        })
    }

    /// `stageRemeasure(page, scale)`: stage the update of every measurement on
    /// `page` to `scale`, at once, so a group takes it into its change. What
    /// it staged is `updated`; `error` is set, and nothing staged, when the
    /// annotations aren't loaded.
    pub fn stage_remeasure(&self, page: &Value, scale: Value) -> Result<Value, Error> {
        let number = page_object_number(page);
        let mut report =
            json!({ "page": page, "scale": scale, "updated": [], "skipped": [], "failed": [] });
        if self.records.get_status() != ResourceStatus::Ready {
            report["error"] =
                serialize_error(&Error::new("the annotations of this document are not loaded"));
            return Ok(report);
        }
        let model = self.model();
        let truths = self.records.get();
        let mut candidates = Vec::new();
        for stored in by_key(&truths).values() {
            let dto = dto_of(stored);
            let Some(record) =
                model.by_id.get(&key_or_none(dto.get("ref").unwrap_or(&Value::Null)))
            else {
                continue;
            };
            let subtype = dto.get("subtype").and_then(Value::as_str).unwrap_or("");
            let intent = dto.get("intent").and_then(Value::as_str);
            if dto.get("page").map(page_object_number) == Some(number)
                && is_dimension(subtype, intent)
            {
                candidates.push((dto.clone(), record.clone()));
            }
        }
        let mut updates = Vec::new();
        for (dto, record) in candidates {
            let reference = dto.get("ref").cloned().unwrap_or(Value::Null);
            let measure =
                dto.get("measure").filter(|m| epdf_annotation_core::json::truthy(Some(m)));
            let mut with_scale = dto.as_object().cloned().unwrap_or_default();
            with_scale.insert("measure".into(), scale.clone());
            let readout = measurement_readout(&DimensionInput::of(&with_scale));
            let reason = if record.authority.as_ref().is_some_and(|a| !a.update) {
                Some("no-authority")
            } else if !annot_transformable(&*record) || !annot_contents_editable(&*record) {
                Some("locked")
            } else if measure
                .is_some_and(|m| m.get("subtype").and_then(Value::as_str) != Some("rectilinear"))
            {
                Some("foreign-measure")
            } else if matches!(readout, Readout::Unavailable(_)) {
                Some("unavailable")
            } else {
                None
            };
            if let Some(reason) = reason {
                report["skipped"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({ "ref": reference, "reason": reason }));
                continue;
            }
            let mut patch = Map::new();
            patch.insert("subtype".into(), dto.get("subtype").cloned().unwrap_or(Value::Null));
            patch.insert("measure".into(), scale.clone());
            updates.push(StoreChange::Update {
                reference: reference.clone(),
                patch,
                resources: None,
            });
            report["updated"].as_array_mut().unwrap().push(reference);
        }
        if !updates.is_empty() {
            self.apply(&updates, false)?;
        }
        Ok(report)
    }
}
