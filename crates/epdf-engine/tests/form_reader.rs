//! G8's forms gate: the form agrees with the TypeScript engine.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run forms
//!
//! Whole snapshots, every field, every widget row, read through
//! `Host::call` so the page-space boundary is part of what is compared. The
//! one thing not compared cross-language is `exportData`: FDF and XFDF are
//! serializations PDFium writes, so comparing them would compare one writer
//! to itself, and the output carries a path that differs by working
//! directory. The export's shape is asserted here directly instead.
//!
//! Also ported: the read cases of upstream's
//! `core/test/forms/schema.test.ts`, `runFormConformance` ("lists the
//! reconciled field tree"), `runFieldGroupsConformance` ("a field's group
//! reads from its /EMBD_Metadata"), `runFormAttributionConformance` ("a
//! widget row reports no attribution") and `runWidgetFindingConformance`
//! (its form-read half).

mod common;

use common::replay::Replay;
use epdf_engine::cancel::CancelToken;
use epdf_engine::features::forms::snapshot::{
    FormFieldDto, FormFieldFamily, FormFieldKind, FormKind, FormSnapshot, FormValueEntry,
    read_form_fields,
};
use epdf_engine::host::Host;
use pdfium::{Document, FormDataFormat, Library};
use serde::Deserialize;
use serde_json::{Value, json};

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
    snapshot: Value,
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

/// `forms.list` as the wire answers it.
fn list_request() -> Value {
    json!({ "kind": "forms.list", "effect": "read", "jobId": "<job>" })
}

/// The case's fixture's form, through `Host::call`.
fn read(case: &Case) -> Value {
    let mut replay = Replay::open(&case.fixture, "forms");
    replay
        .replay(&list_request())
        .unwrap_or_else(|e| panic!("{}: forms.list refused: {e}", case.name))
}

/// The form of `bytes`, through `Host::call`.
fn list_bytes(bytes: Vec<u8>) -> Value {
    let mut host = Host::new();
    let open = host.call_filled(
        &json!({ "kind": "open.fatMem", "effect": "open", "jobId": 1, "docId": "d",
            "password": null })
        .to_string(),
        vec![bytes],
    );
    let envelope: Value = serde_json::from_str(&open.json).expect("JSON");
    assert!(envelope.get("error").is_none(), "would not open: {envelope}");
    let response = host.call_filled(
        &json!({ "kind": "forms.list", "effect": "read", "jobId": 2, "docId": "d" }).to_string(),
        Vec::new(),
    );
    let envelope: Value = serde_json::from_str(&response.json).expect("JSON");
    assert!(envelope.get("error").is_none(), "forms.list refused: {envelope}");
    envelope["result"].clone()
}

/// A PDF file of `objects`, numbered from 1, with its cross-reference table
/// (`conformance/pdfOf.ts`).
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

fn field<'a>(snapshot: &'a Value, name: &str) -> &'a Value {
    snapshot["fields"]
        .as_array()
        .expect("fields")
        .iter()
        .find(|f| f["name"] == name)
        .unwrap_or_else(|| panic!("no field named {name}"))
}

