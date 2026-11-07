//! `packages/core/form/test/painter.test.ts` and `editor.test.ts`: the painter
//! on its own, what it says it can't draw exactly and how its drawing lands
//! on the page, and the editor's layout putting the text where the painter
//! draws it.

use std::rc::Rc;

use epdf_annotation_core::TextRunScene;
use epdf_fonts::{FontStyle, StandardFont};
use epdf_form_core::text::{
    Alignment, FieldTextOptions, MountableFontProgram, PaintFont, PaintFonts, default_paint_fonts,
    last_resort_paint_font, layout_field, program_paint_font, standard_paint_font,
};
use epdf_form_core::*;
use serde_json::{Value, json};

fn merge(mut base: Value, patch: Value) -> Value {
    for (key, value) in patch.as_object().expect("patch").clone() {
        base[key] = value;
    }
    base
}

fn row_of(value: Value) -> WidgetRow {
    serde_json::from_value(value).expect("widget row")
}

fn field_of(value: Value) -> FieldFacts {
    serde_json::from_value(value).expect("field")
}

/// `painter.test.ts`'s widget.
fn painter_widget(patch: Value) -> WidgetRow {
    row_of(merge(
        json!({
            "subtype": "widget",
            "rect": { "x": 10, "y": 20, "width": 100, "height": 20 },
            "box": { "x": 10, "y": 20, "width": 100, "height": 20 },
            "rotation": null,
            "appearanceState": null,
            "color": "#000000",
            "interiorColor": "#ffffff",
            "strokeWidth": 1,
            "borderStyle": "solid",
            "dashArray": null,
            "fontFamily": "helvetica",
            "fontSize": 10,
            "fontColor": "#ff0000",
            "textAlign": "left",
            "caption": null,
            "checkStyle": null,
        }),
        patch,
    ))
}

/// `editor.test.ts`'s widget.
fn editor_widget(patch: Value) -> WidgetRow {
    row_of(merge(
        json!({
            "subtype": "widget",
            "rect": { "x": 10, "y": 20, "width": 120, "height": 24 },
            "box": { "x": 10, "y": 20, "width": 120, "height": 24 },
            "rotation": null,
            "appearanceState": null,
            "color": "#000000",
            "interiorColor": null,
            "strokeWidth": 1,
            "borderStyle": "solid",
            "dashArray": null,
            "fontFamily": "helvetica",
            "fontSize": 10,
            "fontColor": null,
            "textAlign": "left",
            "caption": null,
            "checkStyle": null,
        }),
        patch,
    ))
}

fn text_field(patch: Value) -> FieldFacts {
    field_of(merge(
        json!({
            "kind": "text",
            "value": "",
            "multiline": false,
            "password": false,
            "comb": false,
            "maxLength": null,
        }),
        patch,
    ))
}

/// A program font covering `first..=last`, every character 600 thousandths
/// wide, as the tests make one.
fn program_font(name: &str, first: u32, last: u32, ascent: f64, descent: f64) -> Rc<PaintFont> {
    program_paint_font(
        &[(first, last - first + 1)],
        ascent,
        descent,
        MountableFontProgram::new(name, 400, FontStyle::Normal, Rc::new(Vec::new)),
        Rc::new(|text: &[u16], _| char::decode_utf16(text.iter().copied()).count() as f64 * 600.0),
    )
}

/// `painter.test.ts`'s fonts: Helvetica, a Hebrew fallback, the last resort.
fn hebrew_fonts() -> (PaintFonts, Rc<PaintFont>) {
    let hebrew = program_font("hebrew-hash", 0x0590, 0x05ff, 950.0, -250.0);
    let fonts = PaintFonts {
        face: standard_paint_font(StandardFont::Helvetica),
        fallbacks: vec![hebrew.clone()],
        last_resort: Some(last_resort_paint_font()),
    };
    (fonts, hebrew)
}

fn drawing(row: &WidgetRow, field: &FieldFacts) -> WidgetDrawing {
    widget_drawing(row, field, &ShownValue::default()).expect("drawn")
}

fn close(actual: f64, expected: f64, digits: i32) {
    // Vitest's `toBeCloseTo`: |difference| < 10^-digits / 2.
    assert!(
        (actual - expected).abs() < 10f64.powi(-digits) / 2.0,
        "{actual} is not close to {expected} ({digits} digits)"
    );
}

