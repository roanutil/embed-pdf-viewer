//! `model.ts`: the form plugin's data, all pure.
//!
//! [`FieldIndex`] is the form as the engine confirmed it: the field tree and
//! every widget row, indexed by field key and by widget. It is the value of
//! the plugin's `fields` mirror and changes only through [`fold_form_event`]
//! and loads; `sync::predict::predict_form_op` shows this session's changes
//! on top of it. [`page_widgets_of`] is one page's widgets, derived from the
//! index's widget rows.
//!
//! The engine's DTOs (the snapshot, fields, widget rows, refs) stay JSON.
//! The index's own records (`byKey`, `byWidget`...) are JavaScript objects:
//! [`Positions`] keeps their keys as strings, read in the order a plain
//! object iterates them (integer-like keys first, ascending, then the rest
//! as inserted), so a record prints and iterates as the TypeScript's does.

use std::rc::Rc;

use epdf_kernel::Error;
use indexmap::IndexMap;
use serde_json::{Map, Value, json};

use crate::engine::{annotation_key, field_key_of_ref, reorder_part, row_number};
use crate::value::{get, items, number, number_key, sort_js_keys, str_of, truthy};

/// A page-space box (top-left origin, y-down, PDF points).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Box {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Box {
    pub fn read(value: &Value) -> Box {
        let n = |k: &str| get(value, k).and_then(Value::as_f64).unwrap_or(f64::NAN);
        Box { x: n("x"), y: n("y"), width: n("width"), height: n("height") }
    }

    pub fn to_json(self) -> Value {
        json!({
            "x": number(self.x), "y": number(self.y),
            "width": number(self.width), "height": number(self.height),
        })
    }
}

/// `FieldKey`: `obj:<n>` for a durable field, `fqn:<name>` otherwise.
pub type FieldKey = String;

/// `fieldKeyOf(field)`.
pub fn field_key_of(field: &Value) -> FieldKey {
    field_key_of_ref(get(field, "ref").unwrap_or(&Value::Null))
}

/// A JavaScript record of numbers by key (`Record<string, number>`).
pub type Positions = IndexMap<String, f64>;

