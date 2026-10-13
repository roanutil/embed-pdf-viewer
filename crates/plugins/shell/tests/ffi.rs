#![cfg(feature = "uniffi")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use epdf_ffi::{KernelThread, ViewerError};
use epdf_plugin_shell::ffi::{
    ShellEvent, ShellEventListener, ShellHandle, ShellMenuEvent, ShellOpenOptions, ShellSnapshot,
    ShellSurface, ShellSurfaceEvent,
};
use epdf_plugin_shell::{SHELL, shell_plugin};
use futures::executor::block_on;
use plugin_conformance::test_kernel::{TestKernel, one_document};

#[derive(Default)]
struct Events(Mutex<Vec<ShellEvent>>, AtomicUsize);

impl ShellEventListener for Events {
    fn on_event(&self, event: ShellEvent) {
        self.0.lock().unwrap().push(event);
    }
    fn on_end(&self) {
        self.1.fetch_add(1, Ordering::SeqCst);
    }
}

fn handle() -> (KernelThread<TestKernel>, Arc<ShellHandle>) {
    let thread =
        KernelThread::spawn(|_| TestKernel::open(vec![shell_plugin()], one_document())).unwrap();
    let post = thread.post();
    let handle =
        block_on(thread.call(move |k| ShellHandle::new(k.capability(&SHELL).unwrap(), post)))
            .unwrap();
    (thread, handle)
}

fn code(error: &ViewerError) -> Option<&str> {
    match error {
        ViewerError::Kernel { code, .. } => code.as_deref(),
        ViewerError::Closed => None,
    }
}

fn tagged(tag: &str) -> Option<ShellOpenOptions> {
    Some(ShellOpenOptions { exclusive: Some(tag.into()), props_json: None })
}