const FRAME: epdf_geometry::Rect =
    epdf_geometry::Rect { x: 10.0, y: 20.0, width: 100.0, height: 20.0 };

fn text_runs_of(scene: &[SceneNode]) -> Vec<&TextRunScene> {
    scene
        .iter()
        .filter_map(|node| match node {
            SceneNode::TextRun(run) => Some(run),
            _ => None,
        })
        .collect()
}

#[test]
fn it_says_when_its_drawing_may_differ_from_the_engine_picture() {
    let plain = text_field(json!({ "value": "Plain" }));
    assert_eq!(drawing(&painter_widget(json!({})), &plain).approximate, None);
    // WinAnsi can't write Cyrillic: the engine takes another font for it,
    // which the painter wasn't given; given the fonts, a box is what the
    // engine draws too.
    let cyrillic = text_field(json!({ "value": "Привет" }));
    assert_eq!(
        drawing(&painter_widget(json!({})), &cyrillic).approximate,
        Some(Approximation::Glyph)
    );
    let (fonts, _) = hebrew_fonts();
    let given = widget_drawing_in(
        &painter_widget(json!({})),
        &cyrillic,
        &ShownValue::default(),
        Some(&fonts),
    );
    assert_eq!(given.expect("drawn").approximate, None);
    // A font that isn't one of the 14 lays out with Helvetica's numbers.
    assert_eq!(
        drawing(&painter_widget(json!({ "fontFamily": "Roboto" })), &plain).approximate,
        Some(Approximation::Font)
    );
    let abc = text_field(json!({ "value": "abc" }));
    assert_eq!(
        drawing(&painter_widget(json!({ "fontFamily": "symbol" })), &abc).approximate,
        Some(Approximation::SymbolicFont)
    );
}

#[test]
fn it_draws_a_signature_box_and_nothing_of_an_unknown_kind() {
    let row = painter_widget(json!({}));
    let signature = drawing(&row, &field_of(json!({ "kind": "signature" })));
    // The background over the whole box, then the border inside it; no text.
    let kinds: Vec<&str> = signature
        .ops
        .iter()
        .map(|op| match op {
            DrawOp::Fill { .. } => "fill",
            DrawOp::Stroke { .. } => "stroke",
            DrawOp::Clip { .. } => "clip",
            DrawOp::Text { .. } => "text",
        })
        .collect();
    assert_eq!(kinds, ["fill", "stroke"]);
    assert_eq!(signature.approximate, None);
    let unknown = field_of(json!({ "kind": "unknown" }));
    assert!(widget_drawing(&row, &unknown, &ShownValue::default()).is_none());
    assert!(paints(FieldKind::Signature) && !paints(FieldKind::Unknown));
}

#[test]
fn it_shows_the_text_it_is_given_over_the_value() {
    let shown = ShownValue { text: Some("$1,234.50".into()), checked: None };
    let drawn = widget_drawing(
        &painter_widget(json!({})),
        &text_field(json!({ "value": "1234.5" })),
        &shown,
    )
    .expect("drawn");
    let Some(DrawOp::Clip { ops, .. }) =
        drawn.ops.iter().find(|op| matches!(op, DrawOp::Clip { .. }))
    else {
        panic!("no clip")
    };
    let Some(DrawOp::Text { runs, .. }) = ops.first() else { panic!("no text") };
    assert_eq!(runs[0].text, "$1,234.50");
}

#[test]
fn it_fits_a_size_of_0_to_the_box_with_the_engine_sizes() {
    let fitted = |height: f64| {
        layout_field(
            "Hello",
            &default_paint_fonts(Some("helvetica")),
            &FieldTextOptions {
                plate: PdfRect { left: 0.0, bottom: 0.0, right: 200.0, top: height },
                font_size: 0.0,
                alignment: Alignment::Left,
                multiline: false,
                password: false,
                limit: 0,
                cells: 0,
                centered: true,
            },
        )
        .font_size
    };
    // The line is (945 + 224) / 1000 em tall: the biggest step whose line fits.
    assert_eq!([fitted(10.0), fitted(20.0), fitted(40.0)], [8.0, 14.0, 30.0]);
}

