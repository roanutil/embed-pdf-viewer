//! `tools.ts`: the tool registry. A tool is a named authoring preset over a
//! routing subtype: its defaults bag (keyed by `preset`), cursor, the
//! interaction tags it turns on, and for payload kinds how the payload
//! resolves. [`default_tools`] is the built-in table; an embedder merges
//! over it by id, adds new ids, or inherits with `extends`.

use epdf_annotation_core::{
    AnnotationFlagsPatch, AnnotationPropsPatch, ClickCreate, ClickCreateOption,
    InkStraightenOptions,
};
use epdf_kernel::Error;
use serde::de::{self, Deserializer};
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

/// `PromptSourceSpec`: ask the environment (the file-picker port).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PromptSourceSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accept: Option<String>,
}

/// `StampSourceSpec`: fixed bytes, or a prompt. `source` is the
/// `BinarySource` as canonical JSON (`{"$bytes": ...}` or a payload).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum StampSourceSpec {
    Bytes { source: Value },
    Prompt(PromptSourceSpec),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextEditOperation {
    Insert,
    Replace,
}

/// `SelectionAuthoring`: what a committed text selection authors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum SelectionAuthoring {
    Markup,
    TextEdit { operation: TextEditOperation },
}

/// `GhostPolicy`: `false`, or `{ mode: 'footprint' }`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GhostPolicy {
    #[default]
    Off,
    Footprint,
}

impl Serialize for GhostPolicy {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            GhostPolicy::Off => s.serialize_bool(false),
            GhostPolicy::Footprint => json!({ "mode": "footprint" }).serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for GhostPolicy {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::Bool(false) => Ok(GhostPolicy::Off),
            v if v.get("mode").and_then(Value::as_str) == Some("footprint") => {
                Ok(GhostPolicy::Footprint)
            }
            other => Err(de::Error::custom(format!("not a ghost policy: {other}"))),
        }
    }
}

/// `InkAuthoringOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InkAuthoringOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_strokes_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub straighten: Option<InkStraightenOptions>,
}

impl InkAuthoringOptions {
    /// `{ ...self, ...next }`.
    fn spread(&self, next: &InkAuthoringOptions) -> InkAuthoringOptions {
        InkAuthoringOptions {
            group_strokes_ms: next.group_strokes_ms.or(self.group_strokes_ms),
            straighten: next.straighten.or(self.straighten),
        }
    }
}

/// `AnnotationToolDef`: the config vocabulary. Every member but `id` is
/// optional.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationToolDef {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extends: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtype: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub props_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defaults: Option<AnnotationPropsPatch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flags: Option<AnnotationFlagsPatch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enables: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<StampSourceSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<SelectionAuthoring>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ink: Option<InkAuthoringOptions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upright: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub click_create: Option<ClickCreateOption>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ghost: Option<GhostPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Map<String, Value>>,
}

/// A fully resolved tool: what the plugin, the handlers and the
/// registration loop read.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedTool {
    pub id: String,
    /// Routing token for the draw core and the created annotation's subtype.
    pub subtype: String,
    /// The `defaults` key.
    pub preset: String,
    /// The `propsFor` key for the style panel.
    pub props_kind: String,
    pub cursor: String,
    /// A `ReadonlySet<string>`: insertion order, each tag once.
    pub enables: Vec<String>,
    pub defaults: Option<AnnotationPropsPatch>,
    pub flags: Option<AnnotationFlagsPatch>,
    pub source: Option<StampSourceSpec>,
    pub selection: Option<SelectionAuthoring>,
    pub intent: Option<String>,
    pub ink: Option<InkAuthoringOptions>,
    pub upright: bool,
    pub click_create: ClickCreateOption,
    pub ghost: GhostPolicy,
    pub meta: Option<Map<String, Value>>,
}

impl ResolvedTool {
    /// `tool.enables.has(tag)`.
    pub fn enables(&self, tag: &str) -> bool {
        self.enables.iter().any(|t| t == tag)
    }

