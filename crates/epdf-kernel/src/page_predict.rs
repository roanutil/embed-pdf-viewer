//! `packages/engine/core/src/pages/predict.ts` and `pages/labels.ts`: what a
//! page op leaves, worked out before the engine runs it, the document's
//! pages and their label sections as a read of them (`pages.list()`) returns
//! them afterwards. The page view (`page_view.rs`) replays the pending page
//! changes over the engine's pages with these, so a page edit shows at once.
//!
//! An op the engine refuses leaves the list as it is, and the same list
//! comes back (the same [`Rc`]). A viewer replays its pending changes over
//! the engine's pages every time those move, so an op that already applied
//! (its page already gone, its blank page already there) changes nothing.
//! Where pages go follows `positionIndex` and `reorderedList`; where label
//! sections go, the section rules below, and each page's label is read from
//! them ([`label_at`]).
//!
//! Pages, refs and sections are the JSON the engine answers, as everywhere
//! in the kernel. A failure the TypeScript throws (a ref whose object number
//! isn't a positive integer, a member read off `undefined`) is an `Err`,
//! which the replay reports.

use std::collections::HashSet;
use std::rc::Rc;

use serde_json::{Map, Value, json};

use crate::Error;
use crate::js::{json_number, number_to_string};

/// `PAGE_INSERT_BLANK_MAX_COUNT`.
const PAGE_INSERT_BLANK_MAX_COUNT: f64 = 100.0;

/// The pages of a document, in display order, and their label sections:
/// `PageList`. `pages` is `PageLayout[]` and `labels` `PageLabelSection[]`;
/// either can be the `undefined` marker when an engine answer left it out.
#[derive(Debug)]
pub struct PageList {
    pub pages: Rc<Value>,
    pub labels: Rc<Value>,
}

impl PageList {
    pub fn new(pages: Rc<Value>, labels: Rc<Value>) -> Rc<Self> {
        Rc::new(Self { pages, labels })
    }
}

fn type_error(what: &str, member: &str) -> Error {
    Error::named("TypeError", format!("Cannot read properties of {what} (reading '{member}')"))
}

/// `value` as the array a member read gets, or the TypeError reading
/// `member` off it.
fn array<'a>(value: &'a Value, member: &str) -> Result<&'a Vec<Value>, Error> {
    match value {
        Value::Array(items) => Ok(items),
        Value::Null => Err(type_error("null", member)),
        v if v.is_object() && crate::is_undefined(v) => Err(type_error("undefined", member)),
        _ => Err(type_error("undefined", member)),
    }
}

fn member<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    value.get(name).filter(|v| !crate::is_undefined(v))
}

/// `encodePageKey(ref)`: `obj:<n>`, or the RangeError for an object number
/// that isn't a positive integer.
pub fn encode_page_key(page_ref: &Value) -> Result<String, Error> {
    let number = match page_ref {
        Value::Object(_) => member(page_ref, "objectNumber"),
        Value::Null => return Err(type_error("null", "objectNumber")),
        _ => return Err(type_error("undefined", "objectNumber")),
    };
    match number.and_then(Value::as_f64) {
        Some(n) if n.fract() == 0.0 && n > 0.0 => Ok(format!("obj:{}", number_to_string(n))),
        _ => Err(Error::named(
            "RangeError",
            format!(
                "encodePageKey: objectNumber must be a positive integer, got {}",
                shown(number)
            ),
        )),
    }
}

/// A value as a template literal prints it.
fn shown(value: Option<&Value>) -> String {
    match value {
        None => "undefined".into(),
        Some(Value::Null) => "null".into(),
        Some(Value::Number(n)) => number_to_string(n.as_f64().unwrap_or(f64::NAN)),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Array(_)) => String::new(),
        Some(Value::Object(_)) => "[object Object]".into(),
    }
}

/// `keyOf(page)`: the key of a page's `ref`.
fn key_of(page: &Value) -> Result<String, Error> {
    match page {
        Value::Object(_) => match member(page, "ref") {
            Some(page_ref) => encode_page_key(page_ref),
            None => Err(type_error("undefined", "objectNumber")),
        },
        _ => Err(type_error("undefined", "ref")),
    }
}

/// `toPageRef(n)`.
fn to_page_ref(number: &Value) -> Value {
    json!({ "kind": "objectNumber", "objectNumber": number })
}

/// SameValueZero on two JSON numbers, as a `Set` of numbers compares them.
fn same_number(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(a), Some(b)) => a == b,
        _ => a == b,
    }
}

// ── the ops ──────────────────────────────────────────────────────────────────

/// `pagesAfter(list, op)`: `list` once `op` applies, or `list` itself when
/// the engine would refuse it. `op` is one of the predictable page ops
/// (`pages.rotate`, `pages.reorder`, `pages.delete`, `pages.insertBlank`,
/// `pages.copy`); any other answers `list`.
pub fn pages_after(list: &Rc<PageList>, op: &Value) -> Result<Rc<PageList>, Error> {
    match op.get("type").and_then(Value::as_str) {
        Some("pages.rotate") => pages_after_rotate(
            list,
            op.get("pages").unwrap_or(&Value::Null),
            op.get("rotation").unwrap_or(&Value::Null),
        ),
        Some("pages.reorder") => pages_after_reorder(
            list,
            op.get("pages").unwrap_or(&Value::Null),
            op.get("position").unwrap_or(&Value::Null),
        ),
        Some("pages.delete") => pages_after_delete(list, op.get("pages").unwrap_or(&Value::Null)),
        Some("pages.insertBlank") => pages_after_insert_blank(list, op),
        Some("pages.copy") => pages_after_copy(list, op),
        _ => Ok(list.clone()),
    }
}

