//! M21: the form shares its snapshot across folds, predictions and fills,
//! the way the TypeScript shares it by reference. A change replaces one
//! field (or one widget row) and keeps every other one, so a large form
//! pays no copy per keystroke. The guard is a count, not a clock:
//! `snapshot_copies` counts every whole-snapshot copy, and these runs make
//! none. Each run has a hang ceiling, nothing finer.
//!
//! `listWidgets` is memoized as the TypeScript's `memoByKey` is: the same
//! list while the form, the session's rights and `fillable` stay the same.

use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use epdf_kernel::kernel::OpenSource;
use epdf_kernel::{Kernel, KernelOptions, OpenOptions, rt};
use epdf_plugin_annotation::annotation_plugin;
use epdf_plugin_form::model::{
    FieldIndex, Folded, Snapshot, field_by_key, fold_form_event, index_fields, snapshot_copies,
};
use epdf_plugin_form::sync::predict::predict_form_op;
use epdf_plugin_form::{FORM, form_plugin};
use epdf_plugin_interaction::{InteractionConfig, interaction_plugin};
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use serde_json::{Value, json};

/// Long enough that only a hang trips it.
const CEILING: Duration = Duration::from_secs(120);

fn object_ref(n: usize) -> Value {
    json!({ "kind": "objectNumber", "objectNumber": n })
}

fn widget_ref(n: usize) -> Value {
    json!({ "kind": "objectNumber", "page": object_ref(1), "objectNumber": 10_000 + n })
}

fn field(n: usize, value: &str) -> Value {
    json!({
        "ref": object_ref(n), "name": format!("f{n}"), "origin": "acroform",
        "readOnly": false, "required": false, "noExport": false,
        "alternateName": null, "mappingName": null,
        "valueEntry": { "kind": "text", "value": value },
        "defaultValueEntry": { "kind": "none" },
        "groupId": null, "createdBy": null, "createdAt": null,
        "filledBy": null, "filledByName": null, "filledAt": null, "importedBy": null,
        "widgets": [{ "ref": widget_ref(n), "objectNumber": 10_000 + n, "page": object_ref(1) }],
        "family": "text", "value": value, "defaultValue": "", "maxLength": null,
        "multiline": false, "password": false, "comb": false,
    })
}

fn row(n: usize) -> Value {
    json!({
        "subtype": "widget", "ref": widget_ref(n), "page": object_ref(1),
        "rect": { "x": 10, "y": n as f64 * 2.0, "width": 100, "height": 2 },
    })
}

fn snapshot(fields: usize) -> Value {
    json!({
        "formKind": "acroform",
        "needsAppearances": false,
        "fields": (1..=fields).map(|n| field(n, "")).collect::<Vec<_>>(),
        "widgets": (1..=fields).map(row).collect::<Vec<_>>(),
        "calculationOrder": [],
    })
}

fn indexed(fields: usize) -> Rc<FieldIndex> {
    Rc::new(index_fields(Rc::new(Snapshot::new(snapshot(fields))), Default::default()))
}

fn value_set(n: usize, value: &str) -> Value {
    json!({ "type": "forms.valueSet", "field": field(n, value), "widgets": [] })
}

fn set_value(n: usize, value: &str) -> Value {
    json!({ "type": "forms.setValue", "field": object_ref(n), "value": { "value": value } })
}

/// Run `body` on its own thread, failing past the ceiling.
fn within_ceiling(body: impl FnOnce() + Send + 'static) {
    let (done, finished) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        body();
        let _ = done.send(());
    });
    match finished.recv_timeout(CEILING) {
        Ok(()) => worker.join().expect("the run"),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            std::panic::resume_unwind(worker.join().expect_err("the run panicked"))
        }
        Err(mpsc::RecvTimeoutError::Timeout) => panic!("the run passed {CEILING:?}"),
    }
}

