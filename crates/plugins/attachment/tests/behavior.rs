//! The typed API beyond what the vectors print, against
//! `packages/plugin/attachment/src/{controller,model}.ts` and
//! `sync/files.ts`: a confirmed event updates the mirror and fires
//! `on_changed` before the write's promise settles, the permission gates
//! refuse before the engine is asked for anything, both optional plugins
//! may be absent, the reads keep their `Rc` until an input changes, and the
//! reads and writes settle on the TypeScript's turns.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use epdf_kernel::kernel::{OpenOptions, OpenSource};
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{AnyPlugin, Error, Kernel, KernelOptions, Promise, rt};
use epdf_plugin_annotation::annotation_plugin;
use epdf_plugin_attachment::*;
use epdf_plugin_interaction::{InteractionConfig, interaction_plugin};
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use plugin_conformance::scenario::install_case_entropy;
use serde_json::{Value, json};

fn key(name: &str) -> Value {
    json!({ "kind": "key", "key": name })
}

fn att(name: &str, index: usize) -> Value {
    json!({ "ref": key(name), "index": index, "name": name, "mimeType": null, "description": null,
            "size": null, "checksum": null, "createdAt": null, "modifiedAt": null })
}

fn read() -> Value {
    json!({ "path": "attachments.list", "value": { "attachments": [att("a.txt", 0), att("b.txt", 1)] } })
}

fn pr(n: u32) -> Value {
    json!({ "kind": "objectNumber", "objectNumber": n })
}

/// The n-th change's id under the case entropy.
fn op(n: u32) -> String {
    format!("00000000-0000-7000-8000-{n:012x}")
}

fn local(op_id: &str) -> Value {
    json!({ "kind": "local", "sessionId": "s1", "sub": null, "ts": 0, "serverId": null,
            "tx": { "id": op_id, "index": 0, "count": 1 } })
}

fn meta(op_id: &str, changed: Value) -> Value {
    json!({ "affectedPages": [], "cacheDelta": null, "opId": op_id, "undoable": true, "changed": changed })
}

/// The engine's answer to change `n`: its event, then its result.
fn answer(n: u32, item: Value, event: Value) -> Value {
    let id = op(n);
    let mut event = event;
    event["origin"] = local(&id);
    json!({
        "path": "apply", "times": 1,
        "value": { "items": [item], "meta": { "affectedPages": [], "cacheDelta": null, "opId": id, "undoable": true } },
        "events": [event],
    })
}

fn created(n: u32, name: &str) -> Value {
    let m = meta(&op(n), json!([key(name)]));
    answer(
        n,
        json!({ "type": "attachments.create", "attachment": att(name, 2), "meta": m }),
        json!({ "type": "attachments.created", "attachment": att(name, 2), "meta": m }),
    )
}

fn removed(n: u32, name: &str) -> Value {
    let m = meta(&op(n), json!([key(name)]));
    answer(
        n,
        json!({ "type": "attachments.delete", "meta": m }),
        json!({ "type": "attachments.deleted", "deleted": key(name), "meta": m }),
    )
}

fn refused() -> Value {
    json!({ "path": "apply", "times": 1, "error": { "name": "EngineError", "message": "no", "code": "Refused" } })
}

fn annotation_ref() -> Value {
    json!({ "kind": "objectNumber", "page": pr(1), "objectNumber": 20 })
}

/// A file attachment annotation on page 1.
fn file_annotation() -> Value {
    json!({
        "subtype": "file-attachment", "ref": annotation_ref(), "page": pr(1),
        "rect": { "x": 1, "y": 1, "width": 10, "height": 10 },
        "file": { "name": "f.png", "mimeType": "image/png", "description": null, "size": 1,
                  "checksum": null, "createdAt": null, "modifiedAt": null },
    })
}

fn annotations() -> Value {
    json!({ "path": "annotations.list", "value": { "annotations": [file_annotation()] } })
}

fn annotation_deleted(n: u32) -> Value {
    let id = op(n);
    let m = json!({ "affectedPages": [pr(1)], "cacheDelta": null, "opId": id, "undoable": true,
                    "changed": [annotation_ref()] });
    json!({
        "path": "apply", "times": 1,
        "value": { "items": [{ "type": "annotations.delete", "page": pr(1), "meta": m }],
                   "meta": { "affectedPages": [pr(1)], "cacheDelta": null, "opId": id, "undoable": true } },
        "events": [{ "type": "annotations.deleted", "page": pr(1), "deleted": [annotation_ref()],
                     "meta": m, "origin": local(&id) }],
    })
}