/// `pagesAfterRestore(list, restored)`: the list once a delete's undo
/// brings `restored` back, in the order given: each right after the page it
/// followed, or first when that page isn't there, and in the numbering of
/// the section it lands in. A page that is there already stays where it is.
pub fn pages_after_restore(list: &Rc<PageList>, restored: &Value) -> Result<Rc<PageList>, Error> {
    let mut order = array(&list.pages, "length")?.clone();
    let mut labels = list.labels.clone();
    let mut present = HashSet::new();
    for page in &order {
        present.insert(key_of(page)?);
    }
    for each in array(restored, "length")? {
        let page = each.get("page").cloned().unwrap_or(Value::Null);
        let key = key_of(&page)?;
        if present.contains(&key) {
            continue;
        }
        let anchor = match member(each, "after").filter(|a| !a.is_null()) {
            // `order.findIndex(...)` over no pages never encodes `after`.
            Some(_) if order.is_empty() => -1,
            Some(after) => {
                let wanted = encode_page_key(after)?;
                let mut found = -1i64;
                for (index, page) in order.iter().enumerate() {
                    if key_of(page)? == wanted {
                        found = index as i64;
                        break;
                    }
                }
                found
            }
            None => -1,
        };
        let at = (anchor + 1) as usize;
        order.insert(at, page);
        labels = sections_after_insert(&labels, at as f64, 1.0)?;
        present.insert(key);
    }
    if order.len() == array(&list.pages, "length")?.len() {
        return Ok(list.clone());
    }
    list_of(order, labels, list)
}

/// `pagesRelabelled(list, labels)`: the list with `labels` as its label
/// sections, what an undo of a page edit shows, which puts back the
/// sections the edit found.
pub fn pages_relabelled(list: &Rc<PageList>, labels: &Value) -> Result<Rc<PageList>, Error> {
    if same_sections(labels, &list.labels)? {
        return Ok(list.clone());
    }
    let order = array(&list.pages, "map")?.clone();
    list_of(order, Rc::new(labels.clone()), list)
}

/// `restoredPagesOf(pages, refs)`: what deleting `refs` takes, as undoing
/// the delete brings it back: the pages in display order, each with the
/// page it follows now (`{ page, after }`).
pub fn restored_pages_of(pages: &[Value], refs: &[Value]) -> Result<Vec<Value>, Error> {
    let deleting = keys(refs)?;
    let mut out = Vec::new();
    for (index, page) in pages.iter().enumerate() {
        if deleting.contains(&key_of(page)?) {
            let after = if index > 0 {
                pages[index - 1].get("ref").cloned().unwrap_or_else(crate::undefined)
            } else {
                Value::Null
            };
            out.push(json!({ "page": page, "after": after }));
        }
    }
    Ok(out)
}

/// `reorderBackOf(pages, refs)`: the moves that put `refs` back where they
/// are now, once a reorder moved them: each page, in display order, right
/// after the page it follows now (first when it is the first).
pub fn reorder_back_of(pages: &[Value], refs: &[Value]) -> Result<Vec<Value>, Error> {
    Ok(restored_pages_of(pages, refs)?
        .into_iter()
        .map(|restored| {
            let page_ref = restored["page"].get("ref").cloned().unwrap_or_else(crate::undefined);
            let position = match &restored["after"] {
                Value::Null => json!("start"),
                after => json!({ "after": after }),
            };
            json!({ "type": "pages.reorder", "pages": [page_ref], "position": position })
        })
        .collect())
}

/// `rotateBackOf(pages, refs)`: the turns that put `refs` back as they are
/// now, once a rotate turned them: one op for each rotation they have, in
/// the order the pages are named.
pub fn rotate_back_of(pages: &[Value], refs: &[Value]) -> Result<Vec<Value>, Error> {
    let mut rotations: Vec<(String, Option<Value>)> = Vec::new();
    for page in pages {
        let key = key_of(page)?;
        let rotation = member(page, "rotation").cloned();
        match rotations.iter_mut().find(|(k, _)| *k == key) {
            Some(entry) => entry.1 = rotation,
            None => rotations.push((key, rotation)),
        }
    }
    let mut groups: Vec<(Value, Vec<Value>)> = Vec::new();
    for page_ref in refs {
        let key = encode_page_key(page_ref)?;
        let Some((_, Some(rotation))) = rotations.iter().find(|(k, _)| *k == key) else {
            continue;
        };
        match groups.iter_mut().find(|(r, _)| same_number(r, rotation)) {
            Some((_, group)) => group.push(page_ref.clone()),
            None => groups.push((rotation.clone(), vec![page_ref.clone()])),
        }
    }
    Ok(groups
        .into_iter()
        .map(|(rotation, group)| {
            json!({ "type": "pages.rotate", "pages": group, "rotation": rotation })
        })
        .collect())
}

fn keys(refs: &[Value]) -> Result<HashSet<String>, Error> {
    refs.iter().map(encode_page_key).collect()
}

