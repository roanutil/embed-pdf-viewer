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
    security: Rc<Security>,
    events: EventHookSource<Value>,
    closed: Cell<u32>,
    pages: Cell<u32>,
    next: Cell<u32>,
}
impl DocumentHandle for Doc {
    fn id(&self) -> String {
        "doc".into()
    }
    fn capabilities(&self) -> Value {
        json!({})
    }
    fn security(&self) -> Rc<dyn DocumentSecurity> {
        self.security.clone()
    }
    fn has(&self, _: &str) -> bool {
        false
    }
    fn call(&self, path: &str, _: Option<u32>, args: Vec<Value>) -> EngineCall<Value> {
        match path {
            "pages.list" => {
                self.pages.set(self.pages.get() + 1);
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
                self.events.emit(&json!({"type":"metadata.changed","value":9,"origin":{"kind":"local","tx":{"id":args[1]["opId"],"index":0,"count":1}}}));
                Promise::resolved(json!({"applied":true}))
            }
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
    let kernel =
        Kernel::new(Rc::new(TestEngine(document.clone())), vec![plugin], options()).unwrap();
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
    run(kernel.destroy()).unwrap();
}