    /// `clickCreate` when it is a policy.
    pub fn click_policy(&self) -> Option<ClickCreate> {
        match self.click_create {
            ClickCreateOption::Policy(p) => Some(p),
            ClickCreateOption::Off => None,
        }
    }

    /// The tool as the TypeScript prints it: `enables` is a `Set`.
    pub fn to_json(&self) -> Value {
        let mut out = Map::new();
        out.insert("id".into(), json!(self.id));
        out.insert("subtype".into(), json!(self.subtype));
        out.insert("preset".into(), json!(self.preset));
        out.insert("propsKind".into(), json!(self.props_kind));
        out.insert("cursor".into(), json!(self.cursor));
        out.insert("enables".into(), json!({ "$set": self.enables }));
        let mut put = |k: &str, v: Option<Value>| {
            if let Some(v) = v {
                out.insert(k.into(), v);
            }
        };
        put("defaults", self.defaults.as_ref().map(|d| serde_json::to_value(d).unwrap()));
        put("flags", self.flags.as_ref().map(|f| serde_json::to_value(f).unwrap()));
        put("source", self.source.as_ref().map(|s| serde_json::to_value(s).unwrap()));
        put("selection", self.selection.as_ref().map(|s| serde_json::to_value(s).unwrap()));
        put("intent", self.intent.as_ref().map(|i| json!(i)));
        put("ink", self.ink.as_ref().map(|i| serde_json::to_value(i).unwrap()));
        out.insert("upright".into(), json!(self.upright));
        out.insert("clickCreate".into(), serde_json::to_value(self.click_create).unwrap());
        out.insert("ghost".into(), serde_json::to_value(self.ghost).unwrap());
        if let Some(meta) = &self.meta {
            out.insert("meta".into(), Value::Object(meta.clone()));
        }
        Value::Object(out)
    }
}

/// `TOOL_DEFAULT_KEYS`: the creation properties each known authoring kind
/// consumes.
pub fn tool_default_keys(kind: &str) -> Option<&'static [&'static str]> {
    const BOX: &[&str] = &["color", "interiorColor", "opacity", "strokeWidth", "border"];
    Some(match kind {
        "square" | "circle" | "polygon" => BOX,
        "line" | "polyline" => {
            &["color", "interiorColor", "opacity", "strokeWidth", "border", "lineEndings"]
        }
        "ink" => &["color", "opacity", "strokeWidth", "blendMode"],
        "free-text" => &[
            "fontFamily",
            "fontSize",
            "fontColor",
            "textAlign",
            "opacity",
            "interiorColor",
            "color",
            "strokeWidth",
            "border",
        ],
        "free-text-callout" => &[
            "fontFamily",
            "fontSize",
            "fontColor",
            "textAlign",
            "opacity",
            "interiorColor",
            "color",
            "strokeWidth",
            "border",
            "lineEndings",
        ],
        "highlight" | "underline" | "strikeout" | "squiggly" => &["color", "opacity", "blendMode"],
        "caret" => &["color", "opacity"],
        "redact" => &[
            "color",
            "interiorColor",
            "opacity",
            "fontFamily",
            "fontSize",
            "fontColor",
            "textAlign",
        ],
        "stamp" => &[],
        "link" => &["link"],
        "text" | "file-attachment" => &["icon", "color", "opacity"],
        "widget-text" | "widget-choice" => &[
            "color",
            "interiorColor",
            "strokeWidth",
            "border",
            "fontFamily",
            "fontSize",
            "fontColor",
            "textAlign",
        ],
        "widget-toggle" => &["color", "interiorColor", "strokeWidth", "border"],
        _ => return None,
    })
}

/// `isTouchDirect(enables)`: drag-create tools own single-finger touch.
pub fn is_touch_direct(enables: &[String]) -> bool {
    enables.iter().any(|t| t == "annotation-draw" || t == "text-select")
}

