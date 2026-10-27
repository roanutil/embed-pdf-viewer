//! The outline wrapper (`fpdf_doc.h`'s readers and `epdf_outline.h`)
//! against small files built here.

use std::rc::Rc;

use pdfium::{BaseDocument, Document, Library, OUTLINE_TOP, OutlineItem};

/// A PDF file of `objects`, numbered from 1 (`pdfOf`).
fn pdf_of(objects: &[&str]) -> Vec<u8> {
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

/// `OUTLINE_FIXTURE_PDF` of `runOutlineConformance`: A (6, open, child A1
/// 10), B (7, red, bold, a URI), C (8, closed, named destination, child C1
/// 11) and D (9, a script).
fn fixture() -> Vec<u8> {
    pdf_of(&[
        "<< /Type /Catalog /Pages 2 0 R /Outlines 5 0 R /Names << /Dests 12 0 R >> >>",
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] >>",
        "<< /Type /Outlines /First 6 0 R /Last 9 0 R /Count 5 >>",
        "<< /Title (A) /Parent 5 0 R /Next 7 0 R /First 10 0 R /Last 10 0 R /Count 1 \
         /Dest [3 0 R /XYZ 20 280 null] >>",
        "<< /Title (B) /Parent 5 0 R /Prev 6 0 R /Next 8 0 R /C [1 0 0] /F 2 \
         /A << /S /URI /URI (https://example.com/) >> >>",
        "<< /Title (C) /Parent 5 0 R /Prev 7 0 R /Next 9 0 R /First 11 0 R /Last 11 0 R \
         /Count -1 /Dest (chapter-two) >>",
        "<< /Title (D) /Parent 5 0 R /Prev 8 0 R /A << /S /JavaScript /JS (app.alert\\(1\\)) >> >>",
        "<< /Title (A1) /Parent 6 0 R /Dest [4 0 R /Fit] >>",
        "<< /Title (C1) /Parent 8 0 R >>",
        "<< /Names [(chapter-two) [4 0 R /FitH 250]] >>",
    ])
}

fn open() -> Document {
    Document::from_bytes(Library::acquire(), &fixture(), None).expect("open the outline fixture")
}

fn siblings(first: Option<OutlineItem<'_>>) -> Vec<u32> {
    std::iter::successors(first, OutlineItem::next_sibling).map(|i| i.object_number()).collect()
}

fn children(doc: &Document, parent: u32) -> Vec<u32> {
    if parent == OUTLINE_TOP {
        return siblings(doc.outline_first());
    }
    siblings(doc.outline_item(parent).expect("an item").first_child())
}

fn title(doc: &Document, item: u32) -> String {
    doc.outline_item(item).expect("an item").title().expect("a title").unwrap_or_default()
}

#[test]
fn the_readers_walk_the_items_and_read_each_ones_fields() {
    let doc = open();
    assert_eq!(children(&doc, OUTLINE_TOP), [6, 7, 8, 9]);
    assert_eq!(children(&doc, 6), [10]);
    assert_eq!(children(&doc, 8), [11]);
    assert!(children(&doc, 7).is_empty());

    let a = doc.outline_item(6).expect("A");
    assert_eq!(a.title().unwrap().as_deref(), Some("A"));
    assert_eq!(a.count(), 1);
    assert!(!a.has_action());
    let dest = a.destination().expect("A's /Dest");
    assert_eq!(dest.page_object_number(), 3);

    let b = doc.outline_item(7).expect("B");
    assert_eq!(b.style(), 2);
    assert_eq!(b.color(), Some([1.0, 0.0, 0.0]));
    assert!(b.has_action());
    let model = b.action_model().expect("B's action model");
    assert_eq!(model.node_count(), 1);

    // A named destination resolves to the page it names.
    let c = doc.outline_item(8).expect("C");
    assert_eq!(c.count(), -1);
    assert_eq!(c.destination().expect("C's /Dest").page_object_number(), 4);
    assert_eq!(c.color(), None);

    let c1 = doc.outline_item(11).expect("C1");
    assert!(c1.destination().is_none());
    assert!(c1.action_model().is_none());
    assert_eq!(c1.style(), 0);

    // A page is not an item.
    assert!(doc.outline_item(3).is_none());
    assert!(doc.outline_item(99).is_none());
}

