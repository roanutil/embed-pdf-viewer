//! G8's forms gate: the field tree agrees with the TypeScript engine.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run forms
//!
//! Whole snapshots, every field, every widget. The one thing not compared
//! cross-language is `exportData`: FDF and XFDF are serializations PDFium
//! writes, so comparing them would compare one writer to itself, and the
//! output carries a path that differs by working directory. The export's
//! shape is asserted here directly instead.

use epdf_engine::cancel::CancelToken;
use epdf_engine::features::forms::snapshot::{
    FormFieldFamily, FormFieldKind, FormKind, FormValueEntry, read_form_snapshot,
};
use pdfium::{Document, FormDataFormat, Library};
use serde::Deserialize;

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VectorFile {
    schema_version: u32,
    kind: String,
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct Case {
    name: String,
    fixture: String,
    covers: String,
    snapshot: serde_json::Value,
}

fn load() -> VectorFile {
    let path = test_support::corpus::vectors_dir("forms").join("snapshot.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: VectorFile = serde_json::from_str(&text).expect("snapshot.json");
    assert_eq!(file.schema_version, SCHEMA_VERSION, "vector envelope moved");
    assert_eq!(file.kind, "forms.snapshot");
    file
}

fn open(case: &Case) -> Document {
    let bytes = std::fs::read(test_support::workspace_root().join(&case.fixture))
        .unwrap_or_else(|e| panic!("{}: read {}: {e}", case.name, case.fixture));
    Document::from_bytes(Library::acquire(), &bytes, None)
        .unwrap_or_else(|e| panic!("{}: open: {e}", case.name))
}

fn read(case: &Case) -> serde_json::Value {
    let document = open(case);
    let model = document.form_model().unwrap_or_else(|| panic!("{}: no form model", case.name));
    let snapshot = read_form_snapshot(&model, &CancelToken::never())
        .unwrap_or_else(|e| panic!("{}: read: {e}", case.name));
    serde_json::to_value(&snapshot).expect("serialize")
}

/// See `annotation_reader.rs`: the two serializers spell a whole number
/// differently and JSON has one number type.
fn canonical_numbers(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Number(number) => match number.as_f64() {
            Some(float) if float.fract() == 0.0 && float.abs() < 9e15 => {
                serde_json::Value::from(float as i64)
            }
            _ => value.clone(),
        },
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(canonical_numbers).collect())
        }
        serde_json::Value::Object(fields) => serde_json::Value::Object(
            fields.iter().map(|(key, entry)| (key.clone(), canonical_numbers(entry))).collect(),
        ),
        _ => value.clone(),
    }
}

#[test]
fn the_snapshot_agrees_with_the_typescript_engine() {
    let file = load();
    assert!(file.cases.len() >= 3, "the corpus shrank");
    let mut fields = 0usize;

    for case in &file.cases {
        let actual = read(case);
        let theirs = canonical_numbers(&case.snapshot);
        let mine = canonical_numbers(&actual);

        // Compared field by field before the whole, so a failure names which
        // one rather than printing two large objects.
        let mine_fields = mine["fields"].as_array().expect("fields");
        let their_fields = theirs["fields"].as_array().expect("fields");
        assert_eq!(
            mine_fields.len(),
            their_fields.len(),
            "{}: a different number of fields: {}",
            case.name,
            case.covers,
        );
        for (index, (mine, theirs)) in mine_fields.iter().zip(their_fields).enumerate() {
            assert_eq!(
                mine,
                theirs,
                "{}: field {index} ({}): {}",
                case.name,
                theirs["name"].as_str().unwrap_or("?"),
                case.covers,
            );
        }
        assert_eq!(mine, theirs, "{}: the snapshot envelope differs", case.name);
        fields += mine_fields.len();
    }

    assert!(fields > 0, "the corpus has no form fields at all");
}