thread_local! {
    static SNAPSHOT_COPIES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many times this thread copied a whole snapshot: a [`Snapshot`] read
/// from a borrowed value, or one put back together as JSON. Folds,
/// predictions and fills share the snapshot's parts and copy none, so a
/// test can hold this still across them (the M21 guard).
pub fn snapshot_copies() -> usize {
    SNAPSHOT_COPIES.with(std::cell::Cell::get)
}

fn count_copy() {
    SNAPSHOT_COPIES.with(|c| c.set(c.get() + 1));
}

thread_local! {
    static INDEX_SERIALISATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many times this thread serialised a whole [`FieldIndex`] for a
/// store dump. The guard (`tests/sharing.rs`) holds it at one per
/// confirmed form however often a host refetches the store's text.
#[doc(hidden)]
pub fn index_serialisations() -> usize {
    INDEX_SERIALISATIONS.with(std::cell::Cell::get)
}

fn count_serialisation() {
    INDEX_SERIALISATIONS.with(|c| c.set(c.get() + 1));
}

/// The engine's `FormSnapshot`, its `fields` and `widgets` held apart, one
/// `Rc` each and one `Rc` per item. A change replaces one member and shares
/// the others, the way `{ ...snapshot, fields }` shares them in the
/// TypeScript: an upsert copies pointers, never fields.
#[derive(Debug)]
pub struct Snapshot {
    /// Every other member, in order; `fields` and `widgets` keep their
    /// places as `null` when they were lists.
    rest: Rc<Value>,
    fields: Option<Rc<Vec<Rc<Value>>>>,
    widgets: Option<Rc<Vec<Rc<Value>>>>,
    /// The snapshot as one value, put together on the first read that needs
    /// it (`getSnapshot`, the scripts, `resynced`).
    whole: std::cell::OnceCell<Rc<Value>>,
}

impl PartialEq for Snapshot {
    fn eq(&self, other: &Snapshot) -> bool {
        self.rest == other.rest
            && self.fields() == other.fields()
            && self.widgets() == other.widgets()
    }
}

fn take_list(rest: &mut Value, key: &str) -> Option<Rc<Vec<Rc<Value>>>> {
    let slot = rest.as_object_mut()?.get_mut(key)?;
    let Value::Array(list) = slot.take() else { return None };
    Some(Rc::new(list.into_iter().map(Rc::new).collect()))
}

impl Snapshot {
    /// A snapshot the engine handed over, taken apart without a copy.
    pub fn new(mut value: Value) -> Snapshot {
        let fields = take_list(&mut value, "fields");
        let widgets = take_list(&mut value, "widgets");
        // `take_list` left `null` behind only where it took a list; a member
        // that was not one is still in place.
        Snapshot { rest: Rc::new(value), fields, widgets, whole: Default::default() }
    }

    /// A snapshot read from a borrowed value: one copy.
    pub fn of(value: &Value) -> Snapshot {
        count_copy();
        Snapshot::new(value.clone())
    }

    /// `snapshot[key]` for a member other than the two lists.
    pub fn get(&self, key: &str) -> Option<&Value> {
        get(&self.rest, key)
    }

    /// `snapshot[key]` as a list, empty when it is not one.
    pub fn items(&self, key: &str) -> &[Value] {
        items(&self.rest, key)
    }

    pub fn fields(&self) -> &[Rc<Value>] {
        self.fields.as_deref().map_or(&[], Vec::as_slice)
    }

    pub fn widgets(&self) -> &[Rc<Value>] {
        self.widgets.as_deref().map_or(&[], Vec::as_slice)
    }

    /// `{ ...snapshot, [key]: value }` for a member other than the lists.
    pub fn with_member(&self, key: &str, value: Value) -> Snapshot {
        let mut rest = self.rest.as_object().cloned().unwrap_or_default();
        rest.insert(key.to_string(), value);
        Snapshot {
            rest: Rc::new(Value::Object(rest)),
            fields: self.fields.clone(),
            widgets: self.widgets.clone(),
            whole: Default::default(),
        }
    }

    /// `rest` with a list's place made, for a snapshot that had none.
    fn rest_with_place(&self, key: &str, had: bool) -> Rc<Value> {
        if had {
            return self.rest.clone();
        }
        let mut rest = self.rest.as_object().cloned().unwrap_or_default();
        rest.insert(key.to_string(), Value::Null);
        Rc::new(Value::Object(rest))
    }

    /// `{ ...snapshot, fields }`: the widgets and every other member shared.
    pub fn with_fields(&self, fields: Vec<Rc<Value>>) -> Snapshot {
        Snapshot {
            rest: self.rest_with_place("fields", self.fields.is_some()),
            fields: Some(Rc::new(fields)),
            widgets: self.widgets.clone(),
            whole: Default::default(),
        }
    }

    /// `{ ...snapshot, widgets }`: the fields and every other member shared.
    pub fn with_widgets(&self, widgets: Vec<Rc<Value>>) -> Snapshot {
        Snapshot {
            rest: self.rest_with_place("widgets", self.widgets.is_some()),
            fields: self.fields.clone(),
            widgets: Some(Rc::new(widgets)),
            whole: Default::default(),
        }
    }

    /// The snapshot as one value, put together once (one copy) and kept.
    pub fn whole(&self) -> Rc<Value> {
        self.whole
            .get_or_init(|| {
                count_copy();
                Rc::new(self.to_json())
            })
            .clone()
    }

    fn to_json(&self) -> Value {
        let Some(rest) = self.rest.as_object() else { return (*self.rest).clone() };
        let list = |l: &Option<Rc<Vec<Rc<Value>>>>| {
            l.as_ref().map(|l| Value::Array(l.iter().map(|v| (**v).clone()).collect()))
        };
        let mut out = Map::new();
        for (key, value) in rest {
            let taken = match key.as_str() {
                "fields" => list(&self.fields),
                "widgets" => list(&self.widgets),
                _ => None,
            };
            out.insert(key.clone(), taken.unwrap_or_else(|| value.clone()));
        }
        Value::Object(out)
    }
}

/// A [`FieldIndex`] as it prints, members in the TypeScript object's order.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PrintedIndex<'a> {
    snapshot: Option<PrintedSnapshot<'a>>,
    by_key: Value,
    by_name: Value,
    by_widget: Value,
    row_by_widget: Value,
    picture_versions: Value,
}

/// A [`Snapshot`] as [`Snapshot::whole`] holds it, written from its shared
/// parts without putting it together.
struct PrintedSnapshot<'a>(&'a Snapshot);

impl serde::Serialize for PrintedSnapshot<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::{SerializeMap, SerializeSeq};
        struct Items<'a>(&'a [Rc<Value>]);
        impl serde::Serialize for Items<'_> {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
                for item in self.0 {
                    seq.serialize_element(&**item)?;
                }
                seq.end()
            }
        }
        let snapshot = self.0;
        let Some(rest) = snapshot.rest.as_object() else {
            return snapshot.rest.serialize(serializer);
        };
        let mut map = serializer.serialize_map(Some(rest.len()))?;
        for (key, value) in rest {
            let list = match key.as_str() {
                "fields" => snapshot.fields.as_deref(),
                "widgets" => snapshot.widgets.as_deref(),
                _ => None,
            };
            match list {
                Some(list) => map.serialize_entry(key, &Items(list))?,
                None => map.serialize_entry(key, value)?,
            }
        }
        map.end()
    }
}

