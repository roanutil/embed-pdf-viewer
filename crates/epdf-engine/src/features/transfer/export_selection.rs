//! Which annotations an export takes (`core/src/transfer/exportSelection.ts`).

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::bundle::page_key;
use crate::error::{EngineError, EngineErrorCode};
use crate::identity::{AnnotationRef, PageRef};

/// What an export takes besides the selection (`include`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExportInclude {
    /// The selection and only what it points at.
    References,
    /// The selection, what it points at, and every reply below a selected
    /// annotation, so a comment travels with its discussion.
    #[default]
    Threads,
}

/// Which annotations an export takes (`AnnotationExportSelection`). Without
/// `refs` or `pages`, every one in the document.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnnotationExportSelection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refs: Option<Vec<AnnotationRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pages: Option<Vec<PageRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include: Option<ExportInclude>,
}

/// An annotation's ref, as a read writes it.
pub fn ref_of(annotation: &Value) -> Option<AnnotationRef> {
    serde_json::from_value(annotation.get("ref")?.clone()).ok()
}

/// A ref field of an annotation (`popup`, a popup's `parent`, `reply.to`).
fn linked(annotation: &Value, pointer: &str) -> Option<AnnotationRef> {
    let value = annotation.pointer(pointer)?;
    if value.is_null() {
        return None;
    }
    serde_json::from_value(value.clone()).ok()
}

fn key_of(annotation: &Value) -> String {
    ref_of(annotation).map(|r| r.key()).unwrap_or_default()
}

fn is_popup(annotation: &Value) -> bool {
    annotation.get("subtype").and_then(Value::as_str) == Some("popup")
}