#[test]
fn every_family_the_reader_models_appears_in_the_corpus() {
    // A comparison over a corpus that never produces a family says nothing
    // about that family's arm. Signature and pushbutton in particular hold no
    // value and would be easy to leave unexercised.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "families").expect("the families fixture");
    let actual = read(case);
    let families: Vec<&str> = actual["fields"]
        .as_array()
        .expect("fields")
        .iter()
        .map(|f| f["family"].as_str().expect("a family"))
        .collect();

    for family in [
        FormFieldFamily::Text,
        FormFieldFamily::Checkbox,
        FormFieldFamily::Radio,
        FormFieldFamily::Combobox,
        FormFieldFamily::Listbox,
        FormFieldFamily::Pushbutton,
        FormFieldFamily::Signature,
        FormFieldFamily::Unknown,
    ] {
        let name = serde_json::to_value(family).expect("serialize");
        let name = name.as_str().expect("a name");
        assert!(families.contains(&name), "no {name} field in the corpus");
    }
}

#[test]
fn a_radio_group_is_one_field_with_several_widgets() {
    // The shape that separates the field plane from the widget plane. The
    // value is the field's and the checked state is each widget's, and a
    // reader that put both in one place would still look right on a
    // checkbox, which has one of each.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "families").expect("families");
    let document = open(case);
    let model = document.form_model().expect("model");
    let snapshot = read_form_snapshot(&model, &CancelToken::never()).expect("read");

    let radio = snapshot.fields.iter().find(|f| f.base.name == "colour").expect("the radio field");
    let FormFieldKind::Radio { value, widgets, no_toggle_to_off, .. } = &radio.kind else {
        panic!("colour is not a radio field");
    };
    assert_eq!(widgets.len(), 2, "one field, two widgets");
    assert_eq!(widgets.iter().filter(|w| w.checked).count(), 1, "exactly one is checked");
    assert_eq!(value, "Blue");
    assert_eq!(
        widgets.iter().find(|w| w.checked).map(|w| w.export_value.as_str()),
        Some("Blue"),
        "the field's value is the checked widget's export value",
    );
    assert!(no_toggle_to_off);

    // And the two widgets are on the same page but are different annotations,
    // which is what makes the join key per widget rather than per field.
    assert_ne!(widgets[0].widget.annot_object_number, widgets[1].widget.annot_object_number);
    assert_eq!(widgets[0].widget.page_object_number, widgets[1].widget.page_object_number);
}

#[test]
fn the_lossless_value_survives_the_family_convenience() {
    // Every family derives a convenience from `/V` and every one of them
    // loses information. The entry keeps the shape, which is what a write
    // path needs to see: a list box holding one selection and a text field
    // holding one string are the same convenience and different documents.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "families").expect("families");
    let document = open(case);
    let model = document.form_model().expect("model");
    let snapshot = read_form_snapshot(&model, &CancelToken::never()).expect("read");

    let by_name = |name: &str| {
        snapshot
            .fields
            .iter()
            .find(|f| f.base.name == name)
            .unwrap_or_else(|| panic!("no field named {name}"))
    };

    // An array `/V`, which no other field here has.
    let languages = by_name("languages");
    assert_eq!(
        languages.base.value_entry,
        FormValueEntry::Array { values: vec!["en".into(), "fr".into()] },
    );
    let FormFieldKind::Listbox { selected_values, multi_select, options, .. } = &languages.kind
    else {
        panic!("languages is not a list box");
    };
    assert!(multi_select);
    assert_eq!(selected_values, &["en".to_owned(), "fr".to_owned()]);
    assert_eq!(options.len(), 3, "the unselected option is still reported");

    // A `/V` that is a number: present, and not a value this engine models.
    // Different from absent, which is what the field beside it has.
    assert_eq!(by_name("quantity").base.value_entry, FormValueEntry::Unsupported);
    assert_eq!(by_name("notes").base.value_entry, FormValueEntry::None);
}

