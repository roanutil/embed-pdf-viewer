//! `tools/definitions.ts`: the form-tool table, the one source of truth for the palette.
//! The tools live in the annotation registry when that plugin is present,
//! but the commit always goes through `createField`: they enable
//! `form-place`, never `annotation-draw`.

use std::sync::LazyLock;

use serde::Serialize;
use serde_json::{Value, json};

/// A click-create box policy.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct ClickSize {
    pub width: f64,
    pub height: f64,
}

/// `FormToolDef`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormToolDef {
    pub id: &'static str,
    /// The field family `createField` commits (`AuthorableFormFamily`).
    pub family: &'static str,
    /// The client kind the annotation registry routes on.
    pub visual_kind: &'static str,
    /// What a bare click places.
    pub click_create: ClickSize,
    /// Seed drawing defaults, an `AnnotationPropsPatch` as JSON.
    pub defaults: Value,
    pub cursor: &'static str,
}

/// `PLACE_TAGS`: palette tools keep widgets editable right after placement.
pub const PLACE_TAGS: [&str; 2] = ["form-place", "annotation-edit"];

fn field_chrome(font_size: bool) -> Value {
    let mut chrome = json!({ "interiorColor": "#ffffff", "color": "#6b7280", "strokeWidth": 1 });
    if font_size {
        chrome["fontSize"] = json!(12);
    }
    chrome
}

fn tool(
    id: &'static str,
    family: &'static str,
    visual_kind: &'static str,
    (width, height): (f64, f64),
    font_size: bool,
) -> FormToolDef {
    FormToolDef {
        id,
        family,
        visual_kind,
        click_create: ClickSize { width, height },
        defaults: field_chrome(font_size),
        cursor: "crosshair",
    }
}

/// `FORM_TOOLS`.
pub static FORM_TOOLS: LazyLock<Vec<FormToolDef>> = LazyLock::new(|| {
    vec![
        tool("form-text", "text", "widget-text", (160.0, 24.0), true),
        tool("form-checkbox", "checkbox", "widget-toggle", (18.0, 18.0), false),
        tool("form-radio", "radio", "widget-toggle", (18.0, 18.0), false),
        tool("form-combobox", "combobox", "widget-choice", (140.0, 24.0), true),
        tool("form-listbox", "listbox", "widget-choice", (140.0, 72.0), true),
        // A signature field: the bare widget box; the signing act (the
        // signature plugin) draws the mark into it later.
        tool("form-signature", "signature", "widget-box", (160.0, 48.0), false),
    ]
});

/// `FORM_TOOL_BY_ID.get(id)`.
pub fn form_tool_by_id(id: &str) -> Option<&'static FormToolDef> {
    FORM_TOOLS.iter().find(|t| t.id == id)
}
