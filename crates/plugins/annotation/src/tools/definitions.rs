//! `tools/definitions.ts`: which tools exist and what each authors. A tool
//! is a named authoring preset over a kind: an id bound to a routing
//! subtype, its defaults (keyed by the tool's `preset`, never the subtype),
//! a cursor and the interaction tags it turns on.
//!
//! A tool definition is JSON (an app's tool table can come from a file);
//! [`build_tool_registry`] resolves the built-ins and the app's entries into
//! [`ResolvedTool`]s.

use std::rc::Rc;

use epdf_annotation_core::engine::facts::field_names;
use epdf_kernel::Error;
use epdf_plugin_interaction::ToolTouch;
use indexmap::IndexMap;
use serde_json::{Map, Value, json};

use crate::contract::AfterCreate;
use crate::js::{Object, number};

/// The tool `stamps.arm` activates, and the only tool an armed payload
/// survives onto (plus the legacy `annotation-stamp` tag).
pub const ARMED_STAMP_TOOL_ID: &str = "stamp";

/// How opaque a ghost paints unless its tool says.
pub const GHOST_OPACITY: f64 = 0.5;

/// The fields no tool's defaults may set.
const NOT_DEFAULTS: [&str; 27] = [
    "ref",
    "page",
    "index",
    "hasAppearance",
    "appearanceState",
    "nm",
    "rect",
    "box",
    "linePoints",
    "vertices",
    "inkList",
    "quadPoints",
    "calloutLine",
    "captionCenter",
    "measure",
    "reply",
    "popup",
    "parent",
    "groupId",
    "author",
    "createdAt",
    "modifiedAt",
    "userId",
    "createdBy",
    "modifiedBy",
    "importedBy",
    "actions",
];

const WIDGET_BOX_FIELDS: [&str; 4] = ["color", "interiorColor", "strokeWidth", "borderStyle"];
const WIDGET_TEXT_FIELDS: [&str; 8] = [
    "color",
    "interiorColor",
    "strokeWidth",
    "borderStyle",
    "fontFamily",
    "fontSize",
    "fontColor",
    "textAlign",
];

fn draft_defaults(subtype: &str) -> Vec<&'static str> {
    field_names(subtype).into_iter().filter(|name| !NOT_DEFAULTS.contains(name)).collect()
}

/// `TOOL_DEFAULT_FIELDS[kind]`: the fields a known authoring kind's defaults
/// may set; `None` for a custom kind, which stays extensible.
pub fn tool_default_fields(kind: &str) -> Option<Vec<&'static str>> {
    Some(match kind {
        "square" | "circle" | "line" | "polygon" | "polyline" | "ink" | "free-text"
        | "highlight" | "underline" | "strikeout" | "squiggly" | "caret" | "redact" | "stamp"
        | "link" | "text" | "file-attachment" => draft_defaults(kind),
        "free-text-callout" => draft_defaults("free-text"),
        "widget-text" | "widget-choice" | "widget-button" => WIDGET_TEXT_FIELDS.to_vec(),
        "widget-toggle" | "widget-radio" | "widget-box" => WIDGET_BOX_FIELDS.to_vec(),
        _ => return None,
    })
}

/// `toolTouchOf(enables)`: drag-create tools draw with one finger; click-to-place tools tap.
pub fn tool_touch_of(enables: &[String]) -> ToolTouch {
    if enables.iter().any(|t| t == "annotation-draw" || t == "text-select") {
        ToolTouch::Draw
    } else {
        ToolTouch::Tap
    }
}

/// A fully resolved tool (`ResolvedTool`).
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedTool {
    pub id: String,
    /// Routing token for the draw core and the created annotation's subtype.
    pub subtype: String,
    /// The defaults key.
    pub preset: String,
    /// The kind whose properties the style panel shows.
    pub fields_kind: String,
    pub cursor: String,
    /// The tags, each once, in insertion order (a `ReadonlySet`).
    pub enables: Vec<String>,
    pub defaults: Option<Object>,
    /// `/F` seed for created annotations.
    pub flags: Option<Object>,
    /// `StampSourceSpec`, as JSON.
    pub source: Option<Value>,
    /// `SelectionAuthoring`, as JSON.
    pub selection: Option<Value>,
    /// `InkAuthoringOptions`, as JSON.
    pub ink: Option<Object>,
    pub upright: bool,
    /// `ClickCreate | false`.
    pub click_create: Value,
    /// The resolved ghost (`ResolvedGhost`): `false`, or the tool's
    /// `{ opacity }` (any other value is kept as the tool gave it).
    pub ghost: Value,
    pub after_create: Option<AfterCreate>,
    pub capture: bool,
    pub meta: Option<Object>,
}

