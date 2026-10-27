use epdf_kernel::{
    Error, Kernel, KernelOptions, OpenOptions, OpenSource, Promise,
    action::CoreAction,
    define,
    engine::{DocumentHandle, DocumentSecurity, Engine, EngineCall},
    event_hook::EventHookSource,
    rt,
    store::Unsubscribe,
    token::Token,
    types::DocStatus,
};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
struct Security(Cell<bool>);
impl DocumentSecurity for Security {
    fn allows(&self, _: &str) -> bool {
        !self.0.get()
    }
    fn allows_annotation_create(&self) -> bool {
        !self.0.get()
    }
    fn allows_annotation_mutation(&self, _: &str, _: &Value) -> bool {
        !self.0.get()
    }
    fn allows_annotation_group_assignment(&self, _: &str) -> bool {
        !self.0.get()
    }
    fn effective_scope(&self) -> Vec<String> {
        vec![]
    }
    fn identity(&self) -> Value {
        Value::Null
    }
    fn current(&self) -> Value {
        json!({"locked":self.0.get()})
    }
    fn password_prompt(&self) -> Value {
        if self.0.get() {
            json!({"state":"required","incorrect":true})
        } else {
            json!({"state":"none"})
        }
    }
    fn unlock(&self, input: Value) -> EngineCall<Value> {
        if input["password"] != "right" {
            Promise::rejected(Error::engine("DocPasswordIncorrect", "wrong"))
        } else {
            self.0.set(false);
            Promise::resolved(self.current())
        }
    }
}
struct Doc {
    id: RefCell<String>,
    local: Cell<bool>,
    layer: Cell<bool>,
    page_answer: RefCell<Option<Promise<Value>>>,
    apply_answer: RefCell<Option<Promise<Value>>>,
    download_answer: RefCell<Option<Promise<Value>>>,
    calls: RefCell<Vec<String>>,
    security: Rc<Security>,
    events: EventHookSource<Value>,
    closed: Cell<u32>,
    pages: Cell<u32>,
    next: Cell<u32>,
}
impl DocumentHandle for Doc {
    fn id(&self) -> String {
        self.id.borrow().clone()
    }
    fn capabilities(&self) -> Value {
        json!({})
    }
    fn security(&self) -> Rc<dyn DocumentSecurity> {
        self.security.clone()
    }
    fn is_local(&self) -> bool {
        self.local.get()
    }
    fn has(&self, member: &str) -> bool {
        member == "downloadLayer" && self.layer.get()
    }
    fn call(&self, path: &str, _: Option<u32>, args: Vec<Value>) -> EngineCall<Value> {
        self.calls.borrow_mut().push(path.into());
        match path {
            "pages.list" => {
                self.pages.set(self.pages.get() + 1);
                if let Some(answer) = self.page_answer.borrow().clone() {
                    return answer;
                }
                Promise::resolved(
                    json!({"pageCount":1,"pages":[{"ref":{"kind":"objectNumber","objectNumber":3},"boxes":{"crop":{"left":0,"right":100,"top":100,"bottom":0}}}]}),
                )
            }
            "close" => {
                self.closed.set(self.closed.get() + 1);
                Promise::resolved(Value::Null)
            }
            "apply" => {
                assert!(args[0].get("ops").is_some());
                if let Some(answer) = self.apply_answer.borrow().clone() {
                    return answer;
                }
                self.events.emit(&json!({"type":"metadata.changed","value":9,"origin":{"kind":"local","tx":{"id":args[1]["opId"],"index":0,"count":1}}}));
                Promise::resolved(json!({"applied":true}))
            }
            "download" | "downloadLayer" => self
                .download_answer
                .borrow()
                .clone()
                .unwrap_or_else(|| Promise::resolved(json!({"fileOf":self.id()}))),
            _ => Promise::rejected(Error::engine(
                "NotImplemented",
                format!("unexpected call {path}"),
            )),
        }
    }
    fn page_index(&self, number: u32) -> Result<u32, Error> {
        if number == 3 { Ok(0) } else { Err(Error::engine("NotFound", "page")) }
    }
    fn subscribe(&self, listener: Rc<dyn Fn(&Value)>) -> Unsubscribe {
        self.events.on(move |v| {
            listener(v);
            Ok(())
        })
    }
    fn take_object_number(&self) -> Result<Option<u32>, Error> {
        let n = self.next.get();
        self.next.set(n + 1);
        Ok(Some(n))
    }
    fn reserve_object_numbers(&self, _: u32) -> EngineCall<()> {
        Promise::resolved(())
    }
}
struct TestEngine(Rc<Doc>);
impl Engine for TestEngine {
    fn open(&self, _: Value, _: Option<Value>) -> EngineCall<Rc<dyn DocumentHandle>> {
        Promise::resolved(self.0.clone() as Rc<dyn DocumentHandle>)
    }
    fn destroy(&self) -> EngineCall<()> {
        Promise::resolved(())
    }
}
fn doc(locked: bool) -> Rc<Doc> {
    Rc::new(Doc {
        id: RefCell::new("doc".into()),
        local: Cell::new(false),
        layer: Cell::new(false),
        page_answer: RefCell::new(None),
        apply_answer: RefCell::new(None),
        download_answer: RefCell::new(None),
        calls: RefCell::new(vec![]),
        security: Rc::new(Security(Cell::new(locked))),
        events: EventHookSource::new(None),
        closed: Cell::new(0),
        pages: Cell::new(0),
        next: Cell::new(100),
    })
}
fn options() -> KernelOptions {
    KernelOptions {
        report: Some(Rc::new(|e| panic!("unexpected report: {e}"))),
        ..Default::default()
    }
}
fn run<T: Clone + 'static>(p: Promise<T>) -> Result<T, Error> {
    rt::run_until_stalled();
    p.peek().expect("operation must settle")
}
fn input() -> OpenSource {
    OpenSource::Input(json!({"id":"doc"}))
}
#[test]
fn failed_create_or_connect_rolls_back_every_acquisition_without_publishing_ready() {
    for connect in [false, true] {
        rt::reset();
        let document = doc(false);
        let disposed = Rc::new(Cell::new(0));
        let count = disposed.clone();
        let token = Token::<()>::new("test");
        let plugin = define::<Value, CoreAction>("test")
            .document_scoped()
            .create(&token, move |ctx| {
                let count = count.clone();
                ctx.defer(Box::new(move || {
                    count.set(count.get() + 1);
                    Ok(None)
                }));
                if connect {
                    Ok(((), Some(Rc::new(|| Err(Error::new("connect failed"))))))
                } else {
                    Err(Error::new("create failed"))
                }
            })
            .build();
        let kernel =
            Kernel::new(Rc::new(TestEngine(document.clone())), vec![plugin], options()).unwrap();
        run(kernel.start()).unwrap();
        let opened = run(kernel.documents().open(input(), OpenOptions::default()));
        assert!(opened.is_err());
        assert_eq!(disposed.get(), 1);
        assert_eq!(document.closed.get(), 1);
        assert_eq!(kernel.documents().get("doc").unwrap().status, DocStatus::Error);
        assert!(kernel.try_capability(&token, Some("doc")).unwrap().is_none());
        run(kernel.destroy()).unwrap();
    }
}
#[test]
fn wrong_password_parks_open_then_unlock_refreshes_security() {
    rt::reset();
    let document = doc(true);
    let token = Token::<bool>::new("access");
    let plugin = define::<Value, CoreAction>("access")
        .document_scoped()
        .create(&token, |ctx| Ok((ctx.doc()?.security().allows("doc.read"), None)))
        .build();
    let kernel =
        Kernel::new(Rc::new(TestEngine(document.clone())), vec![plugin], options()).unwrap();
    run(kernel.start()).unwrap();
    let opened = run(kernel.documents().open(input(), OpenOptions::default())).unwrap();
    assert_eq!(opened["document"]["status"], "locked");
    assert_eq!(kernel.documents().get("doc").unwrap().password_provided, Some(true));
    assert_eq!(document.pages.get(), 0);
    assert!(run(kernel.documents().unlock("doc", "wrong")).is_err());
    assert_eq!(kernel.documents().get("doc").unwrap().status, DocStatus::Locked);
    assert_eq!(document.closed.get(), 0);
    run(kernel.documents().unlock("doc", "right")).unwrap();
    assert_eq!(kernel.try_capability(&token, Some("doc")).unwrap(), Some(true));
    assert_eq!(document.pages.get(), 1);
    run(kernel.destroy()).unwrap();
}
#[test]
fn unlock_with_signal_matches_unlock_errors_and_ts_settle_turns() {
    fn turns(p: &Promise<()>) -> usize {
        let count = Rc::new(Cell::new(0));
        let (watched, counter) = (p.clone(), count.clone());
        rt::spawn(async move {
            while !watched.is_settled() {
                counter.set(counter.get() + 1);
                rt::tick().await;
            }
        });
        rt::run_until_stalled();
        count.get()
    }
    rt::reset();
    let kernel = Kernel::new(Rc::new(TestEngine(doc(true))), vec![], options()).unwrap();
    run(kernel.start()).unwrap();
    run(kernel.documents().open(input(), OpenOptions::default())).unwrap();
    let plain = kernel.documents().unlock("doc", "wrong");
    let plain_turns = turns(&plain);
    let signal = epdf_kernel::signal::AbortSignal::new();
    let signalled = kernel.documents().unlock_with_signal("doc", "wrong", signal.clone());
    // TS `cancellable` re-settles a signalled call from `task.then`
    // (cancellable.ts:31), one turn later than the bare call.
    assert_eq!(turns(&signalled), plain_turns + 1);
    assert_eq!(
        signalled.peek().unwrap().unwrap_err().to_canonical(),
        plain.peek().unwrap().unwrap_err().to_canonical()
    );
    run(kernel.destroy()).unwrap();
    let plain = run(kernel.documents().unlock("doc", "right")).unwrap_err();
    let signalled = run(kernel.documents().unlock_with_signal("doc", "right", signal)).unwrap_err();
    assert_eq!(signalled.to_canonical(), plain.to_canonical());
}
#[test]
fn close_revokes_retained_state_mirror_and_pending_change() {
    use epdf_kernel::{
        changes::StagedChange,
        mirror::{Fold, Loaded, MirrorSpec},
        plugin::Controller,
    };
    rt::reset();
    let document = doc(false);
    let retained: Rc<RefCell<Option<Controller<Value, CoreAction>>>> = Rc::default();
    let save = retained.clone();
    let token = Token::<()>::new("retained");
    let plugin = define::<Value, CoreAction>("retained")
        .document_scoped()
        .initial_state(|| json!({"n":0}))
        .create(&token, move |ctx| {
            *save.borrow_mut() = Some(ctx);
            Ok(((), None))
        })
        .build();
    let reports = Rc::new(RefCell::new(Vec::<Error>::new()));
    let sink = reports.clone();
    let opts = KernelOptions {
        report: Some(Rc::new(move |e| sink.borrow_mut().push(e.clone()))),
        ..Default::default()
    };
    let kernel = Kernel::new(Rc::new(TestEngine(document.clone())), vec![plugin], opts).unwrap();
    run(kernel.start()).unwrap();
    run(kernel.documents().open(input(), OpenOptions::default())).unwrap();
    let ctx = retained.borrow().clone().unwrap();
    assert!(ctx.update_state(|_| Rc::new(json!({"n":1}))));
    let mirror = ctx
        .mirror(MirrorSpec::new(
            "late",
            json!(0),
            Rc::new(|_, _| Promise::resolved(Loaded { value: json!(0), cursor: None })),
            Rc::new(|_, e| Ok(Fold::Value(Rc::new(e["value"].clone())))),
        ))
        .unwrap();
    let changes = ctx.changes().unwrap();
    assert_eq!(changes.take_object_number().unwrap(), Some(100));
    let held = changes.hold(json!({"key":"edit"}), None).unwrap();
    held.set(vec![json!({"type":"metadata.set"})], vec![]);
    let pending = held.change().unwrap();
    run(kernel.documents().close("doc")).unwrap();
    assert!(!ctx.update_state(|_| Rc::new(json!({"n":2}))));
    assert_eq!(*ctx.state(), json!({"n":1}));
    assert_eq!(pending.result().peek().unwrap().unwrap_err().code(), Some("instance-closed"));
    assert!(!changes.has_pending());
    assert!(
        changes.stage(StagedChange::ops(json!({"key":"x"}), vec![json!({"type":"x"})])).is_err()
    );
    document.events.emit(&json!({"value":7}));
    assert_eq!(*mirror.get(), json!(0));
    assert!(!ctx.update_state(|same| same));
    assert_eq!(reports.borrow().len(), 1);
    assert_eq!(reports.borrow()[0].code(), Some("instance-closed"));
    assert!(reports.borrow()[0].message.contains("doc#1"));
    run(kernel.destroy()).unwrap();
}