fn pages_after_rotate(
    list: &Rc<PageList>,
    refs: &Value,
    rotation: &Value,
) -> Result<Rc<PageList>, Error> {
    let valid = rotation.as_f64().is_some_and(|r| [0.0, 90.0, 180.0, 270.0].contains(&r));
    if !valid {
        return Ok(list.clone());
    }
    // `namesPagesOnce` answers no pages named before it reads the pages.
    let refs = array(refs, "length")?;
    if refs.is_empty() {
        return Ok(list.clone());
    }
    let pages = array(&list.pages, "map")?;
    if !names_pages_once(pages, refs)? {
        return Ok(list.clone());
    }
    let turning = keys(refs)?;
    let mut changed = false;
    let mut next = Vec::with_capacity(pages.len());
    for page in pages {
        if !turning.contains(&key_of(page)?)
            || member(page, "rotation").is_some_and(|r| same_number(r, rotation) && r.is_number())
        {
            next.push(page.clone());
            continue;
        }
        changed = true;
        let mut turned = page.clone();
        if let Value::Object(fields) = &mut turned {
            fields.insert("rotation".into(), rotation.clone());
        }
        next.push(turned);
    }
    if !changed {
        return Ok(list.clone());
    }
    Ok(PageList::new(Rc::new(Value::Array(next)), list.labels.clone()))
}

fn pages_after_reorder(
    list: &Rc<PageList>,
    refs: &Value,
    position: &Value,
) -> Result<Rc<PageList>, Error> {
    let refs = array(refs, "length")?;
    if refs.is_empty() {
        return Ok(list.clone());
    }
    let pages = array(&list.pages, "map")?;
    let mut by_key: Vec<(String, usize)> = Vec::with_capacity(pages.len());
    for (index, page) in pages.iter().enumerate() {
        let key = key_of(page)?;
        match by_key.iter_mut().find(|(k, _)| *k == key) {
            Some(entry) => entry.1 = index,
            None => by_key.push((key, index)),
        }
    }
    let page_refs: Vec<Value> =
        pages.iter().map(|p| p.get("ref").cloned().unwrap_or_else(crate::undefined)).collect();
    let Ok(order) = reordered_list(&page_refs, refs, position) else {
        return Ok(list.clone());
    };
    let mut moved = Vec::with_capacity(order.len());
    for page_ref in &order {
        let key = encode_page_key(page_ref)?;
        let index = by_key.iter().find(|(k, _)| *k == key).map(|(_, i)| *i);
        moved.push(index);
    }
    if moved.iter().enumerate().all(|(index, from)| *from == Some(index)) {
        return Ok(list.clone());
    }
    let moved: Vec<Value> =
        moved.into_iter().map(|from| from.map_or(Value::Null, |i| pages[i].clone())).collect();
    let first = encode_page_key(&refs[0])?;
    let mut at = -1.0;
    for (index, page) in moved.iter().enumerate() {
        if key_of(page)? == first {
            at = index as f64;
            break;
        }
    }
    let labels =
        sections_after_move(&list.labels, pages.len() as f64, &indexes_of(pages, refs)?, at)?;
    list_of(moved, labels, list)
}

fn pages_after_delete(list: &Rc<PageList>, refs: &Value) -> Result<Rc<PageList>, Error> {
    // `namesPagesOnce` answers no pages named before it reads the pages.
    let refs = array(refs, "length")?;
    if refs.is_empty() {
        return Ok(list.clone());
    }
    let pages = array(&list.pages, "map")?;
    if !names_pages_once(pages, refs)? || refs.len() >= pages.len() {
        return Ok(list.clone());
    }
    let deleting = keys(refs)?;
    let mut kept = Vec::with_capacity(pages.len());
    for page in pages {
        if !deleting.contains(&key_of(page)?) {
            kept.push(page.clone());
        }
    }
    let labels =
        sections_after_delete(&list.labels, pages.len() as f64, &indexes_of(pages, refs)?)?;
    list_of(kept, labels, list)
}

fn pages_after_insert_blank(list: &Rc<PageList>, op: &Value) -> Result<Rc<PageList>, Error> {
    let spec = op.get("spec").filter(|v| v.is_object() && !crate::is_undefined(v));
    let Some(spec) = spec else { return Err(type_error("undefined", "size")) };
    // `op.spec.count ?? 1`: a null count is one page too.
    let count_value = member(spec, "count").filter(|v| !v.is_null()).cloned().unwrap_or(json!(1));
    let count = count_value.as_f64();
    let count_valid = count.is_some_and(|c| {
        c.fract() == 0.0 && c.is_finite() && (1.0..=PAGE_INSERT_BLANK_MAX_COUNT).contains(&c)
    });
    // The checks short-circuit: `size.width` is read only for a good count.
    if !count_valid {
        return Ok(list.clone());
    }
    let size = match member(spec, "size") {
        None => return Err(type_error("undefined", "width")),
        Some(Value::Null) => return Err(type_error("null", "width")),
        Some(size) => size,
    };
    // A size that isn't an object has no `width`: the engine refuses it.
    let positive =
        |v: Option<&Value>| v.and_then(Value::as_f64).is_some_and(|n| n.is_finite() && n > 0.0);
    if !size.is_object() || !positive(size.get("width")) || !positive(size.get("height")) {
        return Ok(list.clone());
    }
    let (width, height) = (size["width"].clone(), size["height"].clone());
    inserted(list, op.get("position"), op.get("objectNumbers"), count.unwrap_or(1.0), |page_ref| {
        Ok(blank_page(page_ref, &width, &height))
    })
}