#[test]
fn it_lands_on_the_page_at_its_left_edge_clipped_and_turned_with_the_widget() {
    let field = text_field(json!({ "value": "Hi" }));
    let upright = widget_scene(
        &drawing(&painter_widget(json!({})), &field),
        &WidgetPlacement { box_: FRAME, rotation: None },
    );
    let run = text_runs_of(&upright)[0];
    assert_eq!((run.text.as_str(), run.rtl, run.plain, run.at.x), ("Hi", false, true, 12.0));
    assert!(run.font_family.starts_with("\"epdf-") && run.font_family.contains("\", Helvetica"));
    // The face it draws in comes with it, for whatever draws it to mount.
    let program = &run.program.as_ref().expect("a program").0;
    assert_eq!(program.family(), &run.font_family[1..22]);
    assert_eq!((program.weight(), program.style()), (400.0, "normal"));
    assert!(!program.load().is_empty());
    assert_eq!(run.clip, Some(epdf_geometry::Rect { x: 11.0, y: 21.0, width: 98.0, height: 18.0 }));
    assert_eq!(run.rotation, None);

    let turned = widget_scene(
        &drawing(&painter_widget(json!({ "rotation": 90 })), &field),
        &WidgetPlacement { box_: FRAME, rotation: Some(90.0) },
    );
    let run = text_runs_of(&turned)[0];
    assert_eq!(run.rotation, Some(90.0));
    // Turned a quarter clockwise about the box's middle (60, 30): the clip stands up.
    assert_eq!(
        run.clip,
        Some(epdf_geometry::Rect { x: 51.0, y: -19.0, width: 18.0, height: 98.0 })
    );
}

#[test]
fn it_draws_mixed_scripts_as_runs_in_their_fonts_in_the_engine_order() {
    let (fonts, hebrew) = hebrew_fonts();
    let field = text_field(json!({ "value": "Hello שלום 123" }));
    let drawn =
        widget_drawing_in(&painter_widget(json!({})), &field, &ShownValue::default(), Some(&fonts));
    let scene =
        widget_scene(&drawn.expect("drawn"), &WidgetPlacement { box_: FRAME, rotation: None });
    let runs = text_runs_of(&scene);
    let helvetica = standard_paint_font(StandardFont::Helvetica);
    let first_family = |family: &str| family.split(',').next().expect("a family").to_string();
    let found: Vec<(String, bool, String)> = runs
        .iter()
        .map(|run| (run.text.clone(), run.rtl, first_family(&run.font_family)))
        .collect();
    let face = first_family(&helvetica.font_family);
    let hebrew_family = hebrew.font_family.clone();
    assert_eq!(
        found,
        [
            ("Hello ".to_string(), false, face.clone()),
            ("123".to_string(), false, face.clone()),
            (" ".to_string(), true, face),
            ("שלום".to_string(), true, hebrew_family),
        ]
    );
    // Each starts where the one before it ends.
    let xs: Vec<f64> = runs.iter().map(|run| run.at.x).collect();
    assert!(xs.windows(2).all(|pair| pair[0] <= pair[1]), "{xs:?}");
    let shalom = runs.iter().find(|run| run.rtl && run.text == "שלום").expect("hebrew run");
    assert!(!shalom.plain);
    assert_eq!(shalom.program.as_ref().expect("program").0.family(), hebrew.program.family);
}

#[test]
fn it_draws_a_comb_arabic_letters_in_the_forms_the_whole_word_gives_them() {
    let arabic = program_font("arabic-hash", 0x0600, 0x06ff, 950.0, -250.0);
    let fonts = PaintFonts {
        face: standard_paint_font(StandardFont::Helvetica),
        fallbacks: vec![arabic],
        last_resort: Some(last_resort_paint_font()),
    };
    let comb = text_field(json!({ "value": "مرحبا", "comb": true, "maxLength": 5 }));
    let drawn =
        widget_drawing_in(&painter_widget(json!({})), &comb, &ShownValue::default(), Some(&fonts));
    let scene =
        widget_scene(&drawn.expect("drawn"), &WidgetPlacement { box_: FRAME, rotation: None });
    let joins: Vec<(String, bool, bool)> = text_runs_of(&scene)
        .iter()
        .map(|run| {
            (run.text.clone(), run.joins_previous.unwrap_or(false), run.joins_next.unwrap_or(false))
        })
        .collect();
    assert_eq!(
        joins,
        [
            ("م".to_string(), false, true),
            ("ر".to_string(), true, false),
            ("ح".to_string(), false, true),
            ("ب".to_string(), true, true),
            ("ا".to_string(), true, false),
        ]
    );
}

