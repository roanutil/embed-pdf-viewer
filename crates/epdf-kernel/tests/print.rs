//! The kernel's print (`kernel.ts` "printing", lines 736-962 and the
//! documents capability's print verbs, 1576-1589) and the documents
//! capability as a document's scope sees it (`documentsInScope`,
//! kernel.ts:249-265, registered at 374-376).

use epdf_kernel::{
    DOCUMENTS, Error, Kernel, KernelOptions, OpenOptions, OpenSource, Promise,
    action::CoreAction,
    define,
    engine::{DocumentHandle, DocumentSecurity, Engine, EngineCall},
    event_hook::EventHookSource,
    geometry::PageRef,
    print::{
        HostPrinter, PRINT_HIGH, PRINT_PDF, PRINT_STANDARD, PrintOptions, PrintProgress,
        PrintQuality, PrintSheet, StageOptions, StagedPrint,
    },
    rt,
    signal::AbortSignal,
    store::Unsubscribe,
    token::Token,
};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

type Log = Rc<RefCell<Vec<String>>>;

struct Security(Rc<RefCell<Vec<&'static str>>>);
impl DocumentSecurity for Security {
    fn allows(&self, permission: &str) -> bool {
        self.0.borrow().contains(&permission)
    }
    fn allows_annotation_create(&self) -> bool {
        false
    }
    fn allows_annotation_mutation(&self, _: &str, _: &Value) -> bool {
        false
    }
    fn allows_annotation_group_assignment(&self, _: &str) -> bool {
        false
    }
    fn effective_scope(&self) -> Vec<String> {
        vec![]
    }
    fn identity(&self) -> Value {
        Value::Null
    }
    fn current(&self) -> Value {
        json!({ "locked": false })
    }
    fn password_prompt(&self) -> Value {
        json!({ "state": "none" })
    }
    fn unlock(&self, _: Value) -> EngineCall<Value> {
        Promise::resolved(self.current())
    }
}

struct Doc {
    id: String,
    log: Log,
    security: Rc<Security>,
    events: EventHookSource<Value>,
}

fn page(pon: u32, width: f64, height: f64) -> Value {
    json!({
        "ref": { "kind": "objectNumber", "objectNumber": pon },
        "size": { "width": width, "height": height },
    })
}

impl DocumentHandle for Doc {
    fn id(&self) -> String {
        self.id.clone()
    }
    fn capabilities(&self) -> Value {
        json!({})
    }
    fn security(&self) -> Rc<dyn DocumentSecurity> {
        self.security.clone()
    }
    fn has(&self, member: &str) -> bool {
        member == "pages.extract"
    }
    fn call(&self, path: &str, pon: Option<u32>, args: Vec<Value>) -> EngineCall<Value> {
        match path {
            "pages.list" => Promise::resolved(json!({
                "pageCount": 2,
                "pages": [page(3, 612.0, 792.0), page(7, 300.0, 400.0)],
            })),
            "close" => Promise::resolved(Value::Null),
            "page.render.image" => {
                self.log.borrow_mut().push(format!(
                    "{}:render {} {}",
                    self.id,
                    pon.unwrap(),
                    args[0]
                ));
                Promise::resolved(json!({ "png": pon }))
            }
            "pages.extract" => {
                self.log.borrow_mut().push(format!("{}:extract {} {}", self.id, args[0], args[1]));
                Promise::resolved(json!({ "pdf": args[0] }))
            }
            _ => Promise::rejected(Error::engine("NotImplemented", path.to_string())),
        }
    }
    fn page_index(&self, number: u32) -> Result<u32, Error> {
        match number {
            3 => Ok(0),
            7 => Ok(1),
            _ => Err(Error::engine("NotFound", "page")),
        }
    }
    fn subscribe(&self, listener: Rc<dyn Fn(&Value)>) -> Unsubscribe {
        self.events.on(move |v| {
            listener(v);
            Ok(())
        })
    }
}

struct TestEngine {
    log: Log,
    permissions: Rc<RefCell<Vec<&'static str>>>,
    docs: RefCell<Vec<Rc<Doc>>>,
}
impl Engine for TestEngine {
    fn open(&self, input: Value, _: Option<Value>) -> EngineCall<Rc<dyn DocumentHandle>> {
        let doc = Rc::new(Doc {
            id: input["id"].as_str().unwrap_or("doc").into(),
            log: self.log.clone(),
            security: Rc::new(Security(self.permissions.clone())),
            events: EventHookSource::new(None),
        });
        self.docs.borrow_mut().push(doc.clone());
        Promise::resolved(doc as Rc<dyn DocumentHandle>)
    }
    fn destroy(&self) -> EngineCall<()> {
        Promise::resolved(())
    }
}

/// A printer that logs what it stages, prints and frees. `refuse_pdf`
/// turns the PDF copy down while staging; `fail_pdf_print` lets it stage,
/// then fails its `print()`.
struct Printer {
    log: Log,
    pdf: bool,
    needs_tap: bool,
    refuse_pdf: bool,
    fail_pdf_print: bool,
    staged: Cell<u32>,
}

struct Staged {
    log: Log,
    name: String,
    fail: bool,
}
impl StagedPrint for Staged {
    fn print(&self) -> Promise<()> {
        self.log.borrow_mut().push(format!("print {}", self.name));
        if self.fail {
            Promise::rejected(Error::new("the printer refused the PDF"))
        } else {
            Promise::resolved(())
        }
    }
    fn dispose(&self) {
        self.log.borrow_mut().push(format!("dispose {}", self.name));
    }
}

impl HostPrinter for Printer {
    fn needs_tap(&self) -> bool {
        self.needs_tap
    }
    fn stage(
        &self,
        sheets: Vec<PrintSheet>,
        options: StageOptions,
    ) -> Promise<Rc<dyn StagedPrint>> {
        self.staged.set(self.staged.get() + 1);
        let name = format!("sheets#{}", self.staged.get());
        let sizes: Vec<String> =
            sheets.iter().map(|s| format!("{}x{} {}", s.width_pt, s.height_pt, s.image)).collect();
        self.log.borrow_mut().push(format!("stage {name} {} [{}]", options.name, sizes.join(", ")));
        Promise::resolved(
            Rc::new(Staged { log: self.log.clone(), name, fail: false }) as Rc<dyn StagedPrint>
        )
    }
    fn prints_pdf(&self) -> bool {
        self.pdf
    }
    fn stage_pdf(&self, pdf: Value, options: StageOptions) -> Promise<Rc<dyn StagedPrint>> {
        if self.refuse_pdf {
            self.log.borrow_mut().push("stagePdf refused".into());
            return Promise::rejected(Error::new("no PDF viewer"));
        }
        self.staged.set(self.staged.get() + 1);
        let name = format!("pdf#{}", self.staged.get());
        self.log.borrow_mut().push(format!("stagePdf {name} {} {pdf}", options.name));
        Promise::resolved(
            Rc::new(Staged { log: self.log.clone(), name, fail: self.fail_pdf_print })
                as Rc<dyn StagedPrint>,
        )
    }
}

fn printer(log: &Log) -> Printer {
    Printer {
        log: log.clone(),
        pdf: false,
        needs_tap: false,
        refuse_pdf: false,
        fail_pdf_print: false,
        staged: Cell::new(0),
    }
}

struct Rig {
    kernel: Kernel,
    engine: Rc<TestEngine>,
    log: Log,
    permissions: Rc<RefCell<Vec<&'static str>>>,
}

/// `log` is shared by the engine and the printer.
fn rig(log: &Log, printer: Option<Printer>, plugins: Vec<epdf_kernel::AnyPlugin>) -> Rig {
    rt::reset();
    let log = log.clone();
    let permissions = Rc::new(RefCell::new(vec!["doc.print", "doc.annotate.read"]));
    let engine = Rc::new(TestEngine {
        log: log.clone(),
        permissions: permissions.clone(),
        docs: RefCell::default(),
    });
    let options = KernelOptions {
        report: Some(Rc::new(|e| panic!("unexpected report: {e}"))),
        printer: printer.map(|p| Rc::new(p) as Rc<dyn HostPrinter>),
        ..Default::default()
    };
    let kernel = Kernel::new(engine.clone(), plugins, options).unwrap();
    run(kernel.start()).unwrap();
    Rig { kernel, engine, log, permissions }
}

fn run<T: Clone + 'static>(p: Promise<T>) -> Result<T, Error> {
    rt::run_until_stalled();
    p.peek().expect("operation must settle")
}