/// Each copy reads as its page does, under its own number.
fn pages_after_copy(list: &Rc<PageList>, op: &Value) -> Result<Rc<PageList>, Error> {
    let pages = array(&list.pages, "map")?;
    let mut by_key: Vec<(String, usize)> = Vec::with_capacity(pages.len());
    for (index, page) in pages.iter().enumerate() {
        let key = key_of(page)?;
        match by_key.iter_mut().find(|(k, _)| *k == key) {
            Some(entry) => entry.1 = index,
            None => by_key.push((key, index)),
        }
    }
    let refs = array(op.get("pages").unwrap_or(&Value::Null), "map")?;
    let mut sources = Vec::with_capacity(refs.len());
    for page_ref in refs {
        let key = encode_page_key(page_ref)?;
        sources.push(by_key.iter().find(|(k, _)| *k == key).map(|(_, i)| pages[*i].clone()));
    }
    if sources.is_empty() || sources.iter().any(Option::is_none) {
        return Ok(list.clone());
    }
    let sources: Vec<Value> = sources.into_iter().flatten().collect();
    let mut next = 0;
    inserted(list, op.get("position"), op.get("objectNumbers"), sources.len() as f64, |page_ref| {
        let mut copy = sources[next].clone();
        next += 1;
        if let Value::Object(fields) = &mut copy {
            fields.insert("ref".into(), page_ref);
        }
        Ok(copy)
    })
}

/// `inserted(list, position, numbers, count, make)`: `count` new pages at
/// `position`, made by `make` under the numbers given, one per page, none
/// twice, none a page has. Without them the engine picks numbers a
/// prediction can't know, so the list stays.
fn inserted(
    list: &Rc<PageList>,
    position: Option<&Value>,
    numbers: Option<&Value>,
    count: f64,
    mut make: impl FnMut(Value) -> Result<Value, Error>,
) -> Result<Rc<PageList>, Error> {
    let pages = array(&list.pages, "map")?;
    let Some(Value::Array(numbers)) = numbers else { return Ok(list.clone()) };
    if numbers.len() as f64 != count {
        return Ok(list.clone());
    }
    let taken: Vec<&Value> = pages
        .iter()
        .map(|p| p.get("ref").and_then(|r| r.get("objectNumber")).unwrap_or(&Value::Null))
        .collect();
    for (index, number) in numbers.iter().enumerate() {
        if numbers[..index].iter().any(|n| same_number(n, number))
            || taken.iter().any(|t| same_number(t, number))
        {
            return Ok(list.clone());
        }
    }
    let page_refs: Vec<Value> =
        pages.iter().map(|p| p.get("ref").cloned().unwrap_or_else(crate::undefined)).collect();
    let end = json!("end");
    let position = position.filter(|p| !crate::is_undefined(p)).unwrap_or(&end);
    let Ok(at) = position_index(&page_refs, position) else { return Ok(list.clone()) };
    let mut order = Vec::with_capacity(pages.len() + numbers.len());
    order.extend_from_slice(&pages[..at]);
    for number in numbers {
        order.push(make(to_page_ref(number))?);
    }
    order.extend_from_slice(&pages[at..]);
    let labels = sections_after_insert(&list.labels, at as f64, count)?;
    list_of(order, labels, list)
}

// ── helpers ─────────────────────────────────────────────────────────────────

/// `namesPagesOnce(pages, refs)`: whether `refs` names pages of `pages`, at
/// least one, none twice: what every page op checks.
fn names_pages_once(pages: &[Value], refs: &[Value]) -> Result<bool, Error> {
    if refs.is_empty() {
        return Ok(false);
    }
    let mut present = HashSet::new();
    for page in pages {
        present.insert(key_of(page)?);
    }
    let named = keys(refs)?;
    Ok(named.len() == refs.len() && named.iter().all(|key| present.contains(key)))
}

/// `indexesOf(pages, refs)`: where each of `refs` is in `pages`.
fn indexes_of(pages: &[Value], refs: &[Value]) -> Result<Vec<f64>, Error> {
    let named = keys(refs)?;
    let mut out = Vec::new();
    for (index, page) in pages.iter().enumerate() {
        if named.contains(&key_of(page)?) {
            out.push(index as f64);
        }
    }
    Ok(out)
}

/// `listOf(order, labels, before)`: `order` under `labels`, each page's
/// index its position and its label read from the sections. A page whose
/// index and label stay is left as it is, and so are the sections when they
/// say what `before`'s do.
fn list_of(
    order: Vec<Value>,
    labels: Rc<Value>,
    before: &Rc<PageList>,
) -> Result<Rc<PageList>, Error> {
    let sections =
        if same_sections(&labels, &before.labels)? { before.labels.clone() } else { labels };
    let mut pages = Vec::with_capacity(order.len());
    for (index, mut page) in order.into_iter().enumerate() {
        let label = label_at(&sections, index as f64)?;
        let same_index = page.get("index").and_then(Value::as_f64) == Some(index as f64);
        let same_label = page.get("label") == Some(&label);
        if !(same_index && same_label)
            && let Value::Object(fields) = &mut page
        {
            fields.insert("index".into(), json!(index));
            fields.insert("label".into(), label);
        }
        pages.push(page);
    }
    Ok(PageList::new(Rc::new(Value::Array(pages)), sections))
}

