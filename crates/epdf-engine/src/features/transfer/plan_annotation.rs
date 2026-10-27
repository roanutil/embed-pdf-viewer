//! What an annotation import creates and leaves out, decided before its
//! first write (`core/src/transfer/planAnnotationImport.ts`), and the facts
//! an import commits (`annotationImportFacts` in `annotationImport.ts`).

use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value, json};

use super::bundle::{
    BundleImportPages, BundleImportTarget, BundlePage, PageMap, bundle_page_mapping, page_key,
};
use super::export_selection::ref_of;
use super::page_refs::{map_page_refs, page_refs_in};
use crate::error::{EngineError, EngineErrorCode};
use crate::features::annotation::subtype::AnnotationSubtype;
use crate::features::annotation::vocabulary::AnnotationReplyType;
use crate::features::annotation::write::prepare::{DraftFields, read_field, required_fields};
use crate::identity::{AnnotationRef, PageRef};

/// One annotation an import creates (`PlannedAnnotation`).
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedAnnotation {
    /// Its item in the bundle.
    pub item: usize,
    /// The page it goes on.
    pub page: PageRef,
    /// What the create writes: the item's data, its pages mapped, without
    /// its links and dropped fields. Page space.
    pub draft: Map<String, Value>,
    /// The annotation it replies to, by its place in `creates`.
    pub reply_to: Option<(usize, AnnotationReplyType)>,
    /// For a popup: the annotation it shows, by its place in `creates`.
    pub parent: Option<usize>,
}

/// `AnnotationImportPlan`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AnnotationImportPlan {
    /// In bundle order.
    pub creates: Vec<PlannedAnnotation>,
    /// What was left out, in bundle order (`AnnotationImportDrop`).
    pub dropped: Vec<Value>,
}

/// The create schema's check of a draft on the page it goes to.
pub type DraftCheck<'a> = dyn FnMut(PageRef, &Map<String, Value>) -> Result<(), EngineError> + 'a;

/// What [`plan_annotation_import`] asks of the document.
pub struct PlanInput<'a> {
    pub pages: &'a [BundlePage],
    pub items: &'a [Value],
    pub page_mapping: &'a BundleImportPages,
    pub target: &'a [BundleImportTarget],
    /// Whether `nm` is used on the target page, as it is before the import.
    pub name_taken: &'a mut dyn FnMut(PageRef, &str) -> Result<bool, EngineError>,
    /// The create schema's check of a draft (`AnnotationDraftSchema`), on
    /// the page it goes to: a refusal names the field in `details.field`.
    pub check: &'a mut DraftCheck<'a>,
}

