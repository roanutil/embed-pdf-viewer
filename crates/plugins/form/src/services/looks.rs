//! `services/looks.ts`: what a shown widget looks like now, the engine's
//! picture or the painter's drawing of it (`epdf-form-core`).
//!
//! A widget's look is everything its picture depends on: its size and turn,
//! its style, and what it shows (the value, as this session formatted it).
//! Its place isn't, nor the state a toggle shows. A widget is `pending`
//! while its look in the view isn't its confirmed look: a value typed or
//! calculated, a restyle, a field just placed. The layer then draws the
//! painter's scene.
//!
//! The TypeScript memoizes each read by its inputs' identities; nothing
//! here compares by identity, so each read is computed when asked.

use std::rc::Rc;

use epdf_annotation_core::types::Painter;
use epdf_form_core::{
    FieldFacts, SceneNode, ShownValue, TextEditorLayout, WidgetPlacement, WidgetRow, paints,
    text_editor_layout, widget_drawing, widget_scene,
};
use epdf_geometry::Rect;
use serde_json::{Value, json};

use crate::model::{
    FieldIndex, ShownWidget, field_for_widget, picture_version_of, shown_widgets_of, widget_number,
    widget_row_of,
};
use crate::value::{get, items, json_stringify, str_of};

/// `WidgetPainting`: a widget as the painter draws it, its scene on the page.
#[derive(Clone, Debug)]
pub struct WidgetPainting {
    pub scene: Vec<SceneNode>,
}

/// `numberOf(ref)`: an object-number ref's number, else -1.
fn number_of(reference: &Value) -> f64 {
    if str_of(reference, "kind") == Some("objectNumber") {
        get(reference, "objectNumber").and_then(Value::as_f64).unwrap_or(f64::NAN)
    } else {
        -1.0
    }
}

fn or_null(v: Option<&Value>) -> Value {
    match v {
        Some(v) if !v.is_null() => v.clone(),
        _ => Value::Null,
    }
}

fn rect_of(value: &Value) -> Rect {
    let n = |k: &str| get(value, k).and_then(Value::as_f64).unwrap_or(f64::NAN);
    Rect { x: n("x"), y: n("y"), width: n("width"), height: n("height") }
}

impl crate::controller::Core {
    /// `lookKeyOf(row, field)`: what the widget's picture depends on, as one string.
    fn look_key_of(&self, row: &Value, field: Option<&Value>) -> String {
        let bx = get(row, "box").filter(|b| !b.is_null()).or_else(|| get(row, "rect"));
        let member = |key: &str| field.and_then(|f| get(f, key)).cloned().unwrap_or(Value::Null);
        let formatted = |field: &Value| {
            self.formatted.text_of(field).map(Value::String).unwrap_or_else(|| member("value"))
        };
        let content = match field.and_then(|f| str_of(f, "family")) {
            Some("text") => json!([
                formatted(field.unwrap()),
                member("multiline"),
                member("password"),
                member("comb"),
                member("maxLength"),
            ]),
            Some("combobox") => json!([formatted(field.unwrap())]),
            Some("listbox") => {
                let options: Vec<Value> = field
                    .map(|f| items(f, "options"))
                    .unwrap_or(&[])
                    .iter()
                    .map(|o| json!([or_null(get(o, "label")), or_null(get(o, "selected"))]))
                    .collect();
                json!([member("topIndex"), options])
            }
            _ => json!([field.and_then(|f| get(f, "family")).cloned().unwrap_or(Value::Null)]),
        };
        let size = |key: &str| bx.and_then(|b| get(b, key)).cloned().unwrap_or(Value::Null);
        let rotation = match get(row, "rotation") {
            Some(v) if !v.is_null() => v.clone(),
            _ => json!(0),
        };
        json_stringify(&json!([
            size("width"),
            size("height"),
            rotation,
            or_null(get(row, "color")),
            or_null(get(row, "interiorColor")),
            or_null(get(row, "strokeWidth")),
            or_null(get(row, "borderStyle")),
            or_null(get(row, "dashArray")),
            or_null(get(row, "fontFamily")),
            or_null(get(row, "fontSize")),
            or_null(get(row, "fontColor")),
            or_null(get(row, "textAlign")),
            or_null(get(row, "caption")),
            or_null(get(row, "checkStyle")),
            content,
        ]))
    }