impl ResolvedTool {
    pub fn enables(&self, tag: &str) -> bool {
        self.enables.iter().any(|t| t == tag)
    }

    /// `tool.clickCreate` truthy: a click creates.
    pub fn clicks(&self) -> bool {
        self.click_create.is_object()
    }

    /// `tool.ink?.groupStrokesMs ?? 0`.
    pub fn group_strokes_ms(&self) -> f64 {
        self.ink.as_ref().and_then(|i| i.get("groupStrokesMs")).and_then(number).unwrap_or(0.0)
    }

    /// The resolved ghost as JSON: `false` or `{ opacity }`.
    pub fn ghost_json(&self) -> Value {
        self.ghost.clone()
    }

    /// `tool.ghost` is truthy.
    pub fn ghost_shows(&self) -> bool {
        epdf_annotation_core::json::truthy(Some(&self.ghost))
    }

    /// `tool.ghost.opacity`, `undefined` when it has none.
    pub fn ghost_opacity(&self) -> Value {
        self.ghost.get("opacity").cloned().unwrap_or_else(epdf_kernel::undefined)
    }

    /// The tool as `buildToolRegistry` prints it; `enables` as a set.
    pub fn to_json(&self) -> Value {
        let mut out = Map::new();
        out.insert("id".into(), json!(self.id));
        out.insert("subtype".into(), json!(self.subtype));
        out.insert("preset".into(), json!(self.preset));
        out.insert("fieldsKind".into(), json!(self.fields_kind));
        out.insert("cursor".into(), json!(self.cursor));
        out.insert("enables".into(), json!({ "$set": self.enables }));
        if let Some(d) = &self.defaults {
            out.insert("defaults".into(), Value::Object(d.clone()));
        }
        if let Some(f) = &self.flags {
            out.insert("flags".into(), Value::Object(f.clone()));
        }
        if let Some(s) = &self.source {
            out.insert("source".into(), s.clone());
        }
        if let Some(s) = &self.selection {
            out.insert("selection".into(), s.clone());
        }
        if let Some(i) = &self.ink {
            out.insert("ink".into(), Value::Object(i.clone()));
        }
        out.insert("upright".into(), json!(self.upright));
        out.insert("clickCreate".into(), self.click_create.clone());
        out.insert("ghost".into(), self.ghost_json());
        if let Some(a) = &self.after_create {
            out.insert("afterCreate".into(), a.to_json());
        }
        out.insert("capture".into(), json!(self.capture));
        if let Some(m) = &self.meta {
            out.insert("meta".into(), Value::Object(m.clone()));
        }
        Value::Object(out)
    }
}

/// The tools whose line gesture is captured instead of created.
const CAPTURE_TOOLS: [&str; 1] = ["calibrate"];

const HOUSE_RED: &str = "#e5484d";

