//! Behaviour the vector comparison holds only implicitly, asserted on its
//! own (the brief's named checks), and the Rust-side wiring no vector
//! reaches.
//!
//! - A whole reset writes every resettable field but reports only the ones
//!   off their defaults (`tooling/plugin-conformance/test/form-live.test.ts`,
//!   the `reset-all` assertions).
//! - A palette placement writes the tool's look, and the shown widget keeps it.
//! - The three palette placement paths stay apart: a form-only success, a
//!   refusal the handler awaits and logs, and the form-only early return,
//!   where the refusal reaches no console line (the Task 14 edge).
//! - Lost object numbers leave the form's held pool
//!   (`services/object-numbers.ts`, `objectNumbers.onLost`).

use std::cell::RefCell;
use std::rc::Rc;

use epdf_kernel::engine::{
    DocumentHandle, DocumentSecurity, Engine, EngineCall, ObjectNumbersLost,
    ObjectNumbersLostReason,
};
use epdf_kernel::kernel::OpenSource;
use epdf_kernel::{Kernel, KernelOptions, OpenOptions, rt};
use epdf_plugin_actions::harness::Actions;
use epdf_plugin_annotation::harness::Annotation;
use epdf_plugin_form::harness::Form;
use epdf_plugin_form::{FORM, form_plugin};
use epdf_plugin_interaction::harness::Interaction;
use epdf_plugin_interaction::{InteractionConfig, interaction_plugin};
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use plugin_conformance::scenario::{Run, run_case};
use plugin_conformance::{Registry, load};
use serde_json::{Value, json};

fn registry() -> Registry {
    Registry::new().with(Interaction).with(Actions).with(Annotation).with(Form)
}