/// What an import of a bundle into the `target` pages creates, and what it
/// leaves out (`planAnnotationImport`). Nothing here writes, so an import
/// refuses before its first write: `InvalidArg` when an item's data isn't
/// valid for its kind, or when a page an item is on or points at maps to no
/// target page; `NotFound` when a page list maps to a page the target
/// doesn't have.
///
/// Left out, in this order: an item of an unsupported kind, and a widget (a
/// field marker leaves out only that field); an item whose `reply.to` or
/// `parent` is no item of the bundle on the same target page
/// (`parent-missing`); a popup whose parent has an earlier one
/// (`popup-taken`); an item whose name is used on its target page, or by an
/// earlier item going there (`name-conflict`); then, until nothing changes,
/// an item whose `reply.to` or `parent` was left out (`parent-dropped`).
pub fn plan_annotation_import(input: PlanInput<'_>) -> Result<AnnotationImportPlan, EngineError> {
    let PlanInput { pages, items, page_mapping, target, name_taken, check } = input;
    let data: Vec<&Value> = items.iter().map(|item| &item["data"]).collect();
    let named: Vec<PageRef> = data.iter().flat_map(|data| pages_named_by(data)).collect();
    let map = bundle_page_mapping(pages, page_mapping, target, &named)?;
    let page_of = |data: &Value| ref_of(data).map(|r| map.map(PageRef::new(r.page())));

    let mut drops: HashMap<usize, &'static str> = HashMap::new();
    let mut field_drops: HashMap<usize, Vec<(String, &'static str)>> = HashMap::new();
    let mut drafts: HashMap<usize, Map<String, Value>> = HashMap::new();
    for (index, data) in data.iter().enumerate() {
        let subtype = data["subtype"].as_str().unwrap_or_default();
        if subtype == "unsupported" {
            drops.insert(index, "unsupported-kind");
            continue;
        }
        if subtype == "widget" {
            drops.insert(index, "widget");
            continue;
        }
        // A popup shows another annotation; one that names none can't be
        // made.
        if subtype == "popup" && data.get("parent") == Some(&Value::Null) {
            drops.insert(index, "parent-missing");
            continue;
        }
        let Some(unwritable) = unwritable_values_of(data) else {
            drops.insert(index, "unsupported-value");
            continue;
        };
        let mut markers = field_markers_of(data);
        markers.extend(unwritable);
        let page = page_of(data).ok_or_else(|| item_refused(index, None, "the item has no ref"))?;
        drafts.insert(index, draft_of(data, &markers, &map, index, page, check)?);
        if !markers.is_empty() {
            field_drops.insert(index, markers);
        }
    }

    let mut by_key: HashMap<String, usize> = HashMap::new();
    for (index, data) in data.iter().enumerate() {
        if let Some(reference) = ref_of(data) {
            by_key.entry(reference.key()).or_insert(index);
        }
    }
    // A reply and a popup are on their parent's page (ISO 32000-2
    // §12.5.6.2), so a link finds its target only there.
    let resolve = |reference: &AnnotationRef, from: usize| -> Option<usize> {
        let found = *by_key.get(&reference.key())?;
        (page_of(data[found]).map(page_key) == page_of(data[from]).map(page_key)).then_some(found)
    };
    let links_of = |data: &Value| -> Vec<AnnotationRef> {
        let mut links = Vec::new();
        if let Some(to) = linked(data, "/reply/to") {
            links.push(to);
        }
        if data["subtype"] == "popup"
            && let Some(parent) = linked(data, "/parent")
        {
            links.push(parent);
        }
        links
    };

    for (index, data) in data.iter().enumerate() {
        if drops.contains_key(&index) {
            continue;
        }
        if links_of(data).iter().any(|reference| resolve(reference, index).is_none()) {
            drops.insert(index, "parent-missing");
        }
    }

    let mut popup_of: HashSet<usize> = HashSet::new();
    for (index, data) in data.iter().enumerate() {
        if drops.contains_key(&index) || data["subtype"] != "popup" {
            continue;
        }
        let Some(parent) = linked(data, "/parent").and_then(|p| resolve(&p, index)) else {
            continue;
        };
        if !popup_of.insert(parent) {
            drops.insert(index, "popup-taken");
        }
    }

    let mut claimed: HashSet<String> = HashSet::new();
    for (index, data) in data.iter().enumerate() {
        let Some(nm) = data["nm"].as_str().filter(|nm| !nm.is_empty()) else { continue };
        if drops.contains_key(&index) {
            continue;
        }
        let page = page_of(data).expect("checked above");
        let claim = format!("{}\u{0}{nm}", page_key(page));
        if claimed.contains(&claim) || name_taken(page, nm)? {
            drops.insert(index, "name-conflict");
        } else {
            claimed.insert(claim);
        }
    }

    let mut changed = true;
    while changed {
        changed = false;
        for (index, data) in data.iter().enumerate() {
            if drops.contains_key(&index) {
                continue;
            }
            let dropped_parent = links_of(data).iter().any(|reference| {
                resolve(reference, index).is_some_and(|parent| drops.contains_key(&parent))
            });
            if dropped_parent {
                drops.insert(index, "parent-dropped");
                changed = true;
            }
        }
    }

    let mut planned: HashMap<usize, usize> = HashMap::new();
    for index in 0..data.len() {
        if !drops.contains_key(&index) {
            planned.insert(index, planned.len());
        }
    }
    let mut kept: Vec<usize> = planned.keys().copied().collect();
    kept.sort_unstable();
    let creates = kept
        .into_iter()
        .map(|index| {
            let item = data[index];
            let reply_to = linked(item, "/reply/to")
                .and_then(|to| resolve(&to, index))
                .and_then(|at| planned.get(&at).copied())
                .map(|at| (at, reply_type_of(item)));
            let parent = (item["subtype"] == "popup")
                .then(|| linked(item, "/parent"))
                .flatten()
                .and_then(|parent| resolve(&parent, index))
                .and_then(|at| planned.get(&at).copied());
            PlannedAnnotation {
                item: index,
                page: page_of(item).expect("checked above"),
                draft: drafts.remove(&index).unwrap_or_default(),
                reply_to,
                parent,
            }
        })
        .collect();

    let mut dropped = Vec::new();
    for (index, data) in data.iter().enumerate() {
        let reference = data.get("ref").cloned().unwrap_or(Value::Null);
        match drops.get(&index) {
            Some(reason) => dropped.push(json!({ "ref": reference, "reason": reason })),
            None => {
                for (field, reason) in field_drops.get(&index).into_iter().flatten() {
                    dropped.push(json!({ "ref": reference, "field": field, "reason": reason }));
                }
            }
        }
    }
    Ok(AnnotationImportPlan { creates, dropped })
}

/// A link field of an item (`reply.to`, a popup's `parent`); `None` when
/// absent or `null`.
fn linked(data: &Value, pointer: &str) -> Option<AnnotationRef> {
    let value = data.pointer(pointer)?;
    if value.is_null() {
        return None;
    }
    serde_json::from_value(value.clone()).ok()
}

fn reply_type_of(data: &Value) -> AnnotationReplyType {
    match data.pointer("/reply/type").and_then(Value::as_str) {
        Some("group") => AnnotationReplyType::Group,
        _ => AnnotationReplyType::Reply,
    }
}

/// The pages an item is on or points at, apart from its links to other
/// annotations, which resolve in the bundle (`pagesNamedBy`).
fn pages_named_by(data: &Value) -> Vec<PageRef> {
    let mut rest = data.as_object().cloned().unwrap_or_default();
    let reference = rest.remove("ref");
    rest.remove("reply");
    rest.remove("popup");
    if data["subtype"] == "popup" {
        rest.remove("parent");
    }
    let mut pages: Vec<PageRef> = reference
        .and_then(|r| serde_json::from_value::<AnnotationRef>(r).ok())
        .map(|r| PageRef::new(r.page()))
        .into_iter()
        .collect();
    pages.extend(page_refs_in(&Value::Object(rest)));
    pages
}

/// The fields of `data` that are markers for something the engine can't
/// write (`fieldMarkersOf`).
fn field_markers_of(data: &Value) -> Vec<(String, &'static str)> {
    let mut markers = Vec::new();
    match data.pointer("/measure/subtype").and_then(Value::as_str) {
        Some("geospatial") => markers.push(("measure".to_owned(), "geospatial")),
        Some("unknown") => markers.push(("measure".to_owned(), "unknown-measure")),
        _ => {}
    }
    let is_link = data["subtype"] == "link";
    let target = data.get("target").filter(|t| is_link && !t.is_null());
    let carried_target = target.is_some_and(is_writable_link_target);
    if target.is_some() && !carried_target {
        markers.push(("target".to_owned(), "unsupported-action"));
    }
    // `actions` reads `/A` and `/AA`, and no write takes it. A copy has only
    // the `/A` a link's target writes: one go-to or URI action, without a
    // chain or an `/IsMap`. A target a write can't make is reported above.
    let lost = data.get("actions").and_then(Value::as_object).is_some_and(|actions| {
        actions.iter().any(|(trigger, tree)| {
            if !is_link || trigger != "activate" {
                true
            } else {
                carried_target && !written_as_is(tree)
            }
        })
    });
    if lost {
        markers.push(("actions".to_owned(), "unsupported-action"));
    }
    markers
}

/// The values `data` reads that a write can't make (a `readBack()` data
/// field's read-only values), each left out; `None` when one is a field
/// every create needs (`unwritableValuesOf`). A link's target is reported
/// by [`field_markers_of`].
fn unwritable_values_of(data: &Value) -> Option<Vec<(String, &'static str)>> {
    let Ok(subtype) = serde_json::from_value::<AnnotationSubtype>(data["subtype"].clone()) else {
        return Some(Vec::new());
    };
    let mut unwritable = Vec::new();
    for name in read_back_data_fields(subtype) {
        let Some(value) = data.get(*name).filter(|v| !v.is_null()) else { continue };
        let writable = match *name {
            "parent" => serde_json::from_value::<AnnotationRef>(value.clone()).is_ok(),
            _ => read_field("create", subtype, name, value, &mut DraftFields::default()).is_ok(),
        };
        if writable {
            continue;
        }
        if required_fields(subtype).contains(name) {
            return None;
        }
        unwritable.push(((*name).to_owned(), "unsupported-value"));
    }
    Some(unwritable)
}

/// The data fields a read may hold values of that a write can't make
/// (`readBack()`), a link's target and a widget's actions apart
/// (`kind-facts.generated.ts`).
fn read_back_data_fields(subtype: AnnotationSubtype) -> &'static [&'static str] {
    use AnnotationSubtype as S;
    match subtype {
        S::Square | S::Circle | S::Line => &["borderStyle"],
        S::Polygon | S::Polyline => &["borderStyle", "vertices"],
        S::Ink => &["strokeWidth", "borderStyle", "inkList"],
        S::FreeText => &["intent", "borderStyle"],
        S::Popup => &["parent"],
        _ => &[],
    }
}

