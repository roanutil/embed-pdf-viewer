//! `write/script-effects.ts`: the actions plugin's commit sink. Document
//! JavaScript patches annotations through this plugin, which owns them.
//! Colors cross the Acrobat-array to engine-hex boundary here; a script's
//! rect, in the file's numbers, is the annotation's `rect` in page space.

use epdf_acrojs::{ScriptColorArray, script_color_to_rgb};
use epdf_kernel::Error;
use epdf_kernel::rt::Promise;
use epdf_plugin_actions::{
    AnnotCommitEntry, AnnotCommitResult, AnnotCommitResultEntry, AnnotCommitStatus,
};
use serde_json::{Map, Value, json};

use crate::controller::Core;
use crate::engine::{color_of, page_box_of};
use crate::js::Object;
use crate::services::staging::StoreChange;
use crate::write::outcomes::applied_or_throw;

fn engine_color(color: &Value) -> Option<Value> {
    let parsed: ScriptColorArray = serde_json::from_value(color.clone()).ok()?;
    let rgb = script_color_to_rgb(&parsed)?;
    Some(json!(color_of(rgb.r * 255.0, rgb.g * 255.0, rgb.b * 255.0)))
}

/// `engineScriptPatch(subtype, patch, crop)`: the engine's patch, or why not.
fn engine_script_patch(
    subtype: &str,
    patch: &Value,
    crop: Option<&Value>,
) -> Result<Object, String> {
    let mut out = Map::new();
    out.insert("subtype".into(), json!(subtype));
    let present = |k: &str| patch.get(k).filter(|v| !epdf_kernel::is_undefined(v));
    let truthy = |k: &str| present(k).is_some_and(|v| epdf_annotation_core::json::truthy(Some(v)));
    if truthy("strokeColor") {
        let Some(color) = engine_color(&patch["strokeColor"]) else {
            return Err("transparent stroke colours are not supported".into());
        };
        out.insert("color".into(), color);
    }
    if truthy("fillColor") {
        out.insert(
            "interiorColor".into(),
            engine_color(&patch["fillColor"]).unwrap_or(Value::Null),
        );
    }
    if let Some(v) = present("opacity") {
        out.insert("opacity".into(), v.clone());
    }
    if let Some(v) = present("width") {
        out.insert("strokeWidth".into(), v.clone());
    }
    if truthy("borderStyle") {
        let dashed = patch["borderStyle"].as_str() == Some("D");
        out.insert("borderStyle".into(), json!(if dashed { "dashed" } else { "solid" }));
    }
    if truthy("dash") {
        out.insert("dashArray".into(), patch["dash"].clone());
    }
    if truthy("rect") {
        let Some(crop) = crop else { return Err("annotation page not loaded".into()) };
        let r: Vec<f64> = patch["rect"]
            .as_array()
            .map(|a| a.iter().map(|v| v.as_f64().unwrap_or(f64::NAN)).collect())
            .unwrap_or_default();
        let at = |i: usize| r.get(i).copied().unwrap_or(f64::NAN);
        let (x1, y1, x2, y2) = (at(0), at(1), at(2), at(3));
        let min = epdf_geometry::js::min;
        let max = epdf_geometry::js::max;
        out.insert(
            "rect".into(),
            page_box_of(min(x1, x2), min(y1, y2), max(x1, x2), max(y1, y2), crop),
        );
    }
    if let Some(v) = present("contents") {
        out.insert("contents".into(), v.clone());
    }
    if let Some(flags) = present("flags").and_then(Value::as_object) {
        for (k, v) in flags {
            out.insert(k.clone(), v.clone());
        }
    }
    Ok(out)
}

impl Core {
    /// `commitScriptEffects(entries)`: stop on the first failure; every later entry is skipped.
    pub fn commit_script_effects(
        &self,
        entries: Vec<AnnotCommitEntry>,
    ) -> Promise<AnnotCommitResult> {
        let me = self.weak();
        Promise::spawn(async move {
            let mut results = Vec::new();
            let mut failed = false;
            for entry in entries {
                let number = entry.annot_object_number;
                let result = |status, error: Option<String>| AnnotCommitResultEntry {
                    annot_object_number: number,
                    status,
                    error,
                };
                if failed {
                    results.push(result(AnnotCommitStatus::Skipped, None));
                    continue;
                }
                let core = me.upgrade().ok_or_else(|| Error::new("closed"))?;
                let model = core.model();
                let key = format!("obj:{}", epdf_annotation_core::js::number_to_string(number));
                let loaded = model.by_id.get(&key).cloned();
                let page = loaded
                    .as_ref()
                    .and_then(|r| r.page_number())
                    .or_else(|| entry.page.map(|p| p.page_object_number as f64));
                let mut reference = loaded.as_ref().and_then(|r| r.annotation.get("ref").cloned());
                let mut subtype = loaded.as_ref().map(|r| r.subtype().to_string());
                if (reference.is_none() || subtype.is_none())
                    && let (Some(page), Some(doc)) = (page, core.doc())
                {
                    let call = doc.call("page.annotations.list", Some(page as u32), vec![]);
                    drop(core);
                    if let Ok(list) = call.await {
                        let found =
                            list.get("annotations").and_then(Value::as_array).and_then(|all| {
                                all.iter()
                                    .find(|a| {
                                        a.pointer("/ref/kind").and_then(Value::as_str)
                                            == Some("objectNumber")
                                            && a.pointer("/ref/objectNumber")
                                                .and_then(Value::as_f64)
                                                == Some(number)
                                    })
                                    .cloned()
                            });
                        if let Some(dto) = found {
                            reference = dto.get("ref").cloned();
                            subtype =
                                dto.get("subtype").and_then(Value::as_str).map(str::to_string);
                        }
                    }
                } else {
                    drop(core);
                }
                let (Some(page), Some(reference), Some(subtype)) = (page, reference, subtype)
                else {
                    results.push(result(
                        AnnotCommitStatus::Failed,
                        Some("annotation not resolved (page not loaded)".into()),
                    ));
                    continue;
                };
                let core = me.upgrade().ok_or_else(|| Error::new("closed"))?;
                let patch = match engine_script_patch(
                    &subtype,
                    &entry.patch,
                    core.crop_of(page).as_ref(),
                ) {
                    Ok(p) => p,
                    Err(message) => {
                        results.push(result(AnnotCommitStatus::Failed, Some(message)));
                        continue;
                    }
                };
                let applied =
                    core.apply(&[StoreChange::Update { reference, patch, resources: None }], false);
                drop(core);
                let outcome = match applied {
                    Ok(applied) => applied_or_throw(&applied).await.map(|_| ()),
                    Err(error) => Err(error),
                };
                match outcome {
                    Ok(()) => results.push(result(AnnotCommitStatus::Applied, None)),
                    Err(error) => {
                        failed = true;
                        results
                            .push(result(AnnotCommitStatus::Failed, Some(error.message.clone())));
                    }
                }
            }
            Ok(AnnotCommitResult { results })
        })
    }
}