/// The annotations an export takes, in document order: page order, then
/// each page's annotation order (`closeExportSelection`). Besides the
/// selection: a reply brings its parent, and the parent's, up to the root;
/// an annotation brings its popup, and a popup its parent; with `threads`,
/// the default, every reply below a selected annotation. `read_page` answers
/// a page's annotations in `/Annots` order and is called once for each page
/// the selection is on. `pages` is the document's pages in order. A
/// selected page or annotation the document doesn't have is `NotFound`.
pub fn close_export_selection(
    selection: &AnnotationExportSelection,
    pages: &[PageRef],
    read_page: &mut dyn FnMut(PageRef) -> Result<Vec<Value>, EngineError>,
) -> Result<Vec<Value>, EngineError> {
    let in_document: HashSet<String> = pages.iter().map(|page| page_key(*page)).collect();
    let mut read: HashMap<String, Vec<Value>> = HashMap::new();
    let mut read_order: Vec<String> = Vec::new();
    let mut by_key: HashMap<String, Value> = HashMap::new();
    // A ref may name a page the document doesn't have (a broken `/IRT`): it
    // finds nothing.
    let mut read_once = |page: PageRef,
                         read: &mut HashMap<String, Vec<Value>>,
                         by_key: &mut HashMap<String, Value>,
                         read_order: &mut Vec<String>|
     -> Result<(), EngineError> {
        let key = page_key(page);
        if read.contains_key(&key) || !in_document.contains(&key) {
            return Ok(());
        }
        let annotations = read_page(page)?;
        for annotation in &annotations {
            by_key.insert(key_of(annotation), annotation.clone());
        }
        read.insert(key.clone(), annotations);
        read_order.push(key);
        Ok(())
    };

    for page in selection.pages.iter().flatten() {
        if !in_document.contains(&page_key(*page)) {
            return Err(EngineError::with_message(
                EngineErrorCode::NotFound,
                format!("export: the document has no page {}", page_key(*page)),
            ));
        }
    }
    let everything = selection.refs.is_none() && selection.pages.is_none();
    let threads = selection.include.unwrap_or_default() == ExportInclude::Threads;
    let first: Vec<PageRef> =
        if everything { pages.to_vec() } else { selection.pages.clone().unwrap_or_default() };
    for page in first {
        read_once(page, &mut read, &mut by_key, &mut read_order)?;
    }

    let mut taken: HashSet<String> = HashSet::new();
    let mut pending: Vec<Value> = Vec::new();
    let take =
        |annotation: Option<Value>, taken: &mut HashSet<String>, pending: &mut Vec<Value>| {
            let Some(annotation) = annotation else { return };
            if taken.insert(key_of(&annotation)) {
                pending.push(annotation);
            }
        };
    macro_rules! find {
        ($reference:expr) => {{
            let reference: AnnotationRef = $reference;
            read_once(PageRef::new(reference.page()), &mut read, &mut by_key, &mut read_order)?;
            by_key.get(&reference.key()).cloned()
        }};
    }

    if everything {
        for key in &read_order {
            for annotation in read[key].clone() {
                take(Some(annotation), &mut taken, &mut pending);
            }
        }
    } else {
        for page in selection.pages.iter().flatten() {
            for annotation in read.get(&page_key(*page)).cloned().unwrap_or_default() {
                take(Some(annotation), &mut taken, &mut pending);
            }
        }
        for reference in selection.refs.iter().flatten() {
            let Some(annotation) = find!(reference.clone()) else {
                return Err(EngineError::with_message(
                    EngineErrorCode::NotFound,
                    format!("export: the document has no annotation {}", reference.key()),
                ));
            };
            take(Some(annotation), &mut taken, &mut pending);
        }
    }

    if threads {
        // A popup is its parent's window: selecting it selects the comment,
        // and so the comment's replies.
        for annotation in pending.clone() {
            if is_popup(&annotation)
                && let Some(parent) = linked(&annotation, "/parent")
            {
                let found = find!(parent);
                take(found, &mut taken, &mut pending);
            }
        }
        let mut replies: HashMap<String, Vec<Value>> = HashMap::new();
        let all: Vec<Value> =
            read_order.iter().flat_map(|key| read[key].clone()).collect::<Vec<_>>();
        for annotation in all {
            let Some(to) = linked(&annotation, "/reply/to") else { continue };
            let Some(parent) = find!(to) else { continue };
            replies.entry(key_of(&parent)).or_default().push(annotation);
        }
        let mut at = 0;
        while at < pending.len() {
            if let Some(below) = replies.get(&key_of(&pending[at])).cloned() {
                for reply in below {
                    take(Some(reply), &mut taken, &mut pending);
                }
            }
            at += 1;
        }
    }

    let mut at = 0;
    while at < pending.len() {
        let annotation = pending[at].clone();
        if let Some(to) = linked(&annotation, "/reply/to") {
            let found = find!(to);
            take(found, &mut taken, &mut pending);
        }
        if let Some(popup) = linked(&annotation, "/popup") {
            let found = find!(popup);
            take(found, &mut taken, &mut pending);
        }
        if is_popup(&annotation)
            && let Some(parent) = linked(&annotation, "/parent")
        {
            let found = find!(parent);
            take(found, &mut taken, &mut pending);
        }
        at += 1;
    }

    let mut ordered = Vec::new();
    for page in pages {
        for annotation in read.get(&page_key(*page)).into_iter().flatten() {
            if taken.contains(&key_of(annotation)) {
                ordered.push(annotation.clone());
            }
        }
    }
    Ok(ordered)
}

#[cfg(test)]
mod tests {
    //! `core/test/transfer/exportSelection.test.ts`.

    use super::*;
    use crate::identity::PageObjectNumber;
    use serde_json::json;

    fn page(number: u32) -> PageRef {
        PageRef::new(PageObjectNumber::new(number).unwrap())
    }

    fn ref_on(page_number: u32, annot: u32) -> Value {
        json!({ "kind": "objectNumber", "page": page(page_number), "objectNumber": annot })
    }

    fn annotation(page_number: u32, annot: u32, fields: Value) -> Value {
        let mut value = json!({ "subtype": "text", "ref": ref_on(page_number, annot), "nm": null,
            "reply": null, "popup": null });
        for (key, field) in fields.as_object().unwrap() {
            value[key] = field.clone();
        }
        value
    }

