//! The transaction time budget on a real host: the sandbox burns wall time
//! while the virtual clock stands still, as it does inside one host drain.

use std::cell::Cell;
use std::rc::Rc;

use epdf_acrojs::{
    ScriptBudget, ScriptDocumentInput, ScriptEnvironment, ScriptHost, ScriptHostOptions,
    ScriptInput, ScriptOutput, ScriptSandbox,
};
use epdf_kernel::Error;
use epdf_kernel::engine::{DocumentHandle, Engine};
use epdf_kernel::rt::{self, Promise};
use epdf_plugin_form::{
    FormCommitStatus, FormScriptingController, FormScriptingControllerOptions, TransactionBody,
    TransactionPort,
};
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use serde_json::{Value, json};

/// Each run takes 30 ms of wall time and none of the virtual clock.
struct SlowSandbox {
    wall: Rc<Cell<f64>>,
    runs: Rc<Cell<usize>>,
}

impl ScriptSandbox for SlowSandbox {
    fn disposed(&self) -> bool {
        false
    }
    fn boot(
        &self,
        _: &[String],
        _: &ScriptInput,
        _: Option<&ScriptBudget>,
    ) -> Result<ScriptOutput, Error> {
        Ok(ScriptOutput::empty())
    }
    fn run(
        &self,
        _: &str,
        _: &ScriptInput,
        _: Option<&ScriptBudget>,
    ) -> Result<ScriptOutput, Error> {
        self.runs.set(self.runs.get() + 1);
        self.wall.set(self.wall.get() + 30.0);
        Ok(ScriptOutput::empty())
    }
    fn dispose(&self) {}
}

fn text(n: u32, calculate: bool) -> Value {
    let page = json!({ "kind": "objectNumber", "pageObjectNumber": 1 });
    let mut field = json!({
        "ref": { "kind": "objectNumber", "fieldObjectNumber": n },
        "fieldObjectNumber": n, "name": format!("f{n}"), "family": "text", "origin": "acroform",
        "flags": { "readOnly": false, "required": false, "noExport": false, "raw": 0 },
        "alternateName": null, "mappingName": null,
        "widgets": [{
            "ref": { "kind": "objectNumber", "page": page, "annotObjectNumber": 100 + n },
            "annotObjectNumber": 100 + n, "page": page
        }],
        "valueEntry": { "kind": "scalar", "value": "1" }, "defaultValueEntry": { "kind": "none" },
        "value": "1", "defaultValue": "", "maxLength": null,
        "multiline": false, "password": false, "comb": false
    });
    if calculate {
        field["actions"] = json!({ "calculate": {
            "root": { "type": "javascript", "subtype": "JavaScript", "script": "while(true){}", "next": [] },
            "incomplete": false, "warningFlags": 0, "warnings": []
        } });
    }
    field
}

#[test]
fn the_transaction_budget_runs_on_wall_time() {
    rt::reset();
    let wall = Rc::new(Cell::new(0.0));
    let clock = wall.clone();
    rt::set_monotonic_clock(Some(Rc::new(move || clock.get())));

    let fields: Vec<Value> = (1..=6).map(|n| text(n, n > 1)).collect();
    let order: Vec<Value> =
        (2..=6).map(|n| json!({ "kind": "objectNumber", "fieldObjectNumber": n })).collect();
    let fake = FakeEngine::new(json!({
        "documents": { "a": { "security": { "allows": "*" }, "responses": [
            { "path": "forms.list", "value": {
                "formKind": "acroform", "needsAppearances": false,
                "fields": fields, "calculationOrder": order } },
            { "path": "forms.applyEffects", "value": { "applied": 0, "results": [] } }
        ] } }
    }));
    let engine = AsEngine(fake);
    let opened =
        engine.open(json!({ "kind": "bytes", "id": "a", "bytes": { "$bytes": "" } }), None);
    rt::run_until_stalled();
    let doc: Rc<dyn DocumentHandle> = opened.peek().expect("opened").expect("a document");

    let runs = Rc::new(Cell::new(0));
    let (sandbox_wall, sandbox_runs) = (wall.clone(), runs.clone());
    let host = ScriptHost::new(ScriptHostOptions {
        sandbox_factory: Rc::new(move || {
            let sandbox: Rc<dyn ScriptSandbox> =
                Rc::new(SlowSandbox { wall: sandbox_wall.clone(), runs: sandbox_runs.clone() });
            Promise::resolved(sandbox)
        }),
        document: Rc::new(|| ScriptDocumentInput {
            id: "a".into(),
            file_name: "a.pdf".into(),
            page_count: 1.0,
            page_number: 0.0,
        }),
        identity: Rc::new(Default::default),
        environment: Rc::new(|_| ScriptEnvironment {
            now_ms: 0.0,
            utc_offset_minutes: 0.0,
            random_seed: 1.0,
        }),
        boot_sources: Rc::new(|| Ok(Promise::resolved(Vec::new()))),
        budget: None,
    });
    let transaction: TransactionPort = Rc::new(move |body: TransactionBody| host.transaction(body));
    let controller = FormScriptingController::new(FormScriptingControllerOptions {
        doc,
        document: Rc::new(|| None),
        transaction,
        budget: None,
    });

    let commit = controller.commit(
        json!({ "kind": "objectNumber", "fieldObjectNumber": 1 }),
        json!({ "type": "text", "value": "2" }),
    );
    rt::run_until_stalled();
    let result = commit.peek().expect("settled").expect("a result");
    rt::set_monotonic_clock(None);

    assert_eq!(rt::now(), 0.0, "the virtual clock never moved");
    assert_eq!(result.status, FormCommitStatus::Failed);
    assert_eq!(
        result.error.map(|e| e.message).as_deref(),
        Some("Form script transaction timed out")
    );
    // 50 ms buys two 30 ms runs; the third never reaches the sandbox.
    assert_eq!(runs.get(), 2);
}