fn tags(list: &[&str]) -> Option<Vec<String>> {
    Some(list.iter().map(|t| t.to_string()).collect())
}

fn patch(value: Value) -> Option<AnnotationPropsPatch> {
    Some(serde_json::from_value(value).expect("a built-in defaults bag"))
}

fn def(id: &str) -> AnnotationToolDef {
    AnnotationToolDef { id: id.into(), ..Default::default() }
}

const DRAW_TAGS: &[&str] = &["annotation-draw", "annotation-edit"];
const MARKUP_TAGS: &[&str] = &["text-select", "annotation-edit"];
const PLACE_TAGS: &[&str] = &["annotation-place", "annotation-edit"];

/// `DEFAULT_TOOLS`: the built-in table, in its TypeScript order.
pub fn default_tools() -> Vec<AnnotationToolDef> {
    let draw = |id: &str, defaults: Value, click: Option<ClickCreate>| AnnotationToolDef {
        subtype: Some(id.into()),
        cursor: Some("crosshair".into()),
        enables: tags(DRAW_TAGS),
        defaults: patch(defaults),
        click_create: click.map(ClickCreateOption::Policy),
        ..def(id)
    };
    let markup = |id: &str, defaults: Value| AnnotationToolDef {
        subtype: Some(id.into()),
        cursor: Some("default".into()),
        enables: tags(MARKUP_TAGS),
        defaults: patch(defaults),
        selection: Some(SelectionAuthoring::Markup),
        ..def(id)
    };
    let noted_flags =
        AnnotationFlagsPatch { no_zoom: Some(true), no_rotate: Some(true), ..Default::default() };
    vec![
        draw("square", json!({ "strokeWidth": 6 }), Some(ClickCreate::boxed(80.0, 60.0, None))),
        draw("circle", json!({ "strokeWidth": 6 }), Some(ClickCreate::boxed(80.0, 60.0, None))),
        draw("line", json!({ "strokeWidth": 6 }), Some(ClickCreate::segment(80.0, None))),
        draw("polygon", json!({ "strokeWidth": 6 }), None),
        draw("polyline", json!({ "strokeWidth": 6 }), None),
        AnnotationToolDef {
            ink: Some(InkAuthoringOptions { group_strokes_ms: Some(800.0), straighten: None }),
            ..draw(
                "ink",
                json!({ "color": "#ef4444", "strokeWidth": 6, "blendMode": "normal" }),
                None,
            )
        },
        AnnotationToolDef {
            extends: Some("ink".into()),
            intent: Some("ink-highlight".into()),
            defaults: patch(
                json!({ "color": "#ffcd45", "strokeWidth": 14, "blendMode": "multiply" }),
            ),
            ink: Some(InkAuthoringOptions {
                group_strokes_ms: None,
                straighten: Some(InkStraightenOptions {
                    deviation_threshold: 0.15,
                    axis_snap_degrees: 15.0,
                }),
            }),
            ..def("ink-highlight")
        },
        AnnotationToolDef {
            upright: Some(true),
            ..draw(
                "free-text",
                json!({ "fontColor": "#ef4444" }),
                Some(ClickCreate::boxed(
                    180.0,
                    40.0,
                    Some(epdf_annotation_core::BoxAnchor::TopLeft),
                )),
            )
        },
        AnnotationToolDef {
            props_kind: Some("free-text".into()),
            upright: Some(true),
            ..draw(
                "free-text-callout",
                json!({ "strokeWidth": 6, "lineEndings": { "end": "open-arrow" } }),
                None,
            )
        },
        markup("highlight", json!({ "color": "#ffe16a", "blendMode": "multiply" })),
        AnnotationToolDef {
            subtype: Some("redact".into()),
            cursor: Some("crosshair".into()),
            enables: tags(&["text-select", "annotation-draw", "annotation-edit"]),
            defaults: patch(json!({
                "color": "#e44234",
                "interiorColor": "#000000",
                "fontColor": "#ffffff",
                "opacity": 1,
            })),
            selection: Some(SelectionAuthoring::Markup),
            ..def("redact")
        },
        markup("underline", json!({ "color": "#ef4444" })),
        markup("strikeout", json!({ "color": "#ef4444" })),
        markup("squiggly", json!({ "color": "#ef4444" })),
        AnnotationToolDef {
            subtype: Some("caret".into()),
            props_kind: Some("caret".into()),
            preset: Some("caret".into()),
            selection: Some(SelectionAuthoring::TextEdit { operation: TextEditOperation::Insert }),
            ..markup("insert-text", json!({ "color": "#ef4444" }))
        },
        AnnotationToolDef {
            subtype: Some("strikeout".into()),
            props_kind: Some("strikeout".into()),
            selection: Some(SelectionAuthoring::TextEdit { operation: TextEditOperation::Replace }),
            ..markup("replace-text", json!({ "color": "#ef4444" }))
        },
        AnnotationToolDef {
            subtype: Some("stamp".into()),
            cursor: Some("copy".into()),
            enables: tags(PLACE_TAGS),
            source: Some(StampSourceSpec::Prompt(PromptSourceSpec {
                accept: Some("image/png,image/jpeg,application/pdf".into()),
            })),
            upright: Some(true),
            ghost: Some(GhostPolicy::Footprint),
            ..def("stamp")
        },
        AnnotationToolDef {
            subtype: Some("link".into()),
            cursor: Some("crosshair".into()),
            enables: tags(DRAW_TAGS),
            ..def("link")
        },
        AnnotationToolDef {
            subtype: Some("text".into()),
            cursor: Some("crosshair".into()),
            enables: tags(PLACE_TAGS),
            defaults: patch(json!({ "icon": "comment", "color": "#facc15" })),
            flags: Some(noted_flags),
            upright: Some(true),
            ghost: Some(GhostPolicy::Footprint),
            ..def("note")
        },
        AnnotationToolDef {
            subtype: Some("file-attachment".into()),
            cursor: Some("copy".into()),
            enables: tags(PLACE_TAGS),
            source: Some(StampSourceSpec::Prompt(PromptSourceSpec { accept: None })),
            defaults: patch(json!({ "icon": "paperclip", "color": "#facc15" })),
            flags: Some(noted_flags),
            upright: Some(true),
            ghost: Some(GhostPolicy::Footprint),
            ..def("attachment")
        },
    ]
}

