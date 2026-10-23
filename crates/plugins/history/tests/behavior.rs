//! The typed API, beyond what the vectors print: the typed settings and the
//! state read, against `packages/plugin/history/src/{contract,state}.ts`.

use std::cell::RefCell;
use std::rc::Rc;

use std::sync::LazyLock;

use epdf_kernel::changes::{ChangeQueue, StagedChange};
use epdf_kernel::kernel::OpenSource;
use epdf_kernel::{AnyPlugin, Kernel, KernelOptions, OpenOptions, Token, define, rt};
use epdf_plugin_history::*;
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use serde_json::{Value, json};

/// A feature plugin whose capability is its own change queue.
static FEATURE: LazyLock<Token<ChangeQueue>> = LazyLock::new(|| Token::new("feature"));

fn feature_plugin() -> AnyPlugin {
    define::<Value, Value>("feature")
        .document_scoped()
        .create(&FEATURE, |cx| Ok((cx.changes()?, None)))
        .build()
}

fn kernel(config: HistoryConfig) -> Kernel {
    rt::reset();
    let applied = json!({ "items": [], "meta": { "affectedPages": [], "cacheDelta": null, "opId": "x", "undoable": true } });
    let doc = json!({ "pages": [], "responses": [{ "path": "apply", "value": applied }] });
    let fake = FakeEngine::new(json!({ "documents": { "a": doc } }));
    let kernel = Kernel::new(
        Rc::new(AsEngine(fake)),
        vec![history_plugin(config), feature_plugin()],
        KernelOptions { report: None, on_dispatch: None, release_closed_sessions: false },
    )
    .unwrap();
    let _ = kernel.start();
    rt::run_until_stalled();
    let _ = kernel
        .documents()
        .open(OpenSource::Input(json!({ "kind": "bytes", "id": "a" })), OpenOptions::default());
    rt::run_until_stalled();
    kernel
}

#[test]
fn the_state_read_is_empty_without_a_document() {
    assert_eq!(history_state(None), HistoryStateView::default());
}

#[test]
fn typed_settings_merge_over_the_defaults_and_reset() {
    let kernel = kernel(HistoryConfig { limit: Some(3.0) });
    let history = kernel.capability(&HISTORY, Some("a")).unwrap();
    assert_eq!(history.get_settings(), HistorySettings { limit: 3.0 });
    let seen = Rc::new(RefCell::new(Vec::new()));
    let log = seen.clone();
    let _off = history.on_settings_changed(move |event| {
        log.borrow_mut().push((event.settings.limit, event.changed.clone()));
        Ok(())
    });
    history.update_settings(&HistoryConfig { limit: Some(1.0) });
    history.update_settings(&HistoryConfig::default());
    history.reset_settings();
    assert_eq!(
        *seen.borrow(),
        [(1.0, vec!["limit".to_string()]), (3.0, vec!["limit".to_string()])]
    );
    assert_eq!(history_defaults().limit, 100.0);
}

#[test]
fn an_answered_undo_reaches_the_view_and_the_event() {
    let kernel = kernel(HistoryConfig::default());
    let history = kernel.capability(&HISTORY, Some("a")).unwrap();
    let undone = Rc::new(RefCell::new(Vec::new()));
    let log = undone.clone();
    let _off = history.on_undone(move |event| {
        log.borrow_mut().push(event.clone());
        Ok(())
    });
    let queue = kernel.capability(&FEATURE, Some("a")).unwrap();
    let op = json!({ "type": "annotations.update" });
    let mut change = StagedChange::ops(json!({ "key": "a" }), vec![op.clone()]);
    change.undo = vec![op];
    queue.stage(change).unwrap();
    rt::run_until_stalled();
    let view = history_state(Some(&history));
    assert!(view.can_undo && !view.can_redo);
    assert_eq!(view.undo_label, Some(json!({ "key": "a" })));
    history.undo().unwrap();
    rt::run_until_stalled();
    let view = history_state(Some(&history));
    assert!(!view.can_undo && view.can_redo);
    assert_eq!(
        *undone.borrow(),
        [HistoryUndoneEvent { label: json!({ "key": "a" }), redo: false, skipped: 0 }]
    );
}
