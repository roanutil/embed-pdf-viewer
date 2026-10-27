//! `scripting/world.ts`: script world building (page-scoped prefetch) and
//! the two-standard event bridge: the /AA keys are ISO 32000-2 Table 200,
//! the event names the JavaScript API layer's (ISO 21757-1, Acrobat).

use epdf_acrojs::{
    ScriptAnnotInput, ScriptColorArray, ScriptEventInput, ScriptEventKind, ScriptWorldInput,
    script_fields_from_snapshot,
};
use std::rc::Rc;

use epdf_kernel::Error;
use serde_json::Value;

use crate::ActionsCapability;
use crate::contract::{
    ActionContext, ActionSource, ActionTriggerEvent, DocumentTriggerEvent, PageEventKind,
    PdfAnnotationEventKind,
};
use crate::services::{get, same_number, str_of, truthy};

/// `parseInt(text, 16)`: the leading hex digits, `NaN` when there are none.
fn parse_hex(text: &str) -> f64 {
    let mut value = f64::NAN;
    for c in text.chars() {
        let Some(digit) = c.to_digit(16) else { break };
        value = if value.is_nan() { 0.0 } else { value } * 16.0 + f64::from(digit);
    }
    value
}

/// `ToInt32(x)`, for the shifts of `rgbOf`.
fn to_int32(x: f64) -> i32 {
    if !x.is_finite() {
        return 0;
    }
    (x.trunc().rem_euclid(4_294_967_296.0) as u32) as i32
}

/// Acrobat's JavaScript keeps colors as color arrays, so an engine color
/// (`#rrggbb`) converts here, at its edge (`rgbOf`).
fn engine_color_to_array(color: Option<&Value>) -> ScriptColorArray {
    match color.filter(|c| truthy(Some(c))) {
        Some(c) => {
            let text = c.as_str().map_or_else(|| epdf_acrojs::js::string_of(c), str::to_string);
            let value = to_int32(parse_hex(text.get(1..).unwrap_or_default()));
            let channel = |shift: i32| f64::from((value >> shift) & 0xff) / 255.0;
            ScriptColorArray::Rgb(channel(16), channel(8), channel(0))
        }
        None => ScriptColorArray::T,
    }
}

/// `pdfRectOf(rect, visible)` as `[llx, lly, urx, ury]`: Acrobat's
/// JavaScript keeps an annotation's rect in the file's numbers.
fn rect_in_file(rect: Option<&Value>, crop: Option<&Value>) -> [f64; 4] {
    let (Some(rect), Some(crop)) = (rect, crop) else { return [0.0, 0.0, 0.0, 0.0] };
    let n = |v: &Value, k: &str| get(v, k).and_then(Value::as_f64).unwrap_or(f64::NAN);
    let left = n(rect, "x") + n(crop, "left");
    let top = n(crop, "top") - n(rect, "y");
    [left, top - n(rect, "height"), left + n(rect, "width"), top]
}

fn script_annot(annotation: &Value, page_index: f64, crop: Option<&Value>) -> ScriptAnnotInput {
    let text = |key: &str| {
        get(annotation, key)
            .filter(|v| !v.is_null())
            .map(epdf_acrojs::js::string_of)
            .unwrap_or_default()
    };
    let flag = |k: &str| get(annotation, k).and_then(Value::as_bool).unwrap_or(false);
    let number_or = |k: &str, fallback: f64| {
        get(annotation, k).filter(|v| !v.is_null()).and_then(Value::as_f64).unwrap_or(fallback)
    };
    let subtype = text("subtype");
    ScriptAnnotInput {
        annot_ref: get(annotation, "ref").cloned().unwrap_or(Value::Null),
        name: text("nm"),
        page: page_index,
        rect: rect_in_file(get(annotation, "rect"), crop),
        contents: text("contents"),
        author: text("author"),
        subject: text("subject"),
        stroke_color: engine_color_to_array(get(annotation, "color")),
        fill_color: engine_color_to_array(get(annotation, "interiorColor")),
        opacity: number_or("opacity", 1.0),
        width: number_or("strokeWidth", 1.0),
        border_style: if str_of(annotation, "borderStyle") == "dashed" { "D" } else { "S" }.into(),
        dash: get(annotation, "dashArray")
            .and_then(Value::as_array)
            .map(|d| d.iter().filter_map(Value::as_f64).collect())
            .unwrap_or_default(),
        hidden: flag("hidden"),
        print: flag("print"),
        read_only: flag("readOnly"),
        locked: flag("locked"),
        no_view: flag("noView"),
        toggle_no_view: flag("toggleNoView"),
        opaque_body: subtype == "stamp",
        subtype,
    }
}