fn layout() -> Value {
    json!({ "index": 0, "ref": pr(1), "label": null, "size": { "width": 600, "height": 800 },
            "rotation": 0, "userUnit": 1, "tabOrder": null,
            "pdfCropBox": { "left": 0, "bottom": 0, "right": 600, "top": 800 }, "boxes": {} })
}

struct Rig {
    kernel: Kernel,
    fake: Rc<FakeEngine>,
    attachment: AttachmentCapability,
    _entropy: plugin_conformance::scenario::CaseEntropy,
}

impl Rig {
    fn with(responses: Vec<Value>, allows: Value, with_annotations: bool) -> Rig {
        rt::reset();
        let entropy = install_case_entropy();
        let fake = FakeEngine::new(json!({ "documents": { "a": {
            "pages": [layout()],
            "security": { "allows": allows },
            "objectNumbers": [500],
            "responses": responses,
        } } }));
        let mut plugins: Vec<AnyPlugin> = vec![attachment_plugin()];
        if with_annotations {
            plugins.push(interaction_plugin(InteractionConfig::default()));
            plugins.push(annotation_plugin(None));
        }
        let kernel =
            Kernel::new(Rc::new(AsEngine(fake.clone())), plugins, KernelOptions::default())
                .expect("kernel");
        kernel.start();
        rt::run_until_stalled();
        kernel.documents().open(
            OpenSource::Input(json!({ "kind": "bytes", "id": "a", "bytes": { "$bytes": "" } })),
            OpenOptions::default(),
        );
        rt::run_until_stalled();
        let attachment = kernel.capability(&ATTACHMENT, Some("a")).expect("attachment");
        Rig { kernel, fake, attachment, _entropy: entropy }
    }

    fn new(responses: Vec<Value>) -> Rig {
        Rig::with(responses, json!("*"), false)
    }

    fn loaded(responses: Vec<Value>) -> Rig {
        let rig = Rig::new(responses);
        settled(&rig.attachment.ensure_loaded(none())).expect("settled").expect("loaded");
        rig
    }

    fn calls(&self, path: &str) -> usize {
        self.fake.log_since(0).iter().filter(|entry| entry["path"] == path).count()
    }
}

fn settled<T: Clone + 'static>(promise: &Promise<T>) -> Option<Result<T, Error>> {
    rt::run_until_stalled();
    promise.peek()
}

fn none() -> OperationOptions {
    OperationOptions::default()
}

fn aborted() -> OperationOptions {
    let signal = AbortSignal::new();
    signal.abort(Error::named("AbortError", "This operation was aborted"));
    OperationOptions { signal: Some(signal) }
}

/// The list's file for an attachment named `name`.
fn doc_file(name: &str) -> Value {
    json!({ "name": name, "mimeType": null, "description": null, "size": null, "checksum": null,
            "createdAt": null, "modifiedAt": null, "id": format!("doc:{name}"),
            "home": { "kind": "document", "ref": key(name) } })
}

fn page_file() -> Value {
    json!({ "name": "f.png", "mimeType": "image/png", "id": "page:obj:20",
            "home": { "kind": "page", "ref": annotation_ref() } })
}

fn ids(files: &[Value]) -> Vec<&str> {
    files.iter().map(|f| f["id"].as_str().unwrap()).collect()
}

#[test]
fn the_list_and_on_changed_lead_the_writes_promise() {
    let rig = Rig::loaded(vec![read(), created(1, "c.txt")]);
    let order: Rc<RefCell<Vec<String>>> = Rc::default();
    let (log, cap) = (order.clone(), rig.attachment.clone());
    let _off = rig.attachment.on_changed(move |event| {
        // The mirror already holds the confirmed file when the event fires.
        let listed = ids(&cap.list()).join(",");
        log.borrow_mut()
            .push(format!("changed {} {listed} {}", event.kind, event.origin["tx"]["id"]));
        Ok(())
    });
    let adding = rig.attachment.add(vec![file_source(b"hi", "c.txt")], none());
    let log = order.clone();
    adding.on_settle(move |result| log.borrow_mut().push(format!("settled {}", result.is_ok())));
    rt::run_until_stalled();
    assert_eq!(
        *order.borrow(),
        [
            format!("changed added doc:a.txt,doc:b.txt,doc:c.txt \"{}\"", op(1)),
            "settled true".to_string()
        ]
    );
    assert_eq!(ids(&adding.peek().unwrap().unwrap()), ["doc:c.txt"]);
    assert!(!rig.attachment.is_pending(&doc_file("c.txt")));
}