    /// The first page holds a comment thread: a note (10) with its popup
    /// (11), a reply to it (12), a reply to that reply (13) and another
    /// reply to the note (14). The second page holds a square (21).
    fn document() -> HashMap<u32, Vec<Value>> {
        let reply_to = |annot| json!({ "to": ref_on(3, annot), "type": "reply" });
        HashMap::from([
            (
                3,
                vec![
                    annotation(3, 10, json!({ "popup": ref_on(3, 11) })),
                    annotation(3, 11, json!({ "subtype": "popup", "parent": ref_on(3, 10) })),
                    annotation(3, 12, json!({ "reply": reply_to(10) })),
                    annotation(3, 13, json!({ "reply": reply_to(12) })),
                    annotation(3, 14, json!({ "reply": reply_to(10) })),
                ],
            ),
            (7, vec![annotation(7, 21, json!({ "subtype": "square" }))]),
        ])
    }

    fn selection(value: Value) -> AnnotationExportSelection {
        serde_json::from_value(value).unwrap()
    }

    fn exported(selected: Value) -> (Vec<u64>, Vec<u32>) {
        let doc = document();
        let mut reads = Vec::new();
        let taken = close_export_selection(&selection(selected), &[page(3), page(7)], &mut |p| {
            reads.push(p.object_number().get());
            Ok(doc.get(&p.object_number().get()).cloned().unwrap_or_default())
        })
        .unwrap();
        (taken.iter().map(|a| a["ref"]["objectNumber"].as_u64().unwrap()).collect(), reads)
    }

    #[test]
    fn takes_every_annotation_in_document_order_without_a_selection() {
        assert_eq!(exported(json!({})).0, [10, 11, 12, 13, 14, 21]);
    }

    #[test]
    fn takes_a_comment_with_its_whole_thread_by_default_reading_only_its_page() {
        assert_eq!(
            exported(json!({ "refs": [ref_on(3, 10)] })),
            (vec![10, 11, 12, 13, 14], vec![3])
        );
    }

    #[test]
    fn takes_only_what_a_selection_points_at_with_references() {
        let only =
            |annot| exported(json!({ "refs": [ref_on(3, annot)], "include": "references" })).0;
        assert_eq!(only(13), [10, 11, 12, 13]);
        assert_eq!(only(10), [10, 11]);
        assert_eq!(only(11), [10, 11]);
    }

    #[test]
    fn takes_the_replies_below_a_reply_and_its_parents_and_a_popup_as_its_comment() {
        assert_eq!(exported(json!({ "refs": [ref_on(3, 12)] })).0, [10, 11, 12, 13]);
        assert_eq!(exported(json!({ "refs": [ref_on(3, 11)] })).0, [10, 11, 12, 13, 14]);
    }

    #[test]
    fn takes_a_page_whole() {
        assert_eq!(exported(json!({ "pages": [page(7)] })), (vec![21], vec![7]));
    }

    #[test]
    fn finds_an_annotation_by_its_base_index() {
        let inline = json!({ "kind": "baseIndex", "page": page(3), "baseIndex": 0 });
        let mut doc = document();
        doc.get_mut(&3).unwrap()[0]["ref"] = inline.clone();
        doc.get_mut(&3).unwrap()[0]["nm"] = json!("note");
        let taken = close_export_selection(
            &selection(json!({ "refs": [inline], "include": "references" })),
            &[page(3), page(7)],
            &mut |p| Ok(doc.get(&p.object_number().get()).cloned().unwrap_or_default()),
        )
        .unwrap();
        let refs: Vec<&Value> = taken.iter().map(|a| &a["ref"]).collect();
        assert_eq!(refs, [&inline, &ref_on(3, 11)]);
    }

    #[test]
    fn refuses_a_page_or_an_annotation_the_document_does_not_have() {
        for selected in [json!({ "pages": [page(99)] }), json!({ "refs": [ref_on(3, 99)] })] {
            let refused =
                close_export_selection(&selection(selected), &[page(3), page(7)], &mut |_| {
                    Ok(Vec::new())
                })
                .unwrap_err();
            assert_eq!(refused.code, EngineErrorCode::NotFound);
        }
    }
}