/// `mergeDefaults(a, b)`: `b` over `a`, line endings per side.
fn merge_defaults(
    a: Option<&AnnotationPropsPatch>,
    b: Option<&AnnotationPropsPatch>,
) -> Option<AnnotationPropsPatch> {
    let Some(a) = a else { return b.cloned() };
    let Some(b) = b else { return Some(a.clone()) };
    let mut merged = a.spread(b);
    if a.line_endings.is_some() || b.line_endings.is_some() {
        let base = a.line_endings.unwrap_or_default();
        merged.line_endings = Some(base.merge(&b.line_endings.unwrap_or_default()));
    }
    Some(merged)
}

fn spread_flags(
    a: Option<&AnnotationFlagsPatch>,
    b: Option<&AnnotationFlagsPatch>,
) -> Option<AnnotationFlagsPatch> {
    if a.is_none() && b.is_none() {
        return None;
    }
    let (a, b) = (a.copied().unwrap_or_default(), b.copied().unwrap_or_default());
    Some(AnnotationFlagsPatch {
        invisible: b.invisible.or(a.invisible),
        hidden: b.hidden.or(a.hidden),
        print: b.print.or(a.print),
        no_zoom: b.no_zoom.or(a.no_zoom),
        no_rotate: b.no_rotate.or(a.no_rotate),
        no_view: b.no_view.or(a.no_view),
        read_only: b.read_only.or(a.read_only),
        locked: b.locked.or(a.locked),
        toggle_no_view: b.toggle_no_view.or(a.toggle_no_view),
        locked_contents: b.locked_contents.or(a.locked_contents),
    })
}

