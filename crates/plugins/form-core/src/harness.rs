//! The conformance adapter: the function table `vectors/core/form-painter.json`
//! calls by name, with arguments and results in canonical JSON.
//!
//! The names are the ones `tooling/plugin-conformance/src/suites/form-painter.ts`
//! records. Five are the suite's wrappers: `layoutFontOf` asks `width` for
//! each code unit the case passes, `layoutVariableText` lays out in the font a
//! case names, `widgetScene` draws the widget before placing it, `paints`
//! takes a family, and `needsBreak` asks the line-break rule for a row of
//! characters.
//!
//! A font program the TypeScript returns carries its `load` method, which the
//! canonical form prints as `{"$function": true}`. [`FontProgram`] serializes
//! its data members only, so the table appends that member.

use epdf_kernel::Error;
use plugin_conformance::FunctionTable;
use plugin_conformance::js::{arg, to_value};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::layout::breaks::needs_break;
use crate::*;

type Out = Result<Value, Error>;

/// Canonical JSON of `value`, each font program with its `load` marker.
fn ok<T: Serialize>(value: T) -> Out {
    let mut value = to_value(&value);
    mark_programs(&mut value);
    Ok(value)
}

fn mark_programs(value: &mut Value) {
    match value {
        Value::Object(members) => {
            for (key, member) in members.iter_mut() {
                if key == "program"
                    && let Value::Object(program) = member
                {
                    program.insert("load".into(), serde_json::json!({ "$function": true }));
                } else {
                    mark_programs(member);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(mark_programs),
        _ => {}
    }
}

#[derive(Serialize)]
struct LayoutFontProbe {
    font: epdf_fonts::StandardFont,
    ascent: i32,
    descent: i32,
    widths: Vec<Option<u32>>,
}

fn layout_font_probe(args: &[Value]) -> Out {
    let family: Option<String> = arg(args, 0)?;
    let codes: Vec<u32> = arg(args, 1)?;
    ok(layout_font_of(family.as_deref()).map(|font| LayoutFontProbe {
        font: font.font,
        ascent: font.ascent,
        descent: font.descent,
        widths: codes.iter().map(|&code| font.width(code)).collect(),
    }))
}

/// `VariableTextOptions` as the cases write it: `alignment` as `/Q`'s number,
/// `substitute` as a string whose first character stands in.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OptionsArg {
    plate: PlateArg,
    font_size: f64,
    alignment: u8,
    multiline: bool,
    #[serde(default)]
    substitute: Option<String>,
    limit: u32,
    cells: u32,
}

#[derive(Deserialize)]
struct PlateArg {
    left: f64,
    bottom: f64,
    right: f64,
    top: f64,
}

fn layout_variable_text_case(args: &[Value]) -> Out {
    let text: String = arg(args, 0)?;
    let family: String = arg(args, 1)?;
    let options: OptionsArg = arg(args, 2)?;
    let font = layout_font_of(Some(&family))
        .ok_or_else(|| Error::new(format!("{family} is not a standard font")))?;
    let alignment = match options.alignment {
        0 => Alignment::Left,
        1 => Alignment::Center,
        2 => Alignment::Right,
        other => return Err(Error::new(format!("alignment {other}"))),
    };
    let PlateArg { left, bottom, right, top } = options.plate;
    ok(layout_variable_text(
        &text,
        &font,
        &VariableTextOptions {
            plate: PdfRect { left, bottom, right, top },
            font_size: options.font_size,
            alignment,
            multiline: options.multiline,
            substitute: options.substitute.and_then(|text| text.chars().next()),
            limit: options.limit,
            cells: options.cells,
        },
    ))
}

fn widget_drawing_case(args: &[Value]) -> Out {
    let row: WidgetRow = arg(args, 0)?;
    let field: FieldFacts = arg(args, 1)?;
    let shown: Option<ShownValue> = arg(args, 2)?;
    ok(widget_drawing(&row, &field, &shown.unwrap_or_default()))
}

fn widget_scene_case(args: &[Value]) -> Out {
    let row: WidgetRow = arg(args, 0)?;
    let field: FieldFacts = arg(args, 1)?;
    let shown: ShownValue = arg(args, 2)?;
    let placement: WidgetPlacement = arg(args, 3)?;
    let drawing = widget_drawing(&row, &field, &shown)
        .ok_or_else(|| Error::new("the painter doesn't draw this widget"))?;
    ok(widget_scene(&drawing, &placement))
}

fn text_editor_layout_case(args: &[Value]) -> Out {
    let row: WidgetRow = arg(args, 0)?;
    let field: FieldFacts = arg(args, 1)?;
    let text: String = arg(args, 2)?;
    ok(text_editor_layout(&row, &field, &text))
}

fn needs_break_row(args: &[Value]) -> Out {
    let previous: u32 = arg(args, 0)?;
    let currents: Vec<u32> = arg(args, 1)?;
    ok(currents.into_iter().map(|current| needs_break(previous, current)).collect::<Vec<_>>())
}

pub fn functions() -> FunctionTable {
    vec![
        ("layoutFontOf", layout_font_probe),
        ("layoutVariableText", layout_variable_text_case),
        ("widgetDrawing", widget_drawing_case),
        ("widgetScene", widget_scene_case),
        ("textEditorLayout", text_editor_layout_case),
        ("paints", |a| ok(paints(arg(a, 0)?))),
        ("needsBreak", needs_break_row),
    ]
}
