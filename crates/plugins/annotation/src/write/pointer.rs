//! `write/pointer.ts`: pointer intents. The gesture samples the interaction
//! handlers forward (edit, marquee, create, hover), each folded into the pure
//! core with this plugin's inputs filled in (page box, chrome geometry,
//! engaged ids, view).

use epdf_annotation_core::engine::measure::{
    DimensionInput, Readout, is_dimension, measurement_readout,
};
use epdf_annotation_core::object::Json;
use epdf_annotation_core::types::{Point, Target};
use epdf_annotation_core::update::defaults_for;
use serde_json::{Map, Value, json};

use crate::controller::Core;
use crate::js::{num, page_object_number};
use crate::read::chrome::HitView;

/// `'down' | 'move' | 'up'`.
pub type Phase = &'static str;

fn inert_json(set: Option<epdf_annotation_core::js::IdSet>) -> Option<Value> {
    set.map(|s| json!({ "$set": s.0 }))
}

/// `viewportForPoint(viewports, point)`: the last viewport whose box holds the point.
fn viewport_for_point(viewports: &[Value], point: Point) -> Option<&Value> {
    viewports.iter().rev().find(|v| {
        let b = &v["bbox"];
        let n = |k: &str| b.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
        point.x >= n("x")
            && point.x <= n("x") + n("width")
            && point.y >= n("y")
            && point.y <= n("y") + n("height")
    })
}

impl Core {
    /// `editPointer(phase, page, point, shift, scale, rotation, zoom, touch)`.
    #[allow(clippy::too_many_arguments)]
    pub fn edit_pointer(
        &self,
        phase: &str,
        page: &Value,
        point: Point,
        shift: bool,
        scale: Option<f64>,
        rotation: Option<f64>,
        zoom: Option<f64>,
        touch: Option<bool>,
    ) -> Result<(), epdf_kernel::Error> {
        let number = page_object_number(page);
        let mut input = Map::new();
        input.insert("page".into(), page.clone());
        input.insert("point".into(), point.json());
        input.insert("shift".into(), json!(shift));
        if let Some(b) = self.page_box_of(number) {
            input.insert("pageBox".into(), b.json());
        }
        input.insert("chrome".into(), self.chrome_geom_at(scale, Core::grab_boost(touch)).json());
        if let Some(s) = scale.filter(|s| *s != 0.0 && !s.is_nan()) {
            input.insert("scale".into(), num(s));
        }
        if let Some(inert) = inert_json(self.engaged_ids_on(number)) {
            input.insert("inert".into(), inert);
        }
        if let Some(z) = zoom {
            input.insert("zoom".into(), num(z));
        }
        if let Some(r) = rotation {
            input.insert("displayRotation".into(), num(r));
        }
        self.commit_json(json!({ "type": "editPointer", "phase": phase, "in": input }))?;
        Ok(())
    }

    /// `marqueePointer(phase, page, point, shift, scale, rotation, zoom)`.
    #[allow(clippy::too_many_arguments)]
    pub fn marquee_pointer(
        &self,
        phase: &str,
        page: &Value,
        point: Point,
        shift: bool,
        rotation: Option<f64>,
        zoom: Option<f64>,
    ) -> Result<(), epdf_kernel::Error> {
        let number = page_object_number(page);
        let mut input = Map::new();
        input.insert("page".into(), page.clone());
        input.insert("point".into(), point.json());
        input.insert("shift".into(), json!(shift));
        if let Some(b) = self.page_box_of(number) {
            input.insert("pageBox".into(), b.json());
        }
        if let Some(inert) = inert_json(self.engaged_ids_on(number)) {
            input.insert("inert".into(), inert);
        }
        if let Some(z) = zoom {
            input.insert("zoom".into(), num(z));
        }
        if let Some(r) = rotation {
            input.insert("displayRotation".into(), num(r));
        }
        self.commit_json(json!({ "type": "marqueePointer", "phase": phase, "in": input }))?;
        Ok(())
    }