#[test]
fn a_fold_replaces_one_field_and_shares_the_rest() {
    let index = indexed(50);
    let before = snapshot_copies();
    let Folded::Index(next) = fold_form_event(&index, &value_set(7, "typed")) else {
        panic!("a value fold keeps the index");
    };
    assert_eq!(snapshot_copies(), before, "the fold copied the snapshot");
    for (at, (old, new)) in index.fields().iter().zip(next.fields()).enumerate() {
        assert_eq!(Rc::ptr_eq(old, new), at != 6, "field {}", at + 1);
    }
    let (old, new) = (index.snapshot.as_ref().unwrap(), next.snapshot.as_ref().unwrap());
    assert_eq!(old.widgets().as_ptr(), new.widgets().as_ptr(), "the widget rows are shared");
    assert_eq!(field_by_key(&next, "obj:7"), Some(&field(7, "typed")));
    assert_eq!(field_by_key(&index, "obj:7"), Some(&field(7, "")), "the old index keeps its field");
    // `{ ...snapshot, fields }` keeps the member order, and putting the
    // whole together once is the one copy.
    let whole = new.whole();
    assert_eq!(snapshot_copies(), before + 1);
    let keys: Vec<&String> = whole.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["formKind", "needsAppearances", "fields", "widgets", "calculationOrder"]);
    assert_eq!(whole["fields"][6], field(7, "typed"));
    assert!(Rc::ptr_eq(&whole, &new.whole()), "kept once put together");
    assert_eq!(snapshot_copies(), before + 1);
}

#[test]
fn a_large_form_folds_and_predicts_without_a_copy() {
    within_ceiling(|| {
        const FIELDS: usize = 5_000;
        let mut current = indexed(FIELDS);
        let untouched_field = current.fields()[FIELDS - 1].clone();
        let untouched_row = current.widgets()[FIELDS - 1].clone();
        let before = snapshot_copies();
        for i in 0..50 {
            let n = i * 25 % FIELDS + 1;
            current = predict_form_op(&current, &set_value(n, "predicted"));
            current = match fold_form_event(&current, &value_set(n, "confirmed")) {
                Folded::Index(next) => next,
                Folded::Reload => panic!("a value fold keeps the index"),
            };
        }
        assert_eq!(snapshot_copies(), before, "a fold or a prediction copied the snapshot");
        assert_eq!(field_by_key(&current, "obj:26"), Some(&field(26, "confirmed")));
        assert_eq!(current.fields().len(), FIELDS);
        // A copy the counter doesn't see would still leave new pointers.
        assert!(Rc::ptr_eq(&untouched_field, &current.fields()[FIELDS - 1]), "the field is shared");
        assert!(Rc::ptr_eq(&untouched_row, &current.widgets()[FIELDS - 1]), "the row is shared");
    });
}

fn page_row() -> Value {
    page_at(0, 1)
}

/// The page at `index` whose ref is object `n`.
fn page_at(index: usize, n: usize) -> Value {
    let size = json!({ "x": 0, "y": 0, "width": 600, "height": 800 });
    json!({
        "index": index, "ref": object_ref(n), "label": null,
        "size": { "width": 600, "height": 800 }, "rotation": 0, "userUnit": 1, "tabOrder": null,
        "pdfCropBox": { "left": 0, "bottom": 0, "right": 600, "top": 800 },
        "boxes": { "media": size, "crop": size, "bleed": size, "trim": size, "art": size }
    })
}

/// A kernel whose document has `fields` text fields and holds every write.
fn kernel_with(fields: usize) -> (Kernel, Rc<FakeEngine>) {
    kernel_over(snapshot(fields))
}

/// A kernel whose document has this form and holds every write.
fn kernel_over(form: Value) -> (Kernel, Rc<FakeEngine>) {
    kernel_on(form, vec![page_row()], false)
}

/// A kernel whose document has these pages and this form, holds every
/// write, and has the annotation plugin when `annotation` is set.
fn kernel_on(form: Value, pages: Vec<Value>, annotation: bool) -> (Kernel, Rc<FakeEngine>) {
    rt::reset();
    let fake = FakeEngine::new(json!({
        "documents": { "a": {
            "pages": pages,
            "security": { "allows": "*" },
            "responses": [
                { "path": "forms.list", "value": form },
                { "path": "annotations.list", "value": { "annotations": [] } },
                { "path": "apply", "defer": "applied" }
            ]
        } }
    }));
    let kernel = Kernel::new(
        Rc::new(AsEngine(fake.clone())),
        if annotation {
            vec![
                interaction_plugin(InteractionConfig::default()),
                annotation_plugin(None),
                form_plugin(None),
            ]
        } else {
            vec![interaction_plugin(InteractionConfig::default()), form_plugin(None)]
        },
        KernelOptions {
            report: None,
            on_dispatch: None,
            release_closed_sessions: false,
            printer: None,
        },
    )
    .expect("the kernel");
    let _ = kernel.start();
    rt::run_until_stalled();
    let _ = kernel.documents().open(
        OpenSource::Input(json!({ "kind": "bytes", "id": "a", "bytes": { "$bytes": "" } })),
        OpenOptions::default(),
    );
    rt::run_until_stalled();
    (kernel, fake)
}