fn spread_ink(
    a: Option<&InkAuthoringOptions>,
    b: Option<&InkAuthoringOptions>,
) -> Option<InkAuthoringOptions> {
    if a.is_none() && b.is_none() {
        return None;
    }
    Some(a.copied().unwrap_or_default().spread(&b.copied().unwrap_or_default()))
}

fn spread_meta(
    a: Option<&Map<String, Value>>,
    b: Option<&Map<String, Value>>,
) -> Option<Map<String, Value>> {
    if a.is_none() && b.is_none() {
        return None;
    }
    let mut out = a.cloned().unwrap_or_default();
    for (k, v) in b.into_iter().flatten() {
        out.insert(k.clone(), v.clone());
    }
    Some(out)
}

/// `mergeDef(base, over)`: configure a built-in.
fn merge_def(base: &AnnotationToolDef, over: &AnnotationToolDef) -> AnnotationToolDef {
    AnnotationToolDef {
        id: over.id.clone(),
        extends: over.extends.clone().or_else(|| base.extends.clone()),
        subtype: over.subtype.clone().or_else(|| base.subtype.clone()),
        props_kind: over.props_kind.clone().or_else(|| base.props_kind.clone()),
        preset: over.preset.clone().or_else(|| base.preset.clone()),
        defaults: merge_defaults(base.defaults.as_ref(), over.defaults.as_ref()),
        flags: spread_flags(base.flags.as_ref(), over.flags.as_ref()),
        cursor: over.cursor.clone().or_else(|| base.cursor.clone()),
        enables: over.enables.clone().or_else(|| base.enables.clone()),
        source: over.source.clone().or_else(|| base.source.clone()),
        selection: over.selection.or(base.selection),
        intent: over.intent.clone().or_else(|| base.intent.clone()),
        ink: spread_ink(base.ink.as_ref(), over.ink.as_ref()),
        upright: over.upright.or(base.upright),
        click_create: over.click_create.or(base.click_create),
        ghost: over.ghost.or(base.ghost),
        meta: spread_meta(base.meta.as_ref(), over.meta.as_ref()),
    }
}

fn validate_defaults(tool: &ResolvedTool) -> Result<(), Error> {
    let Some(defaults) = &tool.defaults else { return Ok(()) };
    let Some(allowed) = tool_default_keys(&tool.subtype) else { return Ok(()) };
    for key in defaults.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(Error::new(format!(
                "[annotation] tool '{}' does not support default '{key}' for '{}'",
                tool.id, tool.subtype
            )));
        }
    }
    Ok(())
}

/// An insertion-ordered map of resolved tools by id: `Map.set` on an
/// existing id keeps its position, a new id appends.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ToolRegistry(Vec<ResolvedTool>);

impl ToolRegistry {
    pub fn get(&self, id: &str) -> Option<&ResolvedTool> {
        self.0.iter().find(|t| t.id == id)
    }

    pub fn set(&mut self, tool: ResolvedTool) {
        match self.0.iter_mut().find(|t| t.id == tool.id) {
            Some(slot) => *slot = tool,
            None => self.0.push(tool),
        }
    }

    pub fn delete(&mut self, id: &str) -> bool {
        let before = self.0.len();
        self.0.retain(|t| t.id != id);
        self.0.len() != before
    }

    pub fn values(&self) -> impl Iterator<Item = &ResolvedTool> {
        self.0.iter()
    }
}

struct Resolver {
    defs: Vec<AnnotationToolDef>,
    out: ToolRegistry,
    resolving: Vec<String>,
}