    /// `createPointer(tool, phase, page, point, finish, displayRotation)`.
    pub fn create_pointer(
        &self,
        tool: &str,
        phase: &str,
        page: &Value,
        point: Point,
        finish: bool,
        display_rotation: Option<f64>,
    ) -> Result<(), epdf_kernel::Error> {
        let number = page_object_number(page);
        let resolved = self.tool(tool);
        let allowed = if resolved.as_ref().is_some_and(|t| t.capture) {
            self.security().is_some_and(|s| s.allows("doc.annotate.modify"))
        } else {
            self.can_create()
        };
        if !allowed {
            return Ok(());
        }
        let laid_out = self.size_of(number).is_some();
        let cache = self.viewports_of(number);
        let model = self.model();
        let preset = resolved.as_ref().map_or_else(|| tool.to_string(), |t| t.preset.clone());
        let continuing = match model.draft() {
            Some(epdf_annotation_core::types::Draft::CreateDistance(d)) => {
                d.page.object_number == number && d.preset == preset
            }
            Some(epdf_annotation_core::types::Draft::CreatePoly(d)) => {
                d.page.object_number == number && d.preset.as_deref() == Some(preset.as_str())
            }
            _ => false,
        };
        // `defaultsFor(store.model(), …)`: the model read again, as the
        // TypeScript reads it (a lender is asked for its records per read).
        let own = match &resolved {
            Some(t) => defaults_for(&self.model(), &t.preset),
            None => Map::new(),
        };
        let intent = own.get("intent").and_then(Value::as_str).map(str::to_string);
        let dimension =
            resolved.as_ref().is_some_and(|t| is_dimension(&t.subtype, intent.as_deref()));
        let has_viewports = cache.as_ref().is_some_and(|c| c.0.is_some());
        if dimension && phase == "down" && !continuing && (!laid_out || !has_viewports) {
            return Ok(());
        }
        let viewport = match (&cache, laid_out) {
            (Some((Some(viewports), _)), true) => viewport_for_point(viewports, point).cloned(),
            _ => None,
        };
        let scale = match &viewport {
            Some(v) => Some(
                v.get("measure")
                    .cloned()
                    .filter(|m| !epdf_kernel::is_undefined(m))
                    .unwrap_or(Value::Null),
            ),
            None => cache.as_ref().map(|c| c.1.clone()),
        };
        let caption_enabled =
            own.get("captionEnabled").filter(|v| !v.is_null()).cloned().unwrap_or(json!(true));
        let measure = if dimension && laid_out && cache.is_some() {
            let scale = scale.clone().unwrap_or(Value::Null);
            Some(if intent.as_deref() == Some("line-dimension") {
                json!({
                    "intent": intent,
                    "measure": scale,
                    "captionEnabled": caption_enabled,
                    "captionPosition": own.get("captionPosition").filter(|v| !v.is_null()).cloned().unwrap_or(json!("inline")),
                    "captionOffset": null,
                    "leader": own.get("leader").filter(|v| !v.is_null()).cloned().unwrap_or(Value::Null),
                    "contents": "",
                })
            } else {
                json!({ "intent": intent, "measure": scale, "captionEnabled": caption_enabled, "contents": "" })
            })
        } else {
            None
        };
        if let (Some(m), Some(t)) = (&measure, &resolved)
            && phase == "down"
            && !continuing
        {
            let probe = json!({
                "subtype": t.subtype,
                "intent": m["intent"],
                "measure": m["measure"],
                "linePoints": { "start": { "x": 0, "y": 0 }, "end": { "x": 1, "y": 0 } },
                "vertices": [{ "x": 0, "y": 0 }, { "x": 1, "y": 0 }, { "x": 1, "y": 1 }],
            });
            let probe = probe.as_object().cloned().unwrap_or_default();
            if matches!(measurement_readout(&DimensionInput::of(&probe)), Readout::Unavailable(_)) {
                return Ok(());
            }
        }
        let mut input = Map::new();
        input.insert("page".into(), page.clone());
        input.insert("point".into(), point.json());
        input.insert("shift".into(), json!(false));
        input.insert("finish".into(), json!(finish));
        if let Some(b) = self.page_box_of(number) {
            input.insert("pageBox".into(), b.json());
        }
        if let Some(r) = display_rotation {
            input.insert("displayRotation".into(), num(r));
        }
        if let Some(t) = &resolved {
            input.insert("upright".into(), json!(t.upright));
        }
        let mut message = Map::new();
        message.insert("type".into(), json!("createPointer"));
        if let Some(m) = measure {
            message.insert("measure".into(), m);
        }
        if resolved.as_ref().is_some_and(|t| t.capture) {
            message.insert("capture".into(), json!(tool));
        }
        message.insert("phase".into(), json!(phase));
        message.insert(
            "subtype".into(),
            json!(resolved.as_ref().map_or(tool, |t| t.subtype.as_str())),
        );
        message.insert("preset".into(), json!(preset));
        if intent.as_deref() == Some("ink-highlight") {
            message.insert("intent".into(), json!("ink-highlight"));
        }
        if let Some(t) = &resolved {
            message.insert("clickCreate".into(), t.click_create.clone());
            if let Some(f) = &t.flags {
                message.insert("flags".into(), Value::Object(f.clone()));
            }
            message.insert("deferInkCommit".into(), json!(t.group_strokes_ms() > 0.0));
            if let Some(straighten) = t.ink.as_ref().and_then(|i| i.get("straighten")) {
                message.insert("straightenInk".into(), straighten.clone());
            }
        } else {
            message.insert("deferInkCommit".into(), json!(false));
        }
        message.insert("in".into(), Value::Object(input));
        let tool_id = resolved.as_ref().map(|t| t.id.clone());
        let committed = self.commit(
            Value::Object(message),
            Some(self.after_create_shape(tool_id.clone())),
            None,
        )?;
        self.after_create_done(tool_id.as_deref(), &committed.effects);
        Ok(())
    }

    /// `hoverAt(at)`: the reducer sees enter/leave transitions only; the
    /// E/X feed hangs off this seam alone.
    pub fn hover_at(&self, at: Option<(&Value, Point, HitView)>) -> Result<(), epdf_kernel::Error> {
        let model = self.model();
        let mut id: Option<String> = None;
        if let Some((page, point, view)) = at
            && let Target::Annot(t) = self.hit_at(page, point, view, 1.0, None)
        {
            id = Some(t.id);
        }
        if model.hovered != id {
            self.hover_feed_hover(id.as_deref());
            self.commit_json(json!({ "type": "hover", "id": id }))?;
        }
        Ok(())
    }
}