struct QueueEngine(RefCell<std::collections::VecDeque<Rc<Doc>>>);
impl Engine for QueueEngine {
    fn open(&self, _: Value, _: Option<Value>) -> EngineCall<Rc<dyn DocumentHandle>> {
        Promise::resolved(
            self.0.borrow_mut().pop_front().expect("expected open") as Rc<dyn DocumentHandle>
        )
    }
    fn destroy(&self) -> EngineCall<()> {
        Promise::resolved(())
    }
}

#[test]
fn cancel_after_rekey_closes_original_instance_not_new_occupant_of_old_id() {
    use epdf_kernel::signal::AbortSignal;
    rt::reset();
    let first = doc(false);
    *first.id.borrow_mut() = "renamed".into();
    let second = doc(false);
    let (snapshot, _land) = Promise::pending();
    *first.page_answer.borrow_mut() = Some(snapshot);
    let kernel = Kernel::new(
        Rc::new(QueueEngine(RefCell::new([first.clone(), second.clone()].into()))),
        vec![],
        options(),
    )
    .unwrap();
    run(kernel.start()).unwrap();
    let signal = AbortSignal::new();
    let opening =
        kernel.documents().open_with_signal(input(), OpenOptions::default(), signal.clone());
    rt::run_until_stalled();
    assert_eq!(first.pages.get(), 1);
    assert!(kernel.documents().has("renamed"));
    run(kernel.documents().open(input(), OpenOptions::default())).unwrap();
    signal.abort(Error::new("cancel caller"));
    rt::run_until_stalled();
    assert!(opening.is_settled(), "rekeyed open must reject promptly");
    assert_eq!(opening.peek().unwrap().unwrap_err().code(), Some("operation-cancelled"));
    assert_eq!(first.closed.get(), 1);
    assert_eq!(second.closed.get(), 0);
    assert_eq!(kernel.documents().get("doc").unwrap().status, DocStatus::Ready);
    run(kernel.destroy()).unwrap();
}

