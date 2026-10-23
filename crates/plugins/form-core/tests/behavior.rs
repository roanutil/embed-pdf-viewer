//! `packages/core/form/test/painter.test.ts` and `editor.test.ts`: the painter
//! on its own, what it says it can't draw exactly and how its drawing lands
//! on the page, and the editor's layout putting the text where the painter
//! draws it.

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
            "rect": { "x": 10, "y": 20, "width": 120, "height": 24 },
            "box": { "x": 10, "y": 20, "width": 120, "height": 24 },
            "appearanceState": null,
            "color": "#000000",
            "interiorColor": null,
            "strokeWidth": 1,
            "borderStyle": "solid",
            "fontFamily": "helvetica",
            "fontSize": 10,
            "fontColor": null,
            "textAlign": "left",
        }),
        patch,
    ))
}

fn text_field(patch: Value) -> FieldFacts {
    field_of(merge(
        json!({
            "family": "text",
            "value": "",
            "multiline": false,
            "password": false,
            "comb": false,
            "maxLength": null,
        }),
        patch,
    ))
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

#[test]
fn it_says_when_its_drawing_may_differ_from_the_engine_picture() {
    let plain = text_field(json!({ "value": "Plain" }));
    assert_eq!(drawing(&painter_widget(json!({})), &plain).approximate, None);
    // WinAnsi can't write Cyrillic: the engine takes another font for it.
    let cyrillic = text_field(json!({ "value": "Привет" }));
    assert_eq!(
        drawing(&painter_widget(json!({})), &cyrillic).approximate,
        Some(Approximation::Glyph)
    );
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
fn it_does_not_draw_a_signature_or_an_unknown_family() {
    let row = painter_widget(json!({}));
    for family in [json!({ "family": "signature" }), json!({ "family": "unknown" })] {
        assert!(widget_drawing(&row, &field_of(family), &ShownValue::default()).is_none());
    }
    assert!(!paints(FieldFamily::Signature) && !paints(FieldFamily::Unknown));
    assert!(paints(FieldFamily::Text) && paints(FieldFamily::Pushbutton));
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
    let font = layout_font_of(Some("helvetica")).expect("helvetica");
    let fitted = |height: f64| {
        layout_variable_text(
            "Hello",
            &font,
            &VariableTextOptions {
                plate: PdfRect { left: 0.0, bottom: 0.0, right: 200.0, top: height },
                font_size: 0.0,
                alignment: Alignment::Left,
                multiline: false,
                substitute: None,
                limit: 0,
                cells: 0,
            },
        )
        .font_size
    };
    // The line is (945 + 224) / 1000 em tall: the biggest step whose line fits.
    assert_eq!([fitted(10.0), fitted(20.0), fitted(40.0)], [8.0, 14.0, 30.0]);
}

fn glyphs_of(scene: &[SceneNode]) -> &SceneNode {
    scene.iter().find(|node| matches!(node, SceneNode::Glyphs { .. })).expect("glyphs")
}

#[test]
fn it_lands_on_the_page_turned_with_the_widget() {
    let field = text_field(json!({ "value": "Hi" }));
    let frame = epdf_geometry::Rect { x: 10.0, y: 20.0, width: 100.0, height: 20.0 };
    let upright = widget_scene(
        &drawing(&painter_widget(json!({})), &field),
        &WidgetPlacement { box_: frame, rotation: None },
    );
    let SceneNode::Glyphs { offsets, font_family, program, clip, rotation, .. } =
        glyphs_of(&upright)
    else {
        unreachable!()
    };
    assert_eq!(offsets, &[0.0, 7.22]);
    assert!(font_family.starts_with("\"epdf-") && font_family.contains("\", Helvetica"));
    // The face it draws in comes with it, for whatever draws it to mount.
    assert_eq!(program.family, font_family[1..22]);
    assert_eq!((program.weight, program.style), (400, epdf_fonts::FontStyle::Normal));
    assert_eq!(*clip, Some(epdf_geometry::Rect { x: 11.0, y: 21.0, width: 98.0, height: 18.0 }));
    assert_eq!(*rotation, None);

    let turned = widget_scene(
        &drawing(&painter_widget(json!({ "rotation": 90 })), &field),
        &WidgetPlacement { box_: frame, rotation: Some(90.0) },
    );
    let SceneNode::Glyphs { rotation, clip, .. } = glyphs_of(&turned) else { unreachable!() };
    assert_eq!(*rotation, Some(90.0));
    // Turned a quarter clockwise about the box's middle (60, 30): the clip stands up.
    assert_eq!(*clip, Some(epdf_geometry::Rect { x: 51.0, y: -19.0, width: 18.0, height: 98.0 }));
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
    let ascent = f64::from(layout_font_of(row.font_family.as_deref()).expect("font").ascent)
        * layout.font_size
        / 1000.0;
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
fn the_editor_is_only_for_text_fields() {
    let checkbox = field_of(json!({ "family": "checkbox" }));
    assert!(text_editor_layout(&editor_widget(json!({})), &checkbox, "").is_none());
}

/// Rust only: a run's text is a `String`, so a comb that splits a surrogate
/// pair into its cells writes each half as U+FFFD, where the TypeScript
/// writes the lone half. The layout's code units keep both halves.
#[test]
fn a_comb_splitting_a_surrogate_pair_writes_replacement_characters() {
    let row = editor_widget(json!({ "fontFamily": "courier" }));
    let field = text_field(json!({ "value": "😀", "comb": true, "maxLength": 4 }));
    let (_, runs) = painted_text(&row, &field);
    let texts: Vec<&str> = runs.iter().map(|run| run.text.as_str()).collect();
    assert_eq!(texts, ["\u{fffd}", "\u{fffd}"]);
    let font = layout_font_of(Some("courier")).expect("courier");
    let layout = layout_variable_text(
        "😀",
        &font,
        &VariableTextOptions {
            plate: PdfRect { left: 0.0, bottom: 0.0, right: 100.0, top: 20.0 },
            font_size: 10.0,
            alignment: Alignment::Left,
            multiline: false,
            substitute: None,
            limit: 0,
            cells: 4,
        },
    );
    let codes: Vec<u16> = layout.lines[0].words.iter().map(|word| word.code).collect();
    assert_eq!(codes, [0xd83d, 0xde00]);
    assert!(layout.approximate);
}