/// The painter's text: its size and runs.
fn painted_text(row: &WidgetRow, field: &FieldFacts) -> (f64, Vec<TextRun>) {
    fn walk(ops: &[DrawOp]) -> Option<(f64, Vec<TextRun>)> {
        ops.iter().find_map(|op| match op {
            DrawOp::Text { size, runs, .. } => Some((*size, runs.clone())),
            DrawOp::Clip { ops, .. } => walk(ops),
            _ => None,
        })
    }
    walk(&drawing(row, field).ops).expect("text")
}

/// Where the CSS line box puts the first baseline, from the box's top.
fn css_baseline(row: &WidgetRow, field: &FieldFacts, text: &str) -> f64 {
    let layout = text_editor_layout(row, field, text).expect("layout");
    let font =
        StandardFont::from_name(row.font_family.as_deref().expect("font")).expect("standard");
    let ascent = standard_paint_font(font).ascent * layout.font_size / 1000.0;
    let height = row.box_.expect("box").height;
    let content = height - layout.padding.top - layout.padding.bottom;
    let multiline = matches!(field, FieldFacts::Text(facts) if facts.multiline);
    let line_top =
        layout.padding.top + if multiline { 0.0 } else { (content - layout.line_height) / 2.0 };
    line_top + ascent
}

#[test]
fn the_editor_centres_one_line_where_the_painter_draws_it() {
    for (font_family, font_size, border_style) in
        [("helvetica", 10, "solid"), ("times-roman", 14, "beveled"), ("courier-bold", 0, "inset")]
    {
        let row = editor_widget(json!({
            "fontFamily": font_family,
            "fontSize": font_size,
            "borderStyle": border_style,
        }));
        let field = text_field(json!({ "value": "Ada Lovelace" }));
        let (size, runs) = painted_text(&row, &field);
        let layout = text_editor_layout(&row, &field, "Ada Lovelace").expect("layout");
        close(layout.font_size, size, 5);
        close(layout.padding.left, runs[0].x, 2);
        close(css_baseline(&row, &field, "Ada Lovelace"), 24.0 - runs[0].y, 2);
    }
}

#[test]
fn the_editor_starts_several_lines_at_the_top_a_line_apart() {
    let row = editor_widget(json!({
        "box": { "x": 10, "y": 20, "width": 120, "height": 80 },
        "fontSize": 9,
    }));
    let field = text_field(json!({ "value": "one\ntwo", "multiline": true }));
    let (_, runs) = painted_text(&row, &field);
    let layout = text_editor_layout(&row, &field, "one\ntwo").expect("layout");
    close(css_baseline(&row, &field, "one\ntwo"), 80.0 - runs[0].y, 2);
    close(runs[0].y - runs[1].y, layout.line_height, 3);
}

#[test]
fn the_editor_fits_the_size_to_the_text_being_typed() {
    let row = editor_widget(json!({ "fontSize": 0 }));
    let field = text_field(json!({}));
    let short = text_editor_layout(&row, &field, "Ada").expect("layout");
    let long =
        text_editor_layout(&row, &field, "Ada Lovelace, Countess of Lovelace").expect("layout");
    assert!(long.font_size < short.font_size);
}

#[test]
fn the_editor_spaces_a_comb_by_its_cells() {
    let row = editor_widget(json!({ "fontFamily": "courier", "fontSize": 12 }));
    let field = text_field(json!({ "value": "1234", "comb": true, "maxLength": 6 }));
    let (_, runs) = painted_text(&row, &field);
    let layout = text_editor_layout(&row, &field, "1234").expect("layout");
    assert!(layout.comb);
    let advance = 600.0 * 0.012;
    for (index, run) in runs.iter().enumerate() {
        close(layout.padding.left + index as f64 * (advance + layout.letter_spacing), run.x, 2);
    }
    // Empty, it says where the first character will go.
    let empty = text_field(json!({ "comb": true, "maxLength": 6 }));
    close(text_editor_layout(&row, &empty, "").expect("layout").padding.left, runs[0].x, 2);
}

#[test]
fn the_editor_reads_the_line_in_the_direction_the_engine_draws_it() {
    let rtl = |text: &str| {
        text_editor_layout(&editor_widget(json!({})), &text_field(json!({})), text)
            .expect("layout")
            .rtl
    };
    assert!(rtl("שלום test 123"));
    assert!(!rtl("test 123 שלום"));
    assert!(rtl("123 שלום"));
    assert!(!rtl(""));
}