#[test]
fn an_opened_surface_reads_back_with_its_props() {
    let (_thread, shell) = handle();
    let options = ShellOpenOptions {
        exclusive: None,
        props_json: Some(r#"{"annotationId":"a1","page":3}"#.into()),
    };
    block_on(shell.open_surface("style".into(), Some(options))).unwrap();
    assert!(block_on(shell.is_open("style".into())).unwrap());
    let surface = block_on(shell.surface("style".into())).unwrap().unwrap();
    assert_eq!(surface.id, "style");
    assert!(surface.open);
    let props: serde_json::Value =
        serde_json::from_str(surface.props_json.as_deref().unwrap()).unwrap();
    assert_eq!(props, serde_json::json!({ "annotationId": "a1", "page": 3 }));
    assert_eq!(block_on(shell.surface("never".into())).unwrap(), None);
}

#[test]
fn props_that_are_not_an_object_are_invalid_input() {
    let (_thread, shell) = handle();
    for bad in ["[1,2]", "3", "{not json"] {
        let options = ShellOpenOptions { exclusive: None, props_json: Some(bad.into()) };
        let error = block_on(shell.open_surface("style".into(), Some(options))).unwrap_err();
        assert_eq!(code(&error), Some("invalid-input"), "{bad}: {error:?}");
        let error = block_on(shell.update_surface_props("style".into(), bad.into())).unwrap_err();
        assert_eq!(code(&error), Some("invalid-input"), "{bad}: {error:?}");
    }
    assert!(!block_on(shell.is_open("style".into())).unwrap());
}

#[test]
fn an_exclusive_open_closes_its_siblings_and_says_so() {
    let (_thread, shell) = handle();
    block_on(shell.open_surface("thumbnails".into(), tagged("left"))).unwrap();
    block_on(shell.open_surface("outline".into(), None)).unwrap();
    let events = Arc::new(Events::default());
    let _sub = shell.subscribe(events.clone());
    block_on(shell.open_surface("search".into(), tagged("left"))).unwrap();
    assert!(!block_on(shell.is_open("thumbnails".into())).unwrap());
    assert!(block_on(shell.is_open("outline".into())).unwrap(), "an untagged surface stays open");
    let seen = events.0.lock().unwrap().clone();
    assert_eq!(
        seen,
        [
            ShellEvent::SurfaceClosed {
                event: ShellSurfaceEvent { id: "thumbnails".into(), props_json: None }
            },
            ShellEvent::SurfaceOpened {
                event: ShellSurfaceEvent { id: "search".into(), props_json: None }
            },
        ]
    );
}

#[test]
fn menus_stack_in_opening_order() {
    let (_thread, shell) = handle();
    let events = Arc::new(Events::default());
    let _sub = shell.subscribe(events.clone());
    block_on(shell.open_menu("file".into())).unwrap();
    block_on(shell.toggle_menu("zoom".into())).unwrap();
    assert_eq!(block_on(shell.open_menus()).unwrap(), ["file", "zoom"]);
    block_on(shell.toggle_menu("zoom".into())).unwrap();
    assert!(!block_on(shell.is_menu_open("zoom".into())).unwrap());
    block_on(shell.close_all_menus()).unwrap();
    assert!(block_on(shell.open_menus()).unwrap().is_empty());
    let seen = events.0.lock().unwrap();
    assert_eq!(seen[0], ShellEvent::MenuOpened { event: ShellMenuEvent { id: "file".into() } });
    assert_eq!(seen[2], ShellEvent::MenuClosed { event: ShellMenuEvent { id: "zoom".into() } });
}

#[test]
fn a_snapshot_round_trips_in_property_order() {
    let (_thread, shell) = handle();
    for id in ["b", "2", "a", "1"] {
        block_on(shell.open_surface(id.into(), None)).unwrap();
    }
    block_on(shell.close_surface("a".into())).unwrap();
    block_on(shell.open_menu("file".into())).unwrap();
    let snapshot = block_on(shell.snapshot()).unwrap();
    let ids: Vec<&str> = snapshot.surfaces.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, ["1", "2", "b", "a"], "integer-like ids first, then insertion order");
    assert!(!snapshot.surfaces[3].open, "a closed surface keeps its record");
    let open: Vec<String> =
        block_on(shell.open_surfaces()).unwrap().into_iter().map(|s| s.id).collect();
    assert_eq!(open, ["1", "2", "b"]);

    block_on(shell.close_all_surfaces()).unwrap();
    block_on(shell.close_all_menus()).unwrap();
    block_on(shell.apply_snapshot(snapshot.clone())).unwrap();
    assert_eq!(block_on(shell.snapshot()).unwrap(), snapshot);
}

#[test]
fn a_snapshot_with_bad_props_is_refused_whole() {
    let (_thread, shell) = handle();
    let snapshot = ShellSnapshot {
        surfaces: vec![
            ShellSurface { id: "ok".into(), open: true, exclusive: None, props_json: None },
            ShellSurface {
                id: "bad".into(),
                open: true,
                exclusive: None,
                props_json: Some("[]".into()),
            },
        ],
        open_menus: vec![],
    };
    let error = block_on(shell.apply_snapshot(snapshot)).unwrap_err();
    assert_eq!(code(&error), Some("invalid-input"), "{error:?}");
    assert!(!block_on(shell.is_open("ok".into())).unwrap());
}

#[test]
fn an_inherited_name_reads_as_closed() {
    let (_thread, shell) = handle();
    let surface = block_on(shell.surface("toString".into())).unwrap().unwrap();
    assert_eq!(
        surface,
        ShellSurface { id: "toString".into(), open: false, exclusive: None, props_json: None }
    );
    assert!(!block_on(shell.is_open("toString".into())).unwrap());
}

#[test]
fn a_closed_documents_capability_refuses_with_instance_closed() {
    let (thread, shell) = handle();
    let events = Arc::new(Events::default());
    let _sub = shell.subscribe(events.clone());
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    assert_eq!(events.1.load(Ordering::SeqCst), 1, "the close ends the stream");
    for error in [
        block_on(shell.is_open("x".into())).map(|_| ()).unwrap_err(),
        block_on(shell.open_surface("x".into(), None)).unwrap_err(),
        block_on(shell.snapshot()).map(|_| ()).unwrap_err(),
    ] {
        assert_eq!(code(&error), Some("instance-closed"), "{error:?}");
    }
}