#[test]
fn an_add_shows_at_once_and_goes_when_the_engine_refuses() {
    let deferred = json!({ "path": "apply", "times": 1, "defer": "x" });
    let rig = Rig::loaded(vec![read(), deferred]);
    let heard = Rc::new(Cell::new(0));
    let count = heard.clone();
    let _off = rig.attachment.on_changed(move |_| {
        count.set(count.get() + 1);
        Ok(())
    });
    let adding = rig.attachment.add(vec![file_source(b"x", "a.txt")], none());
    rt::run_until_stalled();
    assert_eq!(rig.calls("apply"), 1);
    let shown = rig.attachment.list();
    assert_eq!(ids(&shown), ["doc:a (2).txt", "doc:a.txt", "doc:b.txt"]);
    assert_eq!(shown[0]["size"], 1);
    assert!(rig.attachment.is_pending(&shown[0]));
    let refusal = json!({ "name": "EngineError", "message": "no", "code": "Refused" });
    assert!(rig.fake.settle("x", Err(&refusal)));
    let error = settled(&adding).unwrap().unwrap_err();
    assert!(error.is_plugin_error(Some("operation-failed")));
    assert_eq!(ids(&rig.attachment.list()), ["doc:a.txt", "doc:b.txt"]);
    assert_eq!(heard.get(), 0, "a refusal confirms nothing");
}

#[test]
fn the_permission_gates_refuse_before_the_engine() {
    // Without doc.attachments.modify: no read, no apply.
    let rig = Rig::with(vec![read()], json!(["doc.open", "doc.download"]), false);
    assert!(!rig.attachment.can_add());
    assert!(!rig.attachment.can_remove(&doc_file("a.txt")));
    let add = rig.attachment.add(vec![file_source(b"x", "x.txt")], none());
    assert!(settled(&add).unwrap().unwrap_err().is_plugin_error(Some("permission-denied")));
    let remove = rig.attachment.remove(doc_file("a.txt"), none());
    assert!(settled(&remove).unwrap().unwrap_err().is_plugin_error(Some("permission-denied")));
    assert_eq!((rig.calls("attachments.list"), rig.calls("apply")), (0, 0));

    // Without doc.download: no download call.
    let rig = Rig::with(vec![read()], json!(["doc.open", "doc.attachments.modify"]), false);
    assert!(!rig.attachment.can_download());
    let download = rig.attachment.download(doc_file("a.txt"), none());
    let error = settled(&download).unwrap().unwrap_err();
    assert!(error.is_plugin_error(Some("permission-denied")));
    assert_eq!(rig.calls("attachments.download"), 0);

    // A signal that already fired: nothing starts.
    let rig = Rig::loaded(vec![read()]);
    for promise in [
        rig.attachment.add(vec![file_source(b"x", "x.txt")], aborted()).convert(|_| Ok(())),
        rig.attachment.remove(doc_file("a.txt"), aborted()),
        rig.attachment.download(doc_file("a.txt"), aborted()).convert(|_| Ok(())),
    ] {
        assert!(
            settled(&promise).unwrap().unwrap_err().is_plugin_error(Some("operation-cancelled"))
        );
    }
    let unknown = rig.attachment.remove(doc_file("zzz"), none());
    assert!(settled(&unknown).unwrap().unwrap_err().is_plugin_error(Some("not-found")));
    let nameless = rig.attachment.add(vec![json!({ "data": { "$bytes": "eA==" } })], none());
    assert!(settled(&nameless).unwrap().unwrap_err().is_plugin_error(Some("invalid-input")));
    assert_eq!((rig.calls("apply"), rig.calls("attachments.download")), (0, 0));
}

