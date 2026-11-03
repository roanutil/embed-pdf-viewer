//! `EPDFDoc_GetPageLabelSectionCount`, `EPDFDoc_GetPageLabelSectionAt` and
//! `EPDFDoc_SetPageLabelSections`.

use pdfium::{Document, Library, PageLabelSection, PageLabelStyle};

fn open(bytes: &[u8]) -> Document {
    Document::from_bytes(Library::acquire(), bytes, None).expect("the PDF opens")
}

/// `pages` empty pages, with `catalog` added to the catalog dictionary.
fn pdf(pages: usize, catalog: &str) -> Vec<u8> {
    let kids: Vec<String> = (0..pages).map(|i| format!("{} 0 R", 3 + i)).collect();
    let mut objects = vec![
        format!("<< /Type /Catalog /Pages 2 0 R {catalog} >>"),
        format!("<< /Type /Pages /Kids [{}] /Count {pages} >>", kids.join(" ")),
    ];
    objects.extend(
        (0..pages).map(|_| "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>".to_owned()),
    );
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

fn section(
    first_page: u32,
    style: Option<PageLabelStyle>,
    first_number: i32,
    prefix: &str,
) -> PageLabelSection {
    PageLabelSection { first_page, style, first_number, prefix: prefix.to_owned() }
}

fn labels(document: &Document) -> Vec<Option<String>> {
    (0..document.page_count()).map(|index| document.page_label_by_index(index).unwrap()).collect()
}

#[test]
fn a_document_without_page_labels_has_no_sections() {
    assert_eq!(open(&pdf(2, "")).page_label_sections().unwrap(), vec![]);
}

#[test]
fn the_sections_read_as_the_number_tree_writes_them() {
    let doc = open(&pdf(
        5,
        "/PageLabels << /Nums [0 << /S /r >> 2 << /S /D /St 7 /P (A-) >> 4 << /P (Cover) >>] >>",
    ));
    assert_eq!(
        doc.page_label_sections().unwrap(),
        vec![
            section(0, Some(PageLabelStyle::LowerRoman), 1, ""),
            section(2, Some(PageLabelStyle::Decimal), 7, "A-"),
            section(4, None, 1, "Cover"),
        ]
    );
}

#[test]
fn pages_before_the_first_entry_are_a_decimal_section_of_their_own() {
    let doc = open(&pdf(3, "/PageLabels << /Nums [1 << /S /A >>] >>"));
    assert_eq!(
        doc.page_label_sections().unwrap(),
        vec![
            section(0, Some(PageLabelStyle::Decimal), 1, ""),
            section(1, Some(PageLabelStyle::UpperLetters), 1, ""),
        ]
    );
}

#[test]
fn written_sections_read_back_and_label_the_pages() {
    let doc = open(&pdf(4, ""));
    let sections = vec![
        section(0, Some(PageLabelStyle::UpperRoman), 1, ""),
        section(2, Some(PageLabelStyle::LowerLetters), 26, "x"),
    ];
    assert!(doc.set_page_label_sections(&sections));
    assert_eq!(doc.page_label_sections().unwrap(), sections);
    assert_eq!(
        labels(&doc),
        [Some("I".into()), Some("II".into()), Some("xz".into()), Some("xaa".into())]
    );
}

#[test]
fn no_sections_removes_the_labels() {
    let doc = open(&pdf(2, "/PageLabels << /Nums [0 << /S /D >>] >>"));
    assert_eq!(labels(&doc), [Some("1".into()), Some("2".into())]);
    assert!(doc.set_page_label_sections(&[]));
    assert_eq!(doc.page_label_sections().unwrap(), vec![]);
    assert_eq!(labels(&doc), [None, None]);
}

#[test]
fn sections_that_break_the_rules_are_refused_and_nothing_is_written() {
    let doc = open(&pdf(3, "/PageLabels << /Nums [0 << /S /D >>] >>"));
    let before = doc.page_label_sections().unwrap();
    // Not starting at page 0, out of order, past the last page.
    assert!(!doc.set_page_label_sections(&[section(1, None, 1, "")]));
    assert!(!doc.set_page_label_sections(&[
        section(0, None, 1, ""),
        section(2, None, 1, ""),
        section(1, None, 1, ""),
    ]));
    assert!(!doc.set_page_label_sections(&[section(0, None, 1, ""), section(3, None, 1, "")]));
    assert_eq!(doc.page_label_sections().unwrap(), before);
}