/// `FieldIndex`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FieldIndex {
    pub snapshot: Option<Rc<Snapshot>>,
    /// Field key to index into `snapshot.fields`.
    pub by_key: Positions,
    /// Full name to index into `snapshot.fields`.
    pub by_name: Positions,
    /// Widget annotation object number to index into `snapshot.fields`.
    pub by_widget: Positions,
    /// Widget annotation object number to index into `snapshot.widgets`.
    pub row_by_widget: Positions,
    /// Widget annotation object number to the version of its picture.
    pub picture_versions: Positions,
}

fn positions_json(positions: &Positions) -> Value {
    let mut sorted = positions.clone();
    sort_js_keys(&mut sorted);
    Value::Object(sorted.into_iter().map(|(k, v)| (k, number(v))).collect())
}

fn positions_of(value: Option<&Value>) -> Positions {
    value
        .and_then(Value::as_object)
        .map(|o| {
            o.iter()
                .map(|(k, v)| (k.clone(), crate::value::f64_of(&json!({ "v": v }), "v")))
                .collect()
        })
        .unwrap_or_default()
}

/// `position === undefined ? null : list[position] ?? null`.
fn at<'a>(list: &'a [Rc<Value>], position: Option<&f64>) -> Option<&'a Value> {
    shared_at(list, position).map(|v| &**v)
}

fn shared_at<'a>(list: &'a [Rc<Value>], position: Option<&f64>) -> Option<&'a Rc<Value>> {
    let position = *position?;
    if position < 0.0 || position.fract() != 0.0 {
        return None;
    }
    list.get(position as usize)
}

impl FieldIndex {
    /// `snapshot.fields`, none without a snapshot.
    pub fn fields(&self) -> &[Rc<Value>] {
        self.snapshot.as_deref().map_or(&[], Snapshot::fields)
    }

    /// `snapshot.widgets`, none without a snapshot.
    pub fn widgets(&self) -> &[Rc<Value>] {
        self.snapshot.as_deref().map_or(&[], Snapshot::widgets)
    }

    /// `snapshot` as one value (see [`Snapshot::whole`]).
    pub fn whole_snapshot(&self) -> Option<Rc<Value>> {
        self.snapshot.as_deref().map(Snapshot::whole)
    }

    /// The index as the TypeScript object prints. Written from the
    /// snapshot's shared parts: a dump neither puts the snapshot together
    /// nor keeps it (M38).
    pub fn to_json(&self) -> Value {
        count_serialisation();
        serde_json::to_value(self.printed()).expect("JSON values serialise")
    }

    /// [`to_json`](Self::to_json) as text, byte for byte its
    /// `to_string()`, written without building the value.
    pub fn to_text(&self) -> String {
        count_serialisation();
        serde_json::to_string(&self.printed()).expect("JSON values serialise")
    }

    fn printed(&self) -> PrintedIndex<'_> {
        PrintedIndex {
            snapshot: self.snapshot.as_deref().map(PrintedSnapshot),
            by_key: positions_json(&self.by_key),
            by_name: positions_json(&self.by_name),
            by_widget: positions_json(&self.by_widget),
            row_by_widget: positions_json(&self.row_by_widget),
            picture_versions: positions_json(&self.picture_versions),
        }
    }

    /// An index as a caller wrote it, its records as given (consistent or not).
    pub fn from_json(value: &Value) -> FieldIndex {
        FieldIndex {
            snapshot: get(value, "snapshot")
                .filter(|s| !s.is_null())
                .map(|s| Rc::new(Snapshot::of(s))),
            by_key: positions_of(get(value, "byKey")),
            by_name: positions_of(get(value, "byName")),
            by_widget: positions_of(get(value, "byWidget")),
            row_by_widget: positions_of(get(value, "rowByWidget")),
            picture_versions: positions_of(get(value, "pictureVersions")),
        }
    }
}

/// `emptyFieldIndex()`.
pub fn empty_field_index() -> FieldIndex {
    FieldIndex::default()
}

/// `indexFields(snapshot, pictureVersions)`.
pub fn index_fields(snapshot: Rc<Snapshot>, picture_versions: Positions) -> FieldIndex {
    let mut by_key = Positions::new();
    let mut by_name = Positions::new();
    let mut by_widget = Positions::new();
    let mut row_by_widget = Positions::new();
    for (position, field) in snapshot.fields().iter().enumerate() {
        let position = position as f64;
        by_key.insert(field_key_of(field), position);
        let name = crate::value::string_of(get(field, "name"));
        by_name.entry(name).or_insert(position);
        for widget in items(field, "widgets") {
            let n = get(widget, "objectNumber").and_then(Value::as_f64).unwrap_or(f64::NAN);
            if n > 0.0 {
                by_widget.insert(number_key(n), position);
            }
        }
    }
    for (position, row) in snapshot.widgets().iter().enumerate() {
        let reference = get(row, "ref").unwrap_or(&Value::Null);
        if str_of(reference, "kind") == Some("objectNumber") {
            let n = get(reference, "objectNumber").and_then(Value::as_f64).unwrap_or(f64::NAN);
            row_by_widget.insert(number_key(n), position as f64);
        }
    }
    FieldIndex {
        snapshot: Some(snapshot),
        by_key,
        by_name,
        by_widget,
        row_by_widget,
        picture_versions,
    }
}