#[test]
fn no_optional_plugin_is_needed() {
    let rig = Rig::loaded(vec![read()]);
    // No annotation plugin: the attachments alone, and a page's file is
    // never pending, can't be removed, and can't be saved.
    assert_eq!(ids(&rig.attachment.list()), ["doc:a.txt", "doc:b.txt"]);
    assert!(!rig.attachment.is_pending(&page_file()));
    assert!(!rig.attachment.can_remove(&page_file()));
    let remove = rig.attachment.remove(page_file(), none());
    let error = settled(&remove).unwrap().unwrap_err();
    assert!(error.is_plugin_error(Some("not-found")));
    assert_eq!(error.message, "attachment.remove: no annotation plugin");
    let download = rig.attachment.download(page_file(), none());
    assert!(settled(&download).unwrap().unwrap_err().is_plugin_error(Some("not-found")));
    // Nor the stage: reveal does nothing, and throws nothing.
    rig.attachment.reveal(&page_file());

    // With annotations and no stage: reveal selects the annotation.
    let rig = Rig::with(vec![annotations(), read()], json!("*"), true);
    rig.attachment.reveal(&page_file());
    let annotation = rig.kernel.capability(&epdf_plugin_annotation::ANNOTATION, Some("a")).unwrap();
    let selected = annotation.list_selected();
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0]["ref"], annotation_ref());
}

#[test]
fn the_reads_keep_their_rc_until_an_input_changes() {
    let rig = Rig::with(vec![annotations(), read(), created(1, "c.txt")], json!("*"), true);
    let a = &rig.attachment;
    // The pages' files list before the attachments are read.
    let before = a.list();
    assert_eq!(ids(&before), ["page:obj:20"]);
    assert!(Rc::ptr_eq(&before, &a.list()));
    settled(&a.ensure_loaded(none())).unwrap().unwrap();
    let (list, chosen) = (a.list(), a.get_selected());
    assert!(!Rc::ptr_eq(&before, &list), "the read is an input of the list");
    assert!(Rc::ptr_eq(&list, &a.list()));
    assert!(Rc::ptr_eq(&chosen, &a.get_selected()));

    // The selection is an input of `get_selected` only.
    a.select(&[doc_file("b.txt")]);
    assert!(Rc::ptr_eq(&list, &a.list()));
    assert!(!Rc::ptr_eq(&chosen, &a.get_selected()));
    let chosen = a.get_selected();
    assert_eq!(ids(&chosen), ["doc:b.txt"]);
    // The same ids keep the state, and so the read.
    a.select(&[doc_file("b.txt")]);
    assert!(Rc::ptr_eq(&chosen, &a.get_selected()));

    // A write changes the view, every read's input.
    settled(&a.add(vec![file_source(b"c", "c.txt")], none())).unwrap().unwrap();
    assert!(!Rc::ptr_eq(&list, &a.list()));
    assert!(!Rc::ptr_eq(&chosen, &a.get_selected()));
    assert_eq!(ids(&a.get_selected()), ["doc:b.txt"]);
    let (list, chosen) = (a.list(), a.get_selected());

    // A page's file the annotations gained is an input of the list, and
    // through it of `get_selected`.
    let mut late = file_annotation();
    late["ref"]["objectNumber"] = json!(21);
    late["file"]["name"] = json!("late.txt");
    let event = json!({
        "type": "annotations.created", "page": pr(1), "annotation": late,
        "origin": { "kind": "remote", "sessionId": "s2", "sub": "u-9", "ts": 0, "serverId": null },
        "meta": { "affectedPages": [pr(1)], "cacheDelta": null, "changed": [late["ref"].clone()],
                  "opId": "remote-create", "undoable": true },
    });
    rig.fake.emit("a", &event).unwrap();
    rt::run_until_stalled();
    assert_eq!(
        ids(&a.list()),
        ["doc:a.txt", "doc:b.txt", "doc:c.txt", "page:obj:20", "page:obj:21"]
    );
    assert!(!Rc::ptr_eq(&list, &a.list()));
    assert!(!Rc::ptr_eq(&chosen, &a.get_selected()));
    assert!(Rc::ptr_eq(&a.list(), &a.list()));
    assert!(Rc::ptr_eq(&a.get_selected(), &a.get_selected()));
}

