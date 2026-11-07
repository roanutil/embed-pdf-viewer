//! The conformance adapter: the function table `vectors/core/form-painter.json`
//! calls by name, with arguments and results in canonical JSON.
//!
//! The names are the ones `tooling/plugin-conformance/src/suites/form-painter.ts`
//! records, and so are its wrappers. A `fonts` argument names fonts as data: a
//! standard font's name, `'last-resort'`, or a program font from its key, its
//! ranges, its metrics and a measure of `advance` thousandths per character
//! that takes room (marks and joiners take none, and each joiner takes
//! `joined` off); a bare string is `defaultPaintFonts(string)`.
//! `defaultPaintFonts` and `programPaintFont` probe each font with the code
//! points a case passes, `layoutField` prints a run's font as its key,
//! `widgetScene` draws the widget before placing it, `paints` takes a kind,
//! and `needsBreak` asks the line-break rule for a row of characters.
//!
//! A font the TypeScript returns carries its methods (`covers`, `width`) and
//! its program its `load`, which the canonical form prints as
//! `{"$function": true}`. [`PaintFont`] and [`MountableFontProgram`]
//! serialize their data members only, so the table adds those members where
//! the TypeScript object has them.

use std::rc::Rc;

use epdf_fonts::{FontStyle, StandardFont};
use epdf_kernel::Error;
use plugin_conformance::FunctionTable;
use plugin_conformance::js::{arg, to_value};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::text::{
    FieldText, FieldTextOptions, MountableFontProgram, PaintFont, PaintFonts, default_paint_fonts,
    last_resort_paint_font, layout_field, needs_break, program_paint_font, standard_paint_font,
};
use crate::*;

type Out = Result<Value, Error>;

/// Canonical JSON of `value`, each font with its methods and each program
/// with its `load`.
fn ok<T: Serialize>(value: T) -> Out {
    let mut value = to_value(&value);
    mark_functions(&mut value);
    Ok(value)
}

fn function() -> Value {
    json!({ "$function": true })
}

fn mark_functions(value: &mut Value) {
    match value {
        Value::Object(members) => {
            for member in members.values_mut() {
                mark_functions(member);
            }
            let has = |members: &Map<String, Value>, keys: &[&str]| {
                keys.iter().all(|key| members.contains_key(*key))
            };
            if has(members, &["hash", "family", "weight", "style"]) && !members.contains_key("load")
            {
                members.insert("load".into(), function());
            }
            if has(members, &["key", "standard", "program"]) && !members.contains_key("covers") {
                // `covers` and `width` follow `standard`, as the TypeScript
                // object literal has them.
                let mut marked = Map::new();
                for (key, member) in std::mem::take(members) {
                    let after_standard = key == "standard";
                    marked.insert(key, member);
                    if after_standard {
                        marked.insert("covers".into(), function());
                        marked.insert("width".into(), function());
                    }
                }
                *members = marked;
            }
        }
        Value::Array(items) => items.iter_mut().for_each(mark_functions),
        _ => {}
    }
}

/// `ProgramSpec`: a program font as a case writes it.
#[derive(Clone, Deserialize)]
struct ProgramSpec {
    key: String,
    ranges: Vec<(u32, u32)>,
    ascent: Option<f64>,
    descent: Option<f64>,
    advance: f64,
    joined: Option<f64>,
}

/// `/[\p{M}‍]/u`.
fn takes_no_room(code_point: u32) -> bool {
    code_point == 0x200d || crate::text::is_mark(code_point)
}

fn program_font(spec: &ProgramSpec) -> Rc<PaintFont> {
    let covers: Vec<(u32, u32)> =
        spec.ranges.iter().map(|&(from, to)| (from, to - from + 1)).collect();
    let program =
        MountableFontProgram::new(spec.key.clone(), 400, FontStyle::Normal, Rc::new(Vec::new));
    let (advance, joined) = (spec.advance, spec.joined.unwrap_or(0.0));
    program_paint_font(
        &covers,
        spec.ascent.unwrap_or(900.0),
        spec.descent.unwrap_or(-300.0),
        program,
        Rc::new(move |text: &[u16], _rtl: bool| {
            let code_points = crate::text::code_points_of(text);
            let room = code_points.iter().filter(|&&cp| !takes_no_room(cp)).count() as f64;
            let joiners = code_points.iter().filter(|&&cp| cp == 0x200d).count() as f64;
            room * advance - joiners * joined
        }),
    )
}

fn font_of(spec: &Value, programs: &mut Vec<Rc<PaintFont>>) -> Result<Rc<PaintFont>, Error> {
    if let Some(name) = spec.as_str() {
        if name == "last-resort" {
            return Ok(last_resort_paint_font());
        }
        let font = StandardFont::from_name(name)
            .ok_or_else(|| Error::new(format!("not a font: {name}")))?;
        return Ok(standard_paint_font(font));
    }
    let spec: ProgramSpec =
        serde_json::from_value(spec.clone()).map_err(|error| Error::new(error.to_string()))?;
    // The same key is the same font, as the suite caches it.
    if let Some(font) = programs.iter().find(|font| font.key == spec.key) {
        return Ok(font.clone());
    }
    let font = program_font(&spec);
    programs.push(font.clone());
    Ok(font)
}