/// Whether a link's `/A` is what writing its target writes again
/// (`writtenAsIs`).
fn written_as_is(tree: &Value) -> bool {
    if tree.is_null() || tree["incomplete"] == Value::Bool(true) {
        return false;
    }
    let root = &tree["root"];
    if root.is_null() || root["next"].as_array().is_some_and(|next| !next.is_empty()) {
        return false;
    }
    !(root["type"] == "uri" && root["isMap"] == Value::Bool(true))
}

/// Whether a write can make a link's target; the others read as markers
/// (`isWritableLinkTarget`).
fn is_writable_link_target(target: &Value) -> bool {
    match target["kind"].as_str() {
        Some("named") => {
            matches!(
                target["name"].as_str(),
                Some("NextPage" | "PrevPage" | "FirstPage" | "LastPage")
            )
        }
        Some("goto" | "uri") => true,
        _ => false,
    }
}

/// The fields a marker leaves `null` rather than out: those a write takes as
/// `null` (`writeNullable`).
const WRITE_NULLABLE_MARKERS: &[&str] = &["measure", "target", "actions"];

/// The create an item becomes, checked against its kind's create schema on
/// the page it goes to (`draftOf`). The item's own data, not what the check
/// makes of it, so nothing a read carries is reshaped on its way back in.
fn draft_of(
    data: &Value,
    markers: &[(String, &'static str)],
    map: &PageMap,
    index: usize,
    page: PageRef,
    check: &mut DraftCheck<'_>,
) -> Result<Map<String, Value>, EngineError> {
    let mut fields = data.as_object().cloned().unwrap_or_default();
    fields.remove("reply");
    // Left out, the field is not written: `null` where a write takes it,
    // else the kind's default.
    for (field, _) in markers {
        if WRITE_NULLABLE_MARKERS.contains(&field.as_str()) {
            fields.insert(field.clone(), Value::Null);
        } else {
            fields.remove(field);
        }
    }
    let mapped = match map_page_refs(Value::Object(fields), &|p| map.map(p)) {
        Value::Object(mapped) => mapped,
        _ => Map::new(),
    };
    // Checked with the popup's `parent`, which the import then links itself.
    check(page, &mapped).map_err(|refused| {
        let field = refused.details.as_ref().and_then(|d| d["field"].as_str()).map(str::to_owned);
        item_refused(index, field.as_deref(), &refused.message)
    })?;
    let mut draft = mapped;
    if data["subtype"] == "popup" {
        draft.remove("parent");
    }
    Ok(draft)
}

fn item_refused(index: usize, field: Option<&str>, message: &str) -> EngineError {
    let at = field.map(|f| format!(" field '{f}'")).unwrap_or_default();
    let mut details = json!({ "item": index });
    if let Some(field) = field {
        details["field"] = json!(field);
    }
    EngineError::with_message(
        EngineErrorCode::InvalidArg,
        format!("import: item {index}{at}: {message}"),
    )
    .with_details(details)
}

/// The facts an import committed: one `annotations.created` per annotation,
/// in bundle order, each with the envelope of its own page and its own id;
/// the cache pins move once, with the last (`annotationImportFacts`).
/// `annotations` and `meta` are the import's result, as JSON.
pub fn annotation_import_facts(annotations: &[Value], meta: &Value) -> Vec<Value> {
    let last = annotations.len().saturating_sub(1);
    annotations
        .iter()
        .enumerate()
        .map(|(index, annotation)| {
            let page = annotation["ref"]["page"].clone();
            let affected: Vec<Value> = meta["affectedPages"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|affected| affected["objectNumber"] == page["objectNumber"])
                .cloned()
                .collect();
            json!({
                "type": "annotations.created",
                "page": page,
                "annotation": annotation,
                "meta": {
                    "affectedPages": affected,
                    "cacheDelta": if index == last { meta["cacheDelta"].clone() } else { Value::Null },
                    "opId": meta["opId"],
                    "undoable": meta["undoable"],
                    "changed": [annotation["ref"]],
                },
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    //! `core/test/transfer/planAnnotationImport.test.ts`.

    use super::*;
    use crate::identity::PageObjectNumber;

    fn page(number: u32) -> PageRef {
        PageRef::new(PageObjectNumber::new(number).unwrap())
    }

    fn ref_on(on: u32, annot: u32) -> Value {
        json!({ "kind": "objectNumber", "page": page(on), "objectNumber": annot })
    }

    fn the_box() -> Value {
        json!({ "x": 10, "y": 10, "width": 30, "height": 20 })
    }

    /// An annotation as a read returns it, with the fields the import checks.
    fn annotation(on: u32, annot: u32, fields: Value) -> Value {
        let mut value = json!({
            "subtype": "square", "ref": ref_on(on, annot), "page": page(on),
            "hasAppearance": true, "appearanceState": null, "nm": null, "rect": the_box(),
            "reply": null, "popup": null,
        });
        // A square's shape is its box; a note's icon fills its rect.
        if fields.get("subtype").is_none() {
            value["box"] = the_box();
        }
        for (key, field) in fields.as_object().unwrap() {
            value[key] = field.clone();
        }
        value
    }

    /// The bundle's pages and items for `items`: the pages they are on, at
    /// positions 0 and 4.
    fn bundle_of(items: &[Value]) -> (Vec<BundlePage>, Vec<Value>) {
        let size = crate::geom::PdfSize { width: 612.0, height: 792.0 };
        let mut pages = Vec::new();
        for (at, on) in [3, 7].into_iter().enumerate() {
            if items.iter().any(|item| item["ref"]["page"]["objectNumber"] == on) {
                pages.push(BundlePage {
                    page: page(on),
                    position: if at == 0 { 0 } else { 4 },
                    size,
                });
            }
        }
        let pages = pages
            .into_iter()
            .enumerate()
            .map(|(at, mut entry)| {
                entry.position = if at == 0 { 0 } else { 4 };
                entry
            })
            .collect();
        (pages, items.iter().map(|data| json!({ "data": data, "resources": {} })).collect())
    }

    fn same_document() -> Vec<BundleImportTarget> {
        vec![
            BundleImportTarget { page: page(3), position: 0 },
            BundleImportTarget { page: page(7), position: 4 },
        ]
    }

    /// A schema check standing in for the create's: a box field must hold
    /// numbers, as `PageBoxSchema` asks.
    fn schema(_: PageRef, draft: &Map<String, Value>) -> Result<(), EngineError> {
        if let Some(r#box) = draft.get("box").and_then(Value::as_object)
            && r#box.values().any(|v| !v.is_number())
        {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                "expected a number",
            )
            .with_details(json!({ "field": "box" })));
        }
        Ok(())
    }

    fn plan_with(
        items: &[Value],
        pages: BundleImportPages,
        target: Vec<BundleImportTarget>,
        taken: &[&str],
    ) -> Result<AnnotationImportPlan, EngineError> {
        let (bundle_pages, items) = bundle_of(items);
        plan_annotation_import(PlanInput {
            pages: &bundle_pages,
            items: &items,
            page_mapping: &pages,
            target: &target,
            name_taken: &mut |p, nm| Ok(taken.contains(&format!("{}:{nm}", page_key(p)).as_str())),
            check: &mut schema,
        })
    }

    fn plan(items: &[Value]) -> AnnotationImportPlan {
        plan_with(items, BundleImportPages::Same, same_document(), &[]).unwrap()
    }

    fn items_of(plan: &AnnotationImportPlan) -> Vec<usize> {
        plan.creates.iter().map(|c| c.item).collect()
    }

    #[test]
    fn creates_the_items_in_bundle_order_linked_by_their_place_in_the_plan() {
        let note = annotation(3, 10, json!({ "subtype": "text", "popup": ref_on(3, 11) }));
        let popup = annotation(3, 11, json!({ "subtype": "popup", "parent": ref_on(3, 10) }));
        let reply = annotation(
            3,
            12,
            json!({ "subtype": "text",
            "reply": { "to": ref_on(3, 10), "type": "reply" } }),
        );
        let square = annotation(7, 20, json!({}));
        let planned = plan(&[note, popup, reply, square]);
        assert_eq!(planned.dropped, Vec::<Value>::new());
        let placed: Vec<(usize, PageRef)> =
            planned.creates.iter().map(|c| (c.item, c.page)).collect();
        assert_eq!(placed, [(0, page(3)), (1, page(3)), (2, page(3)), (3, page(7))]);
        assert_eq!(planned.creates[1].parent, Some(0));
        assert_eq!(planned.creates[2].reply_to, Some((0, AnnotationReplyType::Reply)));
        // The links are written after every create, so the drafts carry none.
        assert!(!planned.creates[1].draft.contains_key("parent"));
        assert!(!planned.creates[2].draft.contains_key("reply"));
    }

    #[test]
    fn finds_a_link_to_a_target_named_by_its_base_index() {
        let inline = json!({ "kind": "baseIndex", "page": page(3), "baseIndex": 0 });
        let note = annotation(3, 10, json!({ "subtype": "text", "nm": "note", "ref": inline }));
        let reply = annotation(
            3,
            12,
            json!({ "subtype": "text",
            "reply": { "to": inline, "type": "group" } }),
        );
        let planned = plan(&[note, reply]);
        assert_eq!(planned.creates[0].reply_to, None);
        assert_eq!(planned.creates[1].reply_to, Some((0, AnnotationReplyType::Group)));
    }

    #[test]
    fn maps_pages_by_position_destinations_included() {
        let link = annotation(
            3,
            10,
            json!({ "subtype": "link",
            "target": { "kind": "goto", "destination": { "kind": "fit", "page": page(7) } } }),
        );
        let target = vec![
            BundleImportTarget { page: page(50), position: 0 },
            BundleImportTarget { page: page(51), position: 4 },
        ];
        let planned = plan_with(
            &[link, annotation(7, 20, json!({}))],
            BundleImportPages::ByPosition,
            target,
            &[],
        )
        .unwrap();
        let pages: Vec<PageRef> = planned.creates.iter().map(|c| c.page).collect();
        assert_eq!(pages, [page(50), page(51)]);
        assert_eq!(planned.creates[0].draft["target"]["destination"]["page"], json!(page(51)));
    }

    #[test]
    fn maps_exactly_the_pairs_a_list_gives() {
        use super::super::bundle::PagePair;
        let target = vec![BundleImportTarget { page: page(50), position: 0 }];
        let pairs = |pairs: Vec<(u32, u32)>| {
            BundleImportPages::List(
                pairs
                    .into_iter()
                    .map(|(from, to)| PagePair { from: page(from), to: page(to) })
                    .collect(),
            )
        };
        let planned =
            plan_with(&[annotation(3, 10, json!({}))], pairs(vec![(3, 50)]), target.clone(), &[])
                .unwrap();
        assert_eq!(planned.creates[0].page, page(50));
        let twice = plan_with(
            &[annotation(3, 10, json!({}))],
            pairs(vec![(3, 50), (3, 50)]),
            target.clone(),
            &[],
        );
        assert_eq!(twice.unwrap_err().code, EngineErrorCode::InvalidArg);
        let nowhere = plan_with(&[annotation(3, 10, json!({}))], pairs(vec![(3, 99)]), target, &[]);
        assert_eq!(nowhere.unwrap_err().code, EngineErrorCode::NotFound);
    }

    #[test]
    fn refuses_the_import_when_a_page_an_item_is_on_or_points_at_maps_nowhere() {
        let link = annotation(
            3,
            10,
            json!({ "subtype": "link",
            "target": { "kind": "goto", "destination": { "kind": "fit", "page": page(7) } } }),
        );
        let only_first = vec![BundleImportTarget { page: page(3), position: 0 }];
        let refused = plan_with(&[link], BundleImportPages::Same, only_first, &[]).unwrap_err();
        assert_eq!(refused.code, EngineErrorCode::InvalidArg);
        assert_eq!(refused.details, Some(json!({ "pages": ["obj:7"] })));
    }

    #[test]
    fn leaves_out_an_unsupported_kind_and_what_points_at_it() {
        let watermark = annotation(
            3,
            10,
            json!({ "subtype": "unsupported",
            "rawSubtypeCode": 26, "rawSubtypeName": "Watermark" }),
        );
        let reply = annotation(
            3,
            11,
            json!({ "subtype": "text",
            "reply": { "to": ref_on(3, 10), "type": "reply" } }),
        );
        let answer = annotation(
            3,
            12,
            json!({ "subtype": "text",
            "reply": { "to": ref_on(3, 11), "type": "reply" } }),
        );
        let planned = plan(&[answer.clone(), reply.clone(), watermark.clone()]);
        assert!(planned.creates.is_empty());
        assert_eq!(
            planned.dropped,
            [
                json!({ "ref": answer["ref"], "reason": "parent-dropped" }),
                json!({ "ref": reply["ref"], "reason": "parent-dropped" }),
                json!({ "ref": watermark["ref"], "reason": "unsupported-kind" }),
            ]
        );
    }

    #[test]
    fn leaves_out_what_points_at_nothing_in_the_bundle() {
        let reply = annotation(
            3,
            11,
            json!({ "subtype": "text",
            "reply": { "to": ref_on(3, 99), "type": "reply" } }),
        );
        let popup = annotation(3, 12, json!({ "subtype": "popup", "parent": ref_on(3, 11) }));
        let planned = plan(&[reply.clone(), popup.clone()]);
        assert!(planned.creates.is_empty());
        assert_eq!(
            planned.dropped,
            [
                json!({ "ref": reply["ref"], "reason": "parent-missing" }),
                json!({ "ref": popup["ref"], "reason": "parent-dropped" }),
            ]
        );
    }

    #[test]
    fn finds_a_parent_only_on_the_same_page() {
        let note = annotation(3, 10, json!({ "subtype": "text" }));
        let elsewhere = annotation(
            7,
            20,
            json!({ "subtype": "text",
            "reply": { "to": ref_on(3, 10), "type": "reply" } }),
        );
        let planned = plan(&[note, elsewhere.clone()]);
        assert_eq!(items_of(&planned), [0]);
        assert_eq!(
            planned.dropped,
            [json!({ "ref": elsewhere["ref"], "reason": "parent-missing" })]
        );
    }

    #[test]
    fn gives_a_name_to_the_first_item_that_carries_it_if_the_page_has_it_free() {
        let taken = annotation(3, 10, json!({ "nm": "taken" }));
        let once = annotation(3, 11, json!({ "nm": "twice" }));
        let twice = annotation(3, 12, json!({ "nm": "twice" }));
        let elsewhere = annotation(7, 20, json!({ "nm": "twice" }));
        let planned = plan_with(
            &[taken.clone(), once, twice.clone(), elsewhere],
            BundleImportPages::Same,
            same_document(),
            &["obj:3:taken"],
        )
        .unwrap();
        assert_eq!(items_of(&planned), [1, 3]);
        assert_eq!(
            planned.dropped,
            [
                json!({ "ref": taken["ref"], "reason": "name-conflict" }),
                json!({ "ref": twice["ref"], "reason": "name-conflict" }),
            ]
        );
    }

    #[test]
    fn two_pages_mapped_onto_one_share_its_names() {
        use super::super::bundle::PagePair;
        let one = annotation(3, 10, json!({ "nm": "same" }));
        let other = annotation(7, 20, json!({ "nm": "same" }));
        let pages = BundleImportPages::List(vec![
            PagePair { from: page(3), to: page(50) },
            PagePair { from: page(7), to: page(50) },
        ]);
        let target = vec![BundleImportTarget { page: page(50), position: 0 }];
        let planned = plan_with(&[one, other.clone()], pages, target, &[]).unwrap();
        assert_eq!(items_of(&planned), [0]);
        assert_eq!(planned.dropped, [json!({ "ref": other["ref"], "reason": "name-conflict" })]);
    }

    #[test]
    fn imports_an_item_without_a_field_it_can_only_read_as_a_marker() {
        let line = |annot, measure: &str| {
            annotation(
                3,
                annot,
                json!({ "subtype": "line", "measure": { "subtype": measure },
                "linePoints": { "start": { "x": 0, "y": 0 }, "end": { "x": 1, "y": 1 } } }),
            )
        };
        let geo = line(10, "geospatial");
        let unknown = line(11, "unknown");
        let planned = plan(&[geo.clone(), unknown.clone()]);
        assert_eq!(planned.creates.len(), 2);
        assert_eq!(planned.creates[0].draft["measure"], Value::Null);
        assert_eq!(
            planned.dropped,
            [
                json!({ "ref": geo["ref"], "field": "measure", "reason": "geospatial" }),
                json!({ "ref": unknown["ref"], "field": "measure", "reason": "unknown-measure" }),
            ]
        );
    }

    #[test]
    fn imports_a_link_without_an_action_a_write_cannot_make() {
        let script =
            annotation(3, 10, json!({ "subtype": "link", "target": { "kind": "javascript" } }));
        let planned = plan(std::slice::from_ref(&script));
        assert_eq!(planned.creates[0].draft["target"], Value::Null);
        assert_eq!(
            planned.dropped,
            [json!({ "ref": script["ref"], "field": "target", "reason": "unsupported-action" })]
        );
    }

    #[test]
    fn leaves_out_every_widget_in_a_field_or_not() {
        let field = annotation(
            3,
            10,
            json!({ "subtype": "widget",
            "field": { "kind": "objectNumber", "objectNumber": 44 }, "fieldFamily": "text" }),
        );
        let inert = annotation(
            3,
            11,
            json!({ "subtype": "widget", "field": null, "fieldFamily": "unknown" }),
        );
        let planned = plan(&[field.clone(), inert.clone(), annotation(3, 12, json!({}))]);
        assert_eq!(items_of(&planned), [2]);
        assert_eq!(
            planned.dropped,
            [
                json!({ "ref": field["ref"], "reason": "widget" }),
                json!({ "ref": inert["ref"], "reason": "widget" }),
            ]
        );
    }

    #[test]
    fn reports_the_actions_a_copy_leaves_out_and_not_the_one_a_link_target_carries() {
        let goto = json!({ "incomplete": false, "warningFlags": 0, "warnings": [],
            "root": { "type": "goto", "subtype": "GoTo", "next": [],
                "destination": { "kind": "fit", "page": page(3) } } });
        let target = json!({ "kind": "goto", "destination": { "kind": "fit", "page": page(3) } });
        let link = annotation(
            3,
            10,
            json!({ "subtype": "link", "target": target,
            "actions": { "activate": goto } }),
        );
        let hovering = annotation(
            3,
            11,
            json!({ "subtype": "link", "target": target,
            "actions": { "activate": goto, "cursorEnter": goto } }),
        );
        let mut chained_tree = goto.clone();
        chained_tree["root"]["next"] = json!([goto["root"]]);
        let chained = annotation(
            3,
            12,
            json!({ "subtype": "link", "target": target,
            "actions": { "activate": chained_tree } }),
        );
        let square = annotation(3, 13, json!({ "actions": { "activate": goto } }));
        let mut map_tree = goto.clone();
        map_tree["root"] = json!({ "type": "uri", "subtype": "URI", "next": [],
            "uri": "https://example.com/map", "isMap": true });
        let map = annotation(
            3,
            14,
            json!({ "subtype": "link",
            "target": { "kind": "uri", "uri": "https://example.com/map" },
            "actions": { "activate": map_tree } }),
        );
        let planned = plan(&[link, hovering.clone(), chained.clone(), square.clone(), map.clone()]);
        assert_eq!(planned.creates.len(), 5);
        let expected: Vec<Value> = [hovering, chained, square, map]
            .iter()
            .map(|item| json!({ "ref": item["ref"], "field": "actions", "reason": "unsupported-action" }))
            .collect();
        assert_eq!(planned.dropped, expected);
    }

    #[test]
    fn refuses_an_item_whose_data_is_not_valid_for_its_kind_naming_it() {
        let broken = annotation(3, 10, json!({ "box": { "left": "no" } }));
        let refused = plan_with(
            &[annotation(3, 9, json!({})), broken],
            BundleImportPages::Same,
            same_document(),
            &[],
        )
        .unwrap_err();
        assert_eq!(refused.code, EngineErrorCode::InvalidArg);
        assert_eq!(refused.details.unwrap()["item"], 1);
    }
}