fn case(file: &str, name: &str) -> Value {
    load(file)["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .find(|c| c["name"] == name)
        .unwrap_or_else(|| panic!("{file} has no case {name}"))
        .clone()
}

fn engine_calls(run: &Run, path: &str) -> Vec<Value> {
    run.trace
        .iter()
        .flat_map(|entry| entry["engine"].as_array().cloned().unwrap_or_default())
        .filter(|call| call["path"] == path)
        .collect()
}

fn callbacks(run: &Run, name: &str) -> Vec<Value> {
    run.trace
        .iter()
        .flat_map(|entry| entry["callbacks"].as_array().cloned().unwrap_or_default())
        .filter(|call| call["fn"] == name)
        .collect()
}

fn reports(run: &Run) -> Vec<Value> {
    run.trace
        .iter()
        .flat_map(|entry| entry["reports"].as_array().cloned().unwrap_or_default())
        .collect()
}

fn object_ref(n: u32) -> Value {
    json!({ "kind": "objectNumber", "objectNumber": n })
}

/// A form call step on document `a`.
fn form_call(method: &str, args: Value) -> Value {
    json!({ "op": "call", "target": { "token": "form", "doc": "a" }, "method": method, "args": args })
}

// ── a whole reset ───────────────────────────────────────────────────────────

/// `reset()` with no refs writes one `forms.reset` naming every resettable
/// field (the push button left out), and resolves only the fields whose
/// value the reset moved: the checkbox, already at its default, is not one.
#[test]
fn a_whole_reset_reports_only_the_fields_it_moved() {
    let run = run_case(&case("plugins/form-fill.json", "reset-all"), &registry());
    let applied = engine_calls(&run, "apply");
    assert_eq!(
        applied[0]["args"][0]["ops"],
        json!([{ "type": "forms.reset", "fields": [object_ref(1), object_ref(2), object_ref(3), object_ref(4)] }]),
        "the push button is not reset; the four value fields are",
    );
    let resolved: Vec<Value> = run.trace[2]["settled"]
        .as_array()
        .expect("the reset settles in its step")
        .iter()
        .map(|s| s["resolved"].clone())
        .collect();
    let refs: Vec<Value> = resolved[0]["fields"]
        .as_array()
        .expect("the changed fields")
        .iter()
        .map(|f| f["ref"].clone())
        .collect();
    assert_eq!(refs, vec![object_ref(1), object_ref(3), object_ref(4)]);
}

// ── palette placement ───────────────────────────────────────────────────────

/// A drag with the text tool and no annotation plugin writes the tool
/// table's look (white inside, the palette's gray border, 12 point text),
/// and the widget the view shows before the engine answers carries it: the
/// write is held (`defer`), so `getWidget` reads the prediction, not the
/// engine's canned answer.
#[test]
fn a_palette_placement_keeps_the_tool_look() {
    let mut placed = case("plugins/form-design.json", "a-thin-drag-is-a-drag");
    let responses = placed["engine"]["documents"]["a"]["responses"].as_array_mut().unwrap();
    let apply = responses.iter_mut().find(|r| r["path"] == "apply").unwrap();
    apply["defer"] = json!("created");
    let steps = placed["steps"].as_array_mut().unwrap();
    let widget = json!({ "kind": "objectNumber", "page": object_ref(1), "objectNumber": 10002 });
    steps.push(form_call("getWidget", json!([widget])));
    let run = run_case(&placed, &registry());
    let draft = &engine_calls(&run, "apply")[0]["args"][0]["ops"][0]["draft"];
    let placement = &draft["widgets"][0];
    assert_eq!(placement["interiorColor"], json!("#ffffff"));
    assert_eq!(placement["color"], json!("#6b7280"));
    assert_eq!(placement["strokeWidth"], json!(1));
    assert_eq!(placement["fontSize"], json!(12));
    let shown = &run.trace.last().unwrap()["outcome"]["returned"];
    assert_eq!(shown["interiorColor"], json!("#ffffff"), "{shown}");
    assert_eq!(shown["color"], json!("#6b7280"), "{shown}");
}

/// Form-only success: the field is created, and nothing is logged.
#[test]
fn a_form_only_placement_succeeds_quietly() {
    let run = run_case(&case("plugins/form-design.json", "a-thin-drag-is-a-drag"), &registry());
    assert_eq!(engine_calls(&run, "apply").len(), 1);
    assert!(callbacks(&run, "console.error").is_empty());
    let listed = &run.trace.last().unwrap()["outcome"]["returned"];
    assert_eq!(listed[0]["name"], json!("text_1"), "{listed}");
    assert!(reports(&run).is_empty());
}

/// With the annotation plugin the handler awaits the create, so the
/// engine's refusal is logged as `[form] placing the field failed:`.
#[test]
fn an_awaited_placement_refusal_is_logged() {
    let run =
        run_case(&case("plugins/form-design.json", "a-failed-placement-is-logged"), &registry());
    let logged = callbacks(&run, "console.error");
    assert_eq!(logged.len(), 1);
    assert_eq!(logged[0]["args"][0], json!("[form] placing the field failed:"));
    assert_eq!(logged[0]["args"][1]["$error"]["message"], json!("intended placement refusal"));
}

/// Without the annotation plugin a field the view already shows ends the
/// placement before `created` is awaited, as `tools/handlers.ts` returns
/// early. The engine's refusal then reaches no console line and no report:
/// the TypeScript raises an unhandled rejection here, and Rust has no such
/// channel. The refused field leaves the view. This is the Task 14 edge;
/// nothing catches the refusal on the plugin's behalf.
#[test]
fn a_form_only_placement_refusal_ends_before_it_is_awaited() {
    let mut refused = case("plugins/form-design.json", "a-thin-drag-is-a-drag");
    let responses = refused["engine"]["documents"]["a"]["responses"].as_array_mut().unwrap();
    let apply = responses.iter_mut().find(|r| r["path"] == "apply").unwrap();
    *apply = json!({
        "path": "apply", "times": 1,
        "error": { "name": "EngineError", "message": "intended placement refusal", "code": "Refused" },
    });
    let run = run_case(&refused, &registry());
    assert_eq!(engine_calls(&run, "apply").len(), 1, "the create was written");
    assert!(callbacks(&run, "console.error").is_empty(), "no console line");
    assert!(reports(&run).is_empty(), "no report");
    let listed = &run.trace.last().unwrap()["outcome"]["returned"];
    assert_eq!(listed, &json!([]), "the refused field left the view");
}

// ── lost object numbers ─────────────────────────────────────────────────────

type LostListeners = Rc<RefCell<Vec<Rc<dyn Fn(&ObjectNumbersLost)>>>>;

/// The fake engine, with documents whose pool can lose numbers.
struct LosingEngine(AsEngine, LostListeners);

impl Engine for LosingEngine {
    fn open(&self, input: Value, options: Option<Value>) -> EngineCall<Rc<dyn DocumentHandle>> {
        let listeners = self.1.clone();
        self.0.open(input, options).convert(move |inner| {
            Ok(Rc::new(LosingHandle(inner.clone(), listeners.clone())) as Rc<dyn DocumentHandle>)
        })
    }
    fn destroy(&self) -> EngineCall<()> {
        self.0.destroy()
    }
}

struct LosingHandle(Rc<dyn DocumentHandle>, LostListeners);

impl DocumentHandle for LosingHandle {
    fn is_local(&self) -> bool {
        self.0.is_local()
    }
    fn held_object_numbers(&self) -> usize {
        self.0.held_object_numbers()
    }
    fn take_object_number(&self) -> Result<Option<u32>, epdf_kernel::Error> {
        self.0.take_object_number()
    }
    fn on_lost_object_numbers(
        &self,
        listener: Rc<dyn Fn(&ObjectNumbersLost)>,
    ) -> epdf_kernel::Unsubscribe {
        self.1.borrow_mut().push(listener);
        epdf_kernel::Unsubscribe::noop()
    }
    fn reserve_object_numbers(&self, count: u32) -> EngineCall<()> {
        self.0.reserve_object_numbers(count)
    }
    fn id(&self) -> String {
        self.0.id()
    }
    fn capabilities(&self) -> Value {
        self.0.capabilities()
    }
    fn security(&self) -> Rc<dyn DocumentSecurity> {
        self.0.security()
    }
    fn has(&self, member: &str) -> bool {
        self.0.has(member)
    }
    fn call(&self, path: &str, pon: Option<u32>, args: Vec<Value>) -> EngineCall<Value> {
        self.0.call(path, pon, args)
    }
    fn page_index(&self, pon: u32) -> Result<u32, epdf_kernel::Error> {
        self.0.page_index(pon)
    }
    fn subscribe(&self, listener: Rc<dyn Fn(&Value)>) -> epdf_kernel::Unsubscribe {
        self.0.subscribe(listener)
    }
}

fn page_row() -> Value {
    let size = json!({ "x": 0, "y": 0, "width": 600, "height": 800 });
    json!({
        "index": 0, "ref": object_ref(1), "label": null,
        "size": { "width": 600, "height": 800 }, "rotation": 0, "userUnit": 1, "tabOrder": null,
        "pdfCropBox": { "left": 0, "bottom": 0, "right": 600, "top": 800 },
        "boxes": { "media": size, "crop": size, "bleed": size, "trim": size, "art": size }
    })
}

/// `services/object-numbers.ts`: a create that finds the pool short holds
/// what it took and waits for a reservation; a number the engine reports
/// lost meanwhile leaves the hold, so the create names fresh ones.
#[test]
fn a_lost_object_number_leaves_the_held_pool() {
    rt::reset();
    let listeners: LostListeners = Rc::default();
    let engines = listeners.clone();
    let fake = FakeEngine::new(json!({
        "documents": { "a": {
            "pages": [page_row()],
            "security": { "allows": "*" },
            "objectNumbers": [10001],
            "responses": [
                { "path": "forms.list", "value": {
                    "formKind": "acroform", "needsAppearances": false,
                    "fields": [], "widgets": [], "calculationOrder": [] } },
                { "path": "apply", "defer": "created" }
            ]
        } }
    }));
    let kernel = Kernel::new(
        Rc::new(LosingEngine(AsEngine(fake.clone()), engines)),
        vec![interaction_plugin(InteractionConfig::default()), form_plugin(None)],
        KernelOptions { report: None, on_dispatch: None, release_closed_sessions: false },
    )
    .unwrap();
    let _ = kernel.start();
    rt::run_until_stalled();
    let _ = kernel.documents().open(
        OpenSource::Input(json!({ "kind": "bytes", "id": "a", "bytes": { "$bytes": "" } })),
        OpenOptions::default(),
    );
    rt::run_until_stalled();
    let form = kernel.capability(&FORM, Some("a")).expect("the form capability");
    let draft = json!({
        "family": "text", "name": "name",
        "widgets": [{ "page": object_ref(1), "rect": { "x": 10, "y": 20, "width": 100, "height": 24 } }],
    });
    // The field and its widget take two numbers; the pool holds one.
    let _created = form.create(draft, None);
    assert_eq!(listeners.borrow().len(), 1, "watching once the form takes numbers");
    let lost =
        ObjectNumbersLost { numbers: vec![10001], reason: ObjectNumbersLostReason::Versioned };
    let watching: Vec<_> = listeners.borrow().clone();
    for listener in watching {
        listener(&lost);
    }
    rt::run_until_stalled();
    let applied: Vec<Value> =
        fake.log_since(0).into_iter().filter(|call| call["path"] == "apply").collect();
    let op = &applied.last().expect("the create was written")["args"][0]["ops"][0];
    assert_ne!(op["objectNumber"], json!(10001), "{op}");
    assert!(!op["widgetObjectNumbers"].as_array().unwrap().contains(&json!(10001)), "{op}");
    assert_eq!(listeners.borrow().len(), 1, "one subscription");
}

// ── the painter ─────────────────────────────────────────────────────────────

/// No vector paints a widget, so this one does: a text field, a checkbox
/// and a listbox from a vector's snapshot each decode for the painter and
/// are drawn (an unchecked, borderless checkbox's scene is empty), and
/// nothing reaches the console (a DTO the painter can no
/// longer read is logged as `[form] the painter could not read a widget:`).
#[test]
fn the_painter_draws_a_text_field_a_checkbox_and_a_listbox() {
    rt::reset();
    let logged: Rc<RefCell<Vec<Value>>> = Rc::default();
    let sink = logged.clone();
    let previous = epdf_plugin_form::console::set_sink(Some(Rc::new(move |_, args| {
        sink.borrow_mut().push(Value::Array(args));
    })));
    let mut spec = case("plugins/form-fill.json", "hydrates-on-open")["engine"].clone();
    let list = spec["documents"]["a"]["responses"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|r| r["path"] == "forms.list")
        .expect("the form");
    // The case holds the read back; this one answers it at once.
    list.as_object_mut().unwrap().remove("defer");
    let snapshot = list["value"].clone();
    let kernel = Kernel::new(
        Rc::new(AsEngine(FakeEngine::new(spec))),
        vec![interaction_plugin(InteractionConfig::default()), form_plugin(None)],
        KernelOptions { report: None, on_dispatch: None, release_closed_sessions: false },
    )
    .unwrap();
    let _ = kernel.start();
    rt::run_until_stalled();
    let _ = kernel.documents().open(
        OpenSource::Input(json!({ "kind": "bytes", "id": "a", "bytes": { "$bytes": "" } })),
        OpenOptions::default(),
    );
    rt::run_until_stalled();
    let form = kernel.capability(&FORM, Some("a")).expect("the form capability");
    let mut painted = Vec::new();
    for field in snapshot["fields"].as_array().unwrap() {
        let family = field["family"].as_str().unwrap();
        if !["text", "checkbox", "listbox"].contains(&family) {
            continue;
        }
        let widget = &field["widgets"][0]["ref"];
        let painting = form.get_widget_painting(widget);
        assert!(painting.is_some(), "the {family} widget is drawn");
        painted.push(family);
    }
    epdf_plugin_form::console::set_sink(previous);
    painted.sort_unstable();
    assert_eq!(painted, ["checkbox", "listbox", "text"]);
    assert!(logged.borrow().is_empty(), "{:?}", logged.borrow());
}