#[test]
fn the_snapshot_agrees_with_the_typescript_engine() {
    let file = load();
    assert!(file.cases.len() >= 3, "the corpus shrank");
    let mut fields = 0usize;
    let mut widgets = 0usize;

    for case in &file.cases {
        let actual = read(case);
        let theirs = test_support::canonical_numbers(&case.snapshot);
        let mine = test_support::canonical_numbers(&actual);

        // Compared field by field and row by row before the whole, so a
        // failure names which one rather than printing two large objects.
        for list in ["fields", "widgets"] {
            let mine = mine[list].as_array().expect(list);
            let theirs = theirs[list].as_array().expect(list);
            assert_eq!(
                mine.len(),
                theirs.len(),
                "{}: a different number of {list}: {}",
                case.name,
                case.covers,
            );
            for (index, (mine, theirs)) in mine.iter().zip(theirs).enumerate() {
                assert_eq!(
                    mine,
                    theirs,
                    "{}: {list} {index} ({}): {}",
                    case.name,
                    theirs["name"].as_str().or(theirs["nm"].as_str()).unwrap_or("?"),
                    case.covers,
                );
            }
        }
        assert_eq!(mine, theirs, "{}: the snapshot envelope differs", case.name);
        fields += mine["fields"].as_array().map_or(0, Vec::len);
        widgets += mine["widgets"].as_array().map_or(0, Vec::len);
    }

    assert!(fields > 0, "the corpus has no form fields at all");
    assert!(widgets > 0, "the corpus has no widget rows at all");
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
    let snapshot = read_form_fields(&model, &CancelToken::never()).expect("read");

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
fn a_fields_widget_names_its_row_by_ref_and_the_row_names_its_field() {
    // The two halves of the form join both ways: a field's widget carries
    // the row's `ref`, and the row carries the field's.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "families").expect("families");
    let snapshot = read(case);
    let radio = field(&snapshot, "colour");
    let rows = snapshot["widgets"].as_array().expect("widgets");
    for widget in radio["widgets"].as_array().expect("widgets") {
        let row = rows.iter().find(|row| row["ref"] == widget["ref"]).expect("a row");
        assert_eq!(row["field"], radio["ref"]);
        assert_eq!(row["fieldFamily"], "radio");
        assert_eq!(widget["objectNumber"], widget["ref"]["objectNumber"]);
        assert_eq!(widget["page"], row["page"]);
    }
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
    let snapshot = read_form_fields(&model, &CancelToken::never()).expect("read");

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
    let FormFieldKind::Listbox { selected_values, multi_select, options, default_value, .. } =
        &languages.kind
    else {
        panic!("languages is not a list box");
    };
    assert!(multi_select);
    assert_eq!(selected_values, &["en".to_owned(), "fr".to_owned()]);
    assert_eq!(options.len(), 3, "the unselected option is still reported");
    assert!(default_value.is_empty(), "no /DV selects nothing on reset");

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
    let snapshot = read_form_fields(&model, &CancelToken::never()).expect("read");

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
    let snapshot = read(case);

    assert_eq!(
        snapshot["calculationOrder"],
        json!([{ "kind": "objectNumber", "objectNumber": 4 }, null]),
        "the first slot names a real field by its ref; the second names nothing and stays",
    );
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
    let snapshot = read_form_fields(&model, &CancelToken::never()).expect("read");

    assert_eq!(snapshot.form_kind, FormKind::None);
    assert!(snapshot.fields.is_empty());
    assert!(snapshot.calculation_order.is_empty());
    assert!(!snapshot.needs_appearances);
    assert_eq!(read(case)["widgets"], json!([]));
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

// ── runFormConformance: "lists the reconciled field tree" ───────────────────

#[test]
fn lists_the_reconciled_field_tree_with_per_family_dtos() {
    let snapshot =
        list_bytes(test_support::fixture("packages/engine/main/test/fixtures/toggle_fields.pdf"));
    assert_eq!(snapshot["formKind"], "acroform");
    // The answer survives its own schema: it reads back as the snapshot type.
    serde_json::from_value::<FormSnapshot<epdf_engine::page_space::PageCoordinates>>(
        snapshot.clone(),
    )
    .expect("the snapshot reads back");

    let text = field(&snapshot, "maxlen_text");
    assert_eq!(text["family"], "text");
    assert_eq!(text["value"], "abc");
    assert_eq!(text["valueEntry"], json!({ "kind": "scalar", "value": "abc" }));
    assert_eq!(text["defaultValueEntry"], json!({ "kind": "none" }));
    assert_eq!(text["maxLength"], 5);
    assert_eq!(text["widgets"].as_array().map(Vec::len), Some(1));

    let radio = field(&snapshot, "ntto_radio");
    assert_eq!(radio["family"], "radio");
    assert_eq!(radio["noToggleToOff"], true);
    assert_eq!(radio["value"], "x");
    assert_eq!(radio["valueEntry"], json!({ "kind": "scalar", "value": "x" }));
    assert_eq!(radio["defaultValueEntry"], json!({ "kind": "scalar", "value": "x" }));
    let widgets = radio["widgets"].as_array().expect("widgets");
    assert_eq!(widgets.iter().map(|w| w["onState"].clone()).collect::<Vec<_>>(), ["x", "y"]);
    assert_eq!(widgets.iter().map(|w| w["checked"].clone()).collect::<Vec<_>>(), [true, false]);

    assert_eq!(field(&snapshot, "unison_radio")["radiosInUnison"], true);

    let check = field(&snapshot, "opt_check");
    assert_eq!(check["family"], "checkbox");
    assert_eq!(check["checked"], false);
    assert_eq!(check["exportValue"], "Alpha");

    // Hierarchical fields surface under their fully qualified name.
    assert_eq!(field(&snapshot, "billing.name")["family"], "text");
    assert_eq!(snapshot["calculationOrder"], json!([]));
}

// ── runFieldGroupsConformance and runFormAttributionConformance ─────────────

/// `FIELD_GROUPS_PDF`, with attribution on two of its fields: merged
/// field/widgets whose `/EMBD_Metadata` is the field's and the widget's at
/// once.
fn field_groups_pdf() -> Vec<u8> {
    let group = |name: &str| format!("/EMBD_Metadata << /SchemaVersion 1 /GroupID ({name}) >>");
    let text = |name: &str, rect: &str, extra: &str| {
        format!(
            "<< /Type /Annot /Subtype /Widget /FT /Tx /T ({name}) /Rect [{rect}] /F 4 /P 3 0 R \
             {extra} >>"
        )
    };
    let signature = |name: &str, rect: &str, extra: &str| {
        format!(
            "<< /Type /Annot /Subtype /Widget /FT /Sig /T ({name}) /Rect [{rect}] /F 4 /P 3 0 R \
             {extra} >>"
        )
    };
    let attributed = "/EMBD_Metadata << /SchemaVersion 1 /GroupID (buyer) /CreatedBy (u-17) \
        /CreatedAt (D:20261008104200+01'00') /FilledBy (u-42) /FilledByName (Ann de Vries) \
        /FilledAt (D:20261008104300Z) /ImportedBy (u-9) /UserID (u-17) /UpdatedBy (u-42) >>";
    pdf_of(&[
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R 5 0 R 6 0 R 7 0 R 8 0 R 9 0 R] \
         /CO [7 0 R] /DA (/Helv 10 Tf 0 g) /DR << /Font << /Helv << /Type /Font /Subtype /Type1 \
         /BaseFont /Helvetica >> >> >> >> >>"
            .to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Contents 10 0 R /Resources << >> \
         /Annots [4 0 R 5 0 R 6 0 R 7 0 R 8 0 R 9 0 R] >>"
            .to_owned(),
        text("buyer_name", "20 340 180 360", attributed),
        text("buyer_price", "20 300 180 320", &group("buyer")),
        text("seller_name", "200 340 380 360", &group("seller")),
        text("total", "20 260 180 280", "/AA << /C << /S /JavaScript /JS (event.value = 1;) >> >>"),
        signature("buyer_sig", "20 100 180 150", &group("buyer")),
        signature("seller_sig", "200 100 380 150", &group("seller")),
        "<< /Length 0 >>\nstream\n\nendstream".to_owned(),
    ])
}

#[test]
fn a_fields_group_reads_from_its_embed_metadata_and_its_widget_has_none() {
    let snapshot = list_bytes(field_groups_pdf());
    let groups: Vec<(String, Value)> = snapshot["fields"]
        .as_array()
        .expect("fields")
        .iter()
        .map(|f| (f["name"].as_str().expect("a name").to_owned(), f["groupId"].clone()))
        .collect();
    assert_eq!(
        groups,
        [
            ("buyer_name".to_owned(), json!("buyer")),
            ("buyer_price".to_owned(), json!("buyer")),
            ("seller_name".to_owned(), json!("seller")),
            ("total".to_owned(), Value::Null),
            ("buyer_sig".to_owned(), json!("buyer")),
            ("seller_sig".to_owned(), json!("seller")),
        ]
    );
    let rows = snapshot["widgets"].as_array().expect("widgets");
    assert_eq!(rows.len(), 6);
    assert!(rows.iter().all(|row| row["groupId"].is_null()));
}

#[test]
fn a_field_names_who_made_and_filled_it_and_its_widget_row_reports_none() {
    // Who made a field and who last filled it in are the field's; a field
    // merged with its widget shares one `/EMBD_Metadata`, and the row never
    // reads it as its own.
    let snapshot = list_bytes(field_groups_pdf());
    let buyer = field(&snapshot, "buyer_name");
    assert_eq!(buyer["createdBy"], "u-17");
    assert_eq!(buyer["createdAt"], "2026-10-08T10:42:00+01:00");
    assert_eq!(buyer["filledBy"], "u-42");
    assert_eq!(buyer["filledByName"], "Ann de Vries");
    assert_eq!(buyer["filledAt"], "2026-10-08T10:43:00Z");
    assert_eq!(buyer["importedBy"], "u-9");

    let plain = field(&snapshot, "buyer_price");
    for key in ["createdBy", "createdAt", "filledBy", "filledByName", "filledAt", "importedBy"] {
        assert!(plain[key].is_null(), "{key} on a field nobody attributed");
    }

    let row = snapshot["widgets"]
        .as_array()
        .expect("widgets")
        .iter()
        .find(|row| row["field"] == buyer["ref"])
        .expect("buyer_name's row");
    for key in ["groupId", "userId", "createdBy", "modifiedBy", "importedBy"] {
        assert!(row[key].is_null(), "the row reports {key}: {row}");
    }
}

// ── runWidgetFindingConformance: the form read ──────────────────────────────

const FONT_RESOURCES: &str =
    "/DR << /Font << /Helv << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >>";
const PAGE: &str = "/Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << >>";
const WIDGET: &str = "/Type /Annot /Subtype /Widget /Rect [20 20 120 40]";
const LOOK: &str = "<< /Type /XObject /Subtype /Form /BBox [0 0 100 20] /Length 26 >>\nstream\n\
                    0 0 1 rg 0 0 100 20 re f\nendstream";

/// One of `WIDGET_FINDING_CASES`: its name, the file, the page whose
/// `/Annots` shows the widget, and the field the form reads it into.
type WidgetFindingCase = (&'static str, Vec<u8>, Option<u64>, Option<&'static str>);

fn widget_finding_cases() -> Vec<WidgetFindingCase> {
    let acroform = format!(
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [] /DA (/Helv 0 Tf 0 g) \
         {FONT_RESOURCES} >> >>"
    );
    let pages =
        |kids: &str, count: u32| format!("<< /Type /Pages /Kids [{kids}] /Count {count} >>");
    vec![
        (
            "annots-only",
            pdf_of(&[
                acroform.clone(),
                pages("3 0 R", 1),
                format!("<< {PAGE} /Annots [4 0 R] >>"),
                format!("<< {WIDGET} /FT /Tx /T (orphan) /V (hi) /P 3 0 R /AP << /N 5 0 R >> >>"),
                LOOK.to_owned(),
            ]),
            Some(3),
            Some("orphan"),
        ),
        (
            "inline",
            pdf_of(&[
                acroform.clone(),
                pages("3 0 R", 1),
                format!(
                    "<< {PAGE} /Annots [<< {WIDGET} /FT /Tx /T (inline) /AP << /N 4 0 R >> >>] >>"
                ),
                LOOK.to_owned(),
            ]),
            Some(3),
            None,
        ),
        (
            "no-acroform",
            pdf_of(&[
                "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
                pages("3 0 R", 1),
                format!("<< {PAGE} /Annots [4 0 R] >>"),
                format!("<< {WIDGET} /FT /Tx /T (lonely) /P 3 0 R /AP << /N 5 0 R >> >>"),
                LOOK.to_owned(),
            ]),
            Some(3),
            Some("lonely"),
        ),
        (
            "stale-p",
            pdf_of(&[
                format!(
                    "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [5 0 R] \
                     /DA (/Helv 0 Tf 0 g) {FONT_RESOURCES} >> >>"
                ),
                pages("3 0 R 4 0 R", 2),
                format!("<< {PAGE} >>"),
                format!("<< {PAGE} /Annots [5 0 R] >>"),
                format!("<< {WIDGET} /FT /Tx /T (moved) /P 3 0 R /AP << /N 6 0 R >> >>"),
                LOOK.to_owned(),
            ]),
            Some(4),
            Some("moved"),
        ),
        (
            "deleted-page",
            pdf_of(&[
                format!(
                    "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] \
                     /DA (/Helv 0 Tf 0 g) {FONT_RESOURCES} >> >>"
                ),
                pages("3 0 R", 1),
                format!("<< {PAGE} >>"),
                "<< /FT /Tx /T (gone) /Kids [5 0 R] >>".to_owned(),
                format!("<< {WIDGET} /Parent 4 0 R /P 6 0 R /AP << /N 7 0 R >> >>"),
                format!("<< {PAGE} /Annots [5 0 R] >>"),
                LOOK.to_owned(),
            ]),
            None,
            Some("gone"),
        ),
        (
            "no-field-type",
            pdf_of(&[
                acroform,
                pages("3 0 R", 1),
                format!("<< {PAGE} /Annots [4 0 R] >>"),
                format!("<< {WIDGET} /P 3 0 R /AP << /N 5 0 R >> >>"),
                LOOK.to_owned(),
            ]),
            Some(3),
            None,
        ),
    ]
}

#[test]
fn the_form_finds_each_widget_on_the_page_whose_annots_hold_it() {
    for (name, pdf, page, field_name) in widget_finding_cases() {
        let form = list_bytes(pdf);
        let rows = form["widgets"].as_array().expect("widgets");

        // One row, on the page whose /Annots holds it.
        let row_pages: Vec<Option<u64>> =
            rows.iter().map(|row| row["page"]["objectNumber"].as_u64()).collect();
        match page {
            None => assert!(rows.is_empty(), "{name}: rows {rows:?}"),
            Some(page) => assert_eq!(row_pages, [Some(page)], "{name}"),
        }
        if let Some(field_name) = field_name {
            let field = field(&form, field_name);
            let widget_pages: Vec<Option<u64>> = field["widgets"]
                .as_array()
                .expect("widgets")
                .iter()
                .map(|w| w["page"]["objectNumber"].as_u64())
                .collect();
            assert_eq!(widget_pages, [page], "{name}: the field's widget pages");
        }
        for row in rows {
            assert_eq!(row["field"].is_null(), field_name.is_none(), "{name}: {row}");
        }
    }
}

// ── core/test/forms/schema.test.ts ──────────────────────────────────────────

fn base() -> Value {
    json!({
        "origin": "acroform", "readOnly": false, "required": false, "noExport": false,
        "alternateName": null, "mappingName": null, "groupId": null, "createdBy": null,
        "createdAt": null, "filledBy": null, "filledByName": null, "filledAt": null,
        "importedBy": null
    })
}

fn with_base(mut member: Value) -> Value {
    for (key, value) in base().as_object().expect("an object") {
        member[key] = value.clone();
    }
    member
}

fn widget(n: u32) -> Value {
    let page = json!({ "kind": "objectNumber", "objectNumber": 3 });
    json!({ "ref": { "kind": "objectNumber", "page": page, "objectNumber": n },
        "objectNumber": n, "page": page })
}

fn radio() -> Value {
    let toggle = |n: u32, state: &str, checked: bool| {
        let mut widget = widget(n);
        widget["onState"] = json!(state);
        widget["exportValue"] = json!(state);
        widget["checked"] = json!(checked);
        widget
    };
    with_base(json!({
        "ref": { "kind": "objectNumber", "objectNumber": 6 }, "name": "gender",
        "family": "radio", "valueEntry": { "kind": "scalar", "value": "male" },
        "defaultValueEntry": { "kind": "none" }, "value": "male", "radiosInUnison": false,
        "noToggleToOff": false, "widgets": [toggle(8, "male", true), toggle(9, "female", false)]
    }))
}

fn listbox() -> Value {
    with_base(json!({
        "ref": { "kind": "objectNumber", "objectNumber": 9 }, "name": "fruits",
        "family": "listbox", "valueEntry": { "kind": "array", "values": ["Apple", "Cherry"] },
        "defaultValueEntry": { "kind": "array", "values": ["Apple"] },
        "selectedValues": ["Apple", "Cherry"], "defaultValue": ["Apple"], "multiSelect": true,
        "options": [
            { "label": "Apple", "value": "Apple", "selected": true },
            { "label": "Banana", "value": "Banana", "selected": false },
            { "label": "Cherry", "value": "Cherry", "selected": true }
        ],
        "topIndex": 0, "widgets": [widget(9)]
    }))
}

#[test]
fn the_field_dto_union_round_trips_per_family() {
    for member in [radio(), listbox()] {
        let read: FormFieldDto = serde_json::from_value(member.clone()).expect("parses");
        assert_eq!(serde_json::to_value(&read).expect("serializes"), member);
    }
}

#[test]
fn the_family_discriminant_rejects_cross_family_members() {
    // A list box claiming to be text fails: it has no `value` string.
    let mut bad = listbox();
    bad["family"] = json!("text");
    assert!(serde_json::from_value::<FormFieldDto>(bad).is_err());
}

#[test]
fn a_snapshot_reads_back_end_to_end() {
    let snapshot = json!({
        "formKind": "acroform", "needsAppearances": false, "fields": [radio(), listbox()],
        "widgets": [],
        "calculationOrder": [radio()["ref"].clone(), null, listbox()["ref"].clone()]
    });
    let read: FormSnapshot<epdf_engine::page_space::PageCoordinates> =
        serde_json::from_value(snapshot.clone()).expect("parses");
    assert_eq!(serde_json::to_value(&read).expect("serializes"), snapshot);
}
