//! `scripting/world.ts`: script world building (page-scoped prefetch) and
//! the two-standard event bridge: the /AA keys are ISO 32000-2 Table 200,
//! the event names the JavaScript API layer's (ISO 21757-1, Acrobat).

use epdf_acrojs::{
    ScriptAnnotInput, ScriptColorArray, ScriptEventInput, ScriptEventKind, ScriptWorldInput,
    script_fields_from_snapshot,
};
use epdf_kernel::Error;
use serde_json::Value;

use crate::ActionsCapability;
use crate::contract::{
    ActionContext, ActionSource, ActionTriggerEvent, DocumentTriggerEvent, PageEventKind,
    PdfAnnotationEventKind,
};
use crate::services::{get, same_number, str_of, truthy};

fn engine_color_to_array(color: Option<&Value>) -> ScriptColorArray {
    match color.filter(|c| truthy(Some(c))) {
        Some(c) => {
            let channel = |k: &str| get(c, k).and_then(Value::as_f64).unwrap_or(f64::NAN) / 255.0;
            ScriptColorArray::Rgb(channel("r"), channel("g"), channel("b"))
        }
        None => ScriptColorArray::T,
    }
}

fn script_annot(annotation: &Value, page_index: f64) -> ScriptAnnotInput {
    let text =
        |key: &str| get(annotation, key).and_then(Value::as_str).unwrap_or_default().to_string();
    let rect = get(annotation, "rect").cloned().unwrap_or(Value::Null);
    let side = |k: &str| get(&rect, k).and_then(Value::as_f64).unwrap_or(f64::NAN);
    let flags = get(annotation, "flags").cloned().unwrap_or(Value::Null);
    let flag = |k: &str| get(&flags, k).and_then(Value::as_bool).unwrap_or(false);
    let number_or =
        |k: &str, fallback: f64| get(annotation, k).and_then(Value::as_f64).unwrap_or(fallback);
    let subtype = text("subtype");
    ScriptAnnotInput {
        annot_ref: get(annotation, "ref").cloned().unwrap_or(Value::Null),
        name: text("nm"),
        page: page_index,
        rect: [side("left"), side("bottom"), side("right"), side("top")],
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
    /// `pageIndexOf(pon)`: `-1` when the page is not in the document.
    fn page_index_of(&self, pon: u32) -> f64 {
        self.pages()
            .iter()
            .position(|p| {
                get(p, "ref").and_then(|r| get(r, "pageObjectNumber")).and_then(Value::as_f64)
                    == Some(f64::from(pon))
            })
            .map_or(-1.0, |i| i as f64)
    }

    /// The page-scoped world: the form snapshot and the script-addressable
    /// annotations of one page.
    pub(crate) async fn script_world_for(
        self,
        pon: u32,
    ) -> Result<(Value, ScriptWorldInput), Error> {
        let doc = self.doc()?;
        let snapshot = doc.call("forms.list", None, Vec::new()).await?;
        let page_index = epdf_acrojs::js::max(0.0, self.page_index_of(pon));
        doc.page_index(pon)?;
        let listed = doc.call("page.annotations.list", Some(pon), Vec::new()).await?;
        let annots = get(&listed, "annotations")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .iter()
            // Script-addressable: everything except link and widget.
            .filter(|a| {
                let subtype = str_of(a, "subtype");
                subtype != "link" && !subtype.starts_with("widget")
            })
            .map(|a| script_annot(a, page_index))
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
                same_number(get(&field_ref, "fieldObjectNumber"), get(target, "fieldObjectNumber"))
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
