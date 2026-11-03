//! `EPDFDoc_ImportPages`, `EPDFDoc_GetImportFieldRoots` and
//! `EPDFForm_GetFieldPartialName` against upstream's `form_page_import.pdf`.
//!
//! The form (its header says so): `person` (5) holds `name` (6, its widget 7
//! on page 1) and `email` (8, merged with its widget, page 2); `plan` (10)
//! has a widget on each page; `agree` (12) is on page 2; `hidden` (14) has
//! no widget, so no page brings it.

use pdfium::{Document, Library};

const FORM: &str = "packages/engine/main/test/fixtures/form_page_import.pdf";

fn open(bytes: &[u8]) -> Document {
    Document::from_bytes(Library::acquire(), bytes, None).expect("the PDF opens")
}

/// One empty page and no form, built so PDFium reads it as written.
fn blank() -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>",
    ];
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

/// Every field's full name, in model order.
fn field_names(document: &Document) -> Vec<String> {
    let model = document.form_model().expect("a form model");
    (0..model.field_count())
        .map(|index| model.field(index).expect("a field").name().unwrap().unwrap_or_default())
        .collect()
}

#[test]
fn the_import_roots_are_the_top_level_fields_with_a_widget_on_the_pages() {
    let form = open(&test_support::fixture(FORM));
    assert_eq!(form.import_field_roots(&[]).unwrap(), vec![5, 10, 12], "every page");
    assert_eq!(form.import_field_roots(&[0]).unwrap(), vec![5, 10], "page 1");
    assert_eq!(form.import_field_roots(&[1]).unwrap(), vec![5, 10, 12], "page 2");
    assert_eq!(form.import_field_roots(&[2]).unwrap(), Vec::<u32>::new(), "out of range");
}

#[test]
fn a_partial_name_is_the_fields_own_segment() {
    let form = open(&test_support::fixture(FORM));
    assert_eq!(form.field_partial_name(5).unwrap().as_deref(), Some("person"));
    assert_eq!(form.field_partial_name(6).unwrap().as_deref(), Some("name"));
    assert_eq!(form.field_partial_name(14).unwrap().as_deref(), Some("hidden"));
    // A page dictionary has no /T, and nothing is at 999.
    assert_eq!(form.field_partial_name(3).unwrap(), None);
    assert_eq!(form.field_partial_name(999).unwrap(), None);
}

#[test]
fn an_import_brings_the_fields_of_the_copied_widgets() {
    let form = open(&test_support::fixture(FORM));
    let target = open(&blank());
    assert!(target.import_pages(&form, &[], 1, None));
    assert_eq!(target.page_count(), 3);
    let mut names = field_names(&target);
    names.sort();
    assert_eq!(names, ["agree", "person.email", "person.name", "plan"]);
}

#[test]
fn an_import_of_some_pages_brings_only_their_fields() {
    let form = open(&test_support::fixture(FORM));
    let target = open(&blank());
    assert!(target.import_pages(&form, &[0], 0, None));
    let mut names = field_names(&target);
    names.sort();
    assert_eq!(names, ["person.name", "plan"]);
}

#[test]
fn a_renamed_root_comes_in_under_its_new_name() {
    let form = open(&test_support::fixture(FORM));
    assert!(form.set_field_name(5, "person_2"));
    assert_eq!(form.field_partial_name(5).unwrap().as_deref(), Some("person_2"));
    let target = open(&blank());
    assert!(target.import_pages(&form, &[], 1, None));
    let names = field_names(&target);
    assert!(names.contains(&"person_2.name".to_owned()), "{names:?}");
    assert!(!names.iter().any(|name| name.starts_with("person.")), "{names:?}");
}

#[test]
fn a_number_list_of_the_wrong_length_is_refused_before_anything_is_copied() {
    let form = open(&test_support::fixture(FORM));
    let target = open(&blank());
    assert!(!target.import_pages(&form, &[], 1, Some(&[0])));
    assert!(!target.import_pages(&form, &[0], 1, Some(&[0, 0])));
    assert_eq!(target.page_count(), 1);
    // Zero for each page takes the next free numbers.
    assert!(target.import_pages(&form, &[1, 0], 1, Some(&[0, 0])));
    assert_eq!(target.page_count(), 3);
}
