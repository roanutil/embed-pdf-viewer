//! A PDF-supplied URI reaches the UI adapter only when its scheme is allowed.

use std::cell::RefCell;
use std::rc::Rc;

use epdf_kernel::kernel::{OpenOptions, OpenSource};
use epdf_kernel::{Kernel, KernelOptions, rt};
use epdf_plugin_actions::{
    ACTIONS, ActionContext, ActionNodeStatus, ActionUiAdapter, ActionsConfig, OpenSequence,
    actions_plugin,
};
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use serde_json::json;

#[test]
fn only_allowed_schemes_reach_open_uri() {
    rt::reset();
    let fake = FakeEngine::new(json!({
        "documents": { "a": { "security": { "allows": "*" }, "pages": [], "responses": [] } }
    }));
    let config = ActionsConfig { open_sequence: Some(OpenSequence::Off), ..Default::default() };
    let kernel = Kernel::new(
        Rc::new(AsEngine(fake)),
        vec![actions_plugin(config)],
        KernelOptions::default(),
    )
    .expect("kernel");
    kernel.start();
    rt::run_until_stalled();
    kernel.documents().open(
        OpenSource::Input(json!({ "kind": "bytes", "id": "a", "bytes": { "$bytes": "" } })),
        OpenOptions::default(),
    );
    rt::run_until_stalled();
    let actions = kernel.capability(&ACTIONS, Some("a")).expect("actions");

    let opened = Rc::new(RefCell::new(Vec::<String>::new()));
    let log = opened.clone();
    let _adapter = actions.set_ui_adapter(Some(Rc::new(ActionUiAdapter {
        open_uri: Rc::new(move |uri, _| {
            log.borrow_mut().push(uri.to_string());
            Ok(())
        }),
        print: Rc::new(|_| Ok(())),
        alert: None,
        goto_page: None,
    })));

    let ctx: ActionContext = serde_json::from_value(json!({
        "origin": "user", "source": { "kind": "api" }, "event": { "scope": "activate" }
    }))
    .expect("context");
    let mut statuses = Vec::new();
    for uri in ["javascript:alert(1)", "file:///etc/passwd", " https://a.test/ "] {
        let tree = json!({
            "root": { "type": "uri", "subtype": "URI", "uri": uri, "isMap": false, "next": [] },
            "incomplete": false, "warningFlags": 0, "warnings": []
        });
        let result = actions.execute(tree, ctx.clone());
        rt::run_until_stalled();
        statuses.push(result.peek().expect("settled").expect("a result").nodes[0].status);
    }

    assert_eq!(*opened.borrow(), vec!["https://a.test/".to_string()]);
    assert_eq!(
        statuses,
        vec![ActionNodeStatus::Blocked, ActionNodeStatus::Blocked, ActionNodeStatus::Executed]
    );
}
