//! A form's fields and their widgets, made, placed, restyled, turned and
//! removed through the host, as upstream's suites check them:
//! `runFormConformance`'s field and widget cases
//! (`packages/engine/core/src/conformance/runFormConformance.ts`, which
//! `packages/engine/main/test/forms.test.ts` runs) and
//! `runWidgetRotationConformance` (`widget-rotation.test.ts`), each sent as
//! the worker requests `LocalDocumentFormsService` sends.
//!
//! The rotation suite's form bundle case is in `form_transfer.rs`, and
//! `widget-painter-parity.test.ts` compares the engine's appearances with
//! `@embedpdf/core-form`'s widget painter, which has no Rust port yet.

mod common;

use epdf_engine::host::Host;
use serde_json::{Value, json};

const TOGGLE_FIELDS: &str = "packages/engine/main/test/fixtures/toggle_fields.pdf";
const PAGE: u64 = 3;

/// One open document and the calls a test sends it.
struct Doc {
    host: Host,
    next: u64,
}

impl Doc {
    fn open(bytes: Vec<u8>) -> Self {
        let mut host = Host::new();
        let response = host.call(
            &json!({ "kind": "open.fatMem", "effect": "open", "jobId": 1, "docId": "d" })
                .to_string(),
            vec![bytes],
        );
        assert!(!response.json.contains("\"error\""), "{}", response.json);
        Self { host, next: 2 }
    }

    fn toggle_fields() -> Self {
        Self::open(test_support::fixture(TOGGLE_FIELDS))
    }

    /// Send `request` with this document and a fresh job and op id;
    /// answer the envelope's `result` or its `error`.
    fn send(&mut self, mut request: Value) -> Result<Value, Value> {
        let job = self.next;
        self.next += 1;
        request["docId"] = json!("d");
        request["jobId"] = json!(job);
        if request.get("opId").is_none() {
            request["opId"] = json!(format!("op-{job}"));
        }
        let response = self.host.call(&request.to_string(), Vec::new());
        let mut envelope: Value = serde_json::from_str(&response.json).expect("JSON");
        match envelope.get_mut("error") {
            Some(error) => Err(error.take()),
            None => Ok(envelope["result"].take()),
        }
    }

    fn ok(&mut self, request: Value) -> Value {
        let answer = self.send(request.clone());
        answer.unwrap_or_else(|e| panic!("{request} failed: {e}"))
    }

    fn refused(&mut self, request: Value) -> Value {
        let answer = self.send(request.clone());
        answer.expect_err(&format!("{request} should be refused"))
    }

    /// A write's item: the result of its `{result, mutationSequence, event}`.
    fn write(&mut self, request: Value) -> Value {
        self.ok(request)["result"].take()
    }

    fn form(&mut self) -> Value {
        self.ok(json!({ "kind": "forms.list" }))
    }

    fn field(&mut self, reference: &Value) -> Option<Value> {
        let form = self.form();
        form["fields"].as_array().unwrap().iter().find(|f| f["ref"] == *reference).cloned()
    }

    fn field_named(&mut self, name: &str) -> Value {
        let form = self.form();
        form["fields"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["name"] == name)
            .cloned()
            .unwrap_or_else(|| panic!("no field {name}"))
    }

    fn row(&mut self, widget: &Value) -> Option<Value> {
        let form = self.form();
        form["widgets"].as_array().unwrap().iter().find(|w| w["ref"] == *widget).cloned()
    }

    /// The row of the first widget of the field `name`.
    fn row_of(&mut self, name: &str) -> Value {
        let field = self.field_named(name);
        self.row(&field["widgets"][0]["ref"]).unwrap_or_else(|| panic!("no widget of {name}"))
    }

    fn widgets_on(&mut self, page: u64) -> Vec<Value> {
        let form = self.form();
        form["widgets"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|w| w["page"]["objectNumber"] == page)
            .map(|w| w["ref"].clone())
            .collect()
    }

    fn annotations_on(&mut self, page: u64) -> Vec<Value> {
        let list = self.ok(json!({ "kind": "annotations.list", "pages": [page_ref(page)] }));
        list["annotations"].as_array().unwrap().iter().map(|a| a["ref"].clone()).collect()
    }
}

fn page_ref(page: u64) -> Value {
    json!({ "kind": "objectNumber", "objectNumber": page })
}

fn code(error: &Value) -> &str {
    error["code"].as_str().unwrap_or_default()
}

fn close(a: &Value, b: &Value) {
    for edge in ["x", "y", "width", "height"] {
        let (x, y) = (a[edge].as_f64().unwrap(), b[edge].as_f64().unwrap());
        assert!((x - y).abs() < 0.01, "{a} is not {b}");
    }
}

#[test]
fn widgets_live_the_full_form_loop() {
    let mut doc = Doc::toggle_fields();
    // 1. A field gets a widget where its placement says, styled with the
    // house vocabulary.
    let field = doc.write(json!({ "kind": "forms.createField",
        "draft": { "family": "text", "name": "loop_field" } }));
    assert_eq!(field["field"]["widgets"], json!([]));
    let reference = field["field"]["ref"].clone();
    let rect = json!({ "x": 20, "y": 20, "width": 180, "height": 24 });
    let added = doc.write(json!({ "kind": "forms.addWidget", "ref": reference,
        "placement": { "page": page_ref(PAGE), "rect": rect, "interiorColor": "#f6f8fa",
            "color": "#1f6feb", "strokeWidth": 1, "fontSize": 10 } }));
    close(&added["widgets"][0]["rect"], &rect);
    assert_eq!(added["meta"]["changedWidgets"].as_array().unwrap().len(), 1);
    let widget = added["field"]["widgets"][0]["ref"].clone();
    assert_eq!(widget["kind"], "objectNumber");

    // 2. The form holds its row, joined to its field; the annotations never
    // do.
    let row = doc.row(&widget).expect("a widget row");
    assert_eq!(row["field"], reference);
    assert_eq!(row["fieldFamily"], "text");
    close(&row["rect"], &rect);
    assert_eq!(row["interiorColor"], "#f6f8fa");
    let annotations = doc.ok(json!({ "kind": "annotations.list", "pages": [page_ref(PAGE)] }));
    assert!(
        annotations["annotations"].as_array().unwrap().iter().all(|a| a["subtype"] != "widget")
    );

    // 3. Restyled and moved through the form, never the annotation path.
    let moved = json!({ "x": 20, "y": 40, "width": 180, "height": 24 });
    let patched = doc.write(json!({ "kind": "forms.updateWidget", "widget": widget,
        "patch": { "interiorColor": "#fff7db", "rect": moved } }));
    assert_eq!(patched["widget"]["interiorColor"], "#fff7db");
    close(&patched["widget"]["rect"], &moved);
    let as_annotation = doc.refused(json!({ "kind": "annotations.update", "ref": widget,
        "patch": { "subtype": "widget", "interiorColor": "#ffffff" } }));
    assert_eq!(code(&as_annotation), "InvalidArg", "{as_annotation}");
    let deleted = doc.refused(json!({ "kind": "annotations.delete", "ref": widget }));
    assert_eq!(code(&deleted), "InvalidArg", "{deleted}");

    // 4. Removed from its field, it stays on its page as a row in no field.
    let removed = doc.write(json!({ "kind": "forms.detachWidget", "ref": reference,
        "widget": widget }));
    assert_eq!(removed["widgets"][0]["field"], Value::Null);
    assert_eq!(doc.row(&widget).expect("still on its page")["field"], Value::Null);
    // The field survives, unplaced.
    assert_eq!(doc.field(&reference).expect("the field")["widgets"], json!([]));
}