    /// `lookOf(index, number)`.
    fn look_of(&self, index: &FieldIndex, number: f64) -> Option<String> {
        let row = widget_row_of(index, number)?;
        Some(self.look_key_of(row, field_for_widget(index, number)))
    }

    /// `listShownWidgets(page)`: the widgets a page shows, in the order the
    /// page draws them; in design mode none, the annotation layer draws them.
    pub fn list_shown_widgets(&self, page: &Value) -> Vec<ShownWidget> {
        let Some(info) = self.get_page(page) else { return Vec::new() };
        let page = get(&info, "ref").and_then(|r| get(r, "objectNumber")).and_then(Value::as_f64);
        let Some(page) = page else { return Vec::new() };
        if self.lending() {
            return Vec::new();
        }
        let view = self.view();
        let truth = self.truth();
        shown_widgets_of(&view, page, &|row| {
            let number = number_of(get(row, "ref").unwrap_or(&Value::Null));
            (
                self.look_of(&view, number) != self.look_of(&truth, number),
                picture_version_of(&truth, number),
            )
        })
    }

    /// `looks.isPending(ref)`: the widget's look in the view isn't its
    /// confirmed look yet.
    pub fn widget_is_pending(&self, reference: &Value) -> bool {
        let number = number_of(reference);
        self.look_of(&self.view(), number) != self.look_of(&self.truth(), number)
    }

    /// `paint(row, box, rotation)`: the painter's drawing of `row` in the
    /// view's field, as this session formatted it, at `box`, turned.
    fn paint(&self, row: &Value, bx: Rect, rotation: Option<f64>) -> Option<Vec<SceneNode>> {
        let view = self.view();
        let field = field_for_widget(&view, number_of(get(row, "ref").unwrap_or(&Value::Null)))?;
        let text = self.formatted.text_of(field);
        let mut widget: WidgetRow = serde_json::from_value(row.clone()).ok()?;
        widget.box_ = Some(bx);
        let facts: FieldFacts = serde_json::from_value(field.clone()).ok()?;
        let shown = ShownValue { text, checked: None };
        let drawing = widget_drawing(&widget, &facts, &shown)?;
        Some(widget_scene(&drawing, &WidgetPlacement { box_: bx, rotation }))
    }

    /// `getWidgetPainting(widget)`: the painter's drawing of a widget as the
    /// view has it; `None` for one it doesn't draw.
    pub fn get_widget_painting(&self, reference: &Value) -> Option<WidgetPainting> {
        let view = self.view();
        let row = widget_row_of(&view, number_of(reference))?.clone();
        let bx = get(&row, "box").filter(|b| !b.is_null()).or_else(|| get(&row, "rect"));
        let bx = rect_of(bx.unwrap_or(&Value::Null));
        let rotation = get(&row, "rotation").and_then(Value::as_f64);
        self.paint(&row, bx, rotation).map(|scene| WidgetPainting { scene })
    }

    /// `looks.painterOf(row)`: how the painter draws a widget row live,
    /// upright in a box; `None` for one whose field it doesn't draw.
    pub(crate) fn painter_of(&self, row: &Value) -> Painter {
        let view = self.view();
        let Some(field) = field_for_widget(&view, widget_number(row)) else { return Painter(None) };
        let Ok(facts) = serde_json::from_value::<FieldFacts>(field.clone()) else {
            return Painter(None);
        };
        if !paints(facts.family()) {
            return Painter(None);
        }
        let me = self.weak();
        let row = row.clone();
        Painter(Some(Rc::new(move |bx: Rect| {
            me.upgrade().and_then(|core| core.paint(&row, bx, Some(0.0))).unwrap_or_default()
        })))
    }

    /// `getTextEditorLayout(widget, text)`: where a text field's editor
    /// shows `text`; `None` for a widget that isn't a text field's.
    pub fn get_text_editor_layout(
        &self,
        reference: &Value,
        text: &str,
    ) -> Option<TextEditorLayout> {
        let view = self.view();
        let number = number_of(reference);
        let row = widget_row_of(&view, number)?;
        let field = field_for_widget(&view, number)?;
        let widget: WidgetRow = serde_json::from_value(row.clone()).ok()?;
        let facts: FieldFacts = serde_json::from_value(field.clone()).ok()?;
        text_editor_layout(&widget, &facts, text)
    }
}