fn open(kernel: &Kernel, id: &str) {
    run(kernel.documents().open(OpenSource::Input(json!({ "id": id })), OpenOptions::default()))
        .unwrap();
}

fn code(error: &Error) -> String {
    error.member("code").and_then(Value::as_str).unwrap_or_default().to_string()
}

fn take(log: &Log) -> Vec<String> {
    std::mem::take(&mut *log.borrow_mut())
}

#[test]
fn without_a_printer_nothing_prints() {
    // kernel.ts:738 `printer !== null && allowsOn(...)`; 890-896 refuses
    // `unsupported` with the createKernel hint.
    let rig = rig(&Rc::default(), None, vec![]);
    open(&rig.kernel, "doc");
    let docs = rig.kernel.documents();
    assert!(!docs.can_print(None));
    assert_eq!(docs.get_print_quality(None), None);
    let error = run(docs.prepare_print(None, PrintOptions::default())).unwrap_err();
    assert_eq!(code(&error), "unsupported");
    assert_eq!(
        error.message,
        "no printer: pass one to createKernel({ printer }), such as browserPrinter() from @embedpdf/web"
    );
    let error = run(docs.print(None, PrintOptions::default())).unwrap_err();
    assert_eq!(code(&error), "unsupported");
}

#[test]
fn print_quality_follows_the_permissions_and_the_printer() {
    // kernel.ts:745-752 printQualityOf.
    let log: Log = Rc::default();
    let mut pdf_printer = printer(&log);
    pdf_printer.pdf = true;
    let rig = rig(&log, Some(pdf_printer), vec![]);
    open(&rig.kernel, "doc");
    let docs = rig.kernel.documents();
    assert!(docs.can_print(None));
    assert_eq!(docs.get_print_quality(None), Some(PRINT_STANDARD));
    assert_eq!(PRINT_STANDARD.to_json(), json!({ "kind": "images", "dpi": 150 }));
    rig.permissions.borrow_mut().push("doc.print.high");
    assert_eq!(docs.get_print_quality(None), Some(PRINT_HIGH));
    rig.permissions.borrow_mut().push("doc.download.flattened");
    assert_eq!(docs.get_print_quality(None), Some(PRINT_PDF));
    assert_eq!(PRINT_PDF.to_json(), json!({ "kind": "pdf" }));
    rig.permissions.borrow_mut().retain(|p| *p != "doc.print");
    assert!(!docs.can_print(None));
    assert_eq!(docs.get_print_quality(None), None);
    // No document: nothing prints.
    assert!(!docs.can_print(Some("nope")));

    // A printer without stagePdf prints pictures at 300 DPI.
    let log: Log = Rc::default();
    let rig = self::rig(&log, Some(printer(&log)), vec![]);
    open(&rig.kernel, "doc");
    rig.permissions.borrow_mut().extend(["doc.print.high", "doc.download"]);
    assert_eq!(rig.kernel.documents().get_print_quality(None), Some(PRINT_HIGH));
}