fn fonts_of(spec: &Value) -> Result<PaintFonts, Error> {
    if let Some(name) = spec.as_str() {
        return Ok(default_paint_fonts(Some(name)));
    }
    let mut programs = Vec::new();
    let face = font_of(&spec["face"], &mut programs)?;
    let fallbacks = match spec.get("fallbacks") {
        Some(Value::Array(fonts)) => {
            fonts.iter().map(|font| font_of(font, &mut programs)).collect::<Result<_, _>>()?
        }
        _ => Vec::new(),
    };
    let last_resort = match spec.get("lastResort") {
        None => Some(last_resort_paint_font()),
        Some(Value::Null) => None,
        Some(font) => Some(font_of(font, &mut programs)?),
    };
    Ok(PaintFonts { face, fallbacks, last_resort })
}

fn optional_fonts(args: &[Value], index: usize) -> Result<Option<PaintFonts>, Error> {
    match args.get(index) {
        None | Some(Value::Null) => Ok(None),
        Some(spec) => fonts_of(spec).map(Some),
    }
}

/// `String.fromCodePoint(code)` as UTF-16.
fn units_of(code: u32) -> Vec<u16> {
    match char::from_u32(code) {
        Some(c) => c.encode_utf16(&mut [0; 2]).to_vec(),
        None => vec![code as u16],
    }
}

/// A font's data members, and what it answers for each of `codes`.
fn probe(font: &PaintFont, codes: &[u32]) -> Value {
    json!({
        "key": font.key,
        "ascent": font.ascent,
        "descent": font.descent,
        "standard": font.standard,
        "fontFamily": font.font_family,
        "fontWeight": font.font_weight,
        "fontStyle": font.font_style,
        "program": to_value(&*font.program),
        "covers": codes.iter().map(|&code| font.covers(code)).collect::<Vec<_>>(),
        "widths": codes.iter().map(|&code| font.width(&units_of(code), false)).collect::<Vec<_>>(),
        "rtlWidths": codes.iter().map(|&code| font.width(&units_of(code), true)).collect::<Vec<_>>(),
    })
}

fn default_paint_fonts_probe(args: &[Value]) -> Out {
    let family: Option<String> = arg(args, 0)?;
    let codes: Vec<u32> = arg(args, 1)?;
    let fonts = default_paint_fonts(family.as_deref());
    ok(json!({
        "face": probe(&fonts.face, &codes),
        "fallbacks": fonts.fallbacks.iter().map(|font| probe(font, &codes)).collect::<Vec<_>>(),
        "lastResort": fonts.last_resort.as_deref().map(|font| probe(font, &codes)),
    }))
}

fn program_paint_font_probe(args: &[Value]) -> Out {
    let spec: ProgramSpec = arg(args, 0)?;
    let codes: Vec<u32> = arg(args, 1)?;
    ok(probe(&program_font(&spec), &codes))
}

/// A layout with each run's font as its key.
fn keyed(laid: &FieldText) -> Value {
    json!({
        "fontSize": laid.font_size,
        "contentHeight": laid.content_height,
        "lines": laid.lines.iter().map(|line| json!({
            "baseline": line.baseline,
            "runs": line.runs.iter().map(|run| json!({
                "font": run.font.key,
                "text": run.text,
                "rtl": run.rtl,
                "plain": run.plain,
                "x": run.x,
                "width": run.width,
                "joinsPrevious": run.joins_previous,
                "joinsNext": run.joins_next,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "rtl": laid.rtl,
        "missing": laid.missing,
    })
}

fn layout_field_case(args: &[Value]) -> Out {
    let text: String = arg(args, 0)?;
    let fonts = fonts_of(args.get(1).unwrap_or(&Value::Null))?;
    let options: FieldTextOptions = arg(args, 2)?;
    ok(keyed(&layout_field(&text, &fonts, &options)))
}

fn widget_drawing_case(args: &[Value]) -> Out {
    let row: WidgetRow = arg(args, 0)?;
    let field: FieldFacts = arg(args, 1)?;
    let shown: Option<ShownValue> = arg(args, 2)?;
    let fonts = optional_fonts(args, 3)?;
    ok(widget_drawing_in(&row, &field, &shown.unwrap_or_default(), fonts.as_ref()))
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
    let fonts = optional_fonts(args, 3)?;
    ok(text_editor_layout_in(&row, &field, &text, fonts.as_ref()))
}

fn widget_text_case(args: &[Value]) -> Out {
    let row: WidgetRow = arg(args, 0)?;
    let field: FieldFacts = arg(args, 1)?;
    let shown: Option<ShownValue> = arg(args, 2)?;
    ok(widget_text(&row, &field, &shown.unwrap_or_default()))
}

fn needs_break_row(args: &[Value]) -> Out {
    let previous: u32 = arg(args, 0)?;
    let currents: Vec<u32> = arg(args, 1)?;
    ok(currents.into_iter().map(|current| needs_break(previous, current)).collect::<Vec<_>>())
}

pub fn functions() -> FunctionTable {
    vec![
        ("defaultPaintFonts", default_paint_fonts_probe),
        ("programPaintFont", program_paint_font_probe),
        ("layoutField", layout_field_case),
        ("widgetDrawing", widget_drawing_case),
        ("widgetScene", widget_scene_case),
        ("textEditorLayout", text_editor_layout_case),
        ("paints", |a| ok(paints(arg(a, 0)?))),
        ("widgetText", widget_text_case),
        ("needsBreak", needs_break_row),
    ]
}