/// Fills the engine has not answered, folds of what it did, and the reads
/// a control makes after each: no whole-snapshot copy among them.
#[test]
fn fills_folds_and_reads_share_the_live_form() {
    within_ceiling(|| {
        const FIELDS: usize = 500;
        let (kernel, fake) = kernel_with(FIELDS);
        let form = kernel.capability(&FORM, Some("a")).expect("the form capability");
        assert_eq!(form.list(None).len(), FIELDS, "the form is read");
        let page = object_ref(1);
        let untouched_field = form.view().fields()[FIELDS - 1].clone();
        let untouched_row = form.view().widgets()[FIELDS - 1].clone();
        let before = snapshot_copies();
        let mut pending = Vec::new();
        for i in 0..15 {
            let n = i * 25 + 1;
            pending.push(form.set_value(object_ref(n), json!({ "value": "typed" }), None));
            rt::run_until_stalled();
            assert_eq!(
                form.get_value(&object_ref(n)).as_deref(),
                Some(&json!({ "value": "typed" }))
            );
            assert_eq!(form.list_widgets(&page).len(), FIELDS);
            fake.emit("a", &value_set(n + 1, "elsewhere")).expect("the event");
            rt::run_until_stalled();
            assert_eq!(
                form.get_value(&object_ref(n + 1)).as_deref(),
                Some(&json!({ "value": "elsewhere" }))
            );
        }
        assert_eq!(snapshot_copies(), before, "a fill, fold or read copied the snapshot");
        // A copy the counter doesn't see would still leave new pointers.
        assert!(Rc::ptr_eq(&untouched_field, &form.view().fields()[FIELDS - 1]));
        assert!(Rc::ptr_eq(&untouched_row, &form.view().widgets()[FIELDS - 1]));
        // `getSnapshot` puts the form together once, and keeps it.
        let whole = form.get_snapshot().expect("the snapshot");
        assert!(Rc::ptr_eq(&whole, &form.get_snapshot().expect("again")));
        assert_eq!(snapshot_copies(), before + 1);
        drop(pending);
    });
}

#[test]
fn the_fill_projection_is_reference_stable() {
    let (kernel, _fake) = kernel_with(3);
    let form = kernel.capability(&FORM, Some("a")).expect("the form capability");
    let page = object_ref(1);
    let items = form.list_widgets(&page);
    assert_eq!(items.len(), 3);
    assert!(Rc::ptr_eq(&items, &form.list_widgets(&page)), "the same list while nothing changed");
    let _pending = form.set_value(object_ref(2), json!({ "value": "typed" }), None);
    rt::run_until_stalled();
    let after = form.list_widgets(&page);
    assert!(!Rc::ptr_eq(&items, &after), "a new list for a new form");
    assert_eq!(after[1]["value"], json!("typed"), "{}", after[1]);
    assert!(Rc::ptr_eq(&after, &form.list_widgets(&page)));
}

/// M38: a host that refetches the store's text (`epdf-web`'s `state_json`
/// and `slice_text`, `crates/epdf-web/src/exports.rs`) serialises the
/// fields mirror's `FieldIndex` once per confirmed form, however often it
/// asks and whatever else moves, and puts the snapshot together for none of
/// it: the text is written from the shared parts.
#[test]
fn a_host_refetching_the_store_serialises_each_confirmed_form_once() {
    use epdf_plugin_form::model::index_serialisations;
    within_ceiling(|| {
        const FIELDS: usize = 500;
        let (kernel, fake) = kernel_with(FIELDS);
        let form = kernel.capability(&FORM, Some("a")).expect("the form capability");
        let key = "form::a/mirror/fields";
        let first = kernel.state_text();
        let (copies, serialised) = (snapshot_copies(), index_serialisations());
        let mut confirmed = 0;
        let mut pending = Vec::new();
        for i in 0..10 {
            let n = i * 25 + 1;
            // A fill the engine has not answered moves the view, not the cell.
            pending.push(form.set_value(object_ref(n), json!({ "value": "typed" }), None));
            rt::run_until_stalled();
            let _ = kernel.state_text();
            fake.emit("a", &value_set(n + 1, "elsewhere")).expect("the event");
            rt::run_until_stalled();
            confirmed += 1;
            for _ in 0..3 {
                let _ = kernel.state_text();
                let _ = kernel.slice_text(key);
            }
        }
        assert_eq!(snapshot_copies(), copies, "a dump put the snapshot together");
        // A refresh shows the same form as loading, then the form it read.
        let refreshed = form.refresh(None);
        let _ = kernel.state_text();
        rt::run_until_stalled();
        assert!(matches!(refreshed.peek(), Some(Ok(()))));
        let _ = kernel.state_text();
        confirmed += 1;
        assert_eq!(index_serialisations(), serialised + confirmed, "one per confirmed form");
        // The read's `onResynced` puts it together once, as it always has.
        assert_eq!(snapshot_copies(), copies + 1, "a dump put the snapshot together");
        let text = kernel.state_text();
        assert_ne!(text, first);
        assert_eq!(&*text, kernel.state_json().to_string().as_str(), "the text is the JSON's");
        drop(pending);
    });
}