/// `DEFAULT_TOOLS`: the built-in tool table, as data.
pub fn default_tools() -> Vec<Value> {
    let draw = json!(["annotation-draw", "annotation-edit"]);
    let markup = json!(["text-select", "annotation-edit"]);
    vec![
        json!({ "id": "square", "subtype": "square", "cursor": "crosshair", "enables": draw,
            "defaults": { "color": HOUSE_RED, "strokeWidth": 6 },
            "clickCreate": { "width": 80, "height": 60 }, "upright": true }),
        json!({ "id": "circle", "subtype": "circle", "cursor": "crosshair", "enables": draw,
            "defaults": { "color": HOUSE_RED, "strokeWidth": 6 },
            "clickCreate": { "width": 80, "height": 60 }, "upright": true }),
        json!({ "id": "line", "subtype": "line", "cursor": "crosshair", "enables": draw,
            "defaults": { "color": HOUSE_RED, "strokeWidth": 6 },
            "clickCreate": { "length": 80 }, "upright": true }),
        json!({ "id": "distance", "extends": "line", "clickCreate": false,
            "defaults": {
                "intent": "line-dimension",
                "strokeWidth": 1,
                "lineEndings": { "start": "closed-arrow", "end": "closed-arrow" },
                "captionEnabled": true,
                "captionPosition": "inline",
                "leader": { "length": 12, "extension": 5, "offset": 0 },
            } }),
        json!({ "id": "calibrate", "extends": "line", "enables": ["annotation-draw"],
            "defaults": { "strokeWidth": 1 }, "clickCreate": false }),
        json!({ "id": "polygon", "subtype": "polygon", "cursor": "crosshair", "enables": draw,
            "defaults": { "color": HOUSE_RED, "strokeWidth": 6 } }),
        json!({ "id": "polyline", "subtype": "polyline", "cursor": "crosshair", "enables": draw,
            "defaults": { "color": HOUSE_RED, "strokeWidth": 6 } }),
        json!({ "id": "perimeter", "extends": "polyline",
            "defaults": {
                "intent": "polyline-dimension",
                "strokeWidth": 1,
                "lineEndings": { "start": "none", "end": "none" },
                "captionEnabled": true,
            } }),
        json!({ "id": "area", "extends": "polygon",
            "defaults": { "intent": "polygon-dimension", "strokeWidth": 1, "captionEnabled": true } }),
        json!({ "id": "ink", "subtype": "ink", "cursor": "crosshair", "enables": draw,
            "defaults": { "color": "#ef4444", "strokeWidth": 6, "blendMode": "normal" },
            "ink": { "groupStrokesMs": 800 } }),
        json!({ "id": "ink-highlight", "extends": "ink",
            "defaults": { "intent": "ink-highlight", "color": "#ffcd45", "strokeWidth": 14, "blendMode": "multiply" },
            "ink": { "straighten": { "deviationThreshold": 0.15, "axisSnapDegrees": 15 } } }),
        json!({ "id": "free-text", "subtype": "free-text", "cursor": "crosshair", "enables": draw,
            "defaults": { "color": HOUSE_RED, "strokeWidth": 2, "fontColor": "#ef4444" },
            "upright": true,
            "clickCreate": { "width": 180, "height": 40, "anchor": "top-left" } }),
        json!({ "id": "free-text-callout", "subtype": "free-text-callout", "cursor": "crosshair",
            "enables": draw,
            "defaults": { "color": HOUSE_RED, "strokeWidth": 6, "lineEnding": "open-arrow" },
            "upright": true }),
        json!({ "id": "highlight", "subtype": "highlight", "cursor": "default", "enables": markup,
            "defaults": { "color": "#ffe16a", "blendMode": "multiply" },
            "selection": { "kind": "markup" } }),
        json!({ "id": "redact", "subtype": "redact", "cursor": "crosshair",
            "enables": ["text-select", "annotation-draw", "annotation-edit"],
            "defaults": {
                "color": "#e44234",
                "interiorColor": "#000000",
                "fontSize": 14,
                "fontColor": "#ffffff",
                "opacity": 1,
            },
            "selection": { "kind": "markup" } }),
        json!({ "id": "underline", "subtype": "underline", "cursor": "default", "enables": markup,
            "defaults": { "color": "#ef4444" }, "selection": { "kind": "markup" } }),
        json!({ "id": "strikeout", "subtype": "strikeout", "cursor": "default", "enables": markup,
            "defaults": { "color": "#ef4444" }, "selection": { "kind": "markup" } }),
        json!({ "id": "squiggly", "subtype": "squiggly", "cursor": "default", "enables": markup,
            "defaults": { "color": "#ef4444" }, "selection": { "kind": "markup" } }),
        json!({ "id": "insert-text", "subtype": "caret", "fieldsKind": "caret", "preset": "caret",
            "cursor": "default", "enables": markup, "defaults": { "color": "#ef4444" },
            "selection": { "kind": "text-edit", "operation": "insert" } }),
        json!({ "id": "replace-text", "subtype": "strikeout", "fieldsKind": "strikeout",
            "cursor": "default", "enables": markup, "defaults": { "color": "#ef4444" },
            "selection": { "kind": "text-edit", "operation": "replace" } }),
        json!({ "id": ARMED_STAMP_TOOL_ID, "subtype": "stamp", "cursor": "copy",
            "enables": ["annotation-place", "annotation-edit"],
            "source": { "kind": "prompt", "accept": "image/png,image/jpeg,application/pdf" },
            "upright": true, "ghost": true }),
        json!({ "id": "link", "subtype": "link", "cursor": "crosshair", "enables": draw }),
        json!({ "id": "note", "subtype": "text", "cursor": "crosshair",
            "enables": ["annotation-place", "annotation-edit"],
            "defaults": { "icon": "comment", "color": "#facc15" },
            "flags": { "noZoom": true, "noRotate": true }, "upright": true, "ghost": true }),
        json!({ "id": "attachment", "subtype": "file-attachment", "cursor": "crosshair",
            "enables": ["annotation-place", "annotation-edit"], "source": { "kind": "prompt" },
            "defaults": { "icon": "paperclip", "color": "#facc15" },
            "flags": { "noZoom": true, "noRotate": true }, "upright": true, "ghost": true }),
    ]
}