/// `reindex(index, snapshot)`: keeping the index's picture versions.
fn reindex(index: &FieldIndex, snapshot: Snapshot) -> Rc<FieldIndex> {
    Rc::new(index_fields(Rc::new(snapshot), index.picture_versions.clone()))
}

/// `{ ...snapshot, [key]: value }`, the lists shared.
pub fn with_snapshot_member(index: &FieldIndex, key: &str, value: Value) -> Rc<FieldIndex> {
    match &index.snapshot {
        Some(snapshot) => reindex(index, snapshot.with_member(key, value)),
        None => reindex(index, Snapshot::new(json!({ key: value }))),
    }
}

/// `indexLoadedFields(previous, snapshot)`: every picture version moves past
/// the one it had; a new widget starts at 0.
pub fn index_loaded_fields(previous: &FieldIndex, snapshot: Rc<Snapshot>) -> FieldIndex {
    let mut versions = Positions::new();
    for (number, version) in &previous.picture_versions {
        // `Number(number)` as the key again: the same key for every key a
        // number wrote.
        let key = number_key(epdf_kernel::mirror::string_to_number(number));
        versions.insert(key, version + 1.0);
    }
    for row in snapshot.widgets() {
        let n = widget_number(row);
        if n > 0.0 {
            versions.entry(number_key(n)).or_insert(0.0);
        }
    }
    index_fields(snapshot, versions)
}

/// `pictureVersionOf(index, annotObjectNumber)`.
pub fn picture_version_of(index: &FieldIndex, number: f64) -> f64 {
    index.picture_versions.get(&number_key(number)).copied().unwrap_or(0.0)
}

/// `upsertFields(index, fields)`: replace a field by key, or append it.
pub fn upsert_fields(index: &Rc<FieldIndex>, fields: &[Value]) -> Rc<FieldIndex> {
    let Some(snapshot) = &index.snapshot else { return index.clone() };
    if fields.is_empty() {
        return index.clone();
    }
    // `[...snapshot.fields]`: the pointers, not the fields.
    let mut next: Vec<Rc<Value>> = snapshot.fields().to_vec();
    for field in fields {
        let field = Rc::new(field.clone());
        match index.by_key.get(&field_key_of(&field)) {
            None => next.push(field),
            Some(position) => {
                let position = *position;
                if position >= 0.0 && position.fract() == 0.0 {
                    let at = position as usize;
                    if at < next.len() {
                        next[at] = field;
                    } else {
                        // A position past the end writes there, leaving holes.
                        next.resize(at, Rc::new(Value::Null));
                        next.push(field);
                    }
                }
            }
        }
    }
    reindex(index, snapshot.with_fields(next))
}

/// A widget row's object number, `0` for a ref of another kind.
pub fn widget_number(row: &Value) -> f64 {
    row_number(row)
}

/// `upsertWidgets(index, rows)`: replace rows by object number, or append.
pub fn upsert_widgets(index: &Rc<FieldIndex>, rows: &[Value]) -> Rc<FieldIndex> {
    let Some(snapshot) = &index.snapshot else { return index.clone() };
    if rows.is_empty() {
        return index.clone();
    }
    // `new Map(rows.map(...))`: a later row under the same number wins, at
    // the first one's place.
    let mut incoming: IndexMap<String, Rc<Value>> = IndexMap::new();
    for row in rows {
        incoming.insert(number_key(widget_number(row)), Rc::new(row.clone()));
    }
    let mut next: Vec<Rc<Value>> = snapshot
        .widgets()
        .iter()
        .map(|row| match incoming.shift_remove(&number_key(widget_number(row))) {
            Some(replaced) => replaced,
            None => row.clone(),
        })
        .collect();
    next.extend(incoming.into_values());
    reindex(index, snapshot.with_widgets(next))
}

/// `reorderWidgetRows(index, page, order)`: the page's rows in `order`, in
/// their slots.
pub fn reorder_widget_rows(
    index: &Rc<FieldIndex>,
    page: &Value,
    order: &[Value],
) -> Rc<FieldIndex> {
    let Some(snapshot) = &index.snapshot else { return index.clone() };
    let page_number = get(page, "objectNumber").cloned();
    let on_page = |row: &Value| {
        crate::value::strict_eq(
            get(row, "page").and_then(|p| get(p, "objectNumber")),
            page_number.as_ref(),
        )
    };
    let keys: Vec<String> = order.iter().map(annotation_key).collect();
    let key_of = |row: &Value| annotation_key(get(row, "ref").unwrap_or(&Value::Null));
    let in_part = |row: &Rc<Value>| on_page(row);
    let widgets = reorder_part(index.widgets(), &in_part, &keys, &|row: &Rc<Value>| key_of(row));
    reindex(index, snapshot.with_widgets(widgets))
}