#[test]
fn a_field_the_acroform_tree_forgot_is_recovered_and_says_so() {
    // A field reachable only through a page's `/Annots` is a common producer
    // bug. It is reconciled so it behaves like any other field, and its
    // origin is what tells a caller that other PDF processors will not see
    // it until a repair makes the fix durable.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "families").expect("families");
    let document = open(case);
    let model = document.form_model().expect("model");
    let snapshot = read_form_snapshot(&model, &CancelToken::never()).expect("read");

    use epdf_engine::features::forms::snapshot::FormFieldOrigin;
    let orphan = snapshot.fields.iter().find(|f| f.base.name == "orphan").expect("orphan");
    assert_eq!(orphan.base.origin, FormFieldOrigin::Recovered);
    assert_eq!(
        snapshot.fields.iter().filter(|f| f.base.origin == FormFieldOrigin::Recovered).count(),
        1,
        "only the orphan is recovered",
    );
}

#[test]
fn a_calculation_order_slot_naming_nothing_is_preserved_rather_than_dropped() {
    // `/CO` is an ORDER. Dropping a slot that names a field the model does
    // not have would shift every slot after it, which changes which field
    // recalculates first.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "families").expect("families");
    let document = open(case);
    let model = document.form_model().expect("model");
    let snapshot = read_form_snapshot(&model, &CancelToken::never()).expect("read");

    assert_eq!(snapshot.calculation_order.len(), 2);
    assert!(snapshot.calculation_order[0].is_some(), "the first slot names a real field");
    assert!(snapshot.calculation_order[1].is_none(), "the second names nothing and stays");
}

#[test]
fn a_document_with_no_acroform_still_produces_a_snapshot() {
    // An empty answer is a different answer from a failed one, and the
    // distinction is what a caller uses to decide whether to show a form
    // layer at all.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "no-form").expect("the no-form fixture");
    let document = open(case);
    let model = document.form_model().expect("a model is built even with no /AcroForm");
    let snapshot = read_form_snapshot(&model, &CancelToken::never()).expect("read");

    assert_eq!(snapshot.form_kind, FormKind::None);
    assert!(snapshot.fields.is_empty());
    assert!(snapshot.calculation_order.is_empty());
    assert!(!snapshot.needs_appearances);
}

#[test]
fn an_export_serializes_the_reconciled_view() {
    // Not compared against the TypeScript: both engines call one PDFium
    // writer, so a comparison would compare it to itself, and the output
    // carries a source path that differs by working directory. What is worth
    // asserting is that both formats produce their own well-formed envelope
    // and that the field values are in there.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "families").expect("families");
    let document = open(case);

    let fdf = document.export_form_data(FormDataFormat::Fdf).expect("fdf");
    let fdf_text = String::from_utf8_lossy(&fdf);
    assert!(fdf_text.starts_with("%FDF-"), "an FDF export starts with its header");
    assert!(fdf_text.contains("billing.name"), "the field names are in the export");

    let xfdf = document.export_form_data(FormDataFormat::Xfdf).expect("xfdf");
    let xfdf_text = String::from_utf8_lossy(&xfdf);
    assert!(xfdf_text.starts_with("<?xml"), "an XFDF export is XML");
    assert!(xfdf_text.contains("http://ns.adobe.com/xfdf/"), "and declares its namespace");
    // XFDF NESTS a qualified name back into the tree it was flattened from,
    // so `billing.name` appears as two elements and never as that string.
    // FDF carries it flat. The two formats are not one output under two
    // names, and a test that looked for the same text in both would fail on
    // whichever it was not written against.
    assert!(!xfdf_text.contains("billing.name"));
    assert!(
        xfdf_text.contains("<field name=\"billing\"><field name=\"name\">"),
        "the qualified name is nested: {xfdf_text}",
    );
    assert!(xfdf_text.contains("<value>Ada Lovelace</value>"));

    // The two are different serializations of one thing, not two names for
    // one output.
    assert_ne!(fdf, xfdf);
}