#[test]
fn state_events_are_synchronous_per_write_while_global_notifications_are_held() {
    use epdf_kernel::plugin::Controller;
    rt::reset();
    let token = Token::<Controller<Value, CoreAction>>::new("state");
    let plugin = define::<Value, CoreAction>("state")
        .document_scoped()
        .initial_state(|| json!(0))
        .create(&token, |ctx| Ok((ctx, None)))
        .build();
    let reports = Rc::new(RefCell::new(Vec::<Error>::new()));
    let sink = reports.clone();
    let opts = KernelOptions {
        report: Some(Rc::new(move |e| sink.borrow_mut().push(e.clone()))),
        ..Default::default()
    };
    let kernel = Kernel::new(Rc::new(TestEngine(doc(false))), vec![plugin], opts).unwrap();
    run(kernel.start()).unwrap();
    run(kernel.documents().open(input(), OpenOptions::default())).unwrap();
    let ctx = kernel.try_capability(&token, Some("doc")).unwrap().unwrap();
    let events = Rc::new(RefCell::new(vec![]));
    let seen = events.clone();
    ctx.on_state_change(move |next, previous| {
        seen.borrow_mut().push(((*previous).clone(), (*next).clone()));
        Ok(())
    });
    let global = Rc::new(Cell::new(0));
    let count = global.clone();
    kernel.subscribe(Rc::new(move || {
        count.set(count.get() + 1);
        Ok(())
    }));
    let hold = kernel.store().hold();
    ctx.update_state(|_| Rc::new(json!(1)));
    assert_eq!(*events.borrow(), vec![(json!(0), json!(1))]);
    ctx.update_state(|_| Rc::new(json!(2)));
    assert_eq!(*events.borrow(), vec![(json!(0), json!(1)), (json!(1), json!(2))]);
    ctx.update_state(|same| same);
    assert_eq!(events.borrow().len(), 2);
    assert_eq!(global.get(), 0);
    drop(hold);
    assert_eq!(global.get(), 1);
    run(kernel.documents().close("doc")).unwrap();
    assert!(!ctx.update_state(|_| Rc::new(json!(3))));
    assert_eq!(events.borrow().len(), 2);
    assert!(!ctx.update_state(|same| same));
    assert_eq!(reports.borrow().len(), 1);
    assert_eq!(reports.borrow()[0].code(), Some("instance-closed"));
    assert!(reports.borrow()[0].message.contains("doc#1"));
    run(kernel.destroy()).unwrap();
}