impl Resolver {
    fn resolve(&mut self, id: &str) -> Result<ResolvedTool, Error> {
        if let Some(cached) = self.out.get(id) {
            return Ok(cached.clone());
        }
        let Some(def) = self.defs.iter().find(|d| d.id == id).cloned() else {
            return Err(Error::new(format!("[annotation] tool '{id}' extends an unknown tool")));
        };
        let mut base: Option<ResolvedTool> = None;
        if let Some(ext) = def.extends.as_deref().filter(|e| !e.is_empty())
            && ext != id
            && !self.resolving.iter().any(|r| r == ext)
        {
            self.resolving.push(id.to_string());
            let resolved = self.resolve(ext);
            self.resolving.retain(|r| r != id);
            base = Some(resolved?);
        }
        let base = base.as_ref();
        let subtype = def
            .subtype
            .clone()
            .or_else(|| base.map(|b| b.subtype.clone()))
            .unwrap_or_else(|| def.id.clone());
        let mut enables: Vec<String> = Vec::new();
        for tag in
            def.enables.clone().or_else(|| base.map(|b| b.enables.clone())).unwrap_or_default()
        {
            if !enables.contains(&tag) {
                enables.push(tag);
            }
        }
        let resolved = ResolvedTool {
            id: def.id.clone(),
            preset: def.preset.clone().unwrap_or_else(|| def.id.clone()),
            props_kind: def
                .props_kind
                .clone()
                .or_else(|| base.map(|b| b.props_kind.clone()))
                .unwrap_or_else(|| subtype.clone()),
            cursor: def
                .cursor
                .clone()
                .or_else(|| base.map(|b| b.cursor.clone()))
                .unwrap_or_else(|| "crosshair".into()),
            enables,
            defaults: merge_defaults(base.and_then(|b| b.defaults.as_ref()), def.defaults.as_ref()),
            flags: spread_flags(base.and_then(|b| b.flags.as_ref()), def.flags.as_ref()),
            source: def.source.clone().or_else(|| base.and_then(|b| b.source.clone())),
            selection: def.selection.or_else(|| base.and_then(|b| b.selection)),
            intent: def.intent.clone().or_else(|| base.and_then(|b| b.intent.clone())),
            ink: spread_ink(base.and_then(|b| b.ink.as_ref()), def.ink.as_ref()),
            upright: def.upright.or_else(|| base.map(|b| b.upright)).unwrap_or(false),
            click_create: def
                .click_create
                .or_else(|| base.map(|b| b.click_create))
                .unwrap_or(ClickCreateOption::Off),
            ghost: def.ghost.or_else(|| base.map(|b| b.ghost)).unwrap_or(GhostPolicy::Off),
            meta: spread_meta(base.and_then(|b| b.meta.as_ref()), def.meta.as_ref()),
            subtype,
        };
        validate_defaults(&resolved)?;
        self.out.set(resolved.clone());
        Ok(resolved)
    }
}

/// `buildToolRegistry(overrides)`: the built-ins plus embedder overrides,
/// resolved.
pub fn build_tool_registry(overrides: &[AnnotationToolDef]) -> Result<ToolRegistry, Error> {
    let mut defs: Vec<AnnotationToolDef> = Vec::new();
    let mut set = |d: AnnotationToolDef| match defs.iter_mut().find(|x| x.id == d.id) {
        Some(slot) => *slot = d,
        None => defs.push(d),
    };
    for d in default_tools() {
        set(d);
    }
    for o in overrides {
        let merged = match defs.iter().find(|x| x.id == o.id) {
            Some(prev) => merge_def(prev, o),
            None => o.clone(),
        };
        match defs.iter_mut().find(|x| x.id == merged.id) {
            Some(slot) => *slot = merged,
            None => defs.push(merged),
        }
    }
    let ids: Vec<String> = defs.iter().map(|d| d.id.clone()).collect();
    let mut resolver = Resolver { defs, out: ToolRegistry::default(), resolving: Vec::new() };
    for id in ids {
        resolver.resolve(&id)?;
    }
    Ok(resolver.out)
}