#[test]
fn prepare_print_refuses_before_anything_runs() {
    // kernel.ts:887-897 and printPagesOf (757-779).
    let log: Log = Rc::default();
    let rig = rig(&log, Some(printer(&log)), vec![]);
    let docs = rig.kernel.documents();
    let error = run(docs.prepare_print(None, PrintOptions::default())).unwrap_err();
    assert_eq!(
        (code(&error), error.message.as_str()),
        ("not-ready".into(), "no open document to preparePrint")
    );
    open(&rig.kernel, "doc");
    rig.permissions.borrow_mut().retain(|p| *p != "doc.print");
    let error = run(docs.prepare_print(None, PrintOptions::default())).unwrap_err();
    assert_eq!(code(&error), "permission-denied");
    assert_eq!(error.message, "documents.preparePrint requires doc.print");
    assert_eq!(error.member("permission"), Some(&json!("doc.print")));
    rig.permissions.borrow_mut().push("doc.print");
    let pages = |refs: Vec<u32>| PrintOptions {
        pages: Some(refs.into_iter().map(PageRef::new).collect()),
        ..Default::default()
    };
    let error = run(docs.prepare_print(None, pages(vec![]))).unwrap_err();
    assert_eq!(
        (code(&error), error.message.as_str()),
        ("invalid-input".into(), "a print needs at least one page")
    );
    let error = run(docs.prepare_print(None, pages(vec![3, 9]))).unwrap_err();
    assert_eq!(
        (code(&error), error.message.as_str()),
        ("not-found".into(), "page 9 is not in this document")
    );
    let error = run(docs.prepare_print(None, pages(vec![7, 7]))).unwrap_err();
    assert_eq!(
        (code(&error), error.message.as_str()),
        ("invalid-input".into(), "a print names a page twice")
    );
    assert!(log.borrow().is_empty(), "nothing staged: {:?}", log.borrow());
}