fn controller_plugin()
-> (epdf_kernel::AnyPlugin, Token<epdf_kernel::plugin::Controller<Value, CoreAction>>) {
    let token = Token::new("download");
    let plugin = define::<Value, CoreAction>("download")
        .document_scoped()
        .initial_state(|| json!({}))
        .create(&token, |ctx| Ok((ctx, None)))
        .build();
    (plugin, token)
}
#[test]
fn save_and_layer_send_holds_wait_for_apply_and_keep_the_original_session() {
    for layer in [false, true] {
        rt::reset();
        let first = doc(false);
        first.local.set(true);
        first.layer.set(true);
        let second = doc(false);
        *second.id.borrow_mut() = "second".into();
        let (apply, resolve) = Promise::pending();
        *first.apply_answer.borrow_mut() = Some(apply);
        let (plugin, token) = controller_plugin();
        let kernel = Kernel::new(
            Rc::new(QueueEngine(RefCell::new([first.clone(), second.clone()].into()))),
            vec![plugin],
            options(),
        )
        .unwrap();
        run(kernel.start()).unwrap();
        run(kernel.documents().open(input(), OpenOptions::default())).unwrap();
        let ctx = kernel.try_capability(&token, Some("doc")).unwrap().unwrap();
        let held = ctx.changes().unwrap().hold(json!({"key":"typing"}), None).unwrap();
        held.set(vec![json!({"type":"metadata.set","values":{"Title":"held"}})], vec![]);
        let saved = if layer {
            kernel.documents().save_layer(None)
        } else {
            kernel.documents().save(None, None)
        };
        rt::run_until_stalled();
        assert_eq!(
            first.calls.borrow().iter().filter(|c| c.as_str() == "apply").count(),
            1,
            "download sends the hold"
        );
        assert!(!first.calls.borrow().iter().any(|c| c.starts_with("download")));
        assert!(!saved.is_settled());
        run(kernel
            .documents()
            .open(OpenSource::Input(json!({"id":"second"})), OpenOptions::default()))
        .unwrap();
        resolve.resolve(json!({"applied":true}));
        assert_eq!(run(saved).unwrap()["fileOf"], "doc");
        assert!(!second.calls.borrow().iter().any(|c| c.starts_with("download")));
        run(kernel.destroy()).unwrap();
    }
}
#[test]
fn close_during_settle_prevents_download_even_after_apply_answers() {
    rt::reset();
    let document = doc(false);
    let (apply, answer) = Promise::pending();
    *document.apply_answer.borrow_mut() = Some(apply);
    let (plugin, token) = controller_plugin();
    let kernel =
        Kernel::new(Rc::new(TestEngine(document.clone())), vec![plugin], options()).unwrap();
    run(kernel.start()).unwrap();
    run(kernel.documents().open(input(), OpenOptions::default())).unwrap();
    let ctx = kernel.try_capability(&token, Some("doc")).unwrap().unwrap();
    ctx.changes()
        .unwrap()
        .stage(epdf_kernel::changes::StagedChange::ops(
            json!({"key":"x"}),
            vec![json!({"type":"metadata.set"})],
        ))
        .unwrap();
    let saved = kernel.documents().save(None, None);
    run(kernel.documents().close("doc")).unwrap();
    assert_eq!(run(saved).unwrap_err().code(), Some("operation-cancelled"));
    answer.resolve(json!({}));
    rt::run_until_stalled();
    assert!(!document.calls.borrow().iter().any(|c| c.starts_with("download")));
    run(kernel.destroy()).unwrap();
}
#[test]
fn construction_mutations_are_dirty_and_only_unchanged_successful_download_clears() {
    for local in [false, true] {
        rt::reset();
        let document = doc(false);
        document.local.set(local);
        let token = Token::<()>::new("writer");
        let plugin = define::<Value, CoreAction>("writer")
            .document_scoped()
            .create(&token, |ctx| {
                let handle = ctx.doc()?;
                Ok((
                    (),
                    Some(Rc::new(move || {
                        handle.call(
                            "apply",
                            None,
                            vec![
                                json!({"ops":[{"type":"metadata.set"}]}),
                                json!({"opId":"connect"}),
                            ],
                        );
                        Ok(())
                    })),
                ))
            })
            .build();
        let kernel =
            Kernel::new(Rc::new(TestEngine(document.clone())), vec![plugin], options()).unwrap();
        run(kernel.start()).unwrap();
        let opened = run(kernel.documents().open(input(), OpenOptions::default())).unwrap();
        assert_eq!(
            opened["document"]["hasUnsavedChanges"], local,
            "connect-time fact is staged before publish"
        );
        run(kernel.documents().save(None, None)).unwrap();
        assert!(!kernel.documents().get("doc").unwrap().has_unsaved_changes);
        document.events.emit(&json!({"type":"metadata.changed","origin":{"kind":"remote"}}));
        let (read, land) = Promise::pending();
        *document.download_answer.borrow_mut() = Some(read);
        let saving = kernel.documents().save(None, None);
        rt::run_until_stalled();
        document.events.emit(&json!({"type":"metadata.changed","origin":{"kind":"local"}}));
        land.resolve(json!({"bytes":"read"}));
        run(saving).unwrap();
        assert_eq!(kernel.documents().get("doc").unwrap().has_unsaved_changes, local);
        *document.download_answer.borrow_mut() = None;
        run(kernel.documents().save(None, None)).unwrap();
        assert!(!kernel.documents().get("doc").unwrap().has_unsaved_changes);
        run(kernel.destroy()).unwrap();
    }
}