/// M36: the reads hand out the fields and widget rows the form holds, as
/// the TypeScript's do, and copy none of them (a port that copied made
/// 3,045 copies over these three rounds); the TypeScript's memoized reads
/// (`getValue`, `getWidgets`, `exportValues`, `validate`,
/// `getSelectedField`) answer the same object while their inputs stay the
/// same, so the second and third rounds compute nothing.
#[test]
fn reads_share_the_form_and_keep_their_answers() {
    use epdf_plugin_form::read::fields::read_computes;
    within_ceiling(|| {
        const FIELDS: usize = 500;
        let mut form_json = snapshot(FIELDS);
        for n in [3, 40, 400] {
            form_json["fields"][n - 1]["required"] = json!(true);
        }
        let (kernel, _fake) = kernel_over(form_json);
        let form = kernel.capability(&FORM, Some("a")).expect("the form capability");
        let view = form.view();
        let (fields, rows) = (view.fields(), view.widgets());
        let mut first_round = 0;
        for round in 0..3 {
            let before = read_computes();
            let all = form.list(None);
            assert_eq!(all.len(), FIELDS);
            assert!(all.iter().zip(fields).all(|(a, b)| Rc::ptr_eq(a, b)), "list shares");
            let text = form.list(Some(&json!({ "family": "text" })));
            assert!(text.iter().zip(fields).all(|(a, b)| Rc::ptr_eq(a, b)), "a filter shares");
            for n in [1, 250, 500] {
                let (field, row) = (&fields[n - 1], &rows[n - 1]);
                assert!(Rc::ptr_eq(&form.get(&object_ref(n)).unwrap(), field));
                assert!(Rc::ptr_eq(&form.get_field_for_widget(&widget_ref(n)).unwrap(), field));
                assert!(Rc::ptr_eq(&form.get_widget(&widget_ref(n)).unwrap(), row));
                let widgets = form.get_widgets(&object_ref(n));
                assert!(Rc::ptr_eq(&widgets[0], row));
                assert!(Rc::ptr_eq(&widgets, &form.get_widgets(&object_ref(n))));
                let value = form.get_value(&object_ref(n)).expect("a value");
                assert!(Rc::ptr_eq(&value, &form.get_value(&object_ref(n)).unwrap()));
            }
            let validation = form.validate();
            assert_eq!(validation.missing.len(), 3);
            assert!(Rc::ptr_eq(&validation.missing[0], &fields[2]));
            assert!(Rc::ptr_eq(&validation, &form.validate()));
            let exported = form.export_values();
            assert_eq!(exported.as_object().map(|o| o.len()), Some(FIELDS));
            assert!(Rc::ptr_eq(&exported, &form.export_values()));
            assert!(form.get_selected_field().is_none());
            let computed = read_computes() - before;
            if round == 0 {
                first_round = computed;
                assert!(computed > 0);
            } else {
                assert_eq!(computed, 0, "round {round} computed an answer again");
            }
        }
        // 3 values, 3 widget lists, the export, the validation, the selection.
        assert_eq!(first_round, 9);
    });
}