#[test]
fn the_editor_is_only_for_text_fields() {
    let checkbox = field_of(json!({ "kind": "checkbox" }));
    assert!(text_editor_layout(&editor_widget(json!({})), &checkbox, "").is_none());
}

#[test]
fn the_editor_names_the_programs_its_text_is_drawn_in() {
    let fonts = PaintFonts {
        face: standard_paint_font(StandardFont::Helvetica),
        fallbacks: vec![
            program_font("arabic", 0x0600, 0x06ff, 900.0, -200.0),
            program_font("hebrew", 0x0590, 0x05ff, 900.0, -200.0),
        ],
        last_resort: Some(last_resort_paint_font()),
    };
    let layout = |text: &str| {
        text_editor_layout_in(&editor_widget(json!({})), &text_field(json!({})), text, Some(&fonts))
            .expect("layout")
    };
    let families = |text: &str| {
        layout(text).programs.iter().map(|program| program.family.clone()).collect::<Vec<_>>()
    };
    let face = standard_paint_font(StandardFont::Helvetica).program.family.clone();
    let last_resort = last_resort_paint_font().program.family.clone();
    assert_eq!(families("Hello"), [face.clone(), last_resort.clone()]);
    assert_eq!(families("Hello שלום"), [face.clone(), "epdf-hebrew".into(), last_resort.clone()]);
    assert_eq!(
        layout("Hello שלום").font_family,
        format!("\"{face}\", \"epdf-hebrew\", \"{last_resort}\", Helvetica, Arial, sans-serif")
    );
}

#[test]
fn widget_text_is_every_character_the_painter_draws() {
    let row = editor_widget(json!({}));
    let none = ShownValue::default();
    assert_eq!(widget_text(&row, &text_field(json!({ "value": "שלום" })), &none), "שלום");
    let shown = ShownValue { text: Some("$1,234.50".into()), checked: None };
    assert_eq!(widget_text(&row, &text_field(json!({ "value": "1234.5" })), &shown), "$1,234.50");
    let secret = text_field(json!({ "value": "secret", "password": true }));
    assert_eq!(widget_text(&row, &secret, &none), "*");
    assert_eq!(widget_text(&row, &text_field(json!({ "password": true })), &none), "");
    let list = field_of(json!({
        "kind": "listbox",
        "options": [{ "label": "One", "selected": false }, { "label": "שתיים", "selected": true }],
        "topIndex": 0,
    }));
    assert_eq!(widget_text(&row, &list, &none), "One\nשתיים");
    let button = field_of(json!({ "kind": "pushbutton" }));
    assert_eq!(widget_text(&editor_widget(json!({ "caption": "שלח" })), &button, &none), "שלח");
    assert_eq!(widget_text(&row, &field_of(json!({ "kind": "checkbox" })), &none), "");
}

/// `drawing/index.ts`: `appearanceState !== null && !== 'Off'`. An absent
/// member is `undefined`, so a check box without one shows checked.
#[test]
fn an_absent_appearance_state_reads_as_on_and_null_as_off() {
    let checkbox = field_of(json!({ "kind": "checkbox" }));
    let symbols = |patch: Value| {
        let mut widget = json!({
            "rect": { "x": 0, "y": 0, "width": 18, "height": 18 },
            "color": "#000000",
        });
        for (key, value) in patch.as_object().expect("patch") {
            widget[key] = value.clone();
        }
        let row = row_of(widget);
        widget_drawing(&row, &checkbox, &ShownValue::default()).expect("drawn").ops.len()
    };
    let box_only = symbols(json!({ "appearanceState": "Off" }));
    assert_eq!(symbols(json!({})), box_only + 1, "absent");
    assert_eq!(symbols(json!({ "appearanceState": null })), box_only, "null");
    assert_eq!(symbols(json!({ "appearanceState": "Yes" })), box_only + 1, "named");
    assert_eq!(
        row_of(json!({ "rect": { "x": 0, "y": 0, "width": 1, "height": 1 } })).appearance_state,
        None
    );
    assert_eq!(
        row_of(
            json!({ "rect": { "x": 0, "y": 0, "width": 1, "height": 1 }, "appearanceState": null })
        )
        .appearance_state,
        Some(None)
    );
}