#[test]
fn staged_meta_keeps_its_identity_when_dirty_does_not_change() {
    rt::reset();
    let document = doc(false);
    document.local.set(true);
    let seen: Rc<RefCell<Vec<Rc<epdf_kernel::types::DocumentMeta>>>> =
        Rc::new(RefCell::new(Vec::new()));
    let token = Token::<()>::new("writer");
    let sink = seen.clone();
    let plugin = define::<Value, CoreAction>("writer")
        .document_scoped()
        .create(&token, move |ctx| {
            let handle = ctx.doc()?;
            let ctx = ctx.clone();
            let sink = sink.clone();
            Ok((
                (),
                Some(Rc::new(move || {
                    for op in ["one", "two"] {
                        handle.call(
                            "apply",
                            None,
                            vec![json!({"ops":[{"type":"metadata.set"}]}), json!({"opId": op})],
                        );
                        sink.borrow_mut().push(ctx.document_rc().expect("staged meta"));
                    }
                    Ok(())
                })),
            ))
        })
        .build();
    let kernel =
        Kernel::new(Rc::new(TestEngine(document.clone())), vec![plugin], options()).unwrap();
    run(kernel.start()).unwrap();
    run(kernel.documents().open(input(), OpenOptions::default())).unwrap();
    let seen = seen.borrow();
    assert_eq!(seen.len(), 2);
    assert!(seen[0].has_unsaved_changes, "the first local change stages dirty");
    assert!(Rc::ptr_eq(&seen[0], &seen[1]), "a second dirty write leaves the staged Rc alone");
    run(kernel.destroy()).unwrap();
}

#[test]
fn download_flushes_report_failures_and_wraps_preserve_post_read_mutations() {
    rt::reset();
    let document = doc(false);
    document.local.set(true);
    let (plugin, token) = controller_plugin();
    let errors = Rc::new(RefCell::new(vec![]));
    let reports = errors.clone();
    let kernel = Kernel::new(
        Rc::new(TestEngine(document.clone())),
        vec![plugin],
        KernelOptions {
            report: Some(Rc::new(move |e| reports.borrow_mut().push(e.message.clone()))),
            ..Default::default()
        },
    )
    .unwrap();
    run(kernel.start()).unwrap();
    run(kernel.documents().open(input(), OpenOptions::default())).unwrap();
    let ctx = kernel.try_capability(&token, Some("doc")).unwrap().unwrap();
    let log = Rc::new(RefCell::new(vec![]));
    let log1 = log.clone();
    ctx.on_settle(Rc::new(move |_| {
        log1.borrow_mut().push("flush1");
        Err(Error::new("flush failed"))
    }))
    .unwrap();
    let log2 = log.clone();
    ctx.on_settle(Rc::new(move |_| {
        log2.borrow_mut().push("flush2");
        Ok(None)
    }))
    .unwrap();
    for (before, after, mutate) in [("before1", "after1", true), ("before2", "after2", false)] {
        let log = log.clone();
        let document = document.clone();
        ctx.around_download(Rc::new(move |read| {
            log.borrow_mut().push(before);
            let log = log.clone();
            let document = document.clone();
            Ok(Promise::spawn(async move {
                let bytes = read().await?;
                log.borrow_mut().push(after);
                if mutate {
                    document
                        .events
                        .emit(&json!({"type":"metadata.changed","origin":{"kind":"local"}}));
                }
                Ok(bytes)
            }))
        }))
        .unwrap();
    }
    let saved = run(kernel.documents().save(None, None)).unwrap();
    assert_eq!(saved["fileOf"], "doc");
    assert_eq!(*log.borrow(), ["flush1", "flush2", "before1", "before2", "after2", "after1"]);
    assert_eq!(*errors.borrow(), ["flush failed"]);
    assert!(
        kernel.documents().get("doc").unwrap().has_unsaved_changes,
        "a did-save mutation remains dirty"
    );
    run(kernel.destroy()).unwrap();
}

#[test]
fn download_permissions_and_layer_absence_refuse_before_flushes() {
    rt::reset();
    let document = doc(false);
    let (plugin, token) = controller_plugin();
    let kernel =
        Kernel::new(Rc::new(TestEngine(document.clone())), vec![plugin], options()).unwrap();
    run(kernel.start()).unwrap();
    run(kernel.documents().open(input(), OpenOptions::default())).unwrap();
    let ctx = kernel.try_capability(&token, Some("doc")).unwrap().unwrap();
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    ctx.on_settle(Rc::new(move |_| {
        count.set(count.get() + 1);
        Ok(None)
    }))
    .unwrap();
    let unsupported = run(kernel.documents().save_layer(None)).unwrap_err();
    assert_eq!(unsupported.code(), Some("unsupported"));
    assert_eq!(calls.get(), 0);
    document.security.0.set(true);
    for layer in [false, true] {
        let denied = run(if layer {
            kernel.documents().save_layer(None)
        } else {
            kernel.documents().save(None, None)
        })
        .unwrap_err();
        assert_eq!(denied.code(), Some("permission-denied"));
        assert_eq!(denied.member("permission"), Some(&json!("doc.download")));
    }
    assert_eq!(calls.get(), 0);
    assert!(!document.calls.borrow().iter().any(|c| c.starts_with("download")));
    run(kernel.destroy()).unwrap();
    assert_eq!(run(kernel.documents().save(None, None)).unwrap_err().code(), Some("not-ready"));
}