/// A blank page as a read returns it: its visible box is its whole page,
/// unturned.
fn blank_page(page_ref: Value, width: &Value, height: &Value) -> Value {
    let page_box = json!({ "x": 0, "y": 0, "width": width, "height": height });
    json!({
        "index": 0,
        "ref": page_ref,
        "label": null,
        "size": { "width": width, "height": height },
        "rotation": 0,
        "userUnit": 1,
        "tabOrder": null,
        "boxes": {
            "media": page_box,
            "crop": page_box,
            "bleed": page_box,
            "trim": page_box,
            "art": page_box,
        },
        "pdfCropBox": { "left": 0, "bottom": 0, "right": width, "top": height },
    })
}

// ── ListPosition ────────────────────────────────────────────────────────────

/// `anchorOf(position)`: the ref a `{ before }` or `{ after }` names.
fn anchor_of(position: &Value) -> Result<Option<(&Value, bool)>, ()> {
    match position {
        Value::String(s) if s == "start" || s == "end" => Ok(None),
        Value::Object(fields) => {
            if let Some(before) = fields.get("before") {
                Ok(Some((before, true)))
            } else {
                // `position.after`: undefined throws in the key read.
                fields.get("after").map(|after| Some((after, false))).ok_or(())
            }
        }
        _ => Err(()),
    }
}

/// `positionIndex(order, position, encodePageKey)`; `Err` where the
/// TypeScript throws (the caller catches every throw).
fn position_index(order: &[Value], position: &Value) -> Result<usize, ()> {
    if position == "start" {
        return Ok(0);
    }
    if position == "end" {
        return Ok(order.len());
    }
    let (anchor, before) = anchor_of(position)?.ok_or(())?;
    let wanted = encode_page_key(anchor).map_err(|_| ())?;
    for (at, page_ref) in order.iter().enumerate() {
        if encode_page_key(page_ref).map_err(|_| ())? == wanted {
            return Ok(if before { at } else { at + 1 });
        }
    }
    Err(())
}

/// `reorderedList(order, moved, position, encodePageKey)`.
fn reordered_list(order: &[Value], moved: &[Value], position: &Value) -> Result<Vec<Value>, ()> {
    let moved_keys = keys(moved).map_err(|_| ())?;
    if moved_keys.len() != moved.len() {
        return Err(());
    }
    let mut present = HashSet::new();
    for page_ref in order {
        present.insert(encode_page_key(page_ref).map_err(|_| ())?);
    }
    if moved_keys.iter().any(|key| !present.contains(key)) {
        return Err(());
    }
    if let Some((anchor, _)) = anchor_of(position)?
        && moved_keys.contains(&encode_page_key(anchor).map_err(|_| ())?)
    {
        return Err(());
    }
    let mut staying = Vec::with_capacity(order.len());
    for page_ref in order {
        if !moved_keys.contains(&encode_page_key(page_ref).map_err(|_| ())?) {
            staying.push(page_ref.clone());
        }
    }
    let at = position_index(&staying, position)?;
    let mut out = Vec::with_capacity(order.len());
    out.extend_from_slice(&staying[..at]);
    out.extend_from_slice(moved);
    out.extend_from_slice(&staying[at..]);
    Ok(out)
}

// ── labels.ts ───────────────────────────────────────────────────────────────

fn first_page(section: &Value) -> f64 {
    section.get("firstPage").and_then(Value::as_f64).unwrap_or(f64::NAN)
}

/// `labelAt(sections, index)`: the label of the page at `index`, its
/// section's prefix and its number in the section's style, as PDFium writes
/// them (`FPDF_GetPageLabel`). `null` when no section covers it, or the
/// label is empty.
pub fn label_at(sections: &Value, index: f64) -> Result<Value, Error> {
    let mut found = None;
    for each in array(sections, "Symbol(Symbol.iterator)")? {
        if first_page(each) > index {
            break;
        }
        found = Some(each);
    }
    let Some(section) = found else { return Ok(Value::Null) };
    let prefix = match section.get("prefix") {
        Some(Value::String(s)) => s.clone(),
        Some(v) if crate::is_undefined(v) => "undefined".into(),
        other => shown(other),
    };
    let first_number = section.get("firstNumber").and_then(Value::as_f64).unwrap_or(f64::NAN);
    let number = first_number + index - first_page(section);
    let label = prefix + &number_in(section.get("style"), number);
    Ok(if label.is_empty() { Value::Null } else { Value::String(label) })
}

/// `sectionsAfterDelete(sections, pageCount, indexes)`.
pub fn sections_after_delete(
    sections: &Rc<Value>,
    page_count: f64,
    indexes: &[f64],
) -> Result<Rc<Value>, Error> {
    let list = array(sections, "length")?;
    if list.is_empty() || indexes.is_empty() {
        return Ok(sections.clone());
    }
    let count = page_count.max(0.0) as usize;
    let mut kept_before = vec![0usize];
    for index in 0..count {
        let deleted = indexes.contains(&(index as f64));
        kept_before.push(kept_before[index] + usize::from(!deleted));
    }
    let kept = |index: f64| -> usize {
        let at = index.max(0.0).min(page_count);
        kept_before.get(at as usize).copied().unwrap_or(0)
    };
    let mut next = Vec::with_capacity(list.len());
    for (at, section) in list.iter().enumerate() {
        let end = list.get(at + 1).map_or(page_count, first_page);
        if kept(end) == kept(first_page(section)) {
            continue;
        }
        let first = kept(first_page(section)) as f64;
        if first == first_page(section) {
            next.push(section.clone());
        } else {
            let mut moved = section.clone();
            if let Value::Object(fields) = &mut moved {
                fields.insert("firstPage".into(), json_number(first));
            }
            next.push(moved);
        }
    }
    let next = Value::Array(next);
    Ok(if same_sections(&next, sections)? { sections.clone() } else { Rc::new(next) })
}

