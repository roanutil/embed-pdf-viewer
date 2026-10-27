//! Kept divergence H4: the transaction's time budget runs on the host's
//! monotonic clock. The sandbox burns wall time while the virtual clock
//! stands still, as it does inside one host drain, and the chain still runs
//! out of time. The TypeScript measures with `Date.now()`, which a real
//! host advances, so it reaches the same arm.

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;

use epdf_acrojs::{
    ScriptBudget, ScriptDocumentInput, ScriptEnvironment, ScriptHost, ScriptHostOptions,
    ScriptInput, ScriptOutput, ScriptSandbox,
};
use epdf_kernel::Error;
use epdf_kernel::rt::{self, Promise};
use epdf_plugin_form::{
    FormScriptingController, FormScriptingControllerOptions, ScriptStatus, TransactionBody,
    TransactionPort,
};
use serde_json::{Value, json};

/// Puts the host's clock back when the test ends, however it ends.
struct ClockGuard;

impl Drop for ClockGuard {
    fn drop(&mut self) {
        rt::set_monotonic_clock(None);
    }
}

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

fn field_ref(n: u32) -> Value {
    json!({ "kind": "objectNumber", "objectNumber": n })
}

fn text(n: u32, calculate: bool) -> Value {
    let page = json!({ "kind": "objectNumber", "objectNumber": 1 });
    let mut field = json!({
        "ref": field_ref(n), "name": format!("f{n}"), "origin": "acroform",
        "readOnly": false, "required": false, "noExport": false,
        "alternateName": null, "mappingName": null,
        "valueEntry": { "kind": "scalar", "value": "1" }, "defaultValueEntry": { "kind": "none" },
        "groupId": null, "createdBy": null, "createdAt": null, "filledBy": null,
        "filledByName": null, "filledAt": null, "importedBy": null,
        "widgets": [{
            "ref": { "kind": "objectNumber", "page": page, "objectNumber": 100 + n },
            "objectNumber": 100 + n, "page": page
        }],
        "family": "text", "value": "1", "defaultValue": "", "maxLength": null,
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
    let _clock = ClockGuard;

    let fields: Vec<Value> = (1..=6).map(|n| text(n, n > 1)).collect();
    let order: Vec<Value> = (2..=6).map(field_ref).collect();
    let form = Rc::new(json!({
        "formKind": "acroform", "needsAppearances": false,
        "fields": fields, "widgets": [], "calculationOrder": order,
    }));

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
    let transaction: TransactionPort =
        Rc::new(move |body: TransactionBody| host.transaction::<Rc<dyn Any>>(body));
    let controller = FormScriptingController::new(FormScriptingControllerOptions {
        document: Rc::new(|| None),
        transaction,
        budget: None,
    });

    let commit = controller.commit(form, field_ref(1), json!({ "value": "2" }));
    rt::run_until_stalled();
    let result = commit.peek().expect("settled").expect("a result");

    assert_eq!(rt::now(), 0.0, "the virtual clock never moved");
    assert_eq!(result.status, ScriptStatus::Failed);
    assert_eq!(
        result.error.map(|e| e.message).as_deref(),
        Some("Form script transaction timed out")
    );
    // 50 ms buys two 30 ms runs; the third never reaches the sandbox.
    assert_eq!(runs.get(), 2);
}