impl ActionsCapability {
    /// The page-scoped world: the form as the user sees it plus the
    /// script-addressable annotations of one page.
    pub(crate) async fn script_world_for(
        self,
        pon: u32,
    ) -> Result<(Rc<Value>, ScriptWorldInput), Error> {
        let snapshot = self.read_form(None).await?;
        let layout = self.layout_of(pon);
        let page_index = epdf_acrojs::js::max(
            0.0,
            layout.as_ref().and_then(|l| get(l, "index")).and_then(Value::as_f64).unwrap_or(-1.0),
        );
        let listed = self.doc()?.call("page.annotations.list", Some(pon), Vec::new()).await?;
        let crop = layout.as_ref().and_then(|l| get(l, "pdfCropBox"));
        let annots = get(&listed, "annotations")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .iter()
            // Script-addressable: everything except links and widgets
            // (Link and Field objects in Acrobat, separate planes here).
            .filter(|a| {
                let subtype = str_of(a, "subtype");
                subtype != "link" && !subtype.starts_with("widget")
            })
            .map(|a| script_annot(a, page_index, crop))
            .collect::<Vec<_>>();
        let fields = script_fields_from_snapshot(&snapshot)?;
        let page_count = self.0.cx.document().map_or(0.0, |meta| f64::from(meta.page_count));
        let world = ScriptWorldInput {
            fields,
            annots: Some(annots),
            annot_pages: Some(vec![page_index]),
            annots_cover_document: Some(page_count <= 1.0),
            page_number: None,
            event: ScriptEventInput::of(ScriptEventKind::WidgetActivate),
        };
        Ok((snapshot, world))
    }
}

/// `scriptEventFor(ctx, snapshot)`.
pub(crate) fn script_event_for(ctx: &ActionContext, snapshot: &Value) -> ScriptEventInput {
    let event_type = match (&ctx.event, &ctx.source) {
        (ActionTriggerEvent::Page { .. }, _) => "Page",
        (ActionTriggerEvent::Document { .. }, _) => "Doc",
        (_, ActionSource::Widget { .. }) => "Field",
        (_, ActionSource::Link { .. }) => "Link",
        (_, ActionSource::Outline { .. }) => "Bookmark",
        // An EmbedPDF extension: Acrobat has no plain-annotation type.
        _ => "Annot",
    };
    let name = match &ctx.event {
        ActionTriggerEvent::Activate => "Mouse Up",
        ActionTriggerEvent::Annotation { name } => match name {
            PdfAnnotationEventKind::CursorEnter => "Mouse Enter",
            PdfAnnotationEventKind::CursorExit => "Mouse Exit",
            PdfAnnotationEventKind::MouseDown => "Mouse Down",
            PdfAnnotationEventKind::MouseUp => "Mouse Up",
            PdfAnnotationEventKind::Focus => "Focus",
            PdfAnnotationEventKind::Blur => "Blur",
        },
        ActionTriggerEvent::Page { name } => match name {
            PageEventKind::Open => "Open",
            PageEventKind::Close => "Close",
            // No Acrobat equivalent for these two: an EmbedPDF extension.
            PageEventKind::Visible => "Visible",
            PageEventKind::Invisible => "Invisible",
        },
        ActionTriggerEvent::Document { name } => match name {
            DocumentTriggerEvent::Open => "Open",
            DocumentTriggerEvent::WillSave => "WillSave",
            DocumentTriggerEvent::DidSave => "DidSave",
            DocumentTriggerEvent::WillPrint => "WillPrint",
            DocumentTriggerEvent::DidPrint => "DidPrint",
            DocumentTriggerEvent::WillClose => "WillClose",
        },
    };
    let target = match &ctx.source {
        ActionSource::Widget { field, .. } => Some(field.clone()),
        _ => None,
    };
    let target_field = target.as_ref().and_then(|target| {
        get(snapshot, "fields").and_then(Value::as_array)?.iter().find(|field| {
            let field_ref = get(field, "ref").cloned().unwrap_or(Value::Null);
            if str_of(target, "kind") == "objectNumber"
                && str_of(&field_ref, "kind") == "objectNumber"
            {
                same_number(get(&field_ref, "objectNumber"), get(target, "objectNumber"))
            } else {
                str_of(target, "kind") == "fqn" && get(field, "name") == get(target, "name")
            }
        })
    });
    let mut event = ScriptEventInput::of(ScriptEventKind::WidgetActivate);
    event.event_type = Some(event_type.into());
    event.name = Some(name.into());
    event.target = target.clone();
    event.source = target;
    if let Some(entry) = target_field.and_then(|f| get(f, "valueEntry"))
        && str_of(entry, "kind") == "scalar"
    {
        event.value = Some(get(entry, "value").cloned().unwrap_or(Value::Null));
    }
    event
}