/// A counter that moves once per turn of the queue, as
/// `(async () => { for (;;) { await undefined; turn++ } })()` does.
fn turn_counter() -> Rc<Cell<u32>> {
    let turn = Rc::new(Cell::new(0));
    let count = turn.clone();
    rt::spawn(async move {
        for _ in 0..64 {
            rt::tick().await;
            count.set(count.get() + 1);
        }
    });
    turn
}

/// `promise.then(() => at = turn, () => at = turn)`: the turn its reaction
/// runs on.
fn reacts_on<T: Clone + 'static>(promise: Promise<T>, turn: &Rc<Cell<u32>>) -> Rc<Cell<u32>> {
    let at = Rc::new(Cell::new(u32::MAX));
    let (seen, turn) = (at.clone(), turn.clone());
    rt::spawn(async move {
        let _ = promise.await;
        seen.set(turn.get());
    });
    at
}

fn heard_on<E: 'static>(
    subscribe: impl FnOnce(Box<dyn Fn(&E) -> Result<(), Error>>) -> epdf_kernel::Unsubscribe,
    turn: &Rc<Cell<u32>>,
) -> (Rc<Cell<u32>>, epdf_kernel::Unsubscribe) {
    let at = Rc::new(Cell::new(u32::MAX));
    let (seen, turn) = (at.clone(), turn.clone());
    let off = subscribe(Box::new(move |_| {
        seen.set(turn.get());
        Ok(())
    }));
    (at, off)
}