/// M36: `validate` keeps its answer while the document's record is the
/// same object, and builds a new one when the record changes, as
/// `memo(() => [fields.view(), ctx.document(), rights.key()])` does; the
/// record is compared by identity, not by its pages.
#[test]
fn validate_follows_the_documents_identity() {
    use epdf_plugin_form::read::fields::read_computes;
    let mut form_json = snapshot(3);
    form_json["fields"][1]["required"] = json!(true);
    let (kernel, _fake) = kernel_over(form_json);
    let form = kernel.capability(&FORM, Some("a")).expect("the form capability");
    let first = form.validate();
    let before = read_computes();
    assert!(Rc::ptr_eq(&first, &form.validate()), "the same record, the same answer");
    assert_eq!(read_computes(), before);
    kernel.documents().rename("a", "renamed.pdf");
    let renamed = form.validate();
    assert!(!Rc::ptr_eq(&first, &renamed), "a new record, a new answer");
    assert_eq!(*renamed, *first, "with the same contents");
    assert_eq!(read_computes(), before + 1);
    assert!(Rc::ptr_eq(&renamed, &form.validate()));
}

/// `validate()`'s order (`read/fields.ts:168-192`): the missing fields by
/// their first widget's page index, then its `rect.y`, then its `rect.x`,
/// whatever order the form lists them in.
#[test]
fn validate_lists_the_missing_fields_by_page_then_y_then_x() {
    let mut form_json = snapshot(4);
    let places = [(2, 0.0, 0.0), (1, 50.0, 10.0), (1, 10.0, 80.0), (1, 10.0, 20.0)];
    for (i, (page, y, x)) in places.into_iter().enumerate() {
        form_json["fields"][i]["required"] = json!(true);
        form_json["fields"][i]["widgets"][0]["page"] = object_ref(page);
        form_json["widgets"][i]["page"] = object_ref(page);
        form_json["widgets"][i]["rect"]["y"] = json!(y);
        form_json["widgets"][i]["rect"]["x"] = json!(x);
    }
    let (kernel, _fake) = kernel_on(form_json, vec![page_at(0, 1), page_at(1, 2)], false);
    let form = kernel.capability(&FORM, Some("a")).expect("the form capability");
    let validation = form.validate();
    let names: Vec<&str> = validation.missing.iter().map(|f| f["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["f4", "f3", "f2", "f1"]);
}

/// `widgetsByKey` (`read/fields.ts`) compares each row by identity: a row
/// the engine replaced makes a new list, and a widget with no row (`null`)
/// is the same as `null` again, so its list is kept.
#[test]
fn get_widgets_follows_each_rows_identity() {
    use epdf_plugin_form::read::fields::read_computes;
    let mut form_json = snapshot(2);
    // Field 2 also names a widget the form holds no row for.
    form_json["fields"][1]["widgets"].as_array_mut().unwrap().push(json!({
        "ref": widget_ref(99), "objectNumber": 10_099, "page": object_ref(1)
    }));
    let (kernel, fake) = kernel_over(form_json);
    let form = kernel.capability(&FORM, Some("a")).expect("the form capability");
    let before = read_computes();
    let rowless = form.get_widgets(&object_ref(2));
    assert_eq!(rowless.len(), 1, "a widget with no row lists nothing");
    assert!(Rc::ptr_eq(&rowless, &form.get_widgets(&object_ref(2))), "kept");
    assert_eq!(read_computes(), before + 1);
    let first = form.get_widgets(&object_ref(1));
    let mut moved = row(1);
    moved["rect"]["x"] = json!(300);
    fake.emit("a", &json!({ "type": "forms.widgetUpdated", "widget": moved })).unwrap();
    rt::run_until_stalled();
    let after = form.get_widgets(&object_ref(1));
    assert!(!Rc::ptr_eq(&first, &after), "a replaced row, a new list");
    assert_eq!(after[0]["rect"]["x"], 300);
}

/// `getSelectedField` is memoized on the form and the annotation plugin's
/// selection list, each by identity: with the annotation plugin there, the
/// same list answers again without computing.
#[test]
fn get_selected_field_keeps_its_answer_while_the_selection_list_is_the_same() {
    use epdf_plugin_form::read::fields::read_computes;
    let (kernel, _fake) = kernel_on(snapshot(2), vec![page_row()], true);
    let form = kernel.capability(&FORM, Some("a")).expect("the form capability");
    assert!(form.get_selected_field().is_none());
    let before = read_computes();
    assert!(form.get_selected_field().is_none());
    assert_eq!(read_computes(), before, "the same selection list, no new answer");
}
