//! `services/`: the plugin-private services every area is built on (the
//! settings, the events, the policy decision, the registration ports, the
//! serial queue, the catalog read, the form, session authority, the
//! kernel's print and the print latch). The queue, the script-node
//! aggregate and the print latch are fields of
//! [`crate::controller::Inner`]; the rest have modules or helpers here.

pub mod annotation_rows;
pub mod catalog;
pub mod policy;
pub mod ports;

use std::cell::RefCell;
use std::rc::Rc;

use epdf_acrojs::ScriptDiagnostic;
use epdf_kernel::Error;
use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::geometry::PageRef;
use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use serde_json::Value;

use crate::ActionsCapability;
use crate::contract::{
    ActionDiagnostic, ActionDiagnosticReportedEvent, ActionExecutedEvent, ActionSource,
    ActionsSettings, FormSnapshot, OpenSequenceCompletedEvent, PdfActionType, ScriptFailedEvent,
};
use crate::controller::ActionsController;

/// A `diagnose` callback: collect, report, or both.
pub(crate) type Diagnose = Rc<dyn Fn(ActionDiagnostic)>;

/// `services/events.ts`: the plugin's events, disposed with it. A throwing
/// listener is reported through the kernel, as `ctx.events.source` does.
pub(crate) struct Events {
    pub(crate) executed: EventHookSource<ActionExecutedEvent>,
    pub(crate) diagnostic_reported: EventHookSource<ActionDiagnosticReportedEvent>,
    pub(crate) script_diagnostic_reported: EventHookSource<ScriptDiagnostic>,
    pub(crate) script_failed: EventHookSource<ScriptFailedEvent>,
    pub(crate) open_sequence_completed: EventHookSource<OpenSequenceCompletedEvent>,
}

impl Events {
    pub(crate) fn new(cx: &ActionsController) -> Self {
        Events {
            executed: cx.event_source(),
            diagnostic_reported: cx.event_source(),
            script_diagnostic_reported: cx.event_source(),
            script_failed: cx.event_source(),
            open_sequence_completed: cx.event_source(),
        }
    }
}

/// `DiagnosticPlace`: which action a diagnostic is about, and what started
/// it, when one is concerned.
#[derive(Clone, Default)]
pub(crate) struct DiagnosticPlace {
    pub(crate) action: Option<PdfActionType>,
    pub(crate) source: Option<ActionSource>,
}

impl DiagnosticPlace {
    pub(crate) fn source(source: ActionSource) -> Self {
        DiagnosticPlace { action: None, source: Some(source) }
    }

    pub(crate) fn of(action: Option<PdfActionType>, source: ActionSource) -> Self {
        DiagnosticPlace { action, source: Some(source) }
    }
}

// ── JSON reads, as JavaScript reads them ──────────────────────────────────────

pub(crate) fn get<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.get(key).filter(|v| !epdf_kernel::is_undefined(v))
}

pub(crate) fn truthy(value: Option<&Value>) -> bool {
    epdf_acrojs::actions::truthy(value)
}

pub(crate) fn str_of<'a>(value: &'a Value, key: &str) -> &'a str {
    get(value, key).and_then(Value::as_str).unwrap_or_default()
}

pub(crate) fn node_type(node: &Value) -> String {
    str_of(node, "type").to_string()
}

/// `a === b` for two JSON numbers (absent is `undefined`).
pub(crate) fn same_number(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => match (a.as_f64(), b.as_f64()) {
            (Some(x), Some(y)) => x == y,
            _ => a == b,
        },
        _ => false,
    }
}

/// `String(value)` for a template literal: `undefined` for an absent member.
fn js_text(value: Option<&Value>) -> String {
    match value {
        None => "undefined".into(),
        Some(value) => epdf_acrojs::js::string_of(value),
    }
}

/// `annotationKey(ref)`: `obj:<n>` or `base:<page>:<index>`, `None` (the
/// TypeScript's `undefined`) for a ref of another kind.
pub fn annotation_key(reference: &Value) -> Option<String> {
    match str_of(reference, "kind") {
        "objectNumber" => Some(format!("obj:{}", js_text(get(reference, "objectNumber")))),
        "baseIndex" => Some(format!(
            "base:{}:{}",
            js_text(get(reference, "page").and_then(|p| get(p, "objectNumber"))),
            js_text(get(reference, "baseIndex"))
        )),
        _ => None,
    }
}

/// `Promise.all([a, b])` for two promises, with its turns: each element
/// reacts one turn after it settles (an `AbortablePromise` is adopted
/// first), and the aggregate settles at the last reaction, or at the first
/// rejection's.
pub(crate) fn all2<A: Clone + 'static, B: Clone + 'static>(
    a: Promise<A>,
    b: Promise<B>,
) -> Promise<(A, B)> {
    let (all, resolver) = Promise::<(A, B)>::pending();
    let slots: Rc<RefCell<(Option<A>, Option<B>)>> = Rc::default();
    let finish = {
        let (slots, resolver) = (slots.clone(), resolver.clone());
        Rc::new(move || {
            let mut slots = slots.borrow_mut();
            if slots.0.is_some() && slots.1.is_some() {
                resolver.resolve((slots.0.take().unwrap(), slots.1.take().unwrap()));
            }
        })
    };
    {
        let (react, slots, finish, resolver) =
            (react_to(a), slots.clone(), finish.clone(), resolver.clone());
        epdf_kernel::rt::spawn(async move {
            match react.await {
                Ok(value) => {
                    slots.borrow_mut().0 = Some(value);
                    finish();
                }
                Err(error) => resolver.reject(error),
            }
        });
    }
    {
        let react = react_to(b);
        epdf_kernel::rt::spawn(async move {
            match react.await {
                Ok(value) => {
                    slots.borrow_mut().1 = Some(value);
                    finish();
                }
                Err(error) => resolver.reject(error),
            }
        });
    }
    all
}

