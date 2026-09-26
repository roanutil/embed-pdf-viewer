//! `write/script-effects.ts`: the actions plane's commit sink. Document
//! JavaScript patches annotations through this plugin, which owns the model,
//! so the engine write and the visible model can never diverge.

use epdf_acrojs::{ScriptColorArray, script_color_to_rgb};
use epdf_annotation_core::PageObjectNumber;
use epdf_kernel::Error;
use epdf_kernel::rt::Promise;
use epdf_plugin_actions::{
    AnnotCommitEntry, AnnotCommitResult, AnnotCommitResultEntry, AnnotCommitStatus,
};
use serde_json::{Map, Value, json};

use crate::controller::AnnotationCapability;
use crate::promise::{self, await_engine};
use crate::repository::seam::num_json;
use crate::value::{get, str_of, truthy};

fn ref_json(r: &epdf_annotation_core::AnnotationRef) -> Value {
    serde_json::to_value(r).unwrap_or_default()
}

/// `commitScriptEffects`: the script patch as the engine's per-kind patch
/// vocabulary.
fn engine_script_patch(subtype: &str, patch: &Value) -> Result<Value, Error> {
    let mut out = Map::new();
    out.insert("subtype".into(), json!(subtype));
    let to_engine_color = |c: &Value| -> Value {
        let rgb = serde_json::from_value::<ScriptColorArray>(c.clone())
            .ok()
            .and_then(|c| script_color_to_rgb(&c));
        match rgb {
            Some(rgb) => json!({
                "r": num_json(epdf_geometry::js::round(rgb.r * 255.0)),
                "g": num_json(epdf_geometry::js::round(rgb.g * 255.0)),
                "b": num_json(epdf_geometry::js::round(rgb.b * 255.0)),
            }),
            None => Value::Null,
        }
    };
    if let Some(stroke) = get(patch, "strokeColor").filter(|v| truthy(Some(v))) {
        let color = to_engine_color(stroke);
        if color.is_null() {
            return Err(Error::new("transparent stroke colours are not supported"));
        }
        out.insert("color".into(), color);
    }
    if let Some(fill) = get(patch, "fillColor").filter(|v| truthy(Some(v))) {
        out.insert("interiorColor".into(), to_engine_color(fill));
    }
    if let Some(v) = get(patch, "opacity") {
        out.insert("opacity".into(), v.clone());
    }
    if let Some(v) = get(patch, "width") {
        out.insert("strokeWidth".into(), v.clone());
    }
    if let Some(v) = get(patch, "borderStyle").filter(|v| truthy(Some(v))) {
        let style = if v.as_str() == Some("D") { "dashed" } else { "solid" };
        out.insert("borderStyle".into(), json!(style));
    }
    if let Some(v) = get(patch, "dash").filter(|v| truthy(Some(v))) {
        out.insert("dashArray".into(), v.clone());
    }
    if let Some(r) = get(patch, "rect").filter(|v| truthy(Some(v))) {
        let at = |i: usize| r.get(i).cloned().unwrap_or(Value::Null);
        out.insert(
            "rect".into(),
            json!({ "left": at(0), "bottom": at(1), "right": at(2), "top": at(3) }),
        );
    }
    if let Some(v) = get(patch, "contents") {
        out.insert("contents".into(), v.clone());
    }
    if let Some(v) = get(patch, "flags").filter(|v| truthy(Some(v))) {
        out.insert("flags".into(), v.clone());
    }
    Ok(Value::Object(out))
}

impl AnnotationCapability {
    /// `commitScriptEffects(entries)` (host): the actions plane's document
    /// commit door. Per entry, stop on failure; reconciles its own model.
    pub fn commit_script_effects(
        &self,
        entries: Vec<AnnotCommitEntry>,
    ) -> Promise<AnnotCommitResult> {
        let this = self.clone();
        promise::spawn(async move {
            let entry_result = |n: f64, status: AnnotCommitStatus, error: Option<String>| {
                AnnotCommitResultEntry { annot_object_number: n, status, error }
            };
            let Some(doc) = this.doc() else {
                return Ok(AnnotCommitResult {
                    results: entries
                        .iter()
                        .map(|e| {
                            entry_result(
                                e.annot_object_number,
                                AnnotCommitStatus::Failed,
                                Some("no document".into()),
                            )
                        })
                        .collect(),
                });
            };
            let mut results = Vec::new();
            let mut touched: Vec<PageObjectNumber> = Vec::new();
            let mut failed = false;
            for entry in &entries {
                let n = entry.annot_object_number;
                if failed {
                    results.push(entry_result(n, AnnotCommitStatus::Skipped, None));
                    continue;
                }
                let key = format!("obj:{}", epdf_geometry::js::number_to_string(n));
                let loaded = this.model().by_id.get(&key).cloned();
                let pon: Option<PageObjectNumber> = loaded
                    .as_ref()
                    .map(|a| a.page.page_object_number)
                    .or(entry.page.map(|p| p.page_object_number));
                let mut r: Option<Value> =
                    loaded.as_ref().and_then(|a| a.ref_.as_ref().map(ref_json));
                let mut subtype: Option<String> = loaded.as_ref().map(|a| a.subtype.clone());
                if (r.is_none() || subtype.as_deref().is_none_or(str::is_empty))
                    && let Some(pon) = pon
                {
                    let listed = async {
                        doc.page_index(pon)?;
                        await_engine(doc.call("page.annotations.list", Some(pon), Vec::new())).await
                    };
                    if let Ok(listed) = listed.await {
                        let found = get(&listed, "annotations").and_then(Value::as_array).and_then(
                            |list| {
                                list.iter()
                                    .find(|c| {
                                        let cr = get(c, "ref");
                                        cr.and_then(|x| str_of(x, "kind")) == Some("objectNumber")
                                            && cr
                                                .and_then(|x| get(x, "annotObjectNumber"))
                                                .and_then(Value::as_f64)
                                                == Some(n)
                                    })
                                    .cloned()
                            },
                        );
                        if let Some(dto) = found {
                            r = get(&dto, "ref").cloned();
                            subtype = str_of(&dto, "subtype").map(str::to_string);
                        }
                    }
                }
                let (Some(pon), Some(r), Some(subtype)) =
                    (pon, r.filter(|v| truthy(Some(v))), subtype.filter(|s| !s.is_empty()))
                else {
                    results.push(entry_result(
                        n,
                        AnnotCommitStatus::Failed,
                        Some("annotation not resolved (page not loaded)".into()),
                    ));
                    continue;
                };
                let patch = match engine_script_patch(&subtype, &entry.patch) {
                    Ok(p) => p,
                    Err(e) => {
                        results.push(entry_result(n, AnnotCommitStatus::Failed, Some(e.message)));
                        continue;
                    }
                };
                let write = async {
                    doc.page_index(pon)?;
                    await_engine(doc.call(
                        "page.annotations.update",
                        Some(pon),
                        vec![r.clone(), patch],
                    ))
                    .await
                };
                match write.await {
                    Ok(_) => {
                        if !touched.contains(&pon) {
                            touched.push(pon);
                        }
                        results.push(entry_result(n, AnnotCommitStatus::Applied, None));
                    }
                    Err(error) => {
                        failed = true;
                        results.push(entry_result(
                            n,
                            AnnotCommitStatus::Failed,
                            Some(error.message),
                        ));
                    }
                }
            }
            for pon in touched {
                this.reload_page_pon(pon).await?;
            }
            Ok(AnnotCommitResult { results })
        })
    }
}
