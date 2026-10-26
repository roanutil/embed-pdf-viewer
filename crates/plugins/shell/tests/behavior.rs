//! The typed API beyond what the vectors print, against
//! `packages/plugin/shell/src/{controller,model}.ts`: reads keep their
//! identity (`memo`, `memoByKey`) until their input changes, a repeated
//! focused open raises the request without another opened event, a menu
//! change announces closes before opens, surfaces enumerate in JavaScript
//! property order, a closed document's shell is gone and a reopened one
//! starts empty, and the `Object.prototype` names read as the TypeScript
//! reads them.

use std::cell::RefCell;
use std::rc::Rc;

use epdf_kernel::kernel::{OpenOptions, OpenSource};
use epdf_kernel::{Error, Kernel, KernelOptions, rt};
use epdf_plugin_shell::*;
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use serde_json::{Value, json};

struct Rig {
    kernel: Kernel,
    reports: Rc<RefCell<Vec<String>>>,
}

impl Rig {
    fn new() -> Rig {
        rt::reset();
        let reports: Rc<RefCell<Vec<String>>> = Rc::default();
        let heard = reports.clone();
        let fake = FakeEngine::new(json!({ "documents": { "a": { "pages": [] } } }));
        let kernel = Kernel::new(
            Rc::new(AsEngine(fake)),
            vec![shell_plugin()],
            KernelOptions {
                report: Some(Rc::new(move |error: &Error| {
                    heard.borrow_mut().push(error.message.clone())
                })),
                ..KernelOptions::default()
            },
        )
        .expect("kernel");
        kernel.start();
        rt::run_until_stalled();
        let rig = Rig { kernel, reports };
        rig.open("a");
        rig
    }

    fn open(&self, id: &str) {
        self.kernel.documents().open(
            OpenSource::Input(json!({ "kind": "bytes", "id": id, "bytes": { "$bytes": "" } })),
            OpenOptions::default(),
        );
        rt::run_until_stalled();
    }

    fn close(&self, id: &str) {
        let _ = self.kernel.documents().close(id);
        rt::run_until_stalled();
    }

    fn shell(&self, id: &str) -> Option<ShellCapability> {
        self.kernel.capability(&SHELL, Some(id)).ok()
    }
}

/// Every event of `shell`, in order, as `kind:id`.
fn log_events(shell: &ShellCapability) -> Rc<RefCell<Vec<String>>> {
    let log: Rc<RefCell<Vec<String>>> = Rc::default();
    let push = |log: &Rc<RefCell<Vec<String>>>, kind: &'static str| {
        let log = log.clone();
        move |id: &str| -> Result<(), Error> {
            log.borrow_mut().push(format!("{kind}:{id}"));
            Ok(())
        }
    };
    let (opened, closed) = (push(&log, "opened"), push(&log, "closed"));
    let (menu_opened, menu_closed) = (push(&log, "menu-opened"), push(&log, "menu-closed"));
    shell.on_surface_opened(move |e| opened(&e.id));
    shell.on_surface_closed(move |e| closed(&e.id));
    shell.on_menu_opened(move |e| menu_opened(&e.id));
    shell.on_menu_closed(move |e| menu_closed(&e.id));
    log
}

fn focused(exclusive: Option<&str>) -> Option<OpenSurfaceOptions> {
    Some(OpenSurfaceOptions { exclusive: exclusive.map(str::to_string), props: None, focus: true })
}

fn ids(surfaces: &[Rc<SurfaceState>]) -> Vec<&str> {
    surfaces.iter().map(|s| s.id.as_str()).collect()
}

fn snapshot(value: Value) -> ShellSnapshot {
    serde_json::from_value(value).expect("snapshot")
}