/// `removeWidgets(index, gone)`: drop the rows of widgets that went.
pub fn remove_widgets(index: &Rc<FieldIndex>, gone: &[Value]) -> Rc<FieldIndex> {
    let Some(snapshot) = &index.snapshot else { return index.clone() };
    if gone.is_empty() {
        return index.clone();
    }
    let numbers: std::collections::HashSet<String> = gone
        .iter()
        .map(|w| number_key(get(w, "objectNumber").and_then(Value::as_f64).unwrap_or(f64::NAN)))
        .collect();
    let widgets: Vec<Rc<Value>> = index
        .widgets()
        .iter()
        .filter(|row| !numbers.contains(&number_key(widget_number(row))))
        .cloned()
        .collect();
    if widgets.len() == index.widgets().len() {
        return index.clone();
    }
    reindex(index, snapshot.with_widgets(widgets))
}

/// `removeField(index, ref)`.
pub fn remove_field(index: &Rc<FieldIndex>, reference: &Value) -> Rc<FieldIndex> {
    let Some(snapshot) = &index.snapshot else { return index.clone() };
    let by_number = str_of(reference, "kind") == Some("objectNumber");
    let fields: Vec<Rc<Value>> = index
        .fields()
        .iter()
        .filter(|field| {
            let own = get(field, "ref").unwrap_or(&Value::Null);
            if by_number {
                !(str_of(own, "kind") == Some("objectNumber")
                    && crate::value::strict_eq(
                        get(own, "objectNumber"),
                        get(reference, "objectNumber"),
                    ))
            } else {
                !crate::value::strict_eq(get(field, "name"), get(reference, "name"))
            }
        })
        .cloned()
        .collect();
    if fields.len() == index.fields().len() {
        return index.clone();
    }
    reindex(index, snapshot.with_fields(fields))
}

/// What a fold leaves: the index, or a request to read the form again.
pub enum Folded {
    Index(Rc<FieldIndex>),
    Reload,
}

/// `foldFormEvent(index, event)`.
pub fn fold_form_event(index: &Rc<FieldIndex>, event: &Value) -> Folded {
    let redrawn = redrawn_widgets_of(event);
    let index = with_redrawn_pictures(index, &redrawn);
    fold_form_facts(&index, event)
}