fn present<'a>(def: &'a Value, key: &str) -> Option<&'a Value> {
    def.get(key).filter(|v| !v.is_null() && !epdf_kernel::is_undefined(v))
}

fn object_of(value: Option<&Value>) -> Option<Object> {
    value.and_then(Value::as_object).cloned()
}

/// `{ ...left, ...right }` when both are there, else whichever is.
fn merge_objects(left: Option<Object>, right: Option<Object>) -> Option<Object> {
    match (left, right) {
        (Some(l), Some(r)) => Some(crate::js::spread(&l, &r)),
        (l, r) => r.or(l),
    }
}

/// `left || right ? { ...left, ...right } : undefined`.
fn merge_present(left: Option<Object>, right: Option<Object>) -> Option<Object> {
    if left.is_none() && right.is_none() {
        return None;
    }
    Some(crate::js::spread(&left.unwrap_or_default(), &right.unwrap_or_default()))
}

/// `mergeDef(base, over)`: a same-id override over a definition.
fn merge_def(base: &Value, over: &Value) -> Value {
    let mut out = base.as_object().cloned().unwrap_or_default();
    for (k, v) in over.as_object().into_iter().flatten() {
        out.insert(k.clone(), v.clone());
    }
    out.insert(
        "enables".into(),
        present(over, "enables").or(present(base, "enables")).cloned().unwrap_or(Value::Null),
    );
    let set = |out: &mut Object, key: &str, value: Option<Object>| {
        match value {
            Some(v) => out.insert(key.into(), Value::Object(v)),
            None => out.shift_remove(key),
        };
    };
    set(
        &mut out,
        "meta",
        merge_present(object_of(present(base, "meta")), object_of(present(over, "meta"))),
    );
    set(
        &mut out,
        "defaults",
        merge_objects(object_of(present(base, "defaults")), object_of(present(over, "defaults"))),
    );
    set(
        &mut out,
        "flags",
        merge_present(object_of(present(base, "flags")), object_of(present(over, "flags"))),
    );
    set(
        &mut out,
        "ink",
        merge_present(object_of(present(base, "ink")), object_of(present(over, "ink"))),
    );
    set(
        &mut out,
        "afterCreate",
        merge_present(
            object_of(present(base, "afterCreate")),
            object_of(present(over, "afterCreate")),
        ),
    );
    Value::Object(out)
}

fn validate_defaults(tool: &ResolvedTool) -> Result<(), Error> {
    let Some(defaults) = &tool.defaults else { return Ok(()) };
    let Some(allowed) = tool_default_fields(&tool.subtype) else { return Ok(()) };
    for key in defaults.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(Error::new(format!(
                "[annotation] tool '{}' does not support default '{}' for '{}'",
                tool.id, key, tool.subtype
            )));
        }
    }
    Ok(())
}

fn string_of(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(str::to_string)
}

/// `new Set(list)`: each tag once, in order.
fn tag_set(value: Option<&Value>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tag in value.and_then(Value::as_array).into_iter().flatten() {
        if let Some(t) = tag.as_str()
            && !out.iter().any(|o| o == t)
        {
            out.push(t.to_string());
        }
    }
    out
}

/// `resolveGhost(ghost)`: `true` is half see-through.
fn resolve_ghost(ghost: &Value) -> Value {
    match ghost {
        Value::Bool(true) => json!({ "opacity": GHOST_OPACITY }),
        other => other.clone(),
    }
}