#[test]
fn reads_keep_their_identity_until_their_input_changes() {
    let rig = Rig::new();
    let shell = rig.shell("a").expect("shell");
    shell.open("left", None);
    shell.open("free", None);

    let left = shell.get_surface("left").expect("left");
    assert!(Rc::ptr_eq(&left, &shell.get_surface("left").expect("left")));
    let list = shell.list_open_surfaces();
    assert!(Rc::ptr_eq(&list, &shell.list_open_surfaces()));
    // The list hands out the same objects `getSurface` does.
    assert!(Rc::ptr_eq(&list[0], &left));
    let snapshot = shell.get_snapshot();
    assert!(Rc::ptr_eq(&snapshot, &shell.get_snapshot()));
    let menus = shell.list_open_menus();
    assert!(Rc::ptr_eq(&menus, &shell.list_open_menus()));

    // A menu write changes neither surface read, and the menu list is new.
    shell.open_menu("file");
    assert!(Rc::ptr_eq(&list, &shell.list_open_surfaces()));
    assert!(!Rc::ptr_eq(&menus, &shell.list_open_menus()));
    assert!(!Rc::ptr_eq(&snapshot, &shell.get_snapshot()));

    // A write that changes nothing keeps every read.
    let snapshot = shell.get_snapshot();
    shell.close("nope");
    shell.open_menu("file");
    assert!(Rc::ptr_eq(&snapshot, &shell.get_snapshot()));
    assert!(Rc::ptr_eq(&list, &shell.list_open_surfaces()));

    // A surface never opened reads `None` every time.
    assert!(shell.get_surface("nope").is_none());
}

#[test]
fn a_repeated_focused_open_raises_the_request_with_no_opened_event() {
    let rig = Rig::new();
    let shell = rig.shell("a").expect("shell");
    let log = log_events(&shell);
    shell.open("free", None);
    shell.open("left", focused(Some("side")));
    assert_eq!(*log.borrow(), ["opened:free", "opened:left"]);
    let free = shell.get_surface("free").expect("free");
    let first = shell.get_surface("left").expect("left");
    assert_eq!(first.focus_request(), 1.0);
    let list = shell.list_open_surfaces();

    shell.open("left", focused(Some("side")));
    shell.open("left", focused(Some("side")));
    assert_eq!(*log.borrow(), ["opened:free", "opened:left"], "no second opened event");
    let again = shell.get_surface("left").expect("left");
    assert!(!Rc::ptr_eq(&first, &again), "a new record, so a new view");
    assert_eq!(again.focus_request(), 3.0);
    assert!(Rc::ptr_eq(&again, &shell.get_surface("left").expect("left")));
    // The untouched surface keeps its object; the list is rebuilt around it.
    assert!(Rc::ptr_eq(&free, &shell.get_surface("free").expect("free")));
    let relisted = shell.list_open_surfaces();
    assert!(!Rc::ptr_eq(&list, &relisted));
    assert!(Rc::ptr_eq(&relisted[0], &free));

    // An open without focus keeps the count; a close clears it, and the
    // snapshot never carries it.
    shell.open("left", Some(OpenSurfaceOptions::default()));
    assert_eq!(shell.get_surface("left").expect("left").focus_request(), 3.0);
    assert_eq!(
        epdf_kernel::js::to_value(&*shell.get_snapshot())["surfaces"]["left"],
        json!({ "open": true })
    );
    shell.close("left");
    assert_eq!(shell.get_surface("left").expect("left").focus_request(), 0.0);
    assert_eq!(*log.borrow(), ["opened:free", "opened:left", "closed:left"]);
}

#[test]
fn a_menu_change_announces_its_closes_before_its_opens() {
    let rig = Rig::new();
    let shell = rig.shell("a").expect("shell");
    shell.open("gone", None);
    shell.open_menu("file");
    shell.open_menu("edit");
    let log = log_events(&shell);

    shell.apply_snapshot(&snapshot(json!({
        "surfaces": { "kept": { "open": true } },
        "openMenus": ["view", "edit", "help"],
    })));
    assert_eq!(
        *log.borrow(),
        ["closed:gone", "opened:kept", "menu-closed:file", "menu-opened:view", "menu-opened:help"]
    );
    assert_eq!(*shell.list_open_menus(), ["view", "edit", "help"]);

    log.borrow_mut().clear();
    shell.toggle_menu("edit");
    shell.toggle_menu("edit");
    shell.close_all_menus();
    assert_eq!(
        *log.borrow(),
        [
            "menu-closed:edit",
            "menu-opened:edit",
            "menu-closed:view",
            "menu-closed:help",
            "menu-closed:edit",
        ]
    );
}

#[test]
fn surfaces_enumerate_in_javascript_key_order() {
    let rig = Rig::new();
    let shell = rig.shell("a").expect("shell");
    let log = log_events(&shell);
    for id in ["b", "10", "2", "a", "01"] {
        shell.open(id, None);
    }
    assert_eq!(ids(&shell.list_open_surfaces()), ["2", "10", "b", "a", "01"]);
    let keys: Vec<String> = epdf_kernel::js::to_value(&*shell.get_snapshot())["surfaces"]
        .as_object()
        .expect("surfaces")
        .keys()
        .cloned()
        .collect();
    assert_eq!(keys, ["2", "10", "b", "a", "01"]);

    log.borrow_mut().clear();
    shell.close_all();
    assert_eq!(*log.borrow(), ["closed:2", "closed:10", "closed:b", "closed:a", "closed:01"]);
}