/// `redrawnWidgetsOf(event)`.
fn redrawn_widgets_of(event: &Value) -> Vec<Value> {
    let kind = str_of(event, "type").unwrap_or("");
    if kind == "signatures.completed" {
        return match get(event, "signature").and_then(|s| get(s, "widget")) {
            Some(widget) if truthy(Some(widget)) => vec![widget.clone()],
            _ => Vec::new(),
        };
    }
    if !kind.starts_with("forms.") || get(event, "meta").is_none() {
        return Vec::new();
    }
    let meta = get(event, "meta").unwrap_or(&Value::Null);
    get(meta, "redrawnWidgets")
        .filter(|v| !v.is_null())
        .or_else(|| get(meta, "changedWidgets").filter(|v| !v.is_null()))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// `withRedrawnPictures(index, redrawn)`.
fn with_redrawn_pictures(index: &Rc<FieldIndex>, redrawn: &[Value]) -> Rc<FieldIndex> {
    if redrawn.is_empty() {
        return index.clone();
    }
    let mut numbers: IndexMap<String, ()> = IndexMap::new();
    for widget in redrawn {
        let n = get(widget, "objectNumber").and_then(Value::as_f64).unwrap_or(0.0);
        if n != 0.0 && !n.is_nan() {
            numbers.insert(number_key(n), ());
        } else if let Some(page) = get(widget, "page").filter(|p| truthy(Some(p))) {
            for row in index.widgets() {
                if crate::value::strict_eq(
                    get(row, "page").and_then(|p| get(p, "objectNumber")),
                    get(page, "objectNumber"),
                ) {
                    numbers.insert(number_key(widget_number(row)), ());
                }
            }
        }
    }
    if numbers.is_empty() {
        return index.clone();
    }
    let mut versions = index.picture_versions.clone();
    for (key, ()) in numbers {
        let version = versions.get(&key).copied().unwrap_or(0.0);
        versions.insert(key, version + 1.0);
    }
    Rc::new(FieldIndex { picture_versions: versions, ..(**index).clone() })
}

/// `withoutSplitRow(index, field)`.
fn without_split_row(index: &Rc<FieldIndex>, field: &Value) -> Rc<FieldIndex> {
    let reference = get(field, "ref").unwrap_or(&Value::Null);
    if str_of(reference, "kind") != Some("objectNumber") {
        return index.clone();
    }
    let own = get(reference, "objectNumber").and_then(Value::as_f64).unwrap_or(f64::NAN);
    if items(field, "widgets")
        .iter()
        .any(|w| get(w, "objectNumber").and_then(Value::as_f64) == Some(own))
    {
        return index.clone();
    }
    match widget_row_of(index, own) {
        Some(row) => remove_widgets(
            index,
            &[json!({ "ref": row["ref"], "objectNumber": own, "page": row["page"] })],
        ),
        None => index.clone(),
    }
}

fn field_of_event(event: &Value) -> Vec<Value> {
    get(event, "field").filter(|f| truthy(Some(f))).cloned().into_iter().collect()
}

fn fold_form_facts(index: &Rc<FieldIndex>, event: &Value) -> Folded {
    let widgets = items(event, "widgets");
    let order = get(event, "calculationOrder");
    Folded::Index(match str_of(event, "type").unwrap_or("") {
        "forms.widgetAdded" => {
            let field = get(event, "field").cloned().unwrap_or(Value::Null);
            without_split_row(
                &upsert_widgets(&upsert_fields(index, std::slice::from_ref(&field)), widgets),
                &field,
            )
        }
        "forms.valueSet" | "forms.widgetRemoved" | "forms.restored" => {
            let field = get(event, "field").cloned().unwrap_or(Value::Null);
            upsert_widgets(&upsert_fields(index, &[field]), widgets)
        }
        "forms.updated" => {
            let field = get(event, "field").cloned().unwrap_or(Value::Null);
            with_calculation_order(&upsert_widgets(&upsert_fields(index, &[field]), widgets), order)
        }
        "forms.created" => {
            let field = get(event, "field").cloned().unwrap_or(Value::Null);
            let created = upsert_widgets(&upsert_fields(index, &[field]), widgets);
            let formed = match &created.snapshot {
                Some(s) if s.get("formKind").and_then(Value::as_str) == Some("none") => {
                    with_snapshot_member(&created, "formKind", json!("acroform"))
                }
                _ => created,
            };
            with_calculation_order(&formed, order)
        }
        "forms.calculationsReordered" => with_calculation_order(index, order),
        "forms.widgetDeleted" => {
            let changed = get(event, "meta").map(|m| items(m, "changedWidgets")).unwrap_or(&[]);
            let gone = remove_widgets(index, changed);
            let field = field_of_event(event);
            if field.is_empty() { gone } else { upsert_fields(&gone, &field) }
        }
        "forms.widgetRestored" => {
            let field = field_of_event(event);
            let base = if field.is_empty() { index.clone() } else { upsert_fields(index, &field) };
            upsert_widgets(&base, widgets)
        }
        "forms.widgetUpdated" => {
            let widget = get(event, "widget").cloned().unwrap_or(Value::Null);
            upsert_widgets(index, &[widget])
        }
        "forms.widgetsReordered" => reorder_widget_rows(
            index,
            get(event, "page").unwrap_or(&Value::Null),
            items(event, "order"),
        ),
        "forms.deleted" => match get(event, "deleted").filter(|d| truthy(Some(d))) {
            Some(deleted) => {
                let changed = get(event, "meta").map(|m| items(m, "changedWidgets")).unwrap_or(&[]);
                with_calculation_order(
                    &remove_widgets(&remove_field(index, deleted), changed),
                    order,
                )
            }
            None => return Folded::Reload,
        },
        "forms.repaired"
        | "pages.deleted"
        | "pages.inserted"
        | "pages.restored"
        | "pages.flattened"
        | "annotations.flattened"
        | "redaction.applied" => {
            return Folded::Reload;
        }
        _ => index.clone(),
    })
}

/// `formHasScripts(index)`: whether any field has a keystroke, validate,
/// calculate or format script.
pub fn form_has_scripts(index: &FieldIndex) -> bool {
    index.fields().iter().any(|field| {
        let actions = get(field, "actions");
        ["keystroke", "validate", "calculate", "format"]
            .iter()
            .any(|k| truthy(actions.and_then(|a| get(a, k))))
    })
}

/// `withCalculationOrder(index, order)`.
pub fn with_calculation_order(index: &Rc<FieldIndex>, order: Option<&Value>) -> Rc<FieldIndex> {
    let Some(order) = order.filter(|o| truthy(Some(o))) else { return index.clone() };
    if index.snapshot.is_none() {
        return index.clone();
    }
    let order = Value::Array(order.as_array().cloned().unwrap_or_default());
    with_snapshot_member(index, "calculationOrder", order)
}

/// `fieldByKey(index, key)`.
pub fn field_by_key<'a>(index: &'a FieldIndex, key: &str) -> Option<&'a Value> {
    at(index.fields(), index.by_key.get(key))
}