#[test]
fn delete_widget_takes_a_widget_off_its_page_and_out_of_its_field() {
    let mut doc = Doc::toggle_fields();
    let created = doc.ok(json!({ "kind": "forms.createField", "draft": {
        "family": "text", "name": "delete_me", "widgets": [
            { "page": page_ref(PAGE), "rect": { "x": 20, "y": 300, "width": 120, "height": 20 } },
            { "page": page_ref(PAGE), "rect": { "x": 20, "y": 330, "width": 120, "height": 20 } },
        ] } }));
    let field = created["result"]["field"].clone();
    let first = field["widgets"][0]["ref"].clone();
    let second = field["widgets"][1]["ref"].clone();

    // A widget in a field: it leaves the page and the field; the field
    // keeps the other.
    let deleted = doc.ok(json!({ "kind": "forms.deleteWidget", "widget": first }));
    assert_eq!(deleted["result"]["widget"], first);
    assert_eq!(deleted["result"]["page"], page_ref(PAGE));
    let left: Vec<Value> = deleted["result"]["field"]["widgets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["ref"].clone())
        .collect();
    assert_eq!(left, vec![second.clone()]);
    assert!(doc.row(&first).is_none());
    assert_eq!(deleted["event"]["type"], "forms.widgetDeleted");

    // A widget in no field goes too.
    doc.write(json!({ "kind": "forms.detachWidget", "ref": field["ref"], "widget": second }));
    let inert = doc.write(json!({ "kind": "forms.deleteWidget", "widget": second }));
    assert_eq!(inert["field"], Value::Null);
    assert!(doc.row(&second).is_none());
    assert_eq!(doc.field(&field["ref"]).unwrap()["widgets"], json!([]));

    // A merged field/widget is the field: delete the field instead.
    let merged = doc.field_named("opt_check");
    let refusal =
        doc.refused(json!({ "kind": "forms.deleteWidget", "widget": merged["widgets"][0]["ref"] }));
    assert_eq!(code(&refusal), "InvalidArg");
    assert!(refusal["message"].as_str().unwrap().contains("doc.forms.delete"), "{refusal}");

    // An annotation isn't a widget.
    let square = doc.write(json!({ "kind": "annotations.create", "page": page_ref(PAGE),
        "draft": { "subtype": "square", "box": { "x": 300, "y": 300, "width": 40, "height": 40 } } }));
    let not_widget =
        doc.refused(json!({ "kind": "forms.deleteWidget", "widget": square["annotation"]["ref"] }));
    assert_eq!(code(&not_widget), "InvalidArg");
    assert!(not_widget["message"].as_str().unwrap().contains("annotations.delete"), "{not_widget}");
}

#[test]
fn widgets_and_annotations_each_keep_their_own_stacking_order() {
    let mut doc = Doc::toggle_fields();
    let square = doc.write(json!({ "kind": "annotations.create", "page": page_ref(PAGE),
        "draft": { "subtype": "square", "box": { "x": 300, "y": 300, "width": 40, "height": 40 } } }))
        ["annotation"]["ref"]
        .clone();
    let widgets = doc.widgets_on(PAGE);
    let annotations = doc.annotations_on(PAGE);
    assert!(widgets.len() >= 2);
    let (bottom, next) = (widgets[0].clone(), widgets[1].clone());

    // A widget to the top of the widgets: the annotations don't move.
    let to_top = doc.ok(json!({ "kind": "forms.reorderWidgets", "page": page_ref(PAGE),
        "widgets": [bottom], "position": "end" }));
    let mut expected: Vec<Value> = widgets[1..].to_vec();
    expected.push(bottom.clone());
    assert_eq!(to_top["result"]["page"], page_ref(PAGE));
    assert_eq!(to_top["result"]["order"], json!(expected));
    assert_eq!(to_top["result"]["meta"]["changedWidgets"][0]["ref"], bottom);
    assert_eq!(json!(doc.widgets_on(PAGE)), to_top["result"]["order"]);
    assert_eq!(doc.annotations_on(PAGE), annotations);
    assert_eq!(to_top["event"]["type"], "forms.widgetsReordered");
    assert_eq!(to_top["event"]["order"], to_top["result"]["order"]);

    // And back under its neighbour.
    let back = doc.write(json!({ "kind": "forms.reorderWidgets", "page": page_ref(PAGE),
        "widgets": [bottom], "position": { "before": next } }));
    assert_eq!(back["order"], json!(widgets));

    // Each verb orders its own family only, as rows and as neighbours.
    let widget_as_annotation = doc.refused(json!({ "kind": "annotations.reorder",
        "page": page_ref(PAGE), "refs": [bottom], "position": "start" }));
    assert_eq!(code(&widget_as_annotation), "InvalidArg");
    assert!(widget_as_annotation["message"].as_str().unwrap().contains("doc.forms.reorderWidgets"));
    let annotation_as_widget = doc.refused(json!({ "kind": "forms.reorderWidgets",
        "page": page_ref(PAGE), "widgets": [square], "position": "end" }));
    assert_eq!(code(&annotation_as_widget), "InvalidArg");
    assert!(annotation_as_widget["message"].as_str().unwrap().contains("annotations.reorder"));
    let foreign_neighbour = doc.refused(json!({ "kind": "forms.reorderWidgets",
        "page": page_ref(PAGE), "widgets": [bottom], "position": { "before": square } }));
    assert_eq!(code(&foreign_neighbour), "InvalidArg");
    assert_eq!(doc.widgets_on(PAGE), widgets);
}

#[test]
fn create_field_composes_styled_widgets_atomically() {
    let mut doc = Doc::toggle_fields();
    let rects = [
        json!({ "x": 20, "y": 60, "width": 20, "height": 20 }),
        json!({ "x": 60, "y": 60, "width": 20, "height": 20 }),
    ];
    let created = doc.write(json!({ "kind": "forms.createField", "draft": {
        "family": "radio", "name": "authored_radio", "noToggleToOff": true, "widgets": [
            { "page": page_ref(PAGE), "rect": rects[0], "exportValue": "yes", "color": "#000000",
              "strokeWidth": 1 },
            { "page": page_ref(PAGE), "rect": rects[1], "exportValue": "no" },
        ] } }));
    let field = created["field"].clone();
    assert_eq!(field["noToggleToOff"], true);
    let exports: Vec<&Value> =
        field["widgets"].as_array().unwrap().iter().map(|w| &w["exportValue"]).collect();
    assert_eq!(exports, vec![&json!("yes"), &json!("no")]);
    // Each widget lands where it was placed, and the form reads back the
    // same rects the create answered.
    for (at, rect) in rects.iter().enumerate() {
        close(&created["widgets"][at]["rect"], rect);
        let row = doc.row(&field["widgets"][at]["ref"]).expect("a row");
        close(&row["rect"], rect);
    }
    assert_eq!(doc.row(&field["widgets"][0]["ref"]).unwrap()["color"], "#000000");

    // A second button joins the group where its placement says.
    let third = doc.write(json!({ "kind": "forms.addWidget", "ref": field["ref"],
        "placement": { "page": page_ref(PAGE), "exportValue": "maybe",
            "rect": { "x": 100, "y": 60, "width": 20, "height": 20 } } }));
    let exports: Vec<Value> = third["field"]["widgets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["exportValue"].clone())
        .collect();
    assert_eq!(exports, vec![json!("yes"), json!("no"), json!("maybe")]);
    // A button needs an export value; a refused add creates nothing.
    let before = doc.form()["widgets"].as_array().unwrap().len();
    let refusal = doc.refused(json!({ "kind": "forms.addWidget", "ref": field["ref"],
        "placement": { "page": page_ref(PAGE),
            "rect": { "x": 140, "y": 60, "width": 20, "height": 20 } } }));
    assert_eq!(code(&refusal), "InvalidArg");
    assert_eq!(doc.form()["widgets"].as_array().unwrap().len(), before);
    assert_eq!(doc.field(&field["ref"]).unwrap()["widgets"].as_array().unwrap().len(), 3);

    // The newborn group fills through the normal value path.
    let filled = doc.write(
        json!({ "kind": "forms.setValue", "ref": field["ref"], "value": { "value": "yes" } }),
    );
    assert_eq!(filled["field"]["value"], "yes");

    // update: no family needed (the ref says it); rename and conflict.
    doc.write(json!({ "kind": "forms.updateField", "ref": field["ref"],
        "patch": { "name": "renamed_radio" } }));
    let conflict = doc.refused(json!({ "kind": "forms.updateField", "ref": field["ref"],
        "patch": { "name": "unison_radio" } }));
    assert_eq!(code(&conflict), "InvalidArg");
    // A member another family has, or a family that isn't the field's, is
    // refused.
    for patch in [json!({ "multiline": true }), json!({ "family": "text", "name": "x" })] {
        let refused = doc
            .refused(json!({ "kind": "forms.updateField", "ref": field["ref"], "patch": patch }));
        assert_eq!(code(&refused), "InvalidArg", "{patch}");
    }

    // delete cascades: the field gone and its widgets gone; meta names both.
    let widgets_before = doc.form()["widgets"].as_array().unwrap().len();
    let removed = doc.write(json!({ "kind": "forms.deleteField", "ref": field["ref"] }));
    assert_eq!(removed.as_object().unwrap().keys().collect::<Vec<_>>(), vec!["meta"]);
    assert_eq!(removed["meta"]["changedFields"], json!([field["ref"]]));
    assert_eq!(removed["meta"]["changedWidgets"].as_array().unwrap().len(), 3);
    assert_eq!(doc.form()["widgets"].as_array().unwrap().len(), widgets_before - 3);
    assert!(doc.field(&field["ref"]).is_none());
}

#[test]
fn a_widgets_colour_change_keeps_its_font_and_size() {
    let mut doc = Doc::toggle_fields();
    let created = doc.write(json!({ "kind": "forms.createField", "draft": {
        "family": "text", "name": "styled", "widgets": [{ "page": page_ref(PAGE),
            "rect": { "x": 20, "y": 160, "width": 160, "height": 24 },
            "fontFamily": "courier", "fontSize": 9 }] } }));
    let widget = created["field"]["widgets"][0]["ref"].clone();
    let recoloured = doc.write(json!({ "kind": "forms.updateWidget", "widget": widget,
        "patch": { "fontColor": "#cc0000" } }))["widget"]
        .clone();
    assert_eq!(
        (&recoloured["fontFamily"], &recoloured["fontSize"], &recoloured["fontColor"]),
        (&json!("courier"), &json!(9.0), &json!("#cc0000"))
    );
    // A new size keeps the font and the colour.
    let resized = doc.write(json!({ "kind": "forms.updateWidget", "widget": widget,
        "patch": { "fontSize": 11 } }))["widget"]
        .clone();
    assert_eq!(
        (&resized["fontFamily"], &resized["fontSize"], &resized["fontColor"]),
        (&json!("courier"), &json!(11.0), &json!("#cc0000"))
    );
    // A standard font that isn't the widget's changes it.
    let refonted = doc.write(json!({ "kind": "forms.updateWidget", "widget": widget,
        "patch": { "fontFamily": "times-roman" } }))["widget"]
        .clone();
    assert_eq!(
        (&refonted["fontFamily"], &refonted["fontSize"], &refonted["fontColor"]),
        (&json!("times-roman"), &json!(11.0), &json!("#cc0000"))
    );
}

#[test]
fn a_push_button_has_a_caption_and_an_action_and_holds_no_value() {
    let mut doc = Doc::toggle_fields();
    let created = doc.write(json!({ "kind": "forms.createField", "draft": {
        "family": "pushbutton", "name": "clear", "widgets": [{ "page": page_ref(PAGE),
            "rect": { "x": 20, "y": 120, "width": 120, "height": 28 },
            "caption": "Clear form", "interiorColor": "#f6f8fa",
            "actions": { "activate": { "type": "reset-form", "fields": null, "exclude": false } } }] } }));
    let field = created["field"].clone();
    assert_eq!(field["family"], "pushbutton");
    let widget = field["widgets"][0]["ref"].clone();
    let row = doc.row(&widget).unwrap();
    assert_eq!(
        (&row["fieldFamily"], &row["caption"], &row["interiorColor"], &row["hasAppearance"]),
        (&json!("pushbutton"), &json!("Clear form"), &json!("#f6f8fa"), &json!(true))
    );
    assert_eq!(row["actions"]["activate"]["root"]["type"], "reset-form", "{row}");

    // A push button holds no value: the value writes refuse it.
    let value = doc.refused(
        json!({ "kind": "forms.setValue", "ref": field["ref"], "value": { "value": "x" } }),
    );
    assert_eq!(code(&value), "InvalidArg");
    // Its field takes the settings every field has, and no family's own.
    doc.write(json!({ "kind": "forms.updateField", "ref": field["ref"],
        "patch": { "alternateName": "Clears every field" } }));
    let multiline = doc.refused(json!({ "kind": "forms.updateField", "ref": field["ref"],
        "patch": { "multiline": true } }));
    assert_eq!(code(&multiline), "InvalidArg");

    // Its drawing is the engine's to keep: a move, new actions or the row
    // sent back keep it; a new caption draws it again.
    let moved = doc.write(json!({ "kind": "forms.updateWidget", "widget": widget,
        "patch": { "rect": { "x": 30, "y": 130, "width": 120, "height": 28 } } }));
    let restyled = doc.write(json!({ "kind": "forms.updateWidget", "widget": widget,
        "patch": { "actions": { "focus": { "type": "reset-form", "fields": null, "exclude": false } } } }));
    let sent_back = doc.row(&widget).unwrap();
    let sent_back = doc.write(json!({ "kind": "forms.updateWidget", "widget": widget,
        "patch": sent_back }));
    for result in [&moved, &restyled, &sent_back] {
        assert_eq!(result["appearance"]["action"], "preserved", "{result}");
    }

    // The caption changes with the widget, and undo puts it back.
    let renamed = doc.ok(json!({ "kind": "forms.updateWidget", "opId": "rename-1",
        "widget": widget, "patch": { "caption": "Start over" } }));
    assert_eq!(renamed["result"]["appearance"]["action"], "regenerated");
    assert_eq!(doc.row(&widget).unwrap()["caption"], "Start over");
    doc.ok(json!({ "kind": "document.apply", "opId": "undo-rename",
        "change": { "undoOf": "rename-1" } }));
    assert_eq!(doc.row(&widget).unwrap()["caption"], "Clear form");
    doc.write(
        json!({ "kind": "forms.updateWidget", "widget": widget, "patch": { "caption": null } }),
    );
    let row = doc.row(&widget).unwrap();
    assert_eq!((&row["caption"], &row["hasAppearance"]), (&Value::Null, &json!(true)));

    // Another widget has no caption: its row reads none, sent back it is
    // kept, and a caption is refused, on its placement too.
    let checkbox = doc.form()["widgets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["fieldFamily"] == "checkbox")
        .cloned()
        .unwrap();
    assert_eq!(checkbox["caption"], Value::Null);
    doc.write(
        json!({ "kind": "forms.updateWidget", "widget": checkbox["ref"], "patch": checkbox }),
    );
    let caption = doc.refused(json!({ "kind": "forms.updateWidget", "widget": checkbox["ref"],
        "patch": { "caption": "Yes" } }));
    assert_eq!(code(&caption), "InvalidArg");
    let placed = doc.refused(json!({ "kind": "forms.createField", "draft": { "family": "text",
        "name": "captioned", "widgets": [{ "page": page_ref(PAGE),
            "rect": { "x": 20, "y": 160, "width": 120, "height": 20 }, "caption": "x" }] } }));
    assert_eq!(code(&placed), "InvalidArg");
}

#[test]
fn create_is_one_change_a_rejected_draft_creates_nothing() {
    let mut doc = Doc::toggle_fields();
    let names = |doc: &mut Doc| -> Vec<Value> {
        doc.form()["fields"].as_array().unwrap().iter().map(|f| f["name"].clone()).collect()
    };
    let names_before = names(&mut doc);
    let widgets_before = doc.widgets_on(PAGE).len();
    let placed = json!({ "page": page_ref(PAGE), "rect": { "x": 20, "y": 100, "width": 100, "height": 20 } });

    // Checked before anything is written.
    let negative = doc.refused(json!({ "kind": "forms.createField", "draft": { "family": "text",
        "name": "rejected_text", "maxLength": -5, "widgets": [placed] } }));
    assert_eq!(code(&negative), "InvalidArg");
    let mut nowhere = placed.clone();
    nowhere["page"] = page_ref(999_999);
    let missing = doc.refused(json!({ "kind": "forms.createField", "draft": { "family": "text",
        "name": "rejected_text", "widgets": [nowhere] } }));
    assert_eq!(code(&missing), "NotFound", "{missing}");
    // Refused after the field exists: the whole create is undone.
    let late = doc.refused(json!({ "kind": "forms.createField", "draft": {
        "family": "combobox", "name": "rejected_choice", "options": [{ "label": "A", "value": "a" }],
        "defaultValue": "not-an-option", "widgets": [placed] } }));
    assert_eq!(code(&late), "InvalidArg");

    assert_eq!(names(&mut doc), names_before);
    assert_eq!(doc.widgets_on(PAGE).len(), widgets_before);

    // The names are still free.
    let created = doc.write(json!({ "kind": "forms.createField", "draft": { "family": "text",
        "name": "rejected_text", "widgets": [placed] } }));
    assert_eq!(created["field"]["name"], "rejected_text");
    doc.write(json!({ "kind": "forms.deleteField", "ref": created["field"]["ref"] }));
}

#[test]
fn flags_list_box_defaults_and_signature_fields_are_written_like_any_other_member() {
    let mut doc = Doc::toggle_fields();
    let list = doc.write(json!({ "kind": "forms.createField", "draft": {
        "family": "listbox", "name": "defaults_list", "multiSelect": true,
        "options": [{ "label": "A", "value": "a" }, { "label": "B", "value": "b" },
            { "label": "C", "value": "c" }],
        "defaultValue": ["a", "c"], "required": true,
        "widgets": [{ "page": page_ref(PAGE), "rect": { "x": 20, "y": 140, "width": 100, "height": 60 } }] } }));
    let field = list["field"].clone();
    assert_eq!(field["defaultValue"], json!(["a", "c"]));
    assert_eq!(
        (&field["readOnly"], &field["required"], &field["noExport"]),
        (&json!(false), &json!(true), &json!(false))
    );

    // A default must be option values.
    let unknown = doc.refused(json!({ "kind": "forms.updateField", "ref": field["ref"],
        "patch": { "defaultValue": ["z"] } }));
    assert_eq!(code(&unknown), "InvalidArg");

    // Reset selects the default.
    doc.write(json!({ "kind": "forms.updateField", "ref": field["ref"],
        "patch": { "defaultValue": ["b"] } }));
    doc.write(json!({ "kind": "forms.setValue", "ref": field["ref"],
        "value": { "selectedValues": ["a"] } }));
    let reset = doc.write(json!({ "kind": "forms.reset", "refs": [field["ref"]] }));
    assert_eq!(reset["fields"][0]["selectedValues"], json!(["b"]));

    // A flag left out keeps its value.
    doc.write(
        json!({ "kind": "forms.updateField", "ref": field["ref"], "patch": { "readOnly": true } }),
    );
    let now = doc.field(&field["ref"]).unwrap();
    assert_eq!((&now["readOnly"], &now["required"]), (&json!(true), &json!(true)));
    doc.write(
        json!({ "kind": "forms.updateField", "ref": field["ref"], "patch": { "required": false } }),
    );
    let now = doc.field(&field["ref"]).unwrap();
    assert_eq!((&now["readOnly"], &now["required"]), (&json!(true), &json!(false)));
    // Flags are members of their own, not one object.
    let flags = doc.refused(json!({ "kind": "forms.updateField", "ref": field["ref"],
        "patch": { "flags": { "required": true } } }));
    assert_eq!(code(&flags), "InvalidArg");

    // A signature field takes the settings every field shares.
    let signature = doc.write(json!({ "kind": "forms.createField", "draft": {
        "family": "signature", "name": "sign_here",
        "widgets": [{ "page": page_ref(PAGE), "rect": { "x": 20, "y": 220, "width": 150, "height": 40 } }] } }));
    let sign = signature["field"]["ref"].clone();
    doc.write(json!({ "kind": "forms.updateField", "ref": sign, "patch": {
        "name": "signed_here", "alternateName": "Sign here", "required": true } }));
    let renamed = doc.field(&sign).unwrap();
    assert_eq!(
        (&renamed["family"], &renamed["name"], &renamed["alternateName"], &renamed["required"]),
        (&json!("signature"), &json!("signed_here"), &json!("Sign here"), &json!(true))
    );
    let multiline = doc.refused(
        json!({ "kind": "forms.updateField", "ref": sign, "patch": { "multiline": true } }),
    );
    assert_eq!(code(&multiline), "InvalidArg");

    doc.write(json!({ "kind": "forms.deleteField", "ref": field["ref"] }));
    doc.write(json!({ "kind": "forms.deleteField", "ref": sign }));
}

#[test]
fn delete_removes_a_merged_field_widget_from_the_tree_and_its_page_in_one_mutation() {
    // maxlen_text and billing.name are each one dictionary that is both the
    // field and its only widget.
    let mut doc = Doc::toggle_fields();
    let mut events = Vec::new();
    for name in ["maxlen_text", "billing.name"] {
        let field = doc.field_named(name);
        let merged = field["ref"]["objectNumber"].clone();
        let widgets: Vec<Value> = field["widgets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|w| w["objectNumber"].clone())
            .collect();
        assert_eq!(widgets, vec![merged.clone()]);
        let removed = doc.ok(json!({ "kind": "forms.deleteField", "ref": field["ref"] }));
        assert_eq!(removed["result"]["meta"]["changedFields"], json!([field["ref"]]));
        let gone: Vec<Value> = removed["result"]["meta"]["changedWidgets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|w| w["objectNumber"].clone())
            .collect();
        assert_eq!(gone, vec![merged.clone()]);
        assert!(
            doc.widgets_on(PAGE).iter().all(|w| w["objectNumber"] != merged),
            "{name} is still on its page"
        );
        assert!(doc.field(&field["ref"]).is_none());
        events.push(removed["event"]["type"].clone());
    }
    assert_eq!(events, vec![json!("forms.deleted"), json!("forms.deleted")]);
    let form = doc.form();
    let names: Vec<&Value> =
        form["fields"].as_array().unwrap().iter().map(|f| &f["name"]).collect();
    assert!(!names.contains(&&json!("maxlen_text")) && !names.contains(&&json!("billing.name")));
    assert!(names.contains(&&json!("opt_check")));
}

#[test]
fn a_merged_field_widget_cannot_be_removed_from_its_field_or_deleted_as_an_annotation() {
    let mut doc = Doc::toggle_fields();
    let field = doc.field_named("opt_check");
    let widget = field["widgets"][0]["ref"].clone();
    assert_eq!(widget["objectNumber"], field["ref"]["objectNumber"]);

    // Removing the field's own dictionary from itself is refused.
    let detach =
        doc.refused(json!({ "kind": "forms.detachWidget", "ref": field["ref"], "widget": widget }));
    assert_eq!(code(&detach), "InvalidArg");
    assert!(detach["message"].as_str().unwrap().contains("merged field/widget"), "{detach}");

    // The annotation plane refuses too, and names the only verb that works.
    let remove = doc.refused(json!({ "kind": "annotations.delete", "ref": widget }));
    assert_eq!(code(&remove), "InvalidArg");
    let message = remove["message"].as_str().unwrap();
    assert!(message.contains("doc.forms.delete") && !message.contains("removeWidget"), "{remove}");

    // Both refusals left the field and its placement untouched.
    let after = doc.field(&field["ref"]).unwrap();
    assert_eq!(after["widgets"][0]["objectNumber"], widget["objectNumber"]);
    assert!(doc.row(&widget).is_some());
}

#[test]
fn a_field_create_update_and_delete_undo_and_redo() {
    // The reverse steps: a create's `field.remove`, an update's captures
    // and properties, a delete's `field.restore`, an added widget's and a
    // deleted widget's.
    let mut doc = Doc::toggle_fields();
    let created = doc.ok(json!({ "kind": "forms.createField", "opId": "create-1", "draft": {
        "family": "text", "name": "undo_me", "defaultValue": "start",
        "widgets": [{ "page": page_ref(PAGE), "rect": { "x": 20, "y": 400, "width": 100, "height": 20 } }] } }));
    let field = created["result"]["field"].clone();
    let reference = field["ref"].clone();
    assert_eq!(field["defaultValue"], "start");

    doc.ok(json!({ "kind": "forms.updateField", "opId": "update-1", "ref": reference,
        "patch": { "name": "renamed", "defaultValue": null, "maxLength": 9 } }));
    let updated = doc.field(&reference).unwrap();
    assert_eq!(
        (&updated["name"], &updated["defaultValueEntry"], &updated["maxLength"]),
        (&json!("renamed"), &json!({ "kind": "none" }), &json!(9))
    );
    doc.ok(json!({ "kind": "document.apply", "opId": "undo-update",
        "change": { "undoOf": "update-1" } }));
    let back = doc.field(&reference).unwrap();
    assert_eq!(
        (&back["name"], &back["defaultValue"], &back["maxLength"]),
        (&json!("undo_me"), &json!("start"), &Value::Null)
    );
    doc.ok(json!({ "kind": "document.apply", "opId": "redo-update",
        "change": { "undoOf": "undo-update" } }));
    assert_eq!(doc.field(&reference).unwrap()["name"], "renamed");

    let widgets = doc.widgets_on(PAGE).len();
    doc.ok(json!({ "kind": "forms.deleteField", "opId": "delete-1", "ref": reference }));
    assert!(doc.field(&reference).is_none());
    assert_eq!(doc.widgets_on(PAGE).len(), widgets - 1);
    doc.ok(json!({ "kind": "document.apply", "opId": "undo-delete",
        "change": { "undoOf": "delete-1" } }));
    let restored = doc.field(&reference).expect("the field is back");
    assert_eq!(restored["name"], "renamed");
    assert_eq!(doc.widgets_on(PAGE).len(), widgets);

    // An added widget's undo leaves it reaching nothing.
    let added = doc.ok(json!({ "kind": "forms.addWidget", "opId": "add-1", "ref": reference,
        "placement": { "page": page_ref(PAGE), "rect": { "x": 200, "y": 400, "width": 100, "height": 20 } } }));
    let new_widget = added["result"]["meta"]["changedWidgets"][0]["ref"].clone();
    assert!(doc.row(&new_widget).is_some());
    doc.ok(json!({ "kind": "document.apply", "opId": "undo-add",
        "change": { "undoOf": "add-1" } }));
    assert!(doc.row(&new_widget).is_none(), "the added widget is still on its page");
    assert_eq!(doc.field(&reference).unwrap()["widgets"].as_array().unwrap().len(), 1);

    // A deleted widget comes back at its place and into its field.
    let widget = doc.field(&reference).unwrap()["widgets"][0]["ref"].clone();
    doc.ok(json!({ "kind": "forms.deleteWidget", "opId": "drop-1", "widget": widget }));
    assert!(doc.row(&widget).is_none());
    doc.ok(json!({ "kind": "document.apply", "opId": "undo-drop",
        "change": { "undoOf": "drop-1" } }));
    assert_eq!(doc.row(&widget).expect("back")["field"], reference);

    // The create's undo deletes the field, while nobody changed it since.
    let fresh = doc.ok(json!({ "kind": "forms.createField", "opId": "create-2", "draft": {
        "family": "checkbox", "name": "fresh" } }));
    let fresh = fresh["result"]["field"]["ref"].clone();
    doc.ok(json!({ "kind": "document.apply", "opId": "undo-create",
        "change": { "undoOf": "create-2" } }));
    assert!(doc.field(&fresh).is_none());
}

#[test]
fn a_calculate_script_keeps_the_field_in_the_calculation_order() {
    // `followCalculateScript` and `forms.reorderCalculations` (98718b780).
    let mut doc = Doc::toggle_fields();
    let script = |text: &str| json!({ "type": "javascript", "script": text });
    let first = doc.ok(json!({ "kind": "forms.createField", "draft": { "family": "text",
        "name": "total_a", "actions": { "calculate": script("event.value = 1") } } }));
    let second = doc.ok(json!({ "kind": "forms.createField", "draft": { "family": "text",
        "name": "total_b", "actions": { "calculate": script("event.value = 2") } } }));
    let a = first["result"]["field"]["ref"].clone();
    let b = second["result"]["field"]["ref"].clone();
    assert_eq!(second["result"]["calculationOrder"], json!([a, b]), "{second}");
    assert_eq!(doc.form()["calculationOrder"], json!([a, b]));
    let calculate = &doc.field(&a).unwrap()["actions"]["calculate"]["root"];
    assert_eq!(calculate["script"], "event.value = 1");

    let reordered = doc.ok(json!({ "kind": "forms.reorderCalculations", "opId": "calc-1",
        "fields": [b], "position": "start" }));
    assert_eq!(reordered["result"]["calculationOrder"], json!([b, a]));
    assert_eq!(reordered["event"]["type"], "forms.calculationsReordered");
    assert_eq!(doc.form()["calculationOrder"], json!([b, a]));
    doc.ok(
        json!({ "kind": "document.apply", "opId": "undo-calc", "change": { "undoOf": "calc-1" } }),
    );
    assert_eq!(doc.form()["calculationOrder"], json!([a, b]));

    // Removing the script takes the field out of the order.
    let updated = doc.ok(json!({ "kind": "forms.updateField", "ref": a,
        "patch": { "actions": { "calculate": null } } }));
    assert_eq!(updated["result"]["calculationOrder"], json!([b]));
    assert_eq!(doc.form()["calculationOrder"], json!([b]));
    // A script field may not be written without `doc.forms.script`.
    let refused = doc.refused(json!({ "kind": "forms.createField", "draft": { "family": "text",
        "name": "sneaky", "actions": { "calculate": script("1") } },
        "authority": { "identity": {}, "grants": { "scope": ["doc.forms.modify", "doc.open"],
            "pdfBits": { "bit3": true, "bit4": true, "bit5": true, "bit6": true, "bit9": true,
                "bit10": true, "bit11": true, "bit12": true } },
            "protection": { "enforced": null, "judged": null, "certification": null,
                "fieldLocks": [], "policyVersion": 4 } } }));
    assert_eq!(code(&refused), "Forbidden", "{refused}");
    assert_eq!(refused["details"]["required"], "doc.forms.script", "{refused}");
}

/// `FIELD_GROUP_TOKENS.designer`: designs the form, in the `legal` group,
/// and may put fields in `buyer`'s.
fn designer() -> Value {
    json!({ "identity": { "userId": "lisa", "groupId": "legal" },
        "grants": { "scope": ["doc.open", "doc.render", "doc.forms.modify",
            "fields:set-group:group=buyer"],
            "pdfBits": { "bit3": true, "bit4": true, "bit5": true, "bit6": true, "bit9": true,
                "bit10": true, "bit11": true, "bit12": true } },
        "protection": { "enforced": null, "judged": null, "certification": null,
            "fieldLocks": [], "policyVersion": 4 } })
}

#[test]
fn a_designer_puts_fields_in_her_own_group_or_one_she_may_set() {
    // `runFieldGroupsConformance`: a create goes in the caller's own group,
    // or one she may set; another is refused, naming the scope it takes.
    let mut doc = Doc::toggle_fields();
    let placed = |x: u32| {
        json!({ "page": page_ref(PAGE),
        "rect": { "x": x, "y": 200, "width": 100, "height": 20 } })
    };
    let own = doc.write(json!({ "kind": "forms.createField", "authority": designer(),
        "draft": { "family": "text", "name": "clause", "widgets": [placed(20)] } }));
    assert_eq!(own["field"]["groupId"], "legal");
    let buyers = doc.write(json!({ "kind": "forms.createField", "authority": designer(),
        "draft": { "family": "text", "name": "buyer_address", "groupId": "buyer",
            "widgets": [placed(140)] } }));
    assert_eq!(buyers["field"]["groupId"], "buyer");
    let refused = doc.refused(json!({ "kind": "forms.createField", "authority": designer(),
        "draft": { "family": "text", "name": "seller_address", "groupId": "seller",
            "widgets": [placed(260)] } }));
    assert_eq!(code(&refused), "Forbidden", "{refused}");
    assert_eq!(refused["details"]["required"], "fields:set-group:group=seller", "{refused}");
    assert!(doc.form()["fields"].as_array().unwrap().iter().all(|f| f["name"] != "seller_address"));

    let moved = doc.write(json!({ "kind": "forms.updateField", "authority": designer(),
        "ref": own["field"]["ref"], "patch": { "groupId": "buyer" } }));
    assert_eq!(moved["field"]["groupId"], "buyer");
    let refused = doc.refused(json!({ "kind": "forms.updateField", "authority": designer(),
        "ref": own["field"]["ref"], "patch": { "groupId": "seller" } }));
    assert_eq!(code(&refused), "Forbidden", "{refused}");
}

// ── Widget rotation (`runWidgetRotationConformance`) ──────────────────────

/// `pdfOf`: a PDF of `objects`, numbered from 1.
fn pdf_of(objects: &[String]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for offset in offsets {
        out.extend(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

fn text_field(name: &str, rect: &str, mk: &str, page: u32) -> String {
    format!(
        "<< /Type /Annot /Subtype /Widget /FT /Tx /T ({name}) /V ({name}) /Rect [{rect}] {mk} /P {page} 0 R >>"
    )
}

/// `TURNED_FIELDS_PDF`: two 300 × 300 pages, the second shown turned 90°,
/// and text fields with `/MK /R` as Acrobat writes it, counterclockwise.
fn turned_fields() -> Vec<u8> {
    pdf_of(&[
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [6 0 R 7 0 R 8 0 R 9 0 R 10 0 R 11 0 R 12 0 R] /DA (/Helv 10 Tf 0 g) /DR << /Font << /Helv << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >> >> >>".into(),
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Contents 5 0 R /Resources << >> /Annots [6 0 R 7 0 R 8 0 R 9 0 R 10 0 R 11 0 R] >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Rotate 90 /Contents 5 0 R /Resources << >> /Annots [12 0 R] >>".into(),
        "<< /Length 0 >>\nstream\n\nendstream".into(),
        text_field("upright", "20 260 180 284", "", 3),
        text_field("r90", "20 20 44 180", "/MK << /R 90 >>", 3),
        text_field("r180", "60 220 220 244", "/MK << /R 180 >>", 3),
        text_field("r270", "250 20 274 180", "/MK << /R 270 >>", 3),
        text_field("odd", "60 180 220 204", "/MK << /R 45 >>", 3),
        text_field("negative", "200 20 224 180", "/MK << /R -90 >>", 3),
        text_field("on-turned", "20 20 44 180", "/MK << /R 90 >>", 4),
    ])
}

/// `box` with its sides swapped about its middle.
fn swapped(rect: &Value) -> Value {
    let (x, y, w, h) = (
        rect["x"].as_f64().unwrap(),
        rect["y"].as_f64().unwrap(),
        rect["width"].as_f64().unwrap(),
        rect["height"].as_f64().unwrap(),
    );
    json!({ "x": x + (w - h) / 2.0, "y": y + (h - w) / 2.0, "width": h, "height": w })
}

#[test]
fn mk_r_reads_as_the_clockwise_turn_with_the_box_it_turns() {
    let mut doc = Doc::open(turned_fields());
    let mut turns = serde_json::Map::new();
    for name in ["upright", "r90", "r180", "r270", "odd", "negative", "on-turned"] {
        turns.insert(name.into(), doc.row_of(name)["rotation"].clone());
    }
    assert_eq!(
        Value::Object(turns),
        json!({ "upright": null, "r90": 270, "r180": 180, "r270": 90, "odd": null,
            "negative": 90, "on-turned": 270 })
    );
    let r90 = doc.row_of("r90");
    close(&r90["rect"], &json!({ "x": 20, "y": 120, "width": 24, "height": 160 }));
    close(&r90["box"], &swapped(&r90["rect"]));
    let r180 = doc.row_of("r180");
    close(&r180["box"], &r180["rect"]);
}

#[test]
fn a_turned_field_fills_and_saves_with_its_turn() {
    let mut doc = Doc::open(turned_fields());
    let field = doc.field_named("r90");
    doc.write(
        json!({ "kind": "forms.setValue", "ref": field["ref"], "value": { "value": "Turned" } }),
    );
    let response = doc.host.call(
        &json!({ "kind": "document.saveBuffer", "jobId": 90, "docId": "d", "mode": "rewrite" })
            .to_string(),
        Vec::new(),
    );
    assert!(!response.json.contains("\"error\""), "{}", response.json);
    let bytes = response.payloads.into_iter().next().expect("the saved bytes");
    let mut saved = Doc::open(bytes);
    let row = saved.row_of("r90");
    assert_eq!(row["rotation"], 270);
    close(&row["rect"], &json!({ "x": 20, "y": 120, "width": 24, "height": 160 }));
    assert_eq!(saved.field_named("r90")["value"], "Turned");
}

#[test]
fn a_placements_rotation_turns_a_new_fields_contents_inside_its_rect() {
    let mut doc = Doc::open(turned_fields());
    let rect = json!({ "x": 100, "y": 40, "width": 24, "height": 160 });
    doc.write(json!({ "kind": "forms.createField", "draft": { "family": "text", "name": "side",
        "widgets": [{ "page": page_ref(3), "rect": rect, "rotation": 270 }] } }));
    let row = doc.row_of("side");
    assert_eq!(row["rotation"], 270);
    close(&row["rect"], &rect);
    close(&row["box"], &swapped(&rect));
    let refused = doc.refused(json!({ "kind": "forms.createField", "draft": { "family": "text",
        "name": "tilted", "widgets": [{ "page": page_ref(3), "rect": rect, "rotation": 45 }] } }));
    assert_eq!(code(&refused), "InvalidArg");
}

#[test]
fn a_placement_gives_where_the_widget_stands_or_its_frame_the_same_widget_either_way() {
    let mut doc = Doc::open(turned_fields());
    let rect = json!({ "x": 150, "y": 40, "width": 24, "height": 160 });
    doc.write(json!({ "kind": "forms.createField", "draft": { "family": "text", "name": "by-box",
        "widgets": [{ "page": page_ref(3), "box": swapped(&rect), "rotation": 270 }] } }));
    let row = doc.row_of("by-box");
    assert_eq!(row["rotation"], 270);
    close(&row["rect"], &rect);
    close(&row["box"], &swapped(&rect));
    let neither = doc.refused(json!({ "kind": "forms.createField", "draft": { "family": "text",
        "name": "nowhere", "widgets": [{ "page": page_ref(3) }] } }));
    assert_eq!(code(&neither), "InvalidArg");
}

#[test]
fn a_turn_turns_the_widget_about_its_middle_and_a_new_box_keeps_it_in_place() {
    let mut doc = Doc::open(turned_fields());
    let before = doc.row_of("upright");
    let turned = doc.write(json!({ "kind": "forms.updateWidget", "widget": before["ref"],
        "patch": { "rotation": 90 } }))["widget"]
        .clone();
    assert_eq!(turned["rotation"], 90);
    close(&turned["box"], &before["box"]);
    close(&turned["rect"], &swapped(&before["rect"]));
    // Acrobat's Orientation: the text turns, the footprint stays.
    let oriented = doc.write(json!({ "kind": "forms.updateWidget", "widget": before["ref"],
        "patch": { "rotation": 270, "box": swapped(&before["rect"]) } }))["widget"]
        .clone();
    assert_eq!(oriented["rotation"], 270);
    close(&oriented["rect"], &before["rect"]);
    let straight = doc.write(json!({ "kind": "forms.updateWidget", "widget": before["ref"],
        "patch": { "rotation": null } }))["widget"]
        .clone();
    assert_eq!(straight["rotation"], Value::Null);
}

#[test]
fn rect_moves_and_resizes_a_quarter_turned_widget_and_its_turn_stays() {
    let mut doc = Doc::open(turned_fields());
    let row = doc.row_of("r90");
    let rect = json!({ "x": 30, "y": 60, "width": 40, "height": 200 });
    let widget = doc.write(json!({ "kind": "forms.updateWidget", "widget": row["ref"],
        "patch": { "rect": rect } }))["widget"]
        .clone();
    assert_eq!(widget["rotation"], 270);
    close(&widget["rect"], &rect);
    close(&widget["box"], &swapped(&rect));
}

#[test]
fn a_widgets_turn_is_a_quarter_turn_and_another_is_refused() {
    let mut doc = Doc::open(turned_fields());
    let row = doc.row_of("upright");
    let refused = doc.refused(json!({ "kind": "forms.updateWidget", "widget": row["ref"],
        "patch": { "rotation": 45 } }));
    assert_eq!(code(&refused), "InvalidArg");
    assert_eq!(doc.row_of("upright")["rotation"], Value::Null);
}

#[test]
fn undo_puts_a_turn_back_and_redo_turns_it_again() {
    let mut doc = Doc::open(turned_fields());
    let row = doc.row_of("r180");
    doc.ok(json!({ "kind": "forms.updateWidget", "opId": "turn-1", "widget": row["ref"],
        "patch": { "rotation": 90 } }));
    doc.ok(json!({ "kind": "document.apply", "opId": "undo-1", "change": { "undoOf": "turn-1" } }));
    let back = doc.row_of("r180");
    assert_eq!(back["rotation"], 180);
    close(&back["rect"], &row["rect"]);
    doc.ok(json!({ "kind": "document.apply", "opId": "redo-1", "change": { "undoOf": "undo-1" } }));
    assert_eq!(doc.row_of("r180")["rotation"], 90);
}
