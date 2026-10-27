//! `tools/definitions.ts`: the form-tool table, the one source of truth for
//! the palette. The tools live in the annotation registry when that plugin
//! is present, but the commit always goes through `form.create()`: they
//! enable `form-place`, never `annotation-draw`.

use std::sync::LazyLock;

use serde_json::{Value, json};

/// `FormToolDef`.
#[derive(Clone, Debug, PartialEq)]
pub struct FormToolDef {
    pub id: &'static str,
    /// The field family the tool places.
    pub family: &'static str,
    /// The client kind the annotation registry routes on.
    pub visual_kind: &'static str,
    /// What a bare click places.
    pub click_create: (f64, f64),
    /// Seed drawing defaults: the widget's appearance fields.
    pub defaults: Value,
    pub cursor: &'static str,
}

impl FormToolDef {
    /// The tool as the TypeScript object prints.
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "family": self.family,
            "visualKind": self.visual_kind,
            "clickCreate": self.click_create_json(),
            "defaults": self.defaults,
            "cursor": self.cursor,
        })
    }

    /// `clickCreate`: `{ width, height }`.
    pub fn click_create_json(&self) -> Value {
        json!({ "width": self.click_create.0, "height": self.click_create.1 })
    }
}

/// `PLACE_TAGS`: palette tools keep widgets editable right after placement.
pub const PLACE_TAGS: [&str; 2] = ["form-place", "annotation-edit"];

fn chrome(interior: &str, font_size: bool) -> Value {
    let mut chrome = json!({ "interiorColor": interior, "color": "#6b7280", "strokeWidth": 1 });
    if font_size {
        chrome["fontSize"] = json!(12);
    }
    chrome
}

fn tool(
    id: &'static str,
    family: &'static str,
    visual_kind: &'static str,
    click_create: (f64, f64),
    defaults: Value,
) -> FormToolDef {
    FormToolDef { id, family, visual_kind, click_create, defaults, cursor: "crosshair" }
}

/// `FORM_TOOLS`.
pub static FORM_TOOLS: LazyLock<Vec<FormToolDef>> = LazyLock::new(|| {
    vec![
        tool("form-text", "text", "widget-text", (160.0, 24.0), chrome("#ffffff", true)),
        tool("form-checkbox", "checkbox", "widget-toggle", (18.0, 18.0), chrome("#ffffff", false)),
        // Round, as the engine draws a radio button: its ghost and its placement too.
        tool("form-radio", "radio", "widget-radio", (18.0, 18.0), chrome("#ffffff", false)),
        tool("form-combobox", "combobox", "widget-choice", (140.0, 24.0), chrome("#ffffff", true)),
        tool("form-listbox", "listbox", "widget-choice", (140.0, 72.0), chrome("#ffffff", true)),
        // A push button: its caption is its text; what it does is its actions.
        tool("form-button", "pushbutton", "widget-button", (100.0, 28.0), chrome("#e5e7eb", true)),
        // A signature field: the bare widget box; the signing act (the
        // signature plugin) draws the mark into it later.
        tool("form-signature", "signature", "widget-box", (160.0, 48.0), chrome("#ffffff", false)),
    ]
});

/// `FORM_TOOL_BY_ID.get(id)`.
pub fn form_tool_by_id(id: &str) -> Option<&'static FormToolDef> {
    FORM_TOOLS.iter().find(|t| t.id == id)
}