/// `fieldByRef(index, ref)`: by object number, or by full name.
pub fn field_by_ref<'a>(index: &'a FieldIndex, reference: &Value) -> Option<&'a Value> {
    shared_field_by_ref(index, reference).map(|v| &**v)
}

/// [`field_by_ref`], the field as the index shares it.
pub fn shared_field_by_ref<'a>(index: &'a FieldIndex, reference: &Value) -> Option<&'a Rc<Value>> {
    let position = if str_of(reference, "kind") == Some("fqn") {
        index.by_name.get(&crate::value::string_of(get(reference, "name")))
    } else {
        index.by_key.get(&field_key_of_ref(reference))
    };
    shared_at(index.fields(), position)
}

/// [`field_by_key`], the field as the index shares it.
pub fn shared_field_by_key<'a>(index: &'a FieldIndex, key: &str) -> Option<&'a Rc<Value>> {
    shared_at(index.fields(), index.by_key.get(key))
}

/// [`field_for_widget`], the field as the index shares it.
pub fn shared_field_for_widget(index: &FieldIndex, number: f64) -> Option<&Rc<Value>> {
    shared_at(index.fields(), index.by_widget.get(&number_key(number)))
}

/// [`widget_row_of`], the row as the index shares it.
pub fn shared_widget_row_of(index: &FieldIndex, number: f64) -> Option<&Rc<Value>> {
    shared_at(index.widgets(), index.row_by_widget.get(&number_key(number)))
}

/// `canonicalKey(index, ref)`.
pub fn canonical_key(index: &FieldIndex, reference: &Value) -> FieldKey {
    match field_by_ref(index, reference) {
        Some(field) => field_key_of(field),
        None => field_key_of_ref(reference),
    }
}

/// `fieldForWidget(index, annotObjectNumber)`.
pub fn field_for_widget(index: &FieldIndex, number: f64) -> Option<&Value> {
    at(index.fields(), index.by_widget.get(&number_key(number)))
}

/// `widgetRowOf(index, annotObjectNumber)`.
pub fn widget_row_of(index: &FieldIndex, number: f64) -> Option<&Value> {
    at(index.widgets(), index.row_by_widget.get(&number_key(number)))
}

fn value_entry_json(field: &Value) -> String {
    let entry = field.as_object().and_then(|o| o.get("valueEntry")).cloned().unwrap_or(Value::Null);
    crate::value::json_stringify(&entry)
}

/// `fieldsWithChangedValues(previous, next)`.
pub fn fields_with_changed_values(previous: &FieldIndex, next: &FieldIndex) -> Vec<Value> {
    next.fields()
        .iter()
        .filter(|field| match field_by_key(previous, &field_key_of(field)) {
            None => true,
            Some(before) => value_entry_json(before) != value_entry_json(field),
        })
        .map(|field| (**field).clone())
        .collect()
}

// ── widgets ─────────────────────────────────────────────────────────────────

/// `FormWidgetLook`, as JSON in the TypeScript's member order.
pub fn look_of_row(row: &Value) -> Value {
    let or_null = |k: &str| get(row, k).cloned().unwrap_or(Value::Null);
    let or_else = |k: &str, fallback: Value| match get(row, k) {
        Some(v) if !v.is_null() => v.clone(),
        _ => fallback,
    };
    json!({
        "border": or_null("color"),
        "borderWidth": or_else("strokeWidth", json!(1)),
        "borderStyle": or_else("borderStyle", json!("solid")),
        "background": or_null("interiorColor"),
        "color": or_null("fontColor"),
        "fontFamily": or_null("fontFamily"),
        "fontSize": or_null("fontSize"),
        "textAlign": or_else("textAlign", json!("left")),
    })
}

/// `PageWidget`: `{ box, look }`.
#[derive(Clone, Debug, PartialEq)]
pub struct PageWidget {
    pub bx: Value,
    pub look: Value,
}

impl PageWidget {
    pub fn to_json(&self) -> Value {
        json!({ "box": self.bx, "look": self.look })
    }

    pub fn read(value: &Value) -> PageWidget {
        PageWidget {
            bx: get(value, "box").cloned().unwrap_or(Value::Null),
            look: get(value, "look").cloned().unwrap_or(Value::Null),
        }
    }
}

/// `PageWidgets`: annotation object number to its place and look, a
/// record (iterated integer keys first).
pub type PageWidgets = IndexMap<String, PageWidget>;

/// `PageWidgets` as JSON, in `Object.keys` order.
pub fn page_widgets_json(widgets: &PageWidgets) -> Value {
    let mut sorted = widgets.clone();
    sort_js_keys(&mut sorted);
    Value::Object(sorted.into_iter().map(|(k, w)| (k, w.to_json())).collect::<Map<_, _>>())
}