#[test]
fn a_print_stages_pictures_and_prints_inside_every_around_print_wrap() {
    // stagePictures (851-871) and print() inside runWrapped(printWraps)
    // (933-955): the first registered wrap is outermost.
    let log: Log = Rc::default();
    let order = log.clone();
    let token = Token::<()>::new("wrapper");
    let plugin = define::<Value, CoreAction>("wrapper")
        .document_scoped()
        .create(&token, move |ctx| {
            for name in ["outer", "inner"] {
                let order = order.clone();
                ctx.around_print(Rc::new(move |print| {
                    order.borrow_mut().push(format!("{name} before"));
                    let order = order.clone();
                    let printed = print();
                    Ok(Promise::spawn(async move {
                        printed.await?;
                        order.borrow_mut().push(format!("{name} after"));
                        Ok(())
                    }))
                }))?;
            }
            Ok(((), None))
        })
        .build();
    let mut tap_printer = printer(&log);
    tap_printer.needs_tap = true;
    let rig = rig(&log, Some(tap_printer), vec![plugin]);
    open(&rig.kernel, "doc");
    let progress: Rc<RefCell<Vec<PrintProgress>>> = Rc::default();
    let seen = progress.clone();
    let options = PrintOptions {
        pages: Some(vec![PageRef::new(7), PageRef::new(3)]),
        include_annotations: Some(false),
        on_progress: Some(Rc::new(move |p| seen.borrow_mut().push(p))),
        signal: None,
    };
    let prepared = run(rig.kernel.documents().prepare_print(None, options)).unwrap();
    assert_eq!(prepared.page_count(), 2);
    assert!(prepared.needs_tap());
    assert_eq!(prepared.quality(), PRINT_STANDARD);
    let layers = r#""includeAnnotations":false,"includeFormFields":false"#;
    assert_eq!(
        take(&rig.log),
        vec![
            format!(
                r#"doc:render 7 {{"usage":"print","viewport":{{"kind":"dpi","dpi":150}},"format":"png",{layers}}}"#
            ),
            format!(
                r#"doc:render 3 {{"usage":"print","viewport":{{"kind":"dpi","dpi":150}},"format":"png",{layers}}}"#
            ),
            r#"stage sheets#1 document [300x400 {"png":7}, 612x792 {"png":3}]"#.to_string(),
        ]
    );
    let counts: Vec<(usize, usize)> =
        progress.borrow().iter().map(|p| (p.pages_rendered, p.page_count)).collect();
    assert_eq!(counts, vec![(0, 2), (1, 2), (2, 2)]);
    run(prepared.print(None)).unwrap();
    assert_eq!(
        take(&rig.log),
        vec!["outer before", "inner before", "print sheets#1", "inner after", "outer after"]
    );
    prepared.dispose();
    prepared.dispose();
    assert_eq!(take(&rig.log), vec!["dispose sheets#1"]);
    let error = run(prepared.print(None)).unwrap_err();
    assert_eq!(
        (code(&error), error.message.as_str()),
        ("invalid-input".into(), "this print was disposed")
    );
}