#[test]
fn a_create_goes_after_the_child_it_names_and_the_writes_set_each_field() {
    let doc = open();
    let created = doc.create_outline_item(0, 6, 10, "After A1");
    assert!(created > 12, "a fresh number: {created}");
    assert_eq!(children(&doc, 6), [10, created]);
    assert_eq!(title(&doc, created), "After A1");
    let first = doc.create_outline_item(0, OUTLINE_TOP, 0, "First");
    assert_eq!(children(&doc, OUTLINE_TOP), [first, 6, 7, 8, 9]);

    assert!(doc.set_outline_title(created, "Renamed"));
    assert_eq!(title(&doc, created), "Renamed");
    assert!(doc.set_outline_color(created, Some([0.0, 1.0, 0.0])));
    assert_eq!(doc.outline_item(created).unwrap().color(), Some([0.0, 1.0, 0.0]));
    assert!(doc.set_outline_color(created, None));
    assert_eq!(doc.outline_item(created).unwrap().color(), None);
    assert!(doc.set_outline_style(created, 3));
    assert_eq!(doc.outline_item(created).unwrap().style(), 3);

    let action = doc.create_uri_action("https://example.com/").expect("a URI action");
    assert!(doc.set_outline_action(created, Some(action)));
    assert!(doc.outline_item(created).unwrap().has_action());
    assert!(doc.set_outline_action(created, None));
    assert!(!doc.outline_item(created).unwrap().has_action());

    // Under a parent that isn't an item, or after a row that isn't a child,
    // nothing is made.
    assert_eq!(doc.create_outline_item(0, 3, 0, "x"), 0);
    assert_eq!(doc.create_outline_item(0, OUTLINE_TOP, 10, "x"), 0);
    assert!(!doc.set_outline_title(3, "x"));
}

#[test]
fn open_is_the_sign_of_the_count_and_a_leaf_has_none() {
    let doc = open();
    assert!(doc.set_outline_open(8, true));
    assert_eq!(doc.outline_item(8).unwrap().count(), 1);
    assert!(doc.set_outline_open(6, false));
    assert_eq!(doc.outline_item(6).unwrap().count(), -1);
    // A leaf: nothing is written.
    assert!(doc.set_outline_open(9, true));
    assert_eq!(doc.outline_item(9).unwrap().count(), 0);
}

#[test]
fn a_move_keeps_the_subtree_and_a_delete_takes_it() {
    let doc = open();
    assert!(doc.move_outline_item(8, 6, 10));
    assert_eq!(children(&doc, OUTLINE_TOP), [6, 7, 9]);
    assert_eq!(children(&doc, 6), [10, 8]);
    assert_eq!(children(&doc, 8), [11]);
    // Under itself, or after itself: refused.
    assert!(!doc.move_outline_item(6, 8, 0));
    assert!(!doc.move_outline_item(7, OUTLINE_TOP, 7));

    assert!(doc.delete_outline_item(8));
    assert_eq!(children(&doc, 6), [10]);
    assert!(doc.outline_item(8).is_none());
    assert!(doc.outline_item(11).is_none());
    assert!(!doc.delete_outline_item(8));
}

#[test]
fn a_capture_puts_back_what_a_delete_removed_in_a_layer_transaction() {
    let base =
        Rc::new(BaseDocument::from_bytes(Library::acquire(), &fixture(), None).expect("base"));
    let layer = base.open_layer().expect("a layer");
    let doc = layer.document();
    let capture = doc.export_outline_item_raw(8).expect("the capture of C");
    assert!(doc.export_outline_item_raw(99).is_none());
    assert!(doc.delete_outline_item(8));
    assert_eq!(children(doc, OUTLINE_TOP), [6, 7, 9]);

    // Outside a transaction an import is refused.
    assert!(!doc.import_outline_item_raw(&capture));
    assert!(doc.layer_begin_transaction());
    assert!(doc.import_outline_item_raw(&capture));
    assert!(doc.layer_commit_transaction());
    assert_eq!(children(doc, OUTLINE_TOP), [6, 7, 8, 9]);
    assert_eq!(children(doc, 8), [11]);
    assert_eq!(title(doc, 11), "C1");

    // Back already: refused.
    assert!(doc.layer_begin_transaction());
    assert!(!doc.import_outline_item_raw(&capture));
    assert!(doc.layer_abort_transaction());
}

#[test]
fn a_create_at_a_number_takes_a_layer_transaction_and_a_raised_ceiling() {
    let base =
        Rc::new(BaseDocument::from_bytes(Library::acquire(), &fixture(), None).expect("base"));
    let layer = base.open_layer().expect("a layer");
    let last = layer.layer_last_object_number();
    assert!(layer.raise_layer_last_object_number(last + 1));
    let doc = layer.document();
    assert!(doc.layer_begin_transaction());
    assert_eq!(doc.create_outline_item(last + 1, 8, 11, "At a number"), last + 1);
    assert!(doc.layer_commit_transaction());
    assert_eq!(children(doc, 8), [11, last + 1]);
}