/// `sectionsAfterInsert(sections, at, count)`: a section that starts at or
/// after `at` moves along with its first page, so the new pages take the
/// numbering of the section before them, or the first section's at the
/// start.
pub fn sections_after_insert(
    sections: &Rc<Value>,
    at: f64,
    count: f64,
) -> Result<Rc<Value>, Error> {
    if count <= 0.0 {
        return Ok(sections.clone());
    }
    let list = array(sections, "map")?;
    let mut changed = false;
    let next: Vec<Value> = list
        .iter()
        .map(|section| {
            let first = first_page(section);
            if first == 0.0 || first < at {
                return section.clone();
            }
            changed = true;
            let mut moved = section.clone();
            if let Value::Object(fields) = &mut moved {
                fields.insert("firstPage".into(), json_number(first + count));
            }
            moved
        })
        .collect();
    Ok(if changed { Rc::new(Value::Array(next)) } else { sections.clone() })
}

/// `sectionsAfterMove(sections, pageCount, indexes, at)`: a delete, then an
/// insert.
pub fn sections_after_move(
    sections: &Rc<Value>,
    page_count: f64,
    indexes: &[f64],
    at: f64,
) -> Result<Rc<Value>, Error> {
    let deleted = sections_after_delete(sections, page_count, indexes)?;
    sections_after_insert(&deleted, at, indexes.len() as f64)
}

/// `sameSections(left, right)`: whether two lists of sections say the same.
pub fn same_sections(left: &Value, right: &Value) -> Result<bool, Error> {
    let left = array(left, "length")?;
    let right = array(right, "length")?;
    let field = |s: &Value, name: &str| s.get(name).cloned().unwrap_or_else(crate::undefined);
    Ok(left.len() == right.len()
        && left.iter().zip(right).all(|(a, b)| {
            ["firstPage", "style", "firstNumber", "prefix"].iter().all(|name| {
                let (x, y) = (field(a, name), field(b, name));
                same_number(&x, &y)
            })
        }))
}

const ROMAN: [(f64, &str); 13] = [
    (1000.0, "m"),
    (900.0, "cm"),
    (500.0, "d"),
    (400.0, "cd"),
    (100.0, "c"),
    (90.0, "xc"),
    (50.0, "l"),
    (40.0, "xl"),
    (10.0, "x"),
    (9.0, "ix"),
    (5.0, "v"),
    (4.0, "iv"),
    (1.0, "i"),
];

/// `numberIn(style, number)`: `number` in `style`, as PDFium writes it;
/// nothing for a number a style can't write, or for no style (`null`). A
/// style that is absent or none of these falls out of the `switch` as
/// `undefined`, which the label then reads as text.
fn number_in(style: Option<&Value>, number: f64) -> String {
    match style {
        Some(Value::Null) => String::new(),
        Some(Value::String(style)) => match style.as_str() {
            "decimal" => number_to_string(number),
            "upperRoman" => roman(number).to_uppercase(),
            "lowerRoman" => roman(number),
            "upperLetters" => letters(number).to_uppercase(),
            "lowerLetters" => letters(number),
            _ => "undefined".into(),
        },
        _ => "undefined".into(),
    }
}

/// Below a million; nothing for 0 and below.
fn roman(number: f64) -> String {
    let mut left = number % 1_000_000.0;
    let mut out = String::new();
    for (value, digits) in ROMAN {
        while left >= value {
            out.push_str(digits);
            left -= value;
        }
    }
    out
}

/// 1 → a, 26 → z, 27 → aa, 52 → zz, 53 → aaa; nothing for 0 and below.
fn letters(number: f64) -> String {
    if number.is_nan() || number <= 0.0 {
        return String::new();
    }
    let from = number - 1.0;
    let letter = char::from_u32((97.0 + (from % 26.0)).floor() as u32).unwrap_or('a');
    let times = ((from / 26.0).floor() + 1.0) % 1000.0;
    std::iter::repeat_n(letter, times as usize).collect()
}