/// The turns were measured on the TypeScript: the real kernel and
/// attachment plugin (with the interaction and annotation plugins for a
/// page's file) over the conformance fake engine (`createFakeEngine`, whose
/// answers are already settled, with the case entropy), with the counter
/// above and `promise.then(() => at = turn)`, under `tsx
/// --conditions=development`. A fresh `ensureLoaded` reacts on 10 (its
/// `onResynced` on 5), a ready one on 1, `refresh` after a load on 10. An
/// `add` of one file reacts on 5 (its `onChanged` on 1), `add([])` on 1, a
/// nameless file rejects on 2, a cancelled add on 1, and an add before the
/// first read on 14 (its `onChanged` on 10). `remove` of an attachment
/// reacts on 4 (`onChanged` on 0), of a key the list lacks on 1, a refused
/// one on 4; of a page's file, through the annotation plugin, on 7, and
/// without the annotation plugin on 1. `download` of an attachment reacts
/// on 5, a cancelled one on 1. A page's file has its own test below.
#[test]
fn reads_and_writes_settle_on_the_typescripts_turns() {
    let rig = Rig::new(vec![read()]);
    let turn = turn_counter();
    let (resynced, _off) = heard_on(|l| rig.attachment.on_resynced(l), &turn);
    let fresh = reacts_on(rig.attachment.ensure_loaded(none()), &turn);
    rt::run_until_stalled();
    assert_eq!((fresh.get(), resynced.get()), (10, 5));
    let turn = turn_counter();
    let ready = reacts_on(rig.attachment.ensure_loaded(none()), &turn);
    rt::run_until_stalled();
    assert_eq!(ready.get(), 1);
    let turn = turn_counter();
    let refreshed = reacts_on(rig.attachment.refresh(none()), &turn);
    rt::run_until_stalled();
    assert_eq!(refreshed.get(), 10);

    let rig = Rig::loaded(vec![read(), created(1, "c.txt")]);
    let turn = turn_counter();
    let (changed, _off) = heard_on(|l| rig.attachment.on_changed(l), &turn);
    let added = reacts_on(rig.attachment.add(vec![file_source(b"hi", "c.txt")], none()), &turn);
    rt::run_until_stalled();
    assert_eq!((added.get(), changed.get()), (5, 1));
    let turn = turn_counter();
    let nothing = reacts_on(rig.attachment.add(Vec::new(), none()), &turn);
    rt::run_until_stalled();
    assert_eq!(nothing.get(), 1);
    let turn = turn_counter();
    let nameless =
        reacts_on(rig.attachment.add(vec![json!({ "data": { "$bytes": "eA==" } })], none()), &turn);
    rt::run_until_stalled();
    assert_eq!(nameless.get(), 2);
    let turn = turn_counter();
    let cancelled = reacts_on(rig.attachment.add(vec![file_source(b"x", "x")], aborted()), &turn);
    rt::run_until_stalled();
    assert_eq!(cancelled.get(), 1);

    let rig = Rig::new(vec![read(), created(1, "c.txt")]);
    let turn = turn_counter();
    let (changed, _off) = heard_on(|l| rig.attachment.on_changed(l), &turn);
    let early = reacts_on(rig.attachment.add(vec![file_source(b"hi", "c.txt")], none()), &turn);
    rt::run_until_stalled();
    assert_eq!((early.get(), changed.get()), (14, 10));

    let rig = Rig::loaded(vec![read(), removed(1, "a.txt"), refused()]);
    let turn = turn_counter();
    let (changed, _off) = heard_on(|l| rig.attachment.on_changed(l), &turn);
    let gone = reacts_on(rig.attachment.remove(doc_file("a.txt"), none()), &turn);
    rt::run_until_stalled();
    assert_eq!((gone.get(), changed.get()), (4, 0));
    let turn = turn_counter();
    let unknown = reacts_on(rig.attachment.remove(doc_file("zzz"), none()), &turn);
    rt::run_until_stalled();
    assert_eq!(unknown.get(), 1);
    let turn = turn_counter();
    let refusal = reacts_on(rig.attachment.remove(doc_file("b.txt"), none()), &turn);
    rt::run_until_stalled();
    assert_eq!(refusal.get(), 4);

    let bytes = json!({ "path": "attachments.download", "value": { "$bytes": "Bw==" } });
    let rig = Rig::loaded(vec![read(), bytes]);
    let turn = turn_counter();
    let downloaded = reacts_on(rig.attachment.download(doc_file("a.txt"), none()), &turn);
    rt::run_until_stalled();
    assert_eq!(downloaded.get(), 5);
    let turn = turn_counter();
    let cancelled = reacts_on(rig.attachment.download(doc_file("a.txt"), aborted()), &turn);
    rt::run_until_stalled();
    assert_eq!(cancelled.get(), 1);

    let resource =
        json!({ "path": "page.annotations.downloadResource", "value": { "$bytes": "CQ==" } });
    let rig =
        Rig::with(vec![annotations(), read(), annotation_deleted(1), resource], json!("*"), true);
    settled(&rig.attachment.ensure_loaded(none())).unwrap().unwrap();
    let turn = turn_counter();
    let deleted = reacts_on(rig.attachment.remove(page_file(), none()), &turn);
    rt::run_until_stalled();
    assert_eq!(deleted.get(), 7);
    assert_eq!(ids(&rig.attachment.list()), ["doc:a.txt", "doc:b.txt"]);

    let rig = Rig::loaded(vec![read()]);
    let turn = turn_counter();
    let missing = reacts_on(rig.attachment.remove(page_file(), none()), &turn);
    rt::run_until_stalled();
    assert_eq!(missing.get(), 1);
}

/// Measured as above: `download` of a page's file reacts on 6, and the
/// annotation plugin's own `downloadResource` on 5. `downloadResource` is an
/// `async` function that returns the engine call
/// (`packages/plugin/annotation/src/write/crud.ts:197-208`), so its promise
/// adopts the call's, one turn after it. The Rust `download_resource` takes
/// that turn since `89e45135d`; this test pins it from the attachment side.
#[test]
fn a_page_files_download_settles_on_the_typescripts_turn() {
    let resource =
        json!({ "path": "page.annotations.downloadResource", "value": { "$bytes": "CQ==" } });
    let rig = Rig::with(vec![annotations(), read(), resource], json!("*"), true);
    settled(&rig.attachment.ensure_loaded(none())).unwrap().unwrap();
    let turn = turn_counter();
    let download = rig.attachment.download(page_file(), none());
    let saved = reacts_on(download.clone(), &turn);
    rt::run_until_stalled();
    let content = download.peek().unwrap().unwrap();
    assert_eq!(content.data(), Some(vec![9]));
    assert_eq!((content.name.as_str(), &content.mime_type), ("f.png", &json!("image/png")));
    assert_eq!(rig.calls("page.annotations.downloadResource"), 1);
    assert_eq!(saved.get(), 6);
}