#[test]
fn caller_cancel_during_flush_stops_waiting_without_closing_the_document() {
    use epdf_kernel::signal::AbortSignal;
    rt::reset();
    let document = doc(false);
    let (plugin, token) = controller_plugin();
    let kernel =
        Kernel::new(Rc::new(TestEngine(document.clone())), vec![plugin], options()).unwrap();
    run(kernel.start()).unwrap();
    run(kernel.documents().open(input(), OpenOptions::default())).unwrap();
    let ctx = kernel.try_capability(&token, Some("doc")).unwrap().unwrap();
    let (flush, finish) = Promise::pending();
    let observed = Rc::new(RefCell::new(None));
    let take = observed.clone();
    ctx.on_settle(Rc::new(move |signal| {
        *take.borrow_mut() = Some(signal);
        Ok(Some(flush.clone()))
    }))
    .unwrap();
    let caller = AbortSignal::new();
    let saved = kernel.documents().save_with_signal(None, None, Some(caller.clone()));
    rt::run_until_stalled();
    caller.abort(Error::new("caller stopped"));
    assert_eq!(run(saved).unwrap_err().code(), Some("operation-cancelled"));
    assert!(observed.borrow().as_ref().unwrap().aborted());
    assert_eq!(caller.listener_count(), 0);
    finish.resolve(());
    rt::run_until_stalled();
    assert_eq!(document.closed.get(), 0);
    assert!(!document.calls.borrow().iter().any(|c| c.starts_with("download")));
    run(kernel.destroy()).unwrap();
}

