//! `write/create.ts`: measurement creation, a measurement annotation with the
//! page's scale, through the annotation plugin.

use epdf_annotation_core::json::truthy;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::geometry::Point;
use epdf_kernel::rt::{self, Promise};
use epdf_kernel::{Error, is_undefined};
use epdf_plugin_annotation::write::crud::CreateOptions;
use serde_json::{Map, Value, json};

use crate::contract::{CreateMeasurementInput, MeasurementKind, OperationOptions, PageInput};
use crate::controller::MeasurementCapability;
use crate::services::store::CAPABILITY;

/// `style.member`, `None` when the tool's defaults leave it `undefined`.
fn defined<'a>(style: &'a Value, key: &str) -> Option<&'a Value> {
    style.get(key).filter(|v| !is_undefined(v))
}

/// `style.member ?? fallback`.
fn or(style: &Value, key: &str, fallback: Value) -> Value {
    defined(style, key).filter(|v| !v.is_null()).cloned().unwrap_or(fallback)
}

/// Insert `value` unless it is `undefined` (an object literal member the
/// engine never sees).
fn put(draft: &mut Map<String, Value>, key: &str, value: Option<&Value>) {
    if let Some(value) = value {
        draft.insert(key.into(), value.clone());
    }
}

fn point(p: &Point) -> Value {
    json!({ "x": p.x, "y": p.y })
}

impl MeasurementCapability {
    /// `draftOf(input, page, tool)`: the tool's current defaults (style,
    /// caption and leader), the dimension intent, and the page's scale. The
    /// engine works out the label from the points and the scale.
    fn draft_of(
        &self,
        input: &CreateMeasurementInput,
        measure: Value,
        tool: &str,
    ) -> Result<Map<String, Value>, Error> {
        let style = self.core().annotation.tools_get_defaults(tool);
        let shared = |draft: &mut Map<String, Value>| {
            put(draft, "color", defined(&style, "color"));
            put(draft, "strokeWidth", defined(&style, "strokeWidth"));
            put(draft, "opacity", defined(&style, "opacity"));
            draft.insert("measure".into(), measure.clone());
            draft.insert("captionEnabled".into(), or(&style, "captionEnabled", json!(true)));
        };
        let mut draft = Map::new();
        match input.kind {
            MeasurementKind::Distance => {
                let [from, to] = input.points.as_slice() else {
                    return Err(plugin_error(
                        code::INVALID_INPUT,
                        CAPABILITY,
                        "a distance needs exactly two points",
                        None,
                        None,
                    ));
                };
                draft.insert("subtype".into(), json!("line"));
                draft.insert("intent".into(), json!("line-dimension"));
                draft
                    .insert("linePoints".into(), json!({ "start": point(from), "end": point(to) }));
                put(&mut draft, "lineEndings", defined(&style, "lineEndings"));
                // The arrowheads fill with the line's color unless the tool
                // sets a fill.
                let fill = defined(&style, "interiorColor")
                    .filter(|v| !v.is_null())
                    .or_else(|| defined(&style, "color"));
                put(&mut draft, "interiorColor", fill);
                shared(&mut draft);
                draft.insert(
                    "captionPosition".into(),
                    or(&style, "captionPosition", json!("inline")),
                );
                if let Some(leader) = defined(&style, "leader").filter(|l| truthy(Some(l))) {
                    draft.insert("leader".into(), leader.clone());
                }
            }
            MeasurementKind::Perimeter => {
                draft.insert("subtype".into(), json!("polyline"));
                draft.insert("intent".into(), json!("polyline-dimension"));
                draft.insert("vertices".into(), input.points.iter().map(point).collect());
                put(&mut draft, "lineEndings", defined(&style, "lineEndings"));
                shared(&mut draft);
            }
            MeasurementKind::Area => {
                draft.insert("subtype".into(), json!("polygon"));
                draft.insert("intent".into(), json!("polygon-dimension"));
                draft.insert("vertices".into(), input.points.iter().map(point).collect());
                put(&mut draft, "interiorColor", defined(&style, "interiorColor"));
                shared(&mut draft);
            }
        }
        Ok(draft)
    }

    /// Create a measurement annotation with the page's scale and the tool's
    /// style. Resolves `{ annotation }`. Rejects `permission-denied` without
    /// `annotations:create`, `not-ready` before the scale is known,
    /// `not-found` for a page that isn't in the document.
    pub fn create_measurement(
        &self,
        input: CreateMeasurementInput,
        options: OperationOptions,
    ) -> Promise<Value> {
        let this = self.clone();
        Promise::spawn(async move {
            let core = this.core();
            core.cx.assert_allowed("annotations:create", "measurement.createMeasurement")?;
            let page = core.registry.page_of(input.page)?.page();
            let scale = this.scale_of(PageInput::Ref(page))?;
            if !scale.ready {
                return Err(plugin_error(
                    code::NOT_READY,
                    CAPABILITY,
                    "the page scale is not known yet",
                    None,
                    None,
                ));
            }
            let tool = input.tool.clone().unwrap_or_else(|| input.kind.as_str().to_string());
            let draft = this.draft_of(&input, scale.measure.clone(), &tool)?;
            let created = core.annotation.create(
                &page.to_json(),
                &draft,
                None,
                CreateOptions { signal: options.signal, ..CreateOptions::default() },
            );
            // `return annotation.create(...)` from an `async` function: the
            // resolve-thenable job, then the adopted promise's reaction.
            rt::tick().await;
            created.reaction().await
        })
    }
}