/// `buildToolRegistry(overrides)`: the built-ins and the app's entries,
/// resolved, keyed by id in table order.
pub fn build_tool_registry(
    overrides: &[Value],
) -> Result<IndexMap<String, Rc<ResolvedTool>>, Error> {
    let mut defs: IndexMap<String, Value> = IndexMap::new();
    for definition in default_tools() {
        defs.insert(string_of(definition.get("id")).unwrap_or_default(), definition);
    }
    for over in overrides {
        let id = match over.get("id") {
            Some(Value::String(s)) => s.clone(),
            Some(other) => crate::js::stringify(Some(other)),
            None => "undefined".into(),
        };
        let next = match defs.get(&id) {
            Some(previous) => merge_def(previous, over),
            None => over.clone(),
        };
        defs.insert(id, next);
    }
    let mut out: IndexMap<String, Rc<ResolvedTool>> = IndexMap::new();
    let mut resolving: Vec<String> = Vec::new();
    fn resolve(
        id: &str,
        defs: &IndexMap<String, Value>,
        out: &mut IndexMap<String, Rc<ResolvedTool>>,
        resolving: &mut Vec<String>,
    ) -> Result<Rc<ResolvedTool>, Error> {
        if let Some(cached) = out.get(id) {
            return Ok(cached.clone());
        }
        let Some(definition) = defs.get(id) else {
            return Err(Error::new(format!("[annotation] tool '{id}' extends an unknown tool")));
        };
        let own_id = string_of(definition.get("id")).unwrap_or_else(|| id.to_string());
        let mut base: Option<Rc<ResolvedTool>> = None;
        if let Some(extends) = string_of(present(definition, "extends"))
            && extends != id
            && !resolving.contains(&extends)
        {
            resolving.push(id.to_string());
            let resolved = resolve(&extends, defs, out, resolving);
            resolving.retain(|r| r != id);
            base = Some(resolved?);
        }
        let base = base.as_deref();
        let subtype = string_of(present(definition, "subtype"))
            .or_else(|| base.map(|b| b.subtype.clone()))
            .unwrap_or_else(|| own_id.clone());
        let ink =
            merge_present(base.and_then(|b| b.ink.clone()), object_of(present(definition, "ink")));
        let after_create = {
            let own = present(definition, "afterCreate").map(AfterCreate::read);
            match (base.and_then(|b| b.after_create.clone()), own) {
                (None, None) => None,
                (b, o) => Some(b.unwrap_or_default().over(&o.unwrap_or_default())),
            }
        };
        let resolved = ResolvedTool {
            id: own_id.clone(),
            subtype: subtype.clone(),
            preset: string_of(present(definition, "preset")).unwrap_or_else(|| own_id.clone()),
            fields_kind: string_of(present(definition, "fieldsKind"))
                .or_else(|| base.map(|b| b.fields_kind.clone()))
                .unwrap_or_else(|| subtype.clone()),
            cursor: string_of(present(definition, "cursor"))
                .or_else(|| base.map(|b| b.cursor.clone()))
                .unwrap_or_else(|| "crosshair".into()),
            enables: match present(definition, "enables") {
                Some(e) => tag_set(Some(e)),
                None => base.map(|b| b.enables.clone()).unwrap_or_default(),
            },
            defaults: merge_objects(
                base.and_then(|b| b.defaults.clone()),
                object_of(present(definition, "defaults")),
            ),
            flags: merge_present(
                base.and_then(|b| b.flags.clone()),
                object_of(present(definition, "flags")),
            ),
            source: present(definition, "source")
                .cloned()
                .or_else(|| base.and_then(|b| b.source.clone())),
            selection: present(definition, "selection")
                .cloned()
                .or_else(|| base.and_then(|b| b.selection.clone())),
            ink,
            upright: match present(definition, "upright") {
                Some(v) => epdf_annotation_core::json::truthy(Some(v)),
                None => base.is_some_and(|b| b.upright),
            },
            click_create: present(definition, "clickCreate")
                .cloned()
                .or_else(|| base.map(|b| b.click_create.clone()))
                .unwrap_or(Value::Bool(false)),
            // `resolveGhost(definition.ghost ?? base?.ghost ?? false)`.
            ghost: resolve_ghost(
                &present(definition, "ghost")
                    .filter(|g| !g.is_null())
                    .cloned()
                    .or_else(|| base.map(|b| b.ghost.clone()))
                    .unwrap_or(Value::Bool(false)),
            ),
            after_create,
            capture: CAPTURE_TOOLS.contains(&own_id.as_str()) || base.is_some_and(|b| b.capture),
            meta: merge_present(
                base.and_then(|b| b.meta.clone()),
                object_of(present(definition, "meta")),
            ),
        };
        validate_defaults(&resolved)?;
        let resolved = Rc::new(resolved);
        out.insert(id.to_string(), resolved.clone());
        Ok(resolved)
    }
    let ids: Vec<String> = defs.keys().cloned().collect();
    for id in ids {
        resolve(&id, &defs, &mut out, &mut resolving)?;
    }
    // In the order the tools resolved, as the TypeScript's map is filled:
    // a tool another extends resolves first.
    Ok(out)
}

/// The kind `fieldsKind` reads when the tool doesn't exist (`tool?.fieldsKind ?? toolId`).
pub fn fields_kind_of(tool: Option<&ResolvedTool>, tool_id: &str) -> String {
    tool.map_or_else(|| tool_id.to_string(), |t| t.fields_kind.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_ins_resolve() {
        let registry = build_tool_registry(&[]).unwrap();
        assert_eq!(registry.len(), 24);
        assert_eq!(registry["insert-text"].preset, "caret");
        assert!(registry["calibrate"].capture);
        assert_eq!(registry["distance"].subtype, "line");
    }
}