#[test]
fn caller_cancel_during_read_preserves_dirty_state_and_ignores_late_bytes() {
    use epdf_kernel::signal::AbortSignal;
    rt::reset();
    let document = doc(false);
    document.local.set(true);
    let kernel = Kernel::new(Rc::new(TestEngine(document.clone())), vec![], options()).unwrap();
    run(kernel.start()).unwrap();
    run(kernel.documents().open(input(), OpenOptions::default())).unwrap();
    document.events.emit(&json!({"type":"metadata.changed"}));
    let (read, land) = Promise::pending();
    let aborts = Rc::new(Cell::new(0));
    let counted = aborts.clone();
    let reject = land.clone();
    let read = read.with_abort(move |reason| {
        counted.set(counted.get() + 1);
        reject.reject(Error::abort(reason));
    });
    *document.download_answer.borrow_mut() = Some(read);
    let caller = AbortSignal::new();
    let saved = kernel.documents().save_with_signal(None, None, Some(caller.clone()));
    rt::run_until_stalled();
    assert!(document.calls.borrow().iter().any(|c| c == "download"));
    caller.abort(Error::new("stop read"));
    assert_eq!(run(saved).unwrap_err().code(), Some("operation-cancelled"));
    assert_eq!(aborts.get(), 1, "caller abort reaches the engine read");
    land.resolve(json!({"bytes":"late"}));
    rt::run_until_stalled();
    assert!(kernel.documents().get("doc").unwrap().has_unsaved_changes);
    assert_eq!(caller.listener_count(), 0);
    run(kernel.destroy()).unwrap();
}
#[test]
fn close_rejects_pending_changes_only_after_the_plugins_event_sources_are_gone() {
    use epdf_kernel::changes::StagedChange;
    rt::reset();
    let document = doc(false);
    // The engine never answers: only the close settles the change.
    let (never, _keep) = Promise::<Value>::pending();
    *document.apply_answer.borrow_mut() = Some(never);
    let (plugin, token) = controller_plugin();
    let kernel = Kernel::new(Rc::new(TestEngine(document)), vec![plugin], options()).unwrap();
    run(kernel.start()).unwrap();
    run(kernel.documents().open(input(), OpenOptions::default())).unwrap();
    let ctx = kernel.capability(&token, Some("doc")).unwrap();
    let source = ctx.event_source::<String>();
    let heard = Rc::new(RefCell::new(Vec::new()));
    let log = heard.clone();
    let _off = source.on(move |e| {
        log.borrow_mut().push(e.clone());
        Ok(())
    });
    let change = ctx
        .changes()
        .unwrap()
        .stage(StagedChange::ops(json!({"key":"x"}), vec![json!({"type":"metadata.set"})]))
        .unwrap();
    // `change.result.then(null, () => source.emit(...))`, as the history plugin does.
    let result = change.result();
    rt::spawn(async move {
        if let Err(error) = result.reaction().await {
            source.emit(&error.code().unwrap_or("").to_string());
        }
    });
    run(kernel.documents().close("doc")).unwrap();
    // TS closeSession (kernel.ts:550-570) closes the queue only in its scope
    // teardown (kernel.ts:509), after the instance's event sources.
    assert_eq!(change.result().peek().unwrap().unwrap_err().code(), Some("instance-closed"));
    assert!(heard.borrow().is_empty(), "heard {:?}", heard.borrow());
    run(kernel.destroy()).unwrap();
}
#[test]
fn a_page_mirror_subscribes_to_the_handle_at_creation_between_the_kernel_and_connect() {
    use epdf_kernel::page_mirror::{Affected, PageMirrorSpec};
    use epdf_kernel::plugin::Controller;
    // `onDocumentEvent` (core/main/src/context.ts:216): a mirror subscribes
    // to `doc.events` itself when it is created, so it sees an event after
    // the kernel's own listener has updated the meta and before the
    // listeners `connect` adds, and it unsubscribes when the instance closes.
    rt::reset();
    let document = doc(false);
    let seen: Rc<RefCell<Vec<(&'static str, u64)>>> = Rc::default();
    let at_creation = Rc::new(Cell::new(usize::MAX));
    let (log, count, handle) = (seen.clone(), at_creation.clone(), document.clone());
    let token = Token::<()>::new("mirrored");
    let plugin = define::<Value, CoreAction>("mirrored")
        .document_scoped()
        .create(&token, move |ctx: Controller<Value, CoreAction>| {
            let before = handle.events.listener_count();
            let (mirror_log, mirror_ctx) = (log.clone(), ctx.clone());
            let mirror = ctx.page_mirror(PageMirrorSpec {
                name: "pages".into(),
                load: Rc::new(|_, _, _| Promise::resolved(json!(0))),
                affected: Rc::new(move |_| {
                    let revision = mirror_ctx.document().map_or(0, |m| m.revision);
                    mirror_log.borrow_mut().push(("mirror", revision));
                    Affected::None
                }),
                fold: None,
                predict: None,
                changed: None,
            })?;
            count.set(handle.events.listener_count() - before);
            let (connect_log, connect_ctx) = (log.clone(), ctx.clone());
            // The instance keeps its mirror for as long as it lives.
            let connect: epdf_kernel::plugin::ConnectFn = Rc::new(move || {
                let _kept = &mirror;
                let (log, ctx) = (connect_log.clone(), connect_ctx.clone());
                connect_ctx.listen(connect_ctx.doc()?.subscribe(Rc::new(move |_| {
                    let revision = ctx.document().map_or(0, |m| m.revision);
                    log.borrow_mut().push(("connect", revision));
                })));
                Ok(())
            });
            Ok(((), Some(connect)))
        })
        .build();
    let kernel =
        Kernel::new(Rc::new(TestEngine(document.clone())), vec![plugin], options()).unwrap();
    run(kernel.start()).unwrap();
    let subscribed_before_open = document.events.listener_count();
    run(kernel.documents().open(input(), OpenOptions::default())).unwrap();
    assert_eq!(at_creation.get(), 1, "the mirror subscribed to the handle when it was created");

    let page = json!({"ref":{"kind":"objectNumber","objectNumber":3}});
    document.events.emit(&json!({
        "type": "pages.rotated",
        "layout": {"pageCount": 1, "pages": [page]},
        "origin": {"kind": "remote"}
    }));
    rt::run_until_stalled();
    assert_eq!(*seen.borrow(), vec![("mirror", 1), ("connect", 1)]);

    run(kernel.documents().close("doc")).unwrap();
    assert_eq!(document.events.listener_count(), subscribed_before_open);
    run(kernel.destroy()).unwrap();
}

/// `workspaceViewOf` (kernel.ts:1005): a workspace capability resolved for
/// a document is its `inScope` view, the same object on every call; a
/// rekey drops it (kernel.ts:537, "a view is bound to the id it was built
/// for") and so does close (kernel.ts:568).
#[test]
fn a_workspace_view_is_kept_per_document_and_dropped_on_rekey_and_close() {
    rt::reset();
    let first = doc(false);
    *first.id.borrow_mut() = "renamed".into();
    let (snapshot, land) = Promise::pending();
    *first.page_answer.borrow_mut() = Some(snapshot);
    let token = Token::<Rc<String>>::new("scoped");
    let built = Rc::new(Cell::new(0));
    let count = built.clone();
    let plugin = define::<Value, CoreAction>("scoped")
        .create(&token, |_| Ok((Rc::new("workspace".to_string()), None)))
        .in_scope(move |_: &Rc<String>, id: &str| {
            count.set(count.get() + 1);
            Rc::new(format!("view:{id}"))
        })
        .build();
    // A document context held across the close keeps the session alive, so
    // only the close-time clear can drop its views.
    let held = Token::<epdf_kernel::plugin::Controller<Value, CoreAction>>::new("held");
    let holder = define::<Value, CoreAction>("held")
        .document_scoped()
        .initial_state(|| json!(0))
        .create(&held, |ctx| Ok((ctx, None)))
        .build();
    let kernel = Kernel::new(
        Rc::new(QueueEngine(RefCell::new([first.clone()].into()))),
        vec![plugin, holder],
        KernelOptions { release_closed_sessions: false, ..options() },
    )
    .unwrap();
    run(kernel.start()).unwrap();
    let unscoped = kernel.capability(&token, None).unwrap();
    assert_eq!(*unscoped, "workspace");
    let opening = kernel.documents().open(input(), OpenOptions::default());
    let before = kernel.capability(&token, Some("doc")).unwrap();
    assert_eq!(*before, "view:doc");
    assert!(Rc::ptr_eq(&before, &kernel.capability(&token, Some("doc")).unwrap()));
    assert_eq!(built.get(), 1);
    rt::run_until_stalled();
    assert!(kernel.documents().has("renamed"));
    let after = kernel.capability(&token, Some("renamed")).unwrap();
    assert_eq!(*after, "view:renamed", "the rekey dropped the view built for the ticket id");
    assert_eq!(built.get(), 2);
    land.resolve(Default::default());
    let _ = run(opening);
    let kept = kernel.try_capability(&held, Some("renamed")).unwrap().unwrap();
    let view = Rc::downgrade(&after);
    drop(after);
    assert!(view.upgrade().is_some(), "the session keeps its view while open");
    run(kernel.documents().close("renamed")).unwrap();
    assert!(view.upgrade().is_none(), "close dropped the view (kernel.ts:568)");
    drop(kept);
    assert_eq!(*kernel.capability(&token, Some("renamed")).unwrap(), "workspace");
    run(kernel.destroy()).unwrap();
}

/// `ctx.watch(select, handler, isEqual)` (context.ts:305): `select` runs
/// once at the call, `handler(next, previous)` runs after a store change
/// that moves it and not after one that leaves it equal, the returned
/// unsubscribe stops it, and the instance's close stops another one.
#[test]
fn watch_runs_its_handler_only_when_the_selection_moves() {
    use epdf_kernel::store::Unsubscribe;
    rt::reset();
    let (plugin, token) = controller_plugin();
    let second = doc(false);
    *second.id.borrow_mut() = "second".into();
    let engine = QueueEngine(RefCell::new([doc(false), second].into()));
    let kernel = Kernel::new(Rc::new(engine), vec![plugin], options()).unwrap();
    run(kernel.start()).unwrap();
    run(kernel.documents().open(input(), OpenOptions::default())).unwrap();
    let ctx = kernel.try_capability(&token, Some("doc")).unwrap().unwrap();
    let selects = Rc::new(Cell::new(0));
    let seen: Rc<RefCell<Vec<(String, i64, i64)>>> = Rc::default();
    let watch = |name: &'static str| -> Unsubscribe {
        let (reader, count, log) = (ctx.clone(), selects.clone(), seen.clone());
        ctx.watch(
            move || {
                count.set(count.get() + 1);
                Ok(reader.state()["n"].as_i64().unwrap_or(0) / 2)
            },
            move |next, previous| {
                log.borrow_mut().push((name.into(), *next, *previous));
                Ok(())
            },
            |a, b| a == b,
        )
        .unwrap()
    };
    let first = watch("first");
    let _second = watch("second");
    assert_eq!(selects.get(), 2, "select runs once at the call");
    let set = |n: i64| {
        ctx.update_state(|_| Rc::new(json!({ "n": n })));
        rt::run_until_stalled();
    };
    set(1);
    assert!(seen.borrow().is_empty(), "1 / 2 selects 0 again");
    set(2);
    assert_eq!(*seen.borrow(), [("first".to_string(), 1, 0), ("second".to_string(), 1, 0)]);
    first.call();
    set(4);
    assert_eq!(seen.borrow().last(), Some(&("second".to_string(), 2, 1)));
    assert_eq!(seen.borrow().len(), 3);
    run(kernel.documents().close("doc")).unwrap();
    let after_close = selects.get();
    // Opening another document changes the store; a watch the close left
    // subscribed would select again.
    run(kernel.documents().open(input(), OpenOptions::default())).unwrap();
    assert_eq!(selects.get(), after_close, "close unsubscribed the second watch");
    run(kernel.destroy()).unwrap();
}

