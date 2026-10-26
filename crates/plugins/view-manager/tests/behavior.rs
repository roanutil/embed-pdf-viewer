//! The typed API beyond what the vectors print, against
//! `packages/plugin/view-manager/src/{controller,model}.ts`: `removePane`
//! hands its documents to the explicit, the focused or the last pane and
//! moves nothing for itself or an unknown target, every intermediate
//! `ctx.state.update` announces its own events in order, pane ids come from
//! the counter and land where `createPane`/`splitPane`/`movePane` put them,
//! and the reads keep their identity (`memo`, `memoByKey`) until their input
//! changes.

use std::cell::RefCell;
use std::rc::Rc;

use epdf_kernel::kernel::{OpenOptions, OpenSource};
use epdf_kernel::{Error, Kernel, KernelOptions, rt};
use epdf_plugin_view_manager::*;
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use serde_json::json;

struct Rig {
    kernel: Kernel,
    reports: Rc<RefCell<Vec<String>>>,
}

impl Rig {
    /// A started kernel with documents `a` to `d` in the engine and none
    /// open.
    fn new() -> Rig {
        rt::reset();
        let reports: Rc<RefCell<Vec<String>>> = Rc::default();
        let heard = reports.clone();
        let fake = FakeEngine::new(json!({ "documents": {
            "a": { "pages": [] }, "b": { "pages": [] }, "c": { "pages": [] }, "d": { "pages": [] }
        } }));
        let kernel = Kernel::new(
            Rc::new(AsEngine(fake)),
            vec![view_manager_plugin()],
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
        Rig { kernel, reports }
    }

    fn open(&self, id: &str) {
        self.kernel.documents().open(
            OpenSource::Input(json!({ "kind": "bytes", "id": id, "bytes": { "$bytes": "" } })),
            OpenOptions::default(),
        );
        rt::run_until_stalled();
    }

    fn vm(&self) -> ViewManagerCapability {
        self.kernel.capability(&VIEW_MANAGER, None).expect("view-manager")
    }
}

/// `pane-id: documents (active)`, in pane order.
fn layout(vm: &ViewManagerCapability) -> Vec<String> {
    vm.list_panes()
        .iter()
        .map(|pane| {
            format!(
                "{}: {} ({})",
                pane.id,
                pane.document_ids.join(","),
                pane.active_document_id.as_deref().unwrap_or("-")
            )
        })
        .collect()
}

/// Every event of `vm`, in order.
fn log_events(vm: &ViewManagerCapability) -> Rc<RefCell<Vec<String>>> {
    let log: Rc<RefCell<Vec<String>>> = Rc::default();
    let push = |log: &Rc<RefCell<Vec<String>>>| {
        let log = log.clone();
        move |line: String| -> Result<(), Error> {
            log.borrow_mut().push(line);
            Ok(())
        }
    };
    let (created, removed, focus, moved) = (push(&log), push(&log), push(&log), push(&log));
    vm.on_pane_created(move |e| created(format!("created {}", e.pane_id)));
    vm.on_pane_removed(move |e| removed(format!("removed {}", e.pane_id)));
    vm.on_focus_changed(move |e| {
        focus(format!("focus {}", e.pane_id.as_deref().unwrap_or("null")))
    });
    vm.on_document_moved(move |e| {
        moved(format!(
            "moved {} {} -> {}",
            e.document_id,
            e.from_pane_id.as_deref().unwrap_or("null"),
            e.to_pane_id.as_deref().unwrap_or("null")
        ))
    });
    log
}

/// Panes `pane-1` to `pane-4` holding `a` to `d`, `pane-4` focused.
fn four_panes() -> (Rig, ViewManagerCapability) {
    let rig = Rig::new();
    let vm = rig.vm();
    rig.open("a");
    for id in ["b", "c", "d"] {
        vm.create_pane(&[]);
        rig.open(id);
    }
    assert_eq!(layout(&vm), ["pane-1: a (a)", "pane-2: b (b)", "pane-3: c (c)", "pane-4: d (d)"]);
    assert_eq!(vm.get_focused_pane_id().as_deref(), Some("pane-4"));
    (rig, vm)
}

#[test]
fn removing_a_pane_moves_its_documents_to_the_explicit_target() {
    let (_rig, vm) = four_panes();
    vm.add_document("pane-4", "c", None);
    vm.remove_pane("pane-4", Some("pane-1"));
    // Each document becomes the target's active one in turn.
    assert_eq!(layout(&vm), ["pane-1: a,d,c (c)", "pane-2: b (b)", "pane-3:  (-)"]);
    assert_eq!(vm.get_focused_pane_id().as_deref(), Some("pane-1"));
}

#[test]
fn removing_a_pane_moves_its_documents_to_the_focused_one() {
    let (_rig, vm) = four_panes();
    vm.set_focused_pane(Some("pane-2"));
    vm.remove_pane("pane-3", None);
    assert_eq!(layout(&vm), ["pane-1: a (a)", "pane-2: b,c (c)", "pane-4: d (d)"]);
    assert_eq!(vm.get_focused_pane_id().as_deref(), Some("pane-2"));
}

#[test]
fn removing_the_focused_pane_moves_its_documents_to_the_last_one() {
    let (_rig, vm) = four_panes();
    vm.set_focused_pane(Some("pane-2"));
    vm.remove_pane("pane-2", None);
    assert_eq!(layout(&vm), ["pane-1: a (a)", "pane-3: c (c)", "pane-4: d,b (b)"]);
    assert_eq!(vm.get_focused_pane_id().as_deref(), Some("pane-4"));
}

#[test]
fn a_stale_focus_falls_through_to_no_move() {
    let (_rig, vm) = four_panes();
    // `focused !== id ? focused : null` keeps a stale id, which is no pane.
    vm.set_focused_pane(Some("ghost"));
    vm.remove_pane("pane-2", None);
    assert_eq!(layout(&vm), ["pane-1: a (a)", "pane-3: c (c)", "pane-4: d (d)"]);
    assert_eq!(vm.get_pane_of_document("b"), None);
}

#[test]
fn a_self_or_unknown_target_moves_nothing() {
    let (_rig, vm) = four_panes();
    vm.remove_pane("pane-2", Some("pane-2"));
    assert_eq!(layout(&vm), ["pane-1: a (a)", "pane-3: c (c)", "pane-4: d (d)"]);
    assert_eq!(vm.get_pane_of_document("b"), None);
    // An explicit target is never replaced by the focused or the last pane.
    vm.remove_pane("pane-3", Some("ghost"));
    assert_eq!(layout(&vm), ["pane-1: a (a)", "pane-4: d (d)"]);
    assert_eq!(vm.get_pane_of_document("c"), None);
    // Unknown panes are ignored.
    vm.remove_pane("ghost", Some("pane-1"));
    assert_eq!(layout(&vm), ["pane-1: a (a)", "pane-4: d (d)"]);
}

#[test]
fn every_intermediate_update_announces_its_own_events() {
    let (rig, vm) = four_panes();
    let log = log_events(&vm);
    // `createPane` writes the pane, then moves each document in one update
    // at a time: created and focus first, then one move per document. An id
    // no pane holds (`zzz`) is added from no pane, as `addDocument` adds it.
    assert_eq!(vm.create_pane(&["c".into(), "a".into(), "zzz".into()]), "pane-5");
    assert_eq!(
        *log.borrow(),
        [
            "created pane-5",
            "focus pane-5",
            "moved c pane-3 -> pane-5",
            "moved a pane-1 -> pane-5",
            "moved zzz null -> pane-5",
        ]
    );
    log.borrow_mut().clear();
    // `removePane` moves each document (focusing the target), then removes.
    vm.remove_pane("pane-5", Some("pane-2"));
    assert_eq!(
        *log.borrow(),
        [
            "focus pane-2",
            "moved c pane-5 -> pane-2",
            "moved a pane-5 -> pane-2",
            "moved zzz pane-5 -> pane-2",
            "removed pane-5",
        ]
    );
    assert_eq!(
        layout(&vm),
        ["pane-1:  (-)", "pane-2: b,c,a,zzz (zzz)", "pane-3:  (-)", "pane-4: d (d)"]
    );
    assert!(rig.reports.borrow().is_empty(), "{:?}", rig.reports.borrow());
}

#[test]
fn document_moves_follow_the_previous_then_the_next_panes_in_key_order() {
    let (_rig, vm) = four_panes();
    vm.set_focused_pane(Some("pane-1"));
    let log = log_events(&vm);
    // One update moves `d` out of pane-4 and into pane-1; documents are
    // enumerated from the previous record's panes first (a, b, c, d).
    vm.move_document_between("pane-4", "pane-1", "d", Some(0.0));
    assert_eq!(*log.borrow(), ["moved d pane-4 -> pane-1"]);
    log.borrow_mut().clear();
    // A write that leaves the panes record alone announces no move.
    vm.set_focused_pane(Some("pane-3"));
    assert_eq!(*log.borrow(), ["focus pane-3"]);
}

#[test]
fn pane_ids_come_from_the_counter_and_land_where_asked() {
    let rig = Rig::new();
    let vm = rig.vm();
    rig.open("a");
    rig.open("b");
    assert_eq!(layout(&vm), ["pane-1: a,b (a)"]);
    // `splitPane` goes beside the focused pane.
    assert_eq!(vm.split_pane("b", None), "pane-2");
    assert_eq!(vm.create_pane(&[]), "pane-3");
    // ... or beside `from`, here between pane-1 and pane-2.
    assert_eq!(vm.split_pane("a", Some("pane-1")), "pane-4");
    assert_eq!(*vm.get_pane_order(), ["pane-1", "pane-4", "pane-2", "pane-3"]);
    // A `from` that is no pane puts it at the end.
    assert_eq!(vm.split_pane("b", Some("ghost")), "pane-5");
    assert_eq!(*vm.get_pane_order(), ["pane-1", "pane-4", "pane-2", "pane-3", "pane-5"]);
    // Removal never reuses an id.
    vm.remove_pane("pane-5", None);
    assert_eq!(vm.create_pane(&[]), "pane-6");
    vm.move_pane("pane-6", 0.0);
    vm.move_pane("pane-1", 2.9);
    vm.move_pane("pane-4", f64::INFINITY);
    assert_eq!(*vm.get_pane_order(), ["pane-6", "pane-1", "pane-2", "pane-3", "pane-4"]);
}

#[test]
fn reads_keep_their_identity_until_their_input_changes() {
    let (_rig, vm) = four_panes();
    let list = vm.list_panes();
    let one = vm.get_pane("pane-1").unwrap();
    let two = vm.get_pane("pane-2").unwrap();
    assert!(Rc::ptr_eq(&list, &vm.list_panes()));
    assert!(Rc::ptr_eq(&one, &vm.get_pane("pane-1").unwrap()));
    assert!(Rc::ptr_eq(&list[0], &one));
    let order = vm.get_pane_order();
    // A focus change leaves panes and order alone.
    vm.set_focused_pane(Some("pane-1"));
    assert!(Rc::ptr_eq(&list, &vm.list_panes()));
    assert!(Rc::ptr_eq(&order, &vm.get_pane_order()));
    // A tab change rebuilds only that pane's view.
    vm.move_document_between("pane-1", "pane-2", "a", None);
    let after = vm.list_panes();
    assert!(!Rc::ptr_eq(&list, &after));
    assert!(!Rc::ptr_eq(&one, &after[0]));
    assert!(!Rc::ptr_eq(&two, &after[1]));
    assert!(Rc::ptr_eq(&list[2], &after[2]));
    assert!(Rc::ptr_eq(&list[3], &after[3]));
    assert!(Rc::ptr_eq(&order, &vm.get_pane_order()));
    // A no-op verb writes nothing, so nothing is rebuilt.
    vm.set_active_document("pane-2", Some("zzz"));
    assert!(Rc::ptr_eq(&after, &vm.list_panes()));
    assert!(vm.get_pane("ghost").is_none());
}