/// `Promise.resolve(x).then(...)`'s reaction: one turn after `x` settles,
/// after the adoption of an `AbortablePromise`; what `.await` on a promise
/// does.
fn react_to<T: Clone + 'static>(promise: Promise<T>) -> epdf_kernel::rt::PromiseFuture<T> {
    std::future::IntoFuture::into_future(promise)
}

impl ActionsCapability {
    /// `ctx.settings().get()`, decoded: read where a setting is used, so a
    /// change applies at once.
    pub(crate) fn settings_now(&self) -> ActionsSettings {
        self.0.table.decode(&self.0.settings.get())
    }

    /// `reportDiagnostic(diagnostic, place)`: fire `onDiagnosticReported`
    /// with the action and source it concerns (`null` for none).
    pub(crate) fn report_diagnostic(&self, diagnostic: &ActionDiagnostic, place: &DiagnosticPlace) {
        self.0.events.diagnostic_reported.emit(&ActionDiagnosticReportedEvent {
            code: diagnostic.code,
            action: place.action.clone(),
            source: place.source.clone(),
        });
    }

    /// A `diagnose` that only reports.
    pub(crate) fn reporter(&self, place: DiagnosticPlace) -> Diagnose {
        let this = self.clone();
        Rc::new(move |d| this.report_diagnostic(&d, &place))
    }

    /// A `diagnose` that collects into `into` and reports.
    pub(crate) fn collector(
        &self,
        into: &Rc<RefCell<Vec<ActionDiagnostic>>>,
        place: DiagnosticPlace,
    ) -> Diagnose {
        let (this, into) = (self.clone(), into.clone());
        Rc::new(move |d: ActionDiagnostic| {
            into.borrow_mut().push(d.clone());
            this.report_diagnostic(&d, &place);
        })
    }

    /// `services/authority.ts`: session authority the action plane enforces
    /// itself (permission, not preference).
    pub(crate) fn allows_print(&self) -> bool {
        self.0.cx.allows("doc.print")
    }

    pub(crate) fn allows(&self, permission: &str) -> bool {
        self.0.cx.allows(permission)
    }

    /// The guarded handle. A document plugin always has one once built.
    pub(crate) fn doc(&self) -> Result<Rc<dyn epdf_kernel::engine::DocumentHandle>, Error> {
        self.0.cx.doc()
    }

    /// `services/form.ts`: the form now, the form plugin's view when it
    /// registered one, else the engine's. `signal` stops the engine read.
    pub(crate) fn read_form(&self, signal: Option<&AbortSignal>) -> Promise<Rc<FormSnapshot>> {
        let reader = self.0.ports.form_reader.borrow().clone();
        if let Some(shown) = reader.and_then(|reader| reader()) {
            return Promise::resolved(shown);
        }
        let task = match self.doc() {
            Ok(doc) => doc.call("forms.list", None, Vec::new()).convert(|v| Ok(Rc::new(v.clone()))),
            Err(error) => Promise::rejected(error),
        };
        self.read(signal, task)
    }

    /// `ctx.cancellable(signal, task)`: the read a caller's signal can stop.
    pub(crate) fn read<T: Clone + 'static>(
        &self,
        signal: Option<&AbortSignal>,
        task: Promise<T>,
    ) -> Promise<T> {
        epdf_kernel::cancellable::cancellable("actions", signal.cloned(), task)
    }

    /// `ctx.getPage(ref)`: the page's `PageLayout`, or `None`.
    pub(crate) fn layout_of(&self, pon: u32) -> Option<Value> {
        self.0.cx.get_page(PageRef::new(pon))
    }

    /// `ctx.document()?.pages[0]?.ref.objectNumber`.
    pub(crate) fn first_pon(&self) -> Option<u32> {
        let meta = self.0.cx.document()?;
        let first = meta.pages.as_array()?.first()?.clone();
        get(&first, "ref").and_then(PageRef::from_json).map(|r| r.page_object_number)
    }
}

/// `services/document-print.ts`: the kernel's print, where the document's
/// own print requests go when the host gave the kernel a printer. The Rust
/// kernel has no print API yet (carried to Task 13.33 with the kernel
/// contract repair), so `available` is false and every request falls back
/// to the UI adapter, as the TypeScript does without a printer.
#[derive(Clone)]
pub(crate) struct DocumentPrint {
    pub(crate) available: Rc<dyn Fn() -> bool>,
    pub(crate) start: Rc<dyn Fn() -> Promise<()>>,
}

impl DocumentPrint {
    pub(crate) fn none() -> Self {
        DocumentPrint {
            available: Rc::new(|| false),
            start: Rc::new(|| Promise::rejected(Error::new("[actions] the kernel has no printer"))),
        }
    }
}