/// `bursts.ts`: the first event of a transaction of more than one event
/// holds the store's notifications, and the transaction's last event lets
/// them go at once, not when the next event arrives. A rename made inside
/// the burst lands on the document's record.
#[test]
fn a_burst_holds_notifications_until_its_last_event() {
    rt::reset();
    let document = doc(false);
    let kernel = Kernel::new(Rc::new(TestEngine(document.clone())), vec![], options()).unwrap();
    run(kernel.start()).unwrap();
    run(kernel.documents().open(input(), OpenOptions::default())).unwrap();
    let woken = Rc::new(Cell::new(0));
    let count = woken.clone();
    kernel.subscribe(Rc::new(move || {
        count.set(count.get() + 1);
        Ok(())
    }));
    let event = |index: u32| json!({"type":"metadata.changed","origin":{"kind":"remote","tx":{"id":"burst","index":index,"count":2}}});
    document.events.emit(&event(0));
    let held = woken.get();
    kernel.documents().rename("doc", "held.pdf");
    assert_eq!(woken.get(), held, "the burst holds the store's notifications");
    document.events.emit(&event(1));
    assert!(woken.get() > held, "its last event lets them go");
    assert_eq!(kernel.documents().get("doc").unwrap().name.as_deref(), Some("held.pdf"));
    run(kernel.destroy()).unwrap();
}

/// `ctx.notify()` wakes the store's readers without an action, and
/// `ctx.serialQueue(key)` is one queue per key for the instance
/// (`controller.ts`): an operation queued under the same key waits for the
/// one before it. An engine that keeps the default `list_fonts` lists none.
#[test]
fn notify_wakes_readers_and_a_serial_queue_is_one_per_key() {
    use epdf_kernel::plugin::Controller;
    rt::reset();
    assert!(TestEngine(doc(false)).list_fonts().is_empty());
    let document = doc(false);
    let retained: Rc<RefCell<Option<Controller<Value, CoreAction>>>> = Rc::default();
    let save = retained.clone();
    let token = Token::<()>::new("queued");
    let plugin = define::<Value, CoreAction>("queued")
        .document_scoped()
        .create(&token, move |ctx| {
            *save.borrow_mut() = Some(ctx);
            Ok(((), None))
        })
        .build();
    let kernel =
        Kernel::new(Rc::new(TestEngine(document.clone())), vec![plugin], options()).unwrap();
    run(kernel.start()).unwrap();
    run(kernel.documents().open(input(), OpenOptions::default())).unwrap();
    let ctx = retained.borrow().clone().unwrap();
    let woken = Rc::new(Cell::new(0));
    let count = woken.clone();
    kernel.subscribe(Rc::new(move || {
        count.set(count.get() + 1);
        Ok(())
    }));
    ctx.notify();
    assert_eq!(woken.get(), 1, "notify wakes the readers once");

    let (gate, open) = Promise::<()>::pending();
    let first = ctx.serial_queue("k").run(move || Ok(gate));
    let started = Rc::new(Cell::new(false));
    let flag = started.clone();
    let second = ctx.serial_queue("k").run(move || {
        flag.set(true);
        Ok(Promise::resolved(()))
    });
    rt::run_until_stalled();
    assert!(!started.get(), "the second operation waits for the first");
    open.resolve(());
    rt::run_until_stalled();
    assert!(started.get());
    assert!(matches!(first.peek(), Some(Ok(()))));
    assert!(matches!(second.peek(), Some(Ok(()))));
    run(kernel.destroy()).unwrap();
}