#[test]
fn documents_print_prepares_prints_and_disposes() {
    // kernel.ts:1578-1585: print() is preparePrint(), print(), dispose().
    let log: Log = Rc::default();
    let rig = rig(&log, Some(printer(&log)), vec![]);
    open(&rig.kernel, "doc");
    run(rig.kernel.documents().print(None, PrintOptions::default())).unwrap();
    let log = take(&rig.log);
    assert_eq!(
        &log[2..],
        [
            "stage sheets#1 document [612x792 {\"png\":3}, 300x400 {\"png\":7}]",
            "print sheets#1",
            "dispose sheets#1"
        ]
    );
}

#[test]
fn a_change_after_preparing_prepares_again_before_printing() {
    // kernel.ts:937 `if (session.changesSeen !== changesSeen) await restage(...)`;
    // changesSeen counts every change on any engine (1171-1172).
    let log: Log = Rc::default();
    let rig = rig(&log, Some(printer(&log)), vec![]);
    open(&rig.kernel, "doc");
    let prepared =
        run(rig.kernel.documents().prepare_print(None, PrintOptions::default())).unwrap();
    take(&rig.log);
    let doc = rig.engine.docs.borrow()[0].clone();
    // Not a change: a print that saw it stays prepared.
    doc.events.emit(&json!({ "type": "document.versioned" }));
    rt::run_until_stalled();
    run(prepared.print(None)).unwrap();
    assert_eq!(take(&rig.log), vec!["print sheets#1"]);
    // A change, even though this engine isn't local.
    doc.events.emit(&json!({ "type": "metadata.changed", "origin": { "kind": "remote" } }));
    rt::run_until_stalled();
    run(prepared.print(None)).unwrap();
    let log = take(&rig.log);
    assert_eq!(log.len(), 5, "{log:?}");
    assert!(log[2].starts_with("stage sheets#2"));
    assert_eq!(&log[3..], ["dispose sheets#1", "print sheets#2"]);
    // Closing the document frees what is staged.
    run(rig.kernel.documents().close("doc")).unwrap();
    assert_eq!(take(&rig.log), vec!["dispose sheets#2"]);
}

#[test]
fn a_refused_pdf_copy_prints_as_pictures() {
    // stagePrint's catch (820-825) and print()'s fallback (943-948).
    let log: Log = Rc::default();
    let mut refusing = printer(&log);
    refusing.pdf = true;
    refusing.refuse_pdf = true;
    let rig = rig(&log, Some(refusing), vec![]);
    rig.permissions.borrow_mut().extend(["doc.print.high", "doc.download", "doc.forms.read"]);
    open(&rig.kernel, "doc");
    let prepared = run(rig.kernel.documents().prepare_print(
        None,
        PrintOptions { pages: Some(vec![PageRef::new(3)]), ..Default::default() },
    ))
    .unwrap();
    assert_eq!(prepared.quality(), PRINT_HIGH);
    assert_eq!(
        take(&rig.log),
        vec![
            r#"doc:extract [{"kind":"objectNumber","objectNumber":3}] {"flatten":{"usage":"print"},"includeAnnotations":true,"includeFormFields":true}"#.to_string(),
            "stagePdf refused".into(),
            r#"doc:render 3 {"usage":"print","viewport":{"kind":"dpi","dpi":300},"format":"png","includeAnnotations":true,"includeFormFields":true}"#.into(),
            r#"stage sheets#1 document [612x792 {"png":3}]"#.into(),
        ]
    );

    // A PDF copy the printer staged but then refused to print: restaged as
    // pictures at 300 DPI and printed again.
    let mut failing = printer(&log);
    failing.pdf = true;
    failing.fail_pdf_print = true;
    let rig = self::rig(&log, Some(failing), vec![]);
    rig.permissions.borrow_mut().extend(["doc.print.high", "doc.download"]);
    open(&rig.kernel, "doc");
    let prepared =
        run(rig.kernel.documents().prepare_print(None, PrintOptions::default())).unwrap();
    assert_eq!(prepared.quality(), PrintQuality::Pdf);
    take(&rig.log);
    run(prepared.print(None)).unwrap();
    let log = take(&rig.log);
    assert_eq!(log[0], "print pdf#1");
    assert!(log[3].starts_with("stage sheets#2"), "{log:?}");
    assert_eq!(&log[4..], ["dispose pdf#1", "print sheets#2"]);
    assert_eq!(prepared.quality(), PRINT_HIGH);
}