/// The JSON object a section is, for tests and callers that build one.
pub fn section(first_page: f64, style: Option<&str>, first_number: f64, prefix: &str) -> Value {
    let mut fields = Map::new();
    fields.insert("firstPage".into(), json_number(first_page));
    fields.insert("style".into(), style.map_or(Value::Null, |s| json!(s)));
    fields.insert("firstNumber".into(), json_number(first_number));
    fields.insert("prefix".into(), json!(prefix));
    Value::Object(fields)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(number: u32, index: usize) -> Value {
        json!({ "index": index, "ref": { "kind": "objectNumber", "objectNumber": number }, "label": null, "rotation": 0 })
    }
    fn r(number: u32) -> Value {
        json!({ "kind": "objectNumber", "objectNumber": number })
    }
    fn list(pages: Vec<Value>, labels: Value) -> Rc<PageList> {
        PageList::new(Rc::new(Value::Array(pages)), Rc::new(labels))
    }
    fn numbers(list: &PageList) -> Vec<u64> {
        list.pages
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["ref"]["objectNumber"].as_u64().unwrap())
            .collect()
    }
    fn labels(list: &PageList) -> Vec<Value> {
        list.pages.as_array().unwrap().iter().map(|p| p["label"].clone()).collect()
    }

    #[test]
    fn labels_are_read_from_sections_in_each_style() {
        let sections = json!([
            section(0.0, Some("lowerRoman"), 1.0, ""),
            section(2.0, Some("decimal"), 1.0, "p-"),
            section(4.0, Some("upperLetters"), 26.0, ""),
            section(6.0, None, 1.0, ""),
        ]);
        let got: Vec<Value> = (0..7).map(|i| label_at(&sections, i as f64).unwrap()).collect();
        assert_eq!(
            got,
            vec![
                json!("i"),
                json!("ii"),
                json!("p-1"),
                json!("p-2"),
                json!("Z"),
                json!("AA"),
                Value::Null
            ]
        );
        assert_eq!(label_at(&json!([]), 0.0).unwrap(), Value::Null);
    }

    #[test]
    fn a_section_whose_first_page_goes_starts_on_its_next_page() {
        let sections = Rc::new(json!([
            section(0.0, Some("decimal"), 1.0, ""),
            section(2.0, Some("lowerRoman"), 1.0, "")
        ]));
        let next = sections_after_delete(&sections, 4.0, &[2.0]).unwrap();
        assert_eq!(
            *next,
            json!([
                section(0.0, Some("decimal"), 1.0, ""),
                section(2.0, Some("lowerRoman"), 1.0, "")
            ])
        );
        // The second section's pages all go: the section goes.
        let next = sections_after_delete(&sections, 4.0, &[2.0, 3.0]).unwrap();
        assert_eq!(*next, json!([section(0.0, Some("decimal"), 1.0, "")]));
        // A delete that moves nothing answers the same sections.
        let same = sections_after_delete(&sections, 4.0, &[3.0]).unwrap();
        assert!(Rc::ptr_eq(&same, &sections));
    }

    #[test]
    fn an_insert_moves_the_sections_after_it() {
        let sections = Rc::new(json!([
            section(0.0, Some("decimal"), 1.0, ""),
            section(2.0, Some("lowerRoman"), 1.0, "")
        ]));
        let next = sections_after_insert(&sections, 2.0, 3.0).unwrap();
        assert_eq!(next[1]["firstPage"], json!(5));
        let same = sections_after_insert(&sections, 3.0, 3.0).unwrap();
        assert!(Rc::ptr_eq(&same, &sections));
    }

    #[test]
    fn a_copy_reads_as_its_page_under_its_own_number_and_relabels() {
        let sections = json!([section(0.0, Some("decimal"), 1.0, "")]);
        let before = list(vec![page(1, 0), page(2, 1)], sections);
        let op = json!({ "type": "pages.copy", "pages": [r(1)], "position": { "after": r(2) }, "objectNumbers": [9] });
        let after = pages_after(&before, &op).unwrap();
        assert_eq!(numbers(&after), vec![1, 2, 9]);
        assert_eq!(labels(&after), vec![json!("1"), json!("2"), json!("3")]);
        // Without names the list stays.
        let unnamed = json!({ "type": "pages.copy", "pages": [r(1)] });
        assert!(Rc::ptr_eq(&pages_after(&before, &unnamed).unwrap(), &before));
        // A copy of a page the list doesn't have stays.
        let missing = json!({ "type": "pages.copy", "pages": [r(7)], "objectNumbers": [9] });
        assert!(Rc::ptr_eq(&pages_after(&before, &missing).unwrap(), &before));
    }

    #[test]
    fn rotate_reorder_delete_and_blank_insert_follow_the_engine() {
        let before = list(vec![page(1, 0), page(2, 1), page(3, 2)], json!([]));
        let rotated = pages_after(
            &before,
            &json!({ "type": "pages.rotate", "pages": [r(2)], "rotation": 90 }),
        )
        .unwrap();
        assert_eq!(rotated.pages[1]["rotation"], json!(90));
        assert!(Rc::ptr_eq(&rotated.labels, &before.labels));
        let bad = pages_after(
            &before,
            &json!({ "type": "pages.rotate", "pages": [r(2)], "rotation": 45 }),
        )
        .unwrap();
        assert!(Rc::ptr_eq(&bad, &before));
        let moved = pages_after(
            &before,
            &json!({ "type": "pages.reorder", "pages": [r(3)], "position": "start" }),
        )
        .unwrap();
        assert_eq!(numbers(&moved), vec![3, 1, 2]);
        assert_eq!(moved.pages[0]["index"], json!(0));
        let gone =
            pages_after(&before, &json!({ "type": "pages.delete", "pages": [r(1)] })).unwrap();
        assert_eq!(numbers(&gone), vec![2, 3]);
        let all =
            pages_after(&before, &json!({ "type": "pages.delete", "pages": [r(1), r(2), r(3)] }))
                .unwrap();
        assert!(Rc::ptr_eq(&all, &before));
        let blank = json!({ "type": "pages.insertBlank", "spec": { "size": { "width": 10, "height": 20 }, "count": 1 }, "position": "start", "objectNumbers": [8] });
        let inserted = pages_after(&before, &blank).unwrap();
        assert_eq!(numbers(&inserted), vec![8, 1, 2, 3]);
        assert_eq!(inserted.pages[0]["size"], json!({ "width": 10, "height": 20 }));
    }

    #[test]
    fn undo_helpers_put_pages_back() {
        let pages = vec![
            page(1, 0),
            json!({ "index": 1, "ref": r(2), "label": null, "rotation": 90 }),
            page(3, 2),
        ];
        let restored = restored_pages_of(&pages, &[r(2), r(1)]).unwrap();
        assert_eq!(restored[0]["after"], Value::Null);
        assert_eq!(restored[1]["after"], r(1));
        let back = reorder_back_of(&pages, &[r(2)]).unwrap();
        assert_eq!(
            back,
            vec![
                json!({ "type": "pages.reorder", "pages": [r(2)], "position": { "after": r(1) } })
            ]
        );
        let turns = rotate_back_of(&pages, &[r(2), r(3), r(1)]).unwrap();
        assert_eq!(
            turns,
            vec![
                json!({ "type": "pages.rotate", "pages": [r(2)], "rotation": 90 }),
                json!({ "type": "pages.rotate", "pages": [r(3), r(1)], "rotation": 0 }),
            ]
        );
        let shown = list(vec![page(1, 0), page(3, 1)], json!([]));
        let undone =
            pages_after_restore(&shown, &json!([{ "page": pages[1], "after": r(1) }])).unwrap();
        assert_eq!(numbers(&undone), vec![1, 2, 3]);
        let relabelled =
            pages_relabelled(&undone, &json!([section(0.0, Some("decimal"), 5.0, "")])).unwrap();
        assert_eq!(labels(&relabelled), vec![json!("5"), json!("6"), json!("7")]);
    }

    #[test]
    fn a_blank_insert_reads_its_spec_as_the_typescript_does() {
        let before = list(vec![page(1, 0)], json!([]));
        // `count ?? 1`: a null count is one page.
        let null_count = json!({ "type": "pages.insertBlank",
            "spec": { "size": { "width": 10, "height": 20 }, "count": null },
            "objectNumbers": [8] });
        assert_eq!(numbers(&pages_after(&before, &null_count).unwrap()), vec![1, 8]);
        // A size that isn't an object has no width: the list stays.
        for size in [json!(5), json!("a4"), json!([10, 20]), json!(true)] {
            let op = json!({ "type": "pages.insertBlank", "spec": { "size": size },
                "objectNumbers": [8] });
            assert!(Rc::ptr_eq(&pages_after(&before, &op).unwrap(), &before), "{size}");
        }
        // A bad count stays before the size is read.
        let no_size = json!({ "type": "pages.insertBlank", "spec": { "count": 0 },
            "objectNumbers": [8] });
        assert!(Rc::ptr_eq(&pages_after(&before, &no_size).unwrap(), &before));
        // A good count reads the size, which is not there.
        let error = pages_after(
            &before,
            &json!({ "type": "pages.insertBlank", "spec": {}, "objectNumbers": [8] }),
        )
        .unwrap_err();
        assert_eq!(error.name, "TypeError");
        let error = pages_after(
            &before,
            &json!({ "type": "pages.insertBlank", "spec": { "size": null }, "objectNumbers": [8] }),
        )
        .unwrap_err();
        assert!(error.message.contains("null"), "{}", error.message);
    }

    #[test]
    fn a_label_in_no_style_it_knows_reads_undefined() {
        let label = |section: Value| label_at(&json!([section]), 0.0).unwrap();
        assert_eq!(
            label(json!({ "firstPage": 0, "firstNumber": 1, "prefix": "p" })),
            json!("pundefined")
        );
        assert_eq!(
            label(json!({ "firstPage": 0, "style": "fancy", "firstNumber": 1, "prefix": "p" })),
            json!("pundefined")
        );
        assert_eq!(
            label(json!({ "firstPage": 0, "style": null, "firstNumber": 1, "prefix": "p" })),
            json!("p")
        );
        assert_eq!(
            label(json!({ "firstPage": 0, "style": null, "firstNumber": 1 })),
            json!("undefined")
        );
    }

    #[test]
    fn ops_that_name_no_pages_answer_before_they_read_the_pages() {
        // An engine answer without its pages: an op that names none never reads them.
        let unread = PageList::new(Rc::new(crate::undefined()), Rc::new(json!([])));
        for op in [
            json!({ "type": "pages.reorder", "pages": [], "position": "start" }),
            json!({ "type": "pages.delete", "pages": [] }),
            json!({ "type": "pages.rotate", "pages": [], "rotation": 90 }),
        ] {
            assert!(Rc::ptr_eq(&pages_after(&unread, &op).unwrap(), &unread), "{op}");
        }
        // One that names a page does read them.
        let named = json!({ "type": "pages.delete", "pages": [r(1)] });
        assert_eq!(pages_after(&unread, &named).unwrap_err().name, "TypeError");
        // A restore into no pages never encodes the page it followed.
        let empty = list(vec![], json!([]));
        let restored = json!([{ "page": page(2, 0),
            "after": { "kind": "objectNumber", "objectNumber": 0 } }]);
        assert_eq!(numbers(&pages_after_restore(&empty, &restored).unwrap()), vec![2]);
        // Into some pages it does.
        let some = list(vec![page(1, 0)], json!([]));
        assert_eq!(pages_after_restore(&some, &restored).unwrap_err().name, "RangeError");
    }

    #[test]
    fn a_ref_that_is_not_a_positive_integer_is_a_range_error() {
        let error =
            encode_page_key(&json!({ "kind": "objectNumber", "objectNumber": 1.5 })).unwrap_err();
        assert_eq!(error.name, "RangeError");
    }
}
