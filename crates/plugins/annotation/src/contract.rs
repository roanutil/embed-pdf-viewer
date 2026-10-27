//! `contract.ts`: the public protocol's settings and constants. The
//! capability itself is [`crate::AnnotationCapability`], one object for the
//! public lens and the host lens (`host-contract.ts`), as the TypeScript's
//! two lenses are one runtime object.
//!
//! The engine's annotations, drafts, patches, refs and targets stay JSON
//! (`serde_json::Value`), as `epdf-annotation-core` keeps them; events carry
//! JSON payloads in the TypeScript's member order.

use std::rc::Rc;

use epdf_annotation_core::object::Json;
use epdf_annotation_core::types::{EditableKinds, SnapSettings};
use serde_json::{Value, json};

/// The tool `stamps.arm` activates.
pub use crate::tools::definitions::ARMED_STAMP_TOOL_ID;

/// `ANNOTATION_DEFAULTS`: the settings when the app registers none.
pub fn annotation_defaults() -> Value {
    json!({
        "tools": [],
        "afterCreate": { "select": true, "tool": "stay", "editText": true },
        "chrome": {
            "accent": null,
            "outline": { "color": null, "style": "solid", "width": 1 },
            "handles": { "shape": "square", "size": 8, "hitSize": 24, "fill": "#ffffff", "stroke": null },
            "rotationHandle": {
                "enabled": true,
                "size": 10,
                "hitSize": 24,
                "offset": 32,
                "stalk": true,
                "fill": null,
                "stroke": null,
            },
            "guides": { "enabled": true, "color": "#e91e63", "rotationColor": null, "style": "solid", "width": 1.5 },
            "marquee": { "fill": null, "stroke": null },
            "textOutline": null,
            "readout": { "enabled": true, "background": "rgb(0 0 0 / 0.8)", "color": "#ffffff" },
        },
        "snap": {
            "alignment": true,
            "alignmentThreshold": 6,
            "rotation": true,
            "rotationAngles": [0, 90, 180, 270],
            "rotationThreshold": 4,
        },
        "editable": true,
    })
}

/// The settings members `update_settings` replaces whole rather than
/// merging: `tools` is a list, and so may `editable` be.
pub const WHOLE_SETTINGS: [&str; 0] = [];

/// The parts of the chrome settings the plugin reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChromeNumbers {
    pub handle_hit_size: f64,
    pub knob_hit_size: f64,
    pub knob_offset: f64,
    pub rotation_handle: bool,
    pub guides: bool,
}

impl ChromeNumbers {
    pub fn read(chrome: &Value) -> ChromeNumbers {
        let n = |path: &str| {
            chrome.pointer(path).and_then(epdf_annotation_core::json::number).unwrap_or(f64::NAN)
        };
        let b = |path: &str| {
            chrome.pointer(path).is_some_and(|v| epdf_annotation_core::json::truthy(Some(v)))
        };
        ChromeNumbers {
            handle_hit_size: n("/handles/hitSize"),
            knob_hit_size: n("/rotationHandle/hitSize"),
            knob_offset: n("/rotationHandle/offset"),
            rotation_handle: b("/rotationHandle/enabled"),
            guides: b("/guides/enabled"),
        }
    }
}

/// `AfterCreateSettings`, possibly partial (a tool's own).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AfterCreate {
    pub select: Option<bool>,
    pub tool: Option<String>,
    pub edit_text: Option<bool>,
}

impl AfterCreate {
    pub fn read(value: &Value) -> AfterCreate {
        let present = |k: &str| value.get(k).filter(|v| !epdf_kernel::is_undefined(v));
        AfterCreate {
            select: present("select").map(|v| epdf_annotation_core::json::truthy(Some(v))),
            tool: present("tool").and_then(Value::as_str).map(str::to_string),
            edit_text: present("editText").map(|v| epdf_annotation_core::json::truthy(Some(v))),
        }
    }

    /// `{ ...self, ...over }`.
    pub fn over(&self, over: &AfterCreate) -> AfterCreate {
        AfterCreate {
            select: over.select.or(self.select),
            tool: over.tool.clone().or_else(|| self.tool.clone()),
            edit_text: over.edit_text.or(self.edit_text),
        }
    }

    pub fn to_json(&self) -> Value {
        let mut out = serde_json::Map::new();
        if let Some(s) = self.select {
            out.insert("select".into(), Value::Bool(s));
        }
        if let Some(t) = &self.tool {
            out.insert("tool".into(), Value::String(t.clone()));
        }
        if let Some(e) = self.edit_text {
            out.insert("editText".into(), Value::Bool(e));
        }
        Value::Object(out)
    }
}

/// The settings as the plugin reads them, parsed once per settings value so
/// `snap` and `editable` keep their identity until the settings change.
#[derive(Clone)]
pub struct ParsedSettings {
    pub value: Rc<Value>,
    pub snap: Rc<SnapSettings>,
    pub editable: Rc<EditableKinds>,
    pub chrome: ChromeNumbers,
    pub after_create: AfterCreate,
}

impl ParsedSettings {
    pub fn parse(value: Rc<Value>) -> ParsedSettings {
        let snap = SnapSettings::read(&value["snap"]).unwrap_or_else(|_| {
            SnapSettings::read(&annotation_defaults()["snap"]).expect("default snap")
        });
        let editable =
            EditableKinds::from_json(value.get("editable")).unwrap_or(EditableKinds::All(true));
        ParsedSettings {
            snap: Rc::new(snap),
            editable: Rc::new(editable),
            chrome: ChromeNumbers::read(&value["chrome"]),
            after_create: AfterCreate::read(&value["afterCreate"]),
            value,
        }
    }

    /// `settings.get().tools`.
    pub fn tools(&self) -> Vec<Value> {
        self.value.get("tools").and_then(Value::as_array).cloned().unwrap_or_default()
    }
}