#[test]
fn a_print_cancelled_before_the_dialog_rejects_operation_cancelled() {
    // kernel.ts:938-942.
    let log: Log = Rc::default();
    let rig = rig(&log, Some(printer(&log)), vec![]);
    open(&rig.kernel, "doc");
    let prepared =
        run(rig.kernel.documents().prepare_print(None, PrintOptions::default())).unwrap();
    take(&rig.log);
    let signal = AbortSignal::new();
    signal.abort_value(Error::new("stop"), json!("stop"));
    let error = run(prepared.print(Some(signal))).unwrap_err();
    assert_eq!(
        (code(&error), error.message.as_str()),
        ("operation-cancelled".into(), "print cancelled")
    );
    assert_eq!(error.member("cause"), Some(&json!("stop")));
    assert!(take(&rig.log).is_empty());
}

#[test]
fn a_document_scope_sees_the_documents_capability_for_its_document() {
    // documentsInScope (kernel.ts:249-265): every call whose document is
    // optional uses the scope's document instead of the active one.
    let views: Rc<RefCell<Vec<(String, epdf_kernel::Documents)>>> = Rc::default();
    let token = Token::<()>::new("scoped");
    let sink = views.clone();
    let plugin = define::<Value, CoreAction>("scoped")
        .document_scoped()
        .create(&token, move |ctx| {
            let id = ctx.document_id().unwrap_or_default().to_string();
            sink.borrow_mut().push((id, ctx.get(&DOCUMENTS)?));
            Ok(((), None))
        })
        .build();
    let log: Log = Rc::default();
    let rig = rig(&log, Some(printer(&log)), vec![plugin]);
    open(&rig.kernel, "a");
    open(&rig.kernel, "b");
    rig.kernel.documents().set_active("b");
    assert_eq!(rig.kernel.documents().get_active_id().as_deref(), Some("b"));
    let views = views.borrow();
    assert_eq!(views.len(), 2);
    for (id, documents) in views.iter() {
        assert_eq!(documents.get_document(None).map(|d| d.id).as_deref(), Some(id.as_str()));
        assert_eq!(documents.list_pages(None).len(), 2);
        assert!(documents.can_print(None));
        assert!(documents.can_download(None) == rig.kernel.documents().can_download(Some(id)));
        assert_eq!(documents.get_print_quality(None), Some(PRINT_STANDARD));
        assert_eq!(documents.get_page_index(PageRef::new(7), None), 1);
        assert!(documents.get_page(PageRef::new(3), None).is_some());
        assert_eq!(documents.get_revision(None), rig.kernel.documents().get_revision(Some(id)));
        // An explicit id still wins.
        assert_eq!(documents.get_document(Some("b")).map(|d| d.id).as_deref(), Some("b"));
    }
    // Without a scope, the active document.
    assert_eq!(rig.kernel.documents().get_document(None).map(|d| d.id).as_deref(), Some("b"));
    // The view in a's scope prints "a", though "b" is active.
    let a = views.iter().find(|(id, _)| id == "a").unwrap().1.clone();
    run(a.print(None, PrintOptions::default())).unwrap();
    assert!(log.borrow().iter().any(|l| l.starts_with("a:render 3")), "{:?}", log.borrow());
    assert!(!log.borrow().iter().any(|l| l.starts_with("b:")));
    // The kernel's own capability, with no document in scope, prints the
    // active one.
    take(&log);
    run(rig.kernel.documents().print(None, PrintOptions::default())).unwrap();
    assert!(log.borrow().iter().any(|l| l.starts_with("b:render 3")), "{:?}", log.borrow());
}