#[test]
fn a_closed_document_drops_its_shell_and_a_reopened_one_starts_empty() {
    let rig = Rig::new();
    let shell = rig.shell("a").expect("shell");
    let log = log_events(&shell);
    shell.open("left", focused(None));
    shell.open_menu("file");
    let before = log.borrow().len();

    rig.close("a");
    assert!(rig.shell("a").is_none(), "no capability for a closed document");

    rig.open("a");
    let reopened = rig.shell("a").expect("shell");
    assert!(!reopened.is_open("left"));
    assert!(reopened.get_surface("left").is_none());
    assert!(reopened.list_open_surfaces().is_empty());
    assert!(reopened.list_open_menus().is_empty());
    assert_eq!(
        epdf_kernel::js::to_value(&*reopened.get_snapshot()),
        json!({ "surfaces": {}, "openMenus": [] })
    );

    // The new instance's writes reach none of the old instance's listeners.
    reopened.open("left", None);
    reopened.open_menu("file");
    assert_eq!(log.borrow().len(), before);
    assert!(rig.reports.borrow().is_empty(), "{:?}", rig.reports.borrow());
}

#[test]
fn object_prototype_names_read_as_the_typescript_reads_them() {
    let rig = Rig::new();
    let shell = rig.shell("a").expect("shell");
    // `getSurface('toString')` finds the inherited function: `{ id }` alone,
    // the same object every time.
    let inherited = shell.get_surface("toString").expect("inherited");
    assert_eq!(epdf_kernel::js::to_value(&*inherited), json!({ "id": "toString" }));
    assert!(!inherited.open());
    assert!(Rc::ptr_eq(&inherited, &shell.get_surface("toString").expect("inherited")));
    assert!(!shell.is_open("constructor"));

    // Closing one changes nothing; toggling one opens an own surface.
    let snapshot = shell.get_snapshot();
    shell.close("toString");
    assert!(Rc::ptr_eq(&snapshot, &shell.get_snapshot()));
    shell.toggle("valueOf", None);
    assert!(shell.is_open("valueOf"));
    assert_eq!(
        epdf_kernel::js::to_value(&*shell.get_surface("valueOf").expect("own")),
        json!({ "id": "valueOf", "open": true, "focusRequest": 0 })
    );
}

#[test]
fn records_print_their_members_in_the_order_they_were_built() {
    let rig = Rig::new();
    let shell = rig.shell("a").expect("shell");
    // A restored record has no props member, so the spread puts it last.
    shell.apply_snapshot(&snapshot(json!({
        "surfaces": { "modal": { "open": false, "exclusive": "overlay" } },
        "openMenus": [],
    })));
    shell.update_surface_props("modal", serde_json::from_value(json!({ "x": 1 })).expect("props"));
    let text = serde_json::to_string(&epdf_kernel::js::to_value(
        &*shell.get_surface("modal").expect("modal"),
    ))
    .expect("json");
    assert_eq!(
        text,
        r#"{"id":"modal","open":false,"exclusive":"overlay","focusRequest":0,"props":{"x":1}}"#
    );
    // An opened record is the literal's order, whatever came before.
    shell.open(
        "modal",
        Some(OpenSurfaceOptions { exclusive: Some("overlay".into()), ..Default::default() }),
    );
    let text = serde_json::to_string(&epdf_kernel::js::to_value(
        &*shell.get_surface("modal").expect("modal"),
    ))
    .expect("json");
    assert_eq!(text, r#"{"id":"modal","open":true,"exclusive":"overlay","focusRequest":0}"#);
}

#[test]
fn a_failing_listener_goes_to_the_kernel_report_and_the_rest_still_hear() {
    let rig = Rig::new();
    let shell = rig.shell("a").expect("shell");
    shell.on_surface_opened(|_| Err(Error::new("listener boom")));
    let log = log_events(&shell);
    shell.open("left", None);
    assert_eq!(*log.borrow(), ["opened:left"]);
    assert_eq!(rig.reports.borrow().len(), 1);
    assert!(rig.reports.borrow()[0].contains("listener boom"), "{:?}", rig.reports.borrow());
}