/// `PageWidgets` from JSON.
pub fn page_widgets_of_json(value: &Value) -> PageWidgets {
    let mut widgets: PageWidgets = value
        .as_object()
        .map(|o| o.iter().map(|(k, v)| (k.clone(), PageWidget::read(v))).collect())
        .unwrap_or_default();
    sort_js_keys(&mut widgets);
    widgets
}

/// `shownOnScreen(row)`: its `hidden` and `noView` flags hide it.
pub fn shown_on_screen(row: &Value) -> bool {
    !truthy(get(row, "hidden")) && !truthy(get(row, "noView"))
}

fn row_page_is(row: &Value, page: f64) -> bool {
    get(row, "page").and_then(|p| get(p, "objectNumber")).and_then(Value::as_f64) == Some(page)
}

/// `pageWidgetsOf(index, pageObjectNumber)`.
pub fn page_widgets_of(index: &FieldIndex, page: f64) -> PageWidgets {
    let mut widgets = PageWidgets::new();
    for row in index.widgets() {
        let reference = get(row, "ref").unwrap_or(&Value::Null);
        if !row_page_is(row, page) || str_of(reference, "kind") != Some("objectNumber") {
            continue;
        }
        if !shown_on_screen(row) {
            continue;
        }
        let n = get(reference, "objectNumber").and_then(Value::as_f64).unwrap_or(f64::NAN);
        widgets.insert(
            number_key(n),
            PageWidget {
                bx: get(row, "rect").cloned().unwrap_or(Value::Null),
                look: look_of_row(row),
            },
        );
    }
    sort_js_keys(&mut widgets);
    widgets
}

/// `ShownWidget`.
#[derive(Clone, Debug, PartialEq)]
pub struct ShownWidget {
    pub reference: Value,
    pub rect: Value,
    pub appearance_state: Value,
    pub pending: bool,
    pub picture_version: f64,
}

impl ShownWidget {
    pub fn to_json(&self) -> Value {
        json!({
            "ref": self.reference,
            "rect": self.rect,
            "appearanceState": self.appearance_state,
            "pending": self.pending,
            "pictureVersion": number(self.picture_version),
        })
    }
}

/// `shownWidgetsOf(index, pageObjectNumber, lookOf)`.
pub fn shown_widgets_of(
    index: &FieldIndex,
    page: f64,
    look_of: &dyn Fn(&Value) -> (bool, f64),
) -> Vec<ShownWidget> {
    index
        .widgets()
        .iter()
        .filter(|row| row_page_is(row, page) && shown_on_screen(row))
        .map(|row| {
            let (pending, picture_version) = look_of(row);
            ShownWidget {
                reference: get(row, "ref").cloned().unwrap_or(Value::Null),
                rect: get(row, "rect").cloned().unwrap_or(Value::Null),
                appearance_state: get(row, "appearanceState").cloned().unwrap_or(Value::Null),
                pending,
                picture_version,
            }
        })
        .collect()
}

/// `WidgetHit`.
#[derive(Clone, Debug, PartialEq)]
pub struct WidgetHit {
    pub annot_object_number: f64,
    pub field: Value,
    pub bx: Value,
}

impl WidgetHit {
    pub fn to_json(&self) -> Value {
        json!({
            "annotObjectNumber": number(self.annot_object_number),
            "field": self.field,
            "box": self.bx,
        })
    }
}

/// `boxContains(box, point)`.
pub fn box_contains(bx: &Box, x: f64, y: f64) -> bool {
    x >= bx.x && x <= bx.x + bx.width && y >= bx.y && y <= bx.y + bx.height
}

/// `widgetAt(index, widgets, point)`: the smallest box under the point.
pub fn widget_at(
    index: &FieldIndex,
    widgets: Option<&PageWidgets>,
    x: f64,
    y: f64,
) -> Option<WidgetHit> {
    let widgets = widgets?;
    let mut ordered = widgets.clone();
    sort_js_keys(&mut ordered);
    let mut best: Option<(WidgetHit, f64)> = None;
    for (key, widget) in &ordered {
        let bx = Box::read(&widget.bx);
        if !box_contains(&bx, x, y) {
            continue;
        }
        let number = epdf_kernel::mirror::string_to_number(key);
        let Some(field) = field_for_widget(index, number) else { continue };
        let area = bx.width * bx.height;
        if best.as_ref().is_none_or(|(_, best_area)| area < *best_area) {
            best = Some((
                WidgetHit {
                    annot_object_number: number,
                    field: field.clone(),
                    bx: widget.bx.clone(),
                },
                area,
            ));
        }
    }
    best.map(|(hit, _)| hit)
}

/// `initialFormState()`: the session state is empty.
pub fn initial_form_state() -> Value {
    json!({})
}

/// A fold or load error as the mirror reports it.
pub fn decode_error(what: &str) -> Error {
    Error::new(format!("[form] {what}"))
}
