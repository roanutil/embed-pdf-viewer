# Swift Wrapper for `actions` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Export the document-scoped `actions` plugin through UniFFI as `ActionsHandle`, map the `actions.read` engine path, wire it into `ViewerHost` behind `ViewerConfig.actions`, and give Swift an `ActionsCapability` whose UI, executor, submit and save/print ports are Swift objects, with PDF JavaScript running in a JavaScriptCore engine Swift supplies.

**Architecture:** The recipe in `2026-10-01-swift-package.md` Appendix A §A.0, with metadata as the template. Action trees, nodes and annotation or field refs cross as three opaque objects (`PdfActionTree`, `PdfActionNode`, `PdfObjectRef`) that wrap the engine's JSON and are echoed back exactly; everything the plugin decides (results, diagnostics, policy, triggers, contexts) crosses as typed records. Each application port becomes a foreign trait answered through a completion object built on `epdf_ffi::ForeignSettle`. JavaScript stays a Rust concern: a `ForeignSandbox` in `ffi.rs` implements `epdf_acrojs::ScriptSandbox` by building the exact program `packages/core/js-sandbox/src/QuickJsSandbox.ts` builds, handing it to a foreign `ScriptEngine` that only evaluates JavaScript, and validating the answer the way that file does. Swift's `JavaScriptCoreEngine` is about 60 lines. Scripting is off until Task 5 lands the sandbox.

**Tech Stack:** Rust 2024, `uniffi =0.32.0`, `futures` 0.3, Swift 6.2 tools, JavaScriptCore, Swift Testing, `swift-custom-dump` 1.3, Xcode 27.0, iPhone 17 simulator (iOS 26.0).

**Spec:** `docs/superpowers/plans/2026-10-06-swift-plugins/00-umbrella.md` and `docs/superpowers/plans/2026-10-01-swift-package.md` (Global Constraints, Decisions, Appendix A §A.0 and §A.4 "actions", Appendix C rows `actions.read` and `forms.submit`). `multiplatform-plugins-plan.md` line 29 names JavaScriptCore as the iOS sandbox. `crates/plugins/acrojs/README.md` describes the sandbox contract (JSON in, JSON out).

## Global Constraints

Every line of the umbrella's Global Constraints applies. The ones this plan meets most often:

- `uniffi` is pinned at `=0.32.0`.
- UniFFI type names are unique across the workspace. Mirrors of generic names get a prefix: `ActionUriOptions`, `ActionAlertOptions`, `ActionsScriptBudget`, `ActionsEvent`.
- No `usize`, `isize`, `Rc` or `serde_json::Value` across the boundary. `Value` trees cross only inside the three opaque objects (see Decisions).
- Facades don't decide PDF semantics. Policy, the dispatcher, the open sequence, the submit chain and the sandbox's output validation stay in Rust; Swift evaluates JavaScript and shows UI.
- Text offsets are UTF-16 code units (the script event's `selStart` and `selEnd` stay inside the JSON the sandbox passes through).
- Swift value assertions use `expectNoDifference`.
- No npm package changes, no changesets.

## Review Focus

1. A runaway script (`while (true) {}`). Expected: the engine gives up at the budget's `maxExecutionMs`, the run reports a `budget` failure, the realm is poisoned (every later run throws "disposed"), and the kernel thread is free again. Pinned in Task 5 (`a_timed_out_evaluation_is_a_budget_failure_and_poisons`) and Task 7 (`aRunawayScriptTimesOut`).
2. An engine that answers something that isn't AcroJS output (a non-string, malformed JSON, a missing member). Expected: an `invalid-output` failure and a poisoned realm, as `QuickJsSandbox.ts` `invoke` does, never a panic. Pinned in Task 5 (`malformed_output_is_invalid_output`).
3. Output larger than the budget's `maxOutputBytes`. Expected: a `budget` failure. Pinned in Task 5 (`oversized_output_is_a_budget_failure`).
4. A PDF URI action with a scheme outside the allowlist (`javascript:`, `file:`). Expected: Swift's `openUri` never sees it. Pinned in Task 2 (`only_allowed_schemes_reach_the_swift_handler`).
5. A Swift executor that drops its completion. Expected: the node is `failed`, the dispatch finishes, and the serial queue isn't stuck behind it. Pinned in Task 2 (`a_dropped_executor_completion_fails_the_node`).

## Decisions

Only the application members cross. The capability's doc comment (`crates/plugins/actions/src/controller.rs`) says the host members (`register_annot_commit_sink`, `register_form_commit_sink`, `register_submit_resolver`, `script_realm`, `create_detached_script_realm`, `surface_script_result`, `surface_script_commit`, `report_page_state`) are "for sibling plugins, not applications". On iOS those siblings (annotation, form, stage, stamp) are Rust crates in the same kernel and call them directly. Exporting them would hand Swift the script transaction port and the page-truth report, which no app should drive. So 19 of the 27 members in `crates/plugins/actions/src/harness.rs` `MEMBERS` get an FFI method and 8 don't. The umbrella's table says "`register_*`/`set_*` foreign traits"; this plan narrows that to `register_executor`, `set_submit_handler` and `set_ui_adapter`, plus `run_document_verb`'s operation. If a native app ever needs one of the eight, it gets its own small change with a reason attached.

Trees cross opaque. `PdfActionTree` is "carried as the engine extracted it and echoed back in results and events exactly" (`contract.rs`). A typed mirror of the node union (GoTo destinations, Hide targets, SubmitForm flags, unknown node types) would be large and lossy, and lossy is wrong for something echoed back. The objects expose `json()` and a couple of read-outs (`root_type`, `node_type`), and Swift builds one with `fromJson`. The cost: an app that wants to inspect a destination parses JSON itself.

No member takes a `CancelToken`. A dispatch is a transaction on the plugin's serial queue, and its document writes aren't rollback-atomic (`ActionDispatchResult` docs); the plugin offers no abort, and an abort half-way would leave exactly the partial state the `partial` status exists to report.

JavaScriptCore can't enforce the whole `ScriptBudget`. Its public API has no execution time limit (`JSContextGroupSetExecutionTimeLimit` is SPI in `JSContextRefPrivate.h`) and no memory or stack limit. `JavaScriptCoreEngine` runs each evaluation on its own serial queue and waits at most `maxExecutionMs`. On a timeout it answers `timedOut`, Rust poisons the realm, and the kernel thread moves on. The runaway script keeps its queue's thread spinning until the process exits, because nothing public can stop it. `maxMemoryBytes` and `maxStackBytes` aren't enforced at all; JSC's own stack limit throws a `RangeError` that the prelude reports. We accept both for now and name them in the README row. If either bites, the follow-up is a QuickJS engine compiled into the Rust archive (the `rquickjs` crate), which enforces all four limits on iOS and Android alike; the `ScriptEngine` trait doesn't change.

`isResourceError` matches JSC's spelling too. `QuickJsSandbox.ts` treats a script error whose message mentions "interrupted", "out of memory" or "stack overflow" as a budget failure. JSC says "Maximum call stack size exceeded." The Rust port adds that phrase, so a JSC stack overflow is a `budget` failure as a QuickJS one is.

The UTC offset is fixed per viewer. `ActionsConfig.utc_offset_minutes` feeds the plugin's `host_utc_offset_minutes`; the Swift facade fills it from `TimeZone.current.secondsFromGMT()` when the viewer is created. A daylight-saving change mid-session is an hour off in script dates until the next viewer. A foreign clock trait would fix that at the cost of a kernel-thread callback per script run; we don't think it carries its weight yet.

JavaScript on a real document needs plans 05 and 06. The script world reads `forms.list` and `page.annotations.list` (`crates/plugins/actions/src/scripting/world.rs:86-89`), which Appendix C assigns to B2 (plans 05 and 06). Until both merge, a JavaScript node on the native engine fails at world-building. This plan tests JavaScript end to end over the conformance `FakeEngine`, and tests `JavaScriptCoreEngine` through `check_script_engine`, which runs a script in a fresh realm without a document.

## File Structure

```
crates/epdf-ffi/src/lib.rs
crates/plugins/actions/Cargo.toml                     uniffi feature, deps
crates/plugins/actions/src/lib.rs                     scaffolding, mod ffi
crates/plugins/actions/src/contract.rs                in-place uniffi derives
crates/plugins/actions/src/controller.rs              lifetime() for the ffi
crates/plugins/actions/src/ffi.rs                     objects, mirrors, ActionsHandle, ActionsEvent
crates/plugins/actions/src/ffi/ports.rs               executor, UI, submit and verb foreign traits
crates/plugins/actions/src/ffi/sandbox.rs             ScriptEngine traits, ForeignSandbox, check_script_engine
crates/plugins/actions/tests/ffi.rs
crates/plugins/actions/tests/ffi_ports.rs
crates/plugins/actions/tests/ffi_sandbox.rs
crates/epdf-viewer-host/src/translate.rs              actions.read, namespace has()
crates/epdf-viewer-host/tests/translate.rs
crates/epdf-viewer-host/Cargo.toml
crates/epdf-viewer-host/src/api.rs                    ViewerConfig.actions
crates/epdf-viewer-host/src/session.rs
crates/epdf-viewer-host/src/lib.rs                    ViewerHost::actions(document_id)
crates/epdf-viewer-host/tests/viewer_host.rs
platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Actions.swift
platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/JavaScriptCoreEngine.swift
platforms/apple/EmbedPDF/Tests/EmbedPDFTests/ActionsTests.swift
platforms/apple/EmbedPDF/Tests/EmbedPDFTests/JavaScriptCoreEngineTests.swift
platforms/apple/build-xcframework.sh                     + document_actions.pdf in FIXTURES
.github/workflows/plugin-rust.yml
platforms/apple/README.md
docs/superpowers/plans/2026-10-01-swift-package.md
```

`ffi.rs` becomes `ffi/mod.rs`-style with two submodules (`src/ffi.rs` plus `src/ffi/ports.rs` and `src/ffi/sandbox.rs`), because the three areas test independently and together run past 1,000 lines.

## Cross-plugin tests

actions has no requirements and no optional dependencies; stage, link, annotation, form and stamp optionally depend on it. Tests that need a sibling belong to the later plan:

- JavaScript end to end on a real PDF (`document_actions.pdf`'s will-close script runs through `JavaScriptCoreEngine`): needs `forms.list` (plan 06) and `page.annotations.list` (plan 05). Owned by plan 06, which merges after 05, as `aDocumentScriptRunsInJavaScriptCore`.
- A SubmitForm node reaching the Swift submit handler on a real PDF: needs the form plugin's submit resolver. Owned by plan 06, as `aSubmitReachesTheSwiftHandler`. This plan tests the handler over a Rust resolver in Task 2.
- A GoTo action moving the stage, and the page lifecycle firing page open/close trees: owned by plan 02 (stage), which registers stage's executor and reports page state.
- Link activation dispatching through actions: owned by plan 04.

---

### Task 1: Objects, records and `ActionsHandle`

**Files:**
- Modify: `crates/plugins/actions/Cargo.toml`
- Modify: `crates/plugins/actions/src/lib.rs`
- Modify: `crates/plugins/actions/src/contract.rs`
- Modify: `crates/plugins/actions/src/controller.rs`
- Create: `crates/plugins/actions/src/ffi.rs`
- Test: `crates/plugins/actions/tests/ffi.rs`

**Interfaces:**
- Consumes: `epdf_ffi::{KernelBound, KernelPost, PageRef, Subscription, ViewerError}`.
- Produces: objects `PdfActionTree`, `PdfActionNode`, `PdfObjectRef` (each with `from_json(json) -> Result<Arc<Self>, ViewerError>` and `json() -> String`); mirrors `ActionSource`, `ActionContext`, `PartialActionContext`, `ActionTrigger`, `ActionTreeSource`, `ActionAnnotationTreeEvent`, `ActionNodeResult`, `ActionDispatchResult`, `ActionStepResult`, `ActionTriggerResult`, `ActionDispatchEvent`, `OpenSequenceCompleted`, `ScriptDiagnostic`, `ScriptDiagnosticCode`, `ScriptExecutionError`, `ScriptErrorKind`; `ActionsEvent` and `ActionsEventListener`; `ActionsHandle::new(cap: ActionsCapability, post: KernelPost) -> Arc<ActionsHandle>` with async `policy`, `update_policy`, `is_scripting_enabled`, `execute`, `can_execute`, `execute_named`, `action_tree`, `dispatch`, `can_dispatch`, `prepare_close` and sync `subscribe`. Task 2 adds the port members to the same `impl`.

Members, from `crates/plugins/actions/src/harness.rs` `MEMBERS` (27). Task 1: `getPolicy` (`policy`), `updatePolicy`, `isScriptingEnabled`, `execute`, `canExecute`, `executeNamed`, `getActionTree` (`action_tree`), `dispatch`, `canDispatch`, `prepareClose`, and `onExecuted`, `onDiagnostic`, `onScriptDiagnostic`, `onScriptError`, `onOpenSequenceCompleted` (all in `subscribe`). Task 2: `registerExecutor`, `setSubmitHandler`, `setUiAdapter`, `runDocumentVerb`. Not exported (Decisions): `registerAnnotCommitSink`, `registerFormCommitSink`, `registerSubmitResolver`, `scriptRealm`, `createDetachedScriptRealm`, `surfaceScriptResult`, `surfaceScriptCommit`, `reportPageState`.

- [x] **Step 1: Create the worktree**

```bash
git worktree add .claude/worktrees/swift-actions -b research/swift-actions research/swift-plugins
cd .claude/worktrees/swift-actions
bash platforms/apple/build-xcframework.sh
stat -f%z target/aarch64-apple-ios-sim/release/libepdf_viewer_host.a
```

Fetch PDFium first as `platforms/apple/README.md` describes. Keep the size for the final commit message.

- [x] **Step 2: Add the feature**

`crates/plugins/actions/Cargo.toml`, under `[features]`:

```toml
# The uniffi exports (`ffi` module), for `epdf-viewer-host`.
uniffi = ["dep:uniffi", "dep:epdf-ffi"]
```

Under `[dependencies]`:

```toml
epdf-ffi = { path = "../../epdf-ffi", optional = true }
uniffi = { version = "=0.32.0", optional = true }
```

Add:

```toml
[dev-dependencies]
futures = { version = "0.3", default-features = false, features = ["std", "executor"] }
plugin-conformance = { path = "../../plugin-conformance" }
```

`crates/plugins/actions/src/lib.rs`, after `pub mod submit_encoding;`:

```rust
#[cfg(feature = "uniffi")]
pub mod ffi;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();
```

`crates/plugins/actions/src/controller.rs`, in the `impl ActionsCapability` block that holds the hooks:

```rust
    /// The instance lifetime, aborted when the document closes. A foreign
    /// handle outlives the close, checks this before every call, and ends
    /// its event streams when it aborts.
    #[cfg(feature = "uniffi")]
    #[doc(hidden)]
    pub fn lifetime(&self) -> &epdf_kernel::signal::AbortSignal {
        self.0.cx.lifetime()
    }
```

- [x] **Step 3: Derive the compatible contract types in place**

In `crates/plugins/actions/src/contract.rs`, add `#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]` under the `#[derive(...)]` line of each of these enums, all of whose variants carry only other types in this list: `ActionOrigin`, `DocumentTriggerEvent`, `PdfAnnotationEventKind`, `PageEventKind`, `PdfFieldEventKind`, `ActionTriggerEvent`, `ActionNodeStatus`, `ActionDiagnosticCode`, `ActionResultStatus`, `ActionPolicyDecision`, `OpenSequence`, `ScriptPhase`, `DocumentVerb`.

Add `#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]` to these structs: `ActionDiagnostic`, `ActionPolicyRow`, `ActionPolicy`, `PartialActionPolicyRow`, `ActionPolicyPatch`, `TriggerGates`, `ActionUiContext`.

Swift constructs the patch records, so give every field of `PartialActionPolicyRow`, `ActionPolicyPatch` and `TriggerGates` a default. For example, `PartialActionPolicyRow` becomes:

```rust
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct PartialActionPolicyRow {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub user: Option<ActionPolicyDecision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub hover: Option<ActionPolicyDecision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub lifecycle: Option<ActionPolicyDecision>,
}
```

Do the same for the eight fields of `ActionPolicyPatch` and the three of `TriggerGates`.

Then check that none of the in-place names collides with an export elsewhere:

```bash
for name in ActionOrigin DocumentTriggerEvent PdfAnnotationEventKind PageEventKind PdfFieldEventKind ActionTriggerEvent ActionNodeStatus ActionDiagnosticCode ActionResultStatus ActionPolicyDecision OpenSequence ScriptPhase DocumentVerb ActionDiagnostic ActionPolicyRow ActionPolicy PartialActionPolicyRow ActionPolicyPatch TriggerGates ActionUiContext; do
  grep -rln "pub \(struct\|enum\) $name\b" crates/*/src crates/plugins/*/src | grep -v 'crates/plugins/actions/' && echo "collision: $name"
done
```

Expected: no `collision:` line. A collision means the other crate exports the same uniffi name (check that it has `uniffi::Record`/`uniffi::Enum` on it); mirror the actions type in `ffi.rs` under an `Action`-prefixed name instead of deriving it in place.

- [x] **Step 4: Write the failing handle tests**

Create `crates/plugins/actions/tests/ffi.rs`:

```rust
#![cfg(feature = "uniffi")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use epdf_ffi::{KernelThread, ViewerError};
use epdf_plugin_actions::ffi::{
    ActionContext, ActionSource, ActionTreeSource, ActionTrigger, ActionsEvent,
    ActionsEventListener, ActionsHandle, PdfActionTree,
};
use epdf_plugin_actions::{
    ACTIONS, ActionDiagnosticCode, ActionOrigin, ActionPolicyDecision, ActionPolicyPatch,
    ActionResultStatus, ActionTriggerEvent, ActionsConfig, DocumentTriggerEvent, OpenSequence,
    PartialActionPolicyRow, actions_plugin,
};
use futures::executor::block_on;
use plugin_conformance::test_kernel::{DOCUMENT, Reply, TestKernel, TestSpec};
use serde_json::{Value, json};

#[derive(Default)]
struct Events(Mutex<Vec<ActionsEvent>>, AtomicUsize);

impl ActionsEventListener for Events {
    fn on_event(&self, event: ActionsEvent) {
        self.0.lock().unwrap().push(event);
    }
    fn on_end(&self) {
        self.1.fetch_add(1, Ordering::SeqCst);
    }
}

pub fn named(name: &str) -> Value {
    json!({
        "root": { "type": "named", "subtype": "Named", "name": name, "next": [] },
        "incomplete": false, "warningFlags": 0, "warnings": []
    })
}

pub fn page(pon: u32) -> Value {
    json!({
        "index": pon - 1,
        "ref": { "kind": "objectNumber", "pageObjectNumber": pon },
        "label": null,
        "size": { "width": 600, "height": 800 },
        "rotation": 0,
        "userUnit": 1,
        "boxes": {
            "media": { "left": 0, "bottom": 0, "right": 600, "top": 800 },
            "crop": { "left": 0, "bottom": 0, "right": 600, "top": 800 }
        }
    })
}

/// One page, the catalog's open and will-save trees, an empty form and no
/// annotations: what the dispatcher and the script world read.
pub fn spec() -> TestSpec {
    TestSpec::new(json!({
        "documents": {
            DOCUMENT: {
                "id": DOCUMENT, "security": { "allows": "*" },
                "services": ["actions"], "pages": [page(1)]
            }
        }
    }))
    .respond("actions.read", |_| {
        Ok(Reply::value(json!({
            "nameTreeScripts": [], "openAction": named("OpenAction"),
            "openDestination": null, "willSave": named("WS")
        })))
    })
    .respond("forms.list", |_| {
        Ok(Reply::value(json!({
            "formKind": "none", "needsAppearances": false, "fields": [], "calculationOrder": []
        })))
    })
    .respond_page("page.annotations.list", |_, _| Ok(Reply::value(json!({ "annotations": [] }))))
}

fn quiet() -> ActionsConfig {
    ActionsConfig { open_sequence: Some(OpenSequence::Off), ..Default::default() }
}

fn handle() -> (KernelThread<TestKernel>, Arc<ActionsHandle>) {
    let thread = KernelThread::spawn(|_| TestKernel::open(vec![actions_plugin(quiet())], spec())).unwrap();
    let post = thread.post();
    let handle =
        block_on(thread.call(move |k| ActionsHandle::new(k.capability(&ACTIONS).unwrap(), post)))
            .unwrap();
    (thread, handle)
}

fn code(error: &ViewerError) -> Option<&str> {
    match error {
        ViewerError::Kernel { code, .. } => code.as_deref(),
        ViewerError::Closed => None,
    }
}

fn user() -> ActionContext {
    ActionContext { origin: ActionOrigin::User, source: ActionSource::Api, event: ActionTriggerEvent::Activate }
}

fn tree(json: Value) -> Arc<PdfActionTree> {
    PdfActionTree::from_json(json.to_string()).unwrap()
}

#[test]
fn the_policy_reads_and_patches() {
    let (_thread, actions) = handle();
    let policy = block_on(actions.policy()).unwrap();
    assert_eq!(policy.uri.user, ActionPolicyDecision::Adapter);
    assert_eq!(policy.uri.hover, ActionPolicyDecision::Report);
    let patch = ActionPolicyPatch {
        uri: Some(PartialActionPolicyRow { user: Some(ActionPolicyDecision::Block), ..Default::default() }),
        ..Default::default()
    };
    block_on(actions.update_policy(patch)).unwrap();
    let policy = block_on(actions.policy()).unwrap();
    assert_eq!(policy.uri.user, ActionPolicyDecision::Block);
    assert_eq!(policy.uri.hover, ActionPolicyDecision::Report);
}

#[test]
fn an_incomplete_tree_is_refused() {
    let (_thread, actions) = handle();
    let mut incomplete = named("NextPage");
    incomplete["incomplete"] = json!(true);
    let incomplete = tree(incomplete);
    assert!(!block_on(actions.can_execute(incomplete.clone(), user())).unwrap());
    let result = block_on(actions.execute(incomplete, user())).unwrap();
    assert_eq!(result.status, ActionResultStatus::Refused);
    assert!(result.diagnostics.iter().any(|d| d.code == ActionDiagnosticCode::IncompleteTree));
}

#[test]
fn a_goto_tree_is_allowed_for_a_user() {
    let (_thread, actions) = handle();
    let goto = tree(json!({
        "root": { "type": "goto", "subtype": "GoTo",
                  "destination": { "kind": "fit", "page": { "kind": "objectNumber", "pageObjectNumber": 1 } },
                  "next": [] },
        "incomplete": false, "warningFlags": 0, "warnings": []
    }));
    assert!(block_on(actions.can_execute(goto, user())).unwrap());
}

#[test]
fn the_document_open_tree_comes_from_the_catalog() {
    let (_thread, actions) = handle();
    let open = block_on(actions.action_tree(ActionTreeSource::Document { event: None })).unwrap().unwrap();
    assert_eq!(open.root_type().as_deref(), Some("named"));
    let none = block_on(actions.action_tree(ActionTreeSource::Document { event: Some(DocumentTriggerEvent::DidPrint) }))
        .unwrap();
    assert!(none.is_none());
}

#[test]
fn a_document_trigger_runs_its_tree_and_is_observed() {
    let (_thread, actions) = handle();
    let events = Arc::new(Events::default());
    let _sub = actions.subscribe(events.clone());
    let trigger = ActionTrigger::Document { event: DocumentTriggerEvent::WillSave };
    assert!(block_on(actions.can_dispatch(trigger.clone())).unwrap());
    let result = block_on(actions.dispatch(trigger)).unwrap();
    assert_eq!(result.steps.len(), 1);
    assert_eq!(result.steps[0].tree.root_type().as_deref(), Some("named"));
    assert!(matches!(result.steps[0].source, ActionSource::Document));
    let seen = events.0.lock().unwrap();
    assert!(
        seen.iter().any(|e| matches!(e, ActionsEvent::Executed { event } if event.tree.root_type().as_deref() == Some("named"))),
        "{seen:?}"
    );
}

#[test]
fn json_that_is_not_an_object_is_invalid_input() {
    let error = PdfActionTree::from_json("[1, 2".into()).err().unwrap();
    assert_eq!(code(&error), Some("invalid-input"), "{error:?}");
}

#[test]
fn a_closed_documents_capability_refuses_with_instance_closed() {
    let (thread, actions) = handle();
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    for error in [
        block_on(actions.policy()).map(|_| ()).unwrap_err(),
        block_on(actions.prepare_close()).map(|_| ()).unwrap_err(),
        block_on(actions.execute(tree(named("NextPage")), user())).map(|_| ()).unwrap_err(),
    ] {
        assert_eq!(code(&error), Some("instance-closed"), "{error:?}");
    }
}

#[test]
fn closing_the_document_ends_its_subscriptions() {
    let (thread, actions) = handle();
    let events = Arc::new(Events::default());
    let _subscription = actions.subscribe(events.clone());
    block_on(thread.call(|_| ())).unwrap();
    assert_eq!(events.1.load(Ordering::SeqCst), 0);
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    assert_eq!(events.1.load(Ordering::SeqCst), 1);
    let late = Arc::new(Events::default());
    let _late = actions.subscribe(late.clone());
    block_on(thread.call(|_| ())).unwrap();
    assert_eq!(late.1.load(Ordering::SeqCst), 1);
}
```

`spec`, `named` and `page` are `pub` so Tasks 2 and 5's test files can copy them; integration test files don't share modules, so each later file repeats them verbatim.

- [x] **Step 5: Run the tests to verify they fail**

Run: `cargo test -p epdf-plugin-actions --features uniffi --test ffi`
Expected: FAIL to compile with "unresolved import `epdf_plugin_actions::ffi`".

- [x] **Step 6: Write the objects and mirrors in `crates/plugins/actions/src/ffi.rs`**

```rust
//! The actions plugin through uniffi: the engine's action trees as opaque
//! objects, the plugin's decisions as typed records, one document handle,
//! and the application ports as foreign traits (`ports`). JavaScript runs in
//! a foreign `ScriptEngine` behind a Rust sandbox (`sandbox`).
//!
//! Only the application members cross. The host members (commit sinks, the
//! submit resolver, script realms, script surfacing, the page-state report)
//! are for sibling plugins, which are Rust crates in the same kernel.

pub mod ports;
pub mod sandbox;

use std::sync::Arc;

use epdf_acrojs as acrojs;
use epdf_ffi::{KernelBound, KernelPost, PageRef, Subscription, ViewerError};
use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::store::Unsubscribe;
use serde_json::Value;

use crate::ActionsCapability;
use crate::contract::{
    self as plugin, ActionDiagnostic, ActionNodeStatus, ActionOrigin, ActionPolicy,
    ActionPolicyPatch, ActionResultStatus, ActionTriggerEvent, DocumentTriggerEvent,
    PageEventKind, PdfAnnotationEventKind, PdfFieldEventKind,
};

fn invalid(message: String) -> Error {
    plugin_error(code::INVALID_INPUT, "actions", message, None, None)
}

fn parse(json: &str, what: &str) -> Result<Value, Error> {
    serde_json::from_str(json).map_err(|e| invalid(format!("{what} is not JSON: {e}")))
}

/// `PdfActionTree`: `{root, incomplete, warningFlags, warnings}` as the
/// engine extracted it. Echoed back exactly; never reshaped here.
#[derive(Debug, PartialEq, uniffi::Object)]
pub struct PdfActionTree(pub(crate) Value);

#[uniffi::export]
impl PdfActionTree {
    #[uniffi::constructor]
    pub fn from_json(json: String) -> Result<Arc<Self>, ViewerError> {
        Ok(Arc::new(PdfActionTree(parse(&json, "an action tree")?)))
    }

    pub fn json(&self) -> String {
        self.0.to_string()
    }

    /// The root node's `type` (`goto`, `uri`, `javascript`, ...).
    pub fn root_type(&self) -> Option<String> {
        self.0.get("root")?.get("type")?.as_str().map(str::to_string)
    }

    pub fn is_incomplete(&self) -> bool {
        self.0.get("incomplete").and_then(Value::as_bool).unwrap_or(false)
    }
}

impl PdfActionTree {
    pub(crate) fn wrap(value: &Value) -> Arc<Self> {
        Arc::new(PdfActionTree(value.clone()))
    }
}

/// One `PdfActionNode`, as an executor receives it.
#[derive(Debug, PartialEq, uniffi::Object)]
pub struct PdfActionNode(pub(crate) Value);

#[uniffi::export]
impl PdfActionNode {
    #[uniffi::constructor]
    pub fn from_json(json: String) -> Result<Arc<Self>, ViewerError> {
        Ok(Arc::new(PdfActionNode(parse(&json, "an action node")?)))
    }

    pub fn json(&self) -> String {
        self.0.to_string()
    }

    pub fn node_type(&self) -> String {
        self.0.get("type").and_then(Value::as_str).unwrap_or_default().to_string()
    }
}

/// An `AnnotationRef` or `FormFieldRef`, as the engine wrote it.
#[derive(Debug, PartialEq, uniffi::Object)]
pub struct PdfObjectRef(pub(crate) Value);

#[uniffi::export]
impl PdfObjectRef {
    #[uniffi::constructor]
    pub fn from_json(json: String) -> Result<Arc<Self>, ViewerError> {
        Ok(Arc::new(PdfObjectRef(parse(&json, "an object ref")?)))
    }

    pub fn json(&self) -> String {
        self.0.to_string()
    }
}

impl PdfObjectRef {
    fn wrap(value: &Value) -> Arc<Self> {
        Arc::new(PdfObjectRef(value.clone()))
    }
}

/// Who initiated a dispatch. Provenance only: policy never reads it.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum ActionSource {
    Widget { field: Arc<PdfObjectRef>, annotation: Arc<PdfObjectRef>, page: PageRef },
    Link { annotation: Option<Arc<PdfObjectRef>>, page: Option<PageRef> },
    Annotation { annotation: Arc<PdfObjectRef>, page: PageRef },
    Page { page: PageRef },
    Document,
    Api,
}

impl From<&plugin::ActionSource> for ActionSource {
    fn from(source: &plugin::ActionSource) -> Self {
        match source {
            plugin::ActionSource::Widget { field, annotation, page } => ActionSource::Widget {
                field: PdfObjectRef::wrap(field),
                annotation: PdfObjectRef::wrap(annotation),
                page: (*page).into(),
            },
            plugin::ActionSource::Link { annotation, page } => ActionSource::Link {
                annotation: annotation.as_ref().map(PdfObjectRef::wrap),
                page: page.map(PageRef::from),
            },
            plugin::ActionSource::Annotation { annotation, page } => ActionSource::Annotation {
                annotation: PdfObjectRef::wrap(annotation),
                page: (*page).into(),
            },
            plugin::ActionSource::Page { page } => ActionSource::Page { page: (*page).into() },
            plugin::ActionSource::Document => ActionSource::Document,
            plugin::ActionSource::Api => ActionSource::Api,
        }
    }
}

impl From<ActionSource> for plugin::ActionSource {
    fn from(source: ActionSource) -> Self {
        match source {
            ActionSource::Widget { field, annotation, page } => plugin::ActionSource::Widget {
                field: field.0.clone(),
                annotation: annotation.0.clone(),
                page: page.into(),
            },
            ActionSource::Link { annotation, page } => plugin::ActionSource::Link {
                annotation: annotation.map(|a| a.0.clone()),
                page: page.map(Into::into),
            },
            ActionSource::Annotation { annotation, page } => {
                plugin::ActionSource::Annotation { annotation: annotation.0.clone(), page: page.into() }
            }
            ActionSource::Page { page } => plugin::ActionSource::Page { page: page.into() },
            ActionSource::Document => plugin::ActionSource::Document,
            ActionSource::Api => plugin::ActionSource::Api,
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ActionContext {
    pub origin: ActionOrigin,
    pub source: ActionSource,
    pub event: ActionTriggerEvent,
}

impl From<&plugin::ActionContext> for ActionContext {
    fn from(ctx: &plugin::ActionContext) -> Self {
        ActionContext { origin: ctx.origin, source: (&ctx.source).into(), event: ctx.event.clone() }
    }
}

impl From<ActionContext> for plugin::ActionContext {
    fn from(ctx: ActionContext) -> Self {
        plugin::ActionContext { origin: ctx.origin, source: ctx.source.into(), event: ctx.event }
    }
}

/// What `execute_named` overrides.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct PartialActionContext {
    #[uniffi(default = None)]
    pub origin: Option<ActionOrigin>,
    #[uniffi(default = None)]
    pub source: Option<ActionSource>,
    #[uniffi(default = None)]
    pub event: Option<ActionTriggerEvent>,
}

impl From<PartialActionContext> for plugin::PartialActionContext {
    fn from(ctx: PartialActionContext) -> Self {
        plugin::PartialActionContext {
            origin: ctx.origin,
            source: ctx.source.map(Into::into),
            event: ctx.event,
        }
    }
}

/// What a feed reports. The dispatcher resolves the trees and derives the
/// origin; `source` can't change it.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum ActionTrigger {
    Activate { annotation: Arc<PdfObjectRef>, page: PageRef, source: Option<ActionSource> },
    Annotation {
        event: PdfAnnotationEventKind,
        annotation: Arc<PdfObjectRef>,
        page: PageRef,
        source: Option<ActionSource>,
    },
    Page { event: PageEventKind, page: PageRef },
    Document { event: DocumentTriggerEvent },
}

impl From<ActionTrigger> for plugin::ActionTrigger {
    fn from(trigger: ActionTrigger) -> Self {
        match trigger {
            ActionTrigger::Activate { annotation, page, source } => plugin::ActionTrigger::Activate {
                annotation_ref: annotation.0.clone(),
                page: page.into(),
                source: source.map(Into::into),
            },
            ActionTrigger::Annotation { event, annotation, page, source } => {
                plugin::ActionTrigger::Annotation {
                    event,
                    annotation_ref: annotation.0.clone(),
                    page: page.into(),
                    source: source.map(Into::into),
                }
            }
            ActionTrigger::Page { event, page } => {
                plugin::ActionTrigger::Page { event, page: page.into() }
            }
            ActionTrigger::Document { event } => plugin::ActionTrigger::Document { event },
        }
    }
}

/// The /A click tree or one /AA key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ActionAnnotationTreeEvent {
    Activate,
    Event { kind: PdfAnnotationEventKind },
}

/// Where a tree lives, for `action_tree`. Annotation and field sources read
/// `page.annotations.list` and `forms.list` (plans 05 and 06 map them).
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum ActionTreeSource {
    Annotation {
        annotation: Arc<PdfObjectRef>,
        page: PageRef,
        event: Option<ActionAnnotationTreeEvent>,
    },
    Field { field: Arc<PdfObjectRef>, event: PdfFieldEventKind },
    Page { page: PageRef, event: Option<PageEventKind> },
    Document { event: Option<DocumentTriggerEvent> },
}

impl From<ActionTreeSource> for plugin::ActionTreeSource {
    fn from(source: ActionTreeSource) -> Self {
        match source {
            ActionTreeSource::Annotation { annotation, page, event } => {
                plugin::ActionTreeSource::Annotation {
                    annotation: annotation.0.clone(),
                    page: page.into(),
                    event: event.map(|e| match e {
                        ActionAnnotationTreeEvent::Activate => {
                            plugin::AnnotationTreeEvent::Activate(plugin::ActivateTag::Activate)
                        }
                        ActionAnnotationTreeEvent::Event { kind } => {
                            plugin::AnnotationTreeEvent::Event(kind)
                        }
                    }),
                }
            }
            ActionTreeSource::Field { field, event } => {
                plugin::ActionTreeSource::Field { field: field.0.clone(), event }
            }
            ActionTreeSource::Page { page, event } => {
                plugin::ActionTreeSource::Page { page: page.into(), event }
            }
            ActionTreeSource::Document { event } => plugin::ActionTreeSource::Document { event },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ActionNodeResult {
    /// Child indexes from the root: `[]` is the root, `[0]` its first `/Next`.
    pub path: Vec<u32>,
    pub node_type: String,
    pub status: ActionNodeStatus,
    pub detail: Option<String>,
}

impl From<&plugin::ActionNodeResult> for ActionNodeResult {
    fn from(node: &plugin::ActionNodeResult) -> Self {
        ActionNodeResult {
            path: node.path.iter().map(|i| u32::try_from(*i).unwrap_or(u32::MAX)).collect(),
            node_type: node.node_type.clone(),
            status: node.status,
            detail: node.detail.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ActionDispatchResult {
    pub status: ActionResultStatus,
    pub nodes: Vec<ActionNodeResult>,
    pub diagnostics: Vec<ActionDiagnostic>,
}

impl From<&plugin::ActionDispatchResult> for ActionDispatchResult {
    fn from(result: &plugin::ActionDispatchResult) -> Self {
        ActionDispatchResult {
            status: result.status,
            nodes: result.nodes.iter().map(Into::into).collect(),
            diagnostics: result.diagnostics.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ActionStepResult {
    pub source: ActionSource,
    pub tree: Arc<PdfActionTree>,
    pub result: ActionDispatchResult,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ActionTriggerResult {
    pub status: ActionResultStatus,
    pub steps: Vec<ActionStepResult>,
    pub diagnostics: Vec<ActionDiagnostic>,
}

impl From<&plugin::ActionTriggerResult> for ActionTriggerResult {
    fn from(result: &plugin::ActionTriggerResult) -> Self {
        ActionTriggerResult {
            status: result.status,
            steps: result
                .steps
                .iter()
                .map(|step| ActionStepResult {
                    source: (&step.source).into(),
                    tree: PdfActionTree::wrap(&step.tree),
                    result: (&step.result).into(),
                })
                .collect(),
            diagnostics: result.diagnostics.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ActionDispatchEvent {
    pub context: ActionContext,
    pub tree: Arc<PdfActionTree>,
    pub result: ActionDispatchResult,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct OpenSequenceCompleted {
    pub result: ActionTriggerResult,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ScriptDiagnosticCode {
    BlockedNetwork,
    UnsupportedApi,
    InvalidFieldValue,
    ScriptError,
    UiEffectSuppressed,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ScriptDiagnostic {
    pub code: ScriptDiagnosticCode,
    pub message: String,
}

impl From<&acrojs::ScriptDiagnostic> for ScriptDiagnostic {
    fn from(diagnostic: &acrojs::ScriptDiagnostic) -> Self {
        use acrojs::ScriptDiagnosticCode as C;
        ScriptDiagnostic {
            code: match diagnostic.code {
                C::BlockedNetwork => ScriptDiagnosticCode::BlockedNetwork,
                C::UnsupportedApi => ScriptDiagnosticCode::UnsupportedApi,
                C::InvalidFieldValue => ScriptDiagnosticCode::InvalidFieldValue,
                C::ScriptError => ScriptDiagnosticCode::ScriptError,
                C::UiEffectSuppressed => ScriptDiagnosticCode::UiEffectSuppressed,
            },
            message: diagnostic.message.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ScriptErrorKind {
    Exception,
    Budget,
    InvalidOutput,
}

impl From<acrojs::ScriptExecutionErrorKind> for ScriptErrorKind {
    fn from(kind: acrojs::ScriptExecutionErrorKind) -> Self {
        match kind {
            acrojs::ScriptExecutionErrorKind::Exception => ScriptErrorKind::Exception,
            acrojs::ScriptExecutionErrorKind::Budget => ScriptErrorKind::Budget,
            acrojs::ScriptExecutionErrorKind::InvalidOutput => ScriptErrorKind::InvalidOutput,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ScriptExecutionError {
    pub kind: ScriptErrorKind,
    pub message: String,
    pub stack: Option<String>,
}

impl From<&acrojs::ScriptExecutionError> for ScriptExecutionError {
    fn from(error: &acrojs::ScriptExecutionError) -> Self {
        ScriptExecutionError {
            kind: error.kind.into(),
            message: error.message.clone(),
            stack: error.stack.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum ActionsEvent {
    /// Every executed tree: dispatch-driven, verb-driven and lifecycle-driven.
    Executed { event: ActionDispatchEvent },
    Diagnostic { event: ActionDiagnostic },
    ScriptDiagnostic { event: ScriptDiagnostic },
    ScriptError { event: ScriptExecutionError },
    OpenSequenceCompleted { event: OpenSequenceCompleted },
}

#[uniffi::export(with_foreign)]
pub trait ActionsEventListener: Send + Sync {
    fn on_event(&self, event: ActionsEvent);
    /// The document closed: no event follows. Called once, and at once for a
    /// subscription made after the close.
    fn on_end(&self);
}

/// The capability, if its document is still open.
fn open(cap: &ActionsCapability) -> Result<&ActionsCapability, Error> {
    if cap.lifetime().aborted() {
        return Err(plugin_error(
            code::INSTANCE_CLOSED,
            "actions",
            "the document this capability belongs to is closed",
            None,
            None,
        ));
    }
    Ok(cap)
}

/// `null` and `undefined` both mean "no tree here".
fn tree_or_none(value: Value) -> Option<Arc<PdfActionTree>> {
    (!value.is_null() && !epdf_kernel::is_undefined(&value)).then(|| Arc::new(PdfActionTree(value)))
}

#[derive(uniffi::Object)]
pub struct ActionsHandle {
    pub(crate) cap: KernelBound<ActionsCapability>,
    pub(crate) post: KernelPost,
}

impl ActionsHandle {
    /// On the kernel thread.
    pub fn new(cap: ActionsCapability, post: KernelPost) -> Arc<Self> {
        Arc::new(ActionsHandle { cap: KernelBound::new(cap, post.clone()), post })
    }
}

#[uniffi::export]
impl ActionsHandle {
    /// The live policy.
    pub async fn policy(&self) -> Result<ActionPolicy, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.get_policy())).await
    }

    /// Row-wise: a row names only the origins it changes.
    pub async fn update_policy(&self, patch: ActionPolicyPatch) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?.update_policy(&patch);
                Ok(())
            })
            .await
    }

    /// True when the config enables JavaScript and supplies an engine.
    pub async fn is_scripting_enabled(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.is_scripting_enabled())).await
    }

    /// Run one tree as one transaction on the document's serial queue.
    pub async fn execute(
        &self,
        tree: Arc<PdfActionTree>,
        context: ActionContext,
    ) -> Result<ActionDispatchResult, ViewerError> {
        self.cap
            .call_promise(
                move |c| Ok(open(c)?.execute(tree.0.clone(), context.into())),
                |result: plugin::ActionDispatchResult| ActionDispatchResult::from(&result),
            )
            .await
    }

    /// Whether the root would be allowed (or handed to the adapter) now.
    pub async fn can_execute(
        &self,
        tree: Arc<PdfActionTree>,
        context: ActionContext,
    ) -> Result<bool, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.can_execute(&tree.0, &context.into()))).await
    }

    /// A Named verb (`NextPage`, `Print`, ...) as a one-node tree.
    pub async fn execute_named(
        &self,
        name: String,
        context: Option<PartialActionContext>,
    ) -> Result<ActionDispatchResult, ViewerError> {
        self.cap
            .call_promise(
                move |c| Ok(open(c)?.execute_named(&name, context.map(Into::into))),
                |result: plugin::ActionDispatchResult| ActionDispatchResult::from(&result),
            )
            .await
    }

    /// `None` when the source has no tree for that event.
    pub async fn action_tree(
        &self,
        source: ActionTreeSource,
    ) -> Result<Option<Arc<PdfActionTree>>, ViewerError> {
        self.cap
            .call_promise(move |c| Ok(open(c)?.get_action_tree(source.into())), tree_or_none)
            .await
    }

    /// Resolve the trigger's trees and run each; a failed step never skips
    /// its siblings.
    pub async fn dispatch(&self, trigger: ActionTrigger) -> Result<ActionTriggerResult, ViewerError> {
        self.cap
            .call_promise(
                move |c| Ok(open(c)?.dispatch(trigger.into())),
                |result: plugin::ActionTriggerResult| ActionTriggerResult::from(&result),
            )
            .await
    }

    pub async fn can_dispatch(&self, trigger: ActionTrigger) -> Result<bool, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.can_dispatch(&trigger.into()))).await
    }

    /// The will-close trigger: call before closing the document.
    pub async fn prepare_close(&self) -> Result<ActionTriggerResult, ViewerError> {
        self.cap
            .call_promise(
                |c| Ok(open(c)?.prepare_close()),
                |result: plugin::ActionTriggerResult| ActionTriggerResult::from(&result),
            )
            .await
    }

    /// The listener is registered on the kernel thread after this returns.
    /// It gets `on_end` when the document closes (at once if it already has).
    pub fn subscribe(&self, listener: Arc<dyn ActionsEventListener>) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.cap.fire(move |c| {
            let ending = listener.clone();
            slot.fill_until_end(
                c.lifetime(),
                || {
                    let hooks: Vec<Unsubscribe> = vec![
                        {
                            let l = listener.clone();
                            c.on_executed(move |e| {
                                l.on_event(ActionsEvent::Executed {
                                    event: ActionDispatchEvent {
                                        context: (&e.ctx).into(),
                                        tree: PdfActionTree::wrap(&e.tree),
                                        result: (&e.result).into(),
                                    },
                                });
                                Ok(())
                            })
                        },
                        {
                            let l = listener.clone();
                            c.on_diagnostic(move |e| {
                                l.on_event(ActionsEvent::Diagnostic { event: e.clone() });
                                Ok(())
                            })
                        },
                        {
                            let l = listener.clone();
                            c.on_script_diagnostic(move |e| {
                                l.on_event(ActionsEvent::ScriptDiagnostic { event: e.into() });
                                Ok(())
                            })
                        },
                        {
                            let l = listener.clone();
                            c.on_script_error(move |e| {
                                l.on_event(ActionsEvent::ScriptError { event: e.into() });
                                Ok(())
                            })
                        },
                        {
                            let l = listener.clone();
                            c.on_open_sequence_completed(move |e| {
                                l.on_event(ActionsEvent::OpenSequenceCompleted {
                                    event: OpenSequenceCompleted { result: (&e.result).into() },
                                });
                                Ok(())
                            })
                        },
                    ];
                    Unsubscribe::new(move || hooks.iter().for_each(Unsubscribe::call))
                },
                move || ending.on_end(),
            );
        });
        subscription
    }
}
```

Create `crates/plugins/actions/src/ffi/ports.rs` and `crates/plugins/actions/src/ffi/sandbox.rs` with a single line each for now, `//! Filled in by Task 2.` and `//! Filled in by Task 5.`, so `pub mod` resolves.

`epdf_kernel::is_undefined` is the sentinel test `crates/epdf-kernel/src/lib.rs:55` exports. The kernel's `PageRef` is `Copy`, which the `(*page).into()` conversions rely on.

- [x] **Step 7: Run the tests to verify they pass**

Run: `cargo test -p epdf-plugin-actions --features uniffi --test ffi`
Expected: PASS, 8 tests.

Run: `cargo test -p epdf-plugin-actions`
Expected: the conformance vectors and `external_uri` still pass without `uniffi`.

- [x] **Step 8: Commit**

```bash
git add crates/plugins/actions
git commit -m "Export the actions plugin's dispatcher through uniffi"
```

### Task 2: The application ports

**Files:**
- Modify: `crates/plugins/actions/src/ffi/ports.rs`
- Test: `crates/plugins/actions/tests/ffi_ports.rs`

**Interfaces:**
- Consumes: Task 1's objects and mirrors; `ActionsHandle { cap, post }`.
- Consumes also: umbrella Task 0c's `epdf_ffi::{ForeignSettle, HostError, RegistrationHandle}`.
- Produces: foreign traits `ActionExecutor` (`execute(node, context, completion)`), `ActionUiHandler` (`open_uri`, `print`, `alert`, `goto_page`, each `-> Result<(), HostError>`), `ActionSubmitHandler` (`submit(request) -> Result<(), HostError>`), `DocumentVerbOperation` (`run(completion)`). Objects `ActionExecutorCompletion` (`executed()`, `inert(reason)`, `failed(error)`), `DocumentVerbCompletion` (`finish()`, `fail(detail)`); registrations answer umbrella Task 0c's `RegistrationHandle`. Records `ActionUiSupport { alert, goto_page }`, `ActionUriOptions`, `ActionAlertOptions`, `ActionSubmitRequest`, `FormSubmissionEntry`, `FormSubmissionValue`. New `ActionsHandle` methods: `register_executor(action_type, executor) -> Arc<RegistrationHandle>`, `set_ui_adapter(handler: Option<Arc<dyn ActionUiHandler>>, support) -> Arc<RegistrationHandle>`, `set_submit_handler(handler: Option<Arc<dyn ActionSubmitHandler>>) -> Arc<RegistrationHandle>`, `run_document_verb(verb, operation) -> ()`.

- [ ] **Step 1: Write the failing port tests**

Create `crates/plugins/actions/tests/ffi_ports.rs`. Copy `named`, `page`, `spec`, `quiet`, `handle`, `user` and `tree` from `tests/ffi.rs` verbatim (integration tests don't share modules), then:

```rust
#![cfg(feature = "uniffi")]

use std::rc::Rc;
use std::sync::{Arc, Mutex};

use epdf_ffi::{HostError, KernelThread};
use epdf_kernel::rt::Promise;
use epdf_plugin_actions::ffi::ports::{
    ActionAlertOptions, ActionExecutor, ActionExecutorCompletion, ActionSubmitHandler,
    ActionSubmitRequest, ActionUiHandler, ActionUiSupport, ActionUriOptions, DocumentVerbCompletion,
    DocumentVerbOperation, FormSubmissionValue,
};
use epdf_plugin_actions::ffi::{ActionContext, ActionSource, ActionsHandle, PdfActionNode, PdfActionTree};
use epdf_plugin_actions::{
    ACTIONS, ActionNodeStatus, ActionOrigin, ActionTriggerEvent, ActionUiContext, ActionsConfig,
    DocumentVerb, FormSubmissionEntry, OpenSequence, actions_plugin,
};
use futures::executor::block_on;
use plugin_conformance::test_kernel::{DOCUMENT, Reply, TestKernel, TestSpec};
use serde_json::{Value, json};

// named, page, spec, quiet, handle, user, tree: copied from tests/ffi.rs

#[derive(Clone, Copy)]
enum Answer {
    Executed,
    Drop,
}

struct Executor {
    answer: Answer,
    seen: Mutex<Vec<(String, ActionOrigin)>>,
}

impl ActionExecutor for Executor {
    fn execute(&self, node: Arc<PdfActionNode>, context: ActionContext, completion: Arc<ActionExecutorCompletion>) {
        self.seen.lock().unwrap().push((node.node_type(), context.origin));
        match self.answer {
            Answer::Executed => completion.executed(),
            Answer::Drop => drop(completion),
        }
    }
}

fn goto() -> Arc<PdfActionTree> {
    tree(json!({
        "root": { "type": "goto", "subtype": "GoTo",
                  "destination": { "kind": "fit", "page": { "kind": "objectNumber", "pageObjectNumber": 1 } },
                  "next": [] },
        "incomplete": false, "warningFlags": 0, "warnings": []
    }))
}

#[test]
fn a_swift_executor_runs_its_node_type() {
    let (_thread, actions) = handle();
    let executor = Arc::new(Executor { answer: Answer::Executed, seen: Mutex::default() });
    let _registration = block_on(actions.register_executor("goto".into(), executor.clone())).unwrap();
    let result = block_on(actions.execute(goto(), user())).unwrap();
    assert_eq!(result.nodes[0].status, ActionNodeStatus::Executed);
    assert_eq!(*executor.seen.lock().unwrap(), [("goto".to_string(), ActionOrigin::User)]);
}

#[test]
fn a_dropped_executor_completion_fails_the_node() {
    let (_thread, actions) = handle();
    let executor = Arc::new(Executor { answer: Answer::Drop, seen: Mutex::default() });
    let _registration = block_on(actions.register_executor("goto".into(), executor)).unwrap();
    let result = block_on(actions.execute(goto(), user())).unwrap();
    assert_eq!(result.nodes[0].status, ActionNodeStatus::Failed);
    // The queue isn't stuck: the next dispatch runs.
    let again = block_on(actions.execute(goto(), user())).unwrap();
    assert_eq!(again.nodes.len(), 1);
}

#[test]
fn an_unregistered_executor_is_not_called() {
    let (thread, actions) = handle();
    let executor = Arc::new(Executor { answer: Answer::Executed, seen: Mutex::default() });
    let registration = block_on(actions.register_executor("goto".into(), executor.clone())).unwrap();
    registration.unregister();
    block_on(thread.call(|_| ())).unwrap();
    let result = block_on(actions.execute(goto(), user())).unwrap();
    assert_ne!(result.nodes[0].status, ActionNodeStatus::Executed);
    assert!(executor.seen.lock().unwrap().is_empty());
}

#[derive(Default)]
struct Ui {
    opened: Mutex<Vec<String>>,
    alerts: Mutex<Vec<String>>,
}

impl ActionUiHandler for Ui {
    fn open_uri(&self, uri: String, _options: ActionUriOptions) -> Result<(), HostError> {
        self.opened.lock().unwrap().push(uri);
        Ok(())
    }
    fn print(&self, _context: Option<ActionUiContext>) -> Result<(), HostError> {
        Ok(())
    }
    fn alert(&self, message: String, _options: ActionAlertOptions) -> Result<(), HostError> {
        self.alerts.lock().unwrap().push(message);
        Ok(())
    }
    fn goto_page(&self, _page: f64, _context: ActionUiContext) -> Result<(), HostError> {
        Ok(())
    }
}

fn uri(uri: &str) -> Arc<PdfActionTree> {
    tree(json!({
        "root": { "type": "uri", "subtype": "URI", "uri": uri, "isMap": false, "next": [] },
        "incomplete": false, "warningFlags": 0, "warnings": []
    }))
}

#[test]
fn only_allowed_schemes_reach_the_swift_handler() {
    let (_thread, actions) = handle();
    let ui = Arc::new(Ui::default());
    let _registration = block_on(actions.set_ui_adapter(
        Some(ui.clone()),
        ActionUiSupport { alert: true, goto_page: true },
    ))
    .unwrap();
    let mut statuses = Vec::new();
    for target in ["javascript:alert(1)", "file:///etc/passwd", " https://a.test/ "] {
        statuses.push(block_on(actions.execute(uri(target), user())).unwrap().nodes[0].status);
    }
    assert_eq!(*ui.opened.lock().unwrap(), ["https://a.test/"]);
    assert_eq!(statuses, [ActionNodeStatus::Blocked, ActionNodeStatus::Blocked, ActionNodeStatus::Executed]);
}

struct Submit(Mutex<Vec<ActionSubmitRequest>>, bool);

impl ActionSubmitHandler for Submit {
    fn submit(&self, request: ActionSubmitRequest) -> Result<(), HostError> {
        self.0.lock().unwrap().push(request);
        if self.1 { Err(HostError::Failed { detail: "offline".into() }) } else { Ok(()) }
    }
}

fn submit_tree() -> Arc<PdfActionTree> {
    tree(json!({
        "root": { "type": "submit-form", "subtype": "SubmitForm",
                  "payload": { "url": "https://home.test/submit", "fields": null,
                               "flags": { "raw": 4, "exclude": false, "includeNoValueFields": false,
                                          "format": "html", "method": "post", "submitCoordinates": false,
                                          "includeAppendSaves": false, "includeAnnotations": false,
                                          "canonicalFormat": false, "exclNonUserAnnots": false,
                                          "exclFKey": false, "embedForm": false } },
                  "next": [] },
        "incomplete": false, "warningFlags": 0, "warnings": []
    }))
}

/// Registers the form plugin's half (the dataset resolver) from Rust, as the
/// form plugin will in plan 06.
fn with_resolver(thread: &KernelThread<TestKernel>) {
    block_on(thread.call(|k| {
        let actions = k.capability(&ACTIONS).unwrap();
        let disposer = actions.register_submit_resolver(Rc::new(
            |intent: &epdf_plugin_actions::SubmitIntent,
             ctx: &epdf_plugin_actions::ActionContext,
             _diagnose: Rc<dyn Fn(epdf_plugin_actions::ActionDiagnostic)>| {
            Ok(Promise::resolved(epdf_plugin_actions::ActionSubmitRequest {
                url: intent.url.clone(),
                method: intent.method.clone(),
                format: intent.format.clone(),
                flags_raw: intent.flags_raw,
                char_set: None,
                entries: vec![
                    FormSubmissionEntry { name: "name".into(), value: json!("Ana") },
                    FormSubmissionEntry { name: "colors".into(), value: json!(["red", "blue"]) },
                    FormSubmissionEntry { name: "flag".into(), value: Value::Null },
                ],
                origin: ctx.origin,
                event: ctx.event.clone(),
            }))
        },
        ));
        // The test kernel lives as long as the test; keep the resolver.
        std::mem::forget(disposer);
    }))
    .unwrap();
}

#[test]
fn a_submit_reaches_the_swift_handler_typed() {
    let (thread, actions) = handle();
    with_resolver(&thread);
    let handler = Arc::new(Submit(Mutex::default(), false));
    let _registration = block_on(actions.set_submit_handler(Some(handler.clone()))).unwrap();
    let result = block_on(actions.execute(submit_tree(), user())).unwrap();
    assert_eq!(result.nodes[0].status, ActionNodeStatus::Executed);
    let seen = handler.0.lock().unwrap();
    assert_eq!(seen[0].url.as_deref(), Some("https://home.test/submit"));
    assert_eq!(seen[0].flags_raw, 4);
    assert_eq!(
        seen[0].entries.iter().map(|e| e.value.clone()).collect::<Vec<_>>(),
        [
            FormSubmissionValue::Text { value: "Ana".into() },
            FormSubmissionValue::List { values: vec!["red".into(), "blue".into()] },
            FormSubmissionValue::NameOnly,
        ]
    );
}

#[test]
fn a_throwing_submit_handler_fails_the_node() {
    let (thread, actions) = handle();
    with_resolver(&thread);
    let _registration =
        block_on(actions.set_submit_handler(Some(Arc::new(Submit(Mutex::default(), true))))).unwrap();
    let result = block_on(actions.execute(submit_tree(), user())).unwrap();
    assert_eq!(result.nodes[0].status, ActionNodeStatus::Failed);
    assert_eq!(result.nodes[0].detail.as_deref(), Some("offline"));
}

struct Save(Mutex<usize>, bool);

impl DocumentVerbOperation for Save {
    fn run(&self, completion: Arc<DocumentVerbCompletion>) {
        *self.0.lock().unwrap() += 1;
        if self.1 { completion.fail("read-only".into()) } else { completion.finish() }
    }
}

#[test]
fn a_document_verb_runs_its_operation_between_the_will_and_did_trees() {
    let (_thread, actions) = handle();
    let save = Arc::new(Save(Mutex::new(0), false));
    block_on(actions.run_document_verb(DocumentVerb::Save, save.clone())).unwrap();
    assert_eq!(*save.0.lock().unwrap(), 1);
    let failing = Arc::new(Save(Mutex::new(0), true));
    let error = block_on(actions.run_document_verb(DocumentVerb::Save, failing)).unwrap_err();
    assert!(format!("{error}").contains("read-only"), "{error}");
}
```

`epdf_plugin_actions::ActionSubmitRequest` and `FormSubmissionEntry` are the crate's types (re-exported by `pub use contract::*`); the test module imports the ffi mirror `ActionSubmitRequest` from `ffi::ports`, so the crate one is spelled with its full path.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p epdf-plugin-actions --features uniffi --test ffi_ports`
Expected: FAIL to compile with "unresolved imports `epdf_plugin_actions::ffi::ports::ActionAlertOptions`" (and the other port names).

- [ ] **Step 3: Write `crates/plugins/actions/src/ffi/ports.rs`**

```rust
//! The application ports as foreign traits: node executors, the UI adapter,
//! the submit handler and a save or print operation. Each is called on the
//! kernel thread. One that answers later does so through a completion
//! object, from any thread; a completion dropped unanswered fails its call.

use std::rc::Rc;
use std::sync::Arc;

use epdf_ffi::{ForeignSettle, HostError, KernelPost, RegistrationHandle, ViewerError};
use epdf_kernel::Error;
use epdf_kernel::rt::Promise;
use serde_json::Value;

use super::{ActionContext, ActionsHandle, PdfActionNode, open};
use crate::contract::{
    self as plugin, ActionOrigin, ActionTriggerEvent, ActionUiContext, Disposer, DocumentVerb,
    ScriptPhase,
};

/// Runs one node type. Never sees the tree or the capability.
#[uniffi::export(with_foreign)]
pub trait ActionExecutor: Send + Sync {
    fn execute(
        &self,
        node: Arc<PdfActionNode>,
        context: ActionContext,
        completion: Arc<ActionExecutorCompletion>,
    );
}

/// The answer to one `ActionExecutor::execute`. The first call wins.
#[derive(uniffi::Object)]
pub struct ActionExecutorCompletion {
    settle: ForeignSettle<plugin::ActionExecutorResult>,
}

#[uniffi::export]
impl ActionExecutorCompletion {
    pub fn executed(&self) {
        self.settle.settle(|| Ok(plugin::ActionExecutorResult::Executed));
    }

    /// Recognized, deliberately not acted on.
    pub fn inert(&self, reason: String) {
        self.settle.settle(move || Ok(plugin::ActionExecutorResult::Inert { reason }));
    }

    pub fn failed(&self, error: String) {
        self.settle.settle(move || Ok(plugin::ActionExecutorResult::Failed { error }));
    }
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ActionUriOptions {
    pub is_map: bool,
    pub origin: ActionOrigin,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ActionAlertOptions {
    pub origin: ActionOrigin,
    pub phase: ScriptPhase,
    /// Acrobat's `app.alert` icon number.
    pub icon: f64,
    pub title: Option<String>,
}

/// Which optional UI members the handler implements. Without `alert`, a
/// script alert is suppressed with a diagnostic; without `goto_page`, a
/// script page change is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ActionUiSupport {
    #[uniffi(default = true)]
    pub alert: bool,
    #[uniffi(default = true)]
    pub goto_page: bool,
}

/// The UI a document's actions reach. A throw propagates into the dispatch.
#[uniffi::export(with_foreign)]
pub trait ActionUiHandler: Send + Sync {
    /// Already through the scheme allowlist (http, https, mailto, tel), but
    /// still document-supplied: treat it as untrusted.
    fn open_uri(&self, uri: String, options: ActionUriOptions) -> Result<(), HostError>;
    /// The Named Print verb and script `print()`.
    fn print(&self, context: Option<ActionUiContext>) -> Result<(), HostError>;
    fn alert(&self, message: String, options: ActionAlertOptions) -> Result<(), HostError>;
    /// `page` is what the script asked for, zero-based and unchecked.
    fn goto_page(&self, page: f64, context: ActionUiContext) -> Result<(), HostError>;
}

fn ui_adapter(handler: Arc<dyn ActionUiHandler>, support: ActionUiSupport) -> plugin::ActionUiAdapter {
    type Alert = Rc<dyn Fn(&str, &plugin::AlertOptions) -> Result<(), Error>>;
    type GotoPage = Rc<dyn Fn(f64, &ActionUiContext) -> Result<(), Error>>;
    let open_uri = {
        let h = handler.clone();
        Rc::new(move |uri: &str, options: &plugin::UriOptions| {
            let options = ActionUriOptions { is_map: options.is_map, origin: options.origin };
            h.open_uri(uri.to_string(), options).map_err(Error::from)
        })
    };
    let print = {
        let h = handler.clone();
        Rc::new(move |context: Option<&ActionUiContext>| h.print(context.cloned()).map_err(Error::from))
    };
    let alert = support.alert.then(|| {
        let h = handler.clone();
        Rc::new(move |message: &str, options: &plugin::AlertOptions| {
            let options = ActionAlertOptions {
                origin: options.origin,
                phase: options.phase,
                icon: options.icon,
                title: options.title.clone(),
            };
            h.alert(message.to_string(), options).map_err(Error::from)
        }) as Alert
    });
    let goto_page = support.goto_page.then(|| {
        let h = handler.clone();
        Rc::new(move |page: f64, context: &ActionUiContext| {
            h.goto_page(page, context.clone()).map_err(Error::from)
        }) as GotoPage
    });
    plugin::ActionUiAdapter { open_uri, print, alert, goto_page }
}

/// A submitted value: a string, a list (multi-select), or name only.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum FormSubmissionValue {
    Text { value: String },
    List { values: Vec<String> },
    NameOnly,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct FormSubmissionEntry {
    pub name: String,
    pub value: FormSubmissionValue,
}

/// The resolved dataset a submit handler receives.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ActionSubmitRequest {
    pub url: Option<String>,
    pub method: String,
    pub format: String,
    pub flags_raw: u32,
    pub char_set: Option<String>,
    pub entries: Vec<FormSubmissionEntry>,
    pub origin: ActionOrigin,
    pub event: ActionTriggerEvent,
}

impl TryFrom<&plugin::ActionSubmitRequest> for ActionSubmitRequest {
    type Error = Error;

    fn try_from(request: &plugin::ActionSubmitRequest) -> Result<Self, Error> {
        let entries = request
            .entries
            .iter()
            .map(|entry| {
                let value = match &entry.value {
                    Value::String(value) => FormSubmissionValue::Text { value: value.clone() },
                    Value::Null => FormSubmissionValue::NameOnly,
                    Value::Array(items) => FormSubmissionValue::List {
                        values: items
                            .iter()
                            .map(|item| item.as_str().map(str::to_string))
                            .collect::<Option<Vec<_>>>()
                            .ok_or_else(|| bad_entry(&entry.name, &entry.value))?,
                    },
                    other => return Err(bad_entry(&entry.name, other)),
                };
                Ok(FormSubmissionEntry { name: entry.name.clone(), value })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(ActionSubmitRequest {
            url: request.url.clone(),
            method: request.method.clone(),
            format: request.format.clone(),
            flags_raw: request.flags_raw as u32,
            char_set: request.char_set.clone(),
            entries,
            origin: request.origin,
            event: request.event.clone(),
        })
    }
}

fn bad_entry(name: &str, value: &Value) -> Error {
    Error::named("InvalidState", format!("[actions] submit entry \"{name}\" has value {value}"))
        .with("code", "invalid-state")
}

/// Sink 1 of the submit chain. Return to accept (the network call is yours,
/// and its outcome is yours to report); throw to fail the node.
#[uniffi::export(with_foreign)]
pub trait ActionSubmitHandler: Send + Sync {
    fn submit(&self, request: ActionSubmitRequest) -> Result<(), HostError>;
}

/// A save or print, run between the document's will- and did- trees.
#[uniffi::export(with_foreign)]
pub trait DocumentVerbOperation: Send + Sync {
    fn run(&self, completion: Arc<DocumentVerbCompletion>);
}

#[derive(uniffi::Object)]
pub struct DocumentVerbCompletion {
    settle: ForeignSettle<()>,
}

#[uniffi::export]
impl DocumentVerbCompletion {
    pub fn finish(&self) {
        self.settle.settle(|| Ok(()));
    }

    pub fn fail(&self, detail: String) {
        self.settle.settle(move || {
            Err(Error::new(detail).with("code", epdf_kernel::error::code::OPERATION_FAILED))
        });
    }
}

/// One registration, as umbrella Task 0c's `RegistrationHandle`. Dropping
/// it keeps what it registered; `unregister` removes it, and only while it
/// is still the one installed. The closures that register run on the kernel
/// thread without `&self`, so this takes the post explicitly.
fn registration(remove: Disposer, post: KernelPost) -> Arc<RegistrationHandle> {
    RegistrationHandle::new(post, move || remove())
}

#[uniffi::export]
impl ActionsHandle {
    /// Last wins: a second executor for a type replaces the first (with a
    /// `duplicate-executor` diagnostic).
    pub async fn register_executor(
        &self,
        action_type: String,
        executor: Arc<dyn ActionExecutor>,
    ) -> Result<Arc<RegistrationHandle>, ViewerError> {
        let post = self.post.clone();
        self.cap
            .call(move |c| {
                let settle_post = post.clone();
                let run: plugin::ActionExecutor =
                    Rc::new(move |node: &Value, ctx: &plugin::ActionContext| {
                        let (promise, resolver) = Promise::pending();
                        let completion = Arc::new(ActionExecutorCompletion {
                            settle: ForeignSettle::new(
                                resolver,
                                settle_post.clone(),
                                "an ActionExecutor completion",
                            ),
                        });
                        executor.execute(Arc::new(PdfActionNode(node.clone())), ctx.into(), completion);
                        Ok(promise)
                    });
                Ok(registration(open(c)?.register_executor(&action_type, run), post))
            })
            .await
    }

    /// `None` uninstalls. Installing one releases an `auto` open sequence.
    pub async fn set_ui_adapter(
        &self,
        handler: Option<Arc<dyn ActionUiHandler>>,
        support: ActionUiSupport,
    ) -> Result<Arc<RegistrationHandle>, ViewerError> {
        let post = self.post.clone();
        self.cap
            .call(move |c| {
                let adapter = handler.map(|h| Rc::new(ui_adapter(h, support)));
                Ok(registration(open(c)?.set_ui_adapter(adapter), post))
            })
            .await
    }

    /// `None` uninstalls.
    pub async fn set_submit_handler(
        &self,
        handler: Option<Arc<dyn ActionSubmitHandler>>,
    ) -> Result<Arc<RegistrationHandle>, ViewerError> {
        let post = self.post.clone();
        self.cap
            .call(move |c| {
                let handler = handler.map(|h| -> plugin::ActionSubmitHandler {
                    Rc::new(move |request: &plugin::ActionSubmitRequest, _home: plugin::SubmitHandlerContext| {
                        h.submit(ActionSubmitRequest::try_from(request)?).map_err(Error::from)?;
                        Ok(None)
                    })
                });
                Ok(registration(open(c)?.set_submit_handler(handler), post))
            })
            .await
    }

    /// Runs the will- trees, then `operation`, then the did- trees, on the
    /// document's serial queue.
    pub async fn run_document_verb(
        &self,
        verb: DocumentVerb,
        operation: Arc<dyn DocumentVerbOperation>,
    ) -> Result<(), ViewerError> {
        let post = self.post.clone();
        self.cap
            .call_promise(
                move |c| {
                    Ok(open(c)?.run_document_verb(verb, move || {
                        let (promise, resolver) = Promise::pending();
                        operation.run(Arc::new(DocumentVerbCompletion {
                            settle: ForeignSettle::new(resolver, post, "a DocumentVerbOperation completion"),
                        }));
                        Ok(promise)
                    }))
                },
                |()| (),
            )
            .await
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p epdf-plugin-actions --features uniffi --test ffi_ports`
Expected: PASS, 7 tests.

- [ ] **Step 5: Commit**

```bash
git add crates/plugins/actions
git commit -m "Add the actions plugin's executor, UI, submit and verb ports as foreign traits"
```

### Task 3: The `actions.read` engine path

**Files:**
- Modify: `crates/epdf-viewer-host/src/translate.rs`
- Test: `crates/epdf-viewer-host/tests/translate.rs`

**Interfaces:**
- Consumes: umbrella Task 0e's `PATHS` and the namespace rule in `maps`.
- Produces: `translate("…", "actions.read", None, &[], job)` builds `{"kind": "actions.read", "jobId", "docId"}` decoded with `Decode::Result`; `maps("actions")` is true (the namespace `catalog.rs` feature-detects with `doc.has("actions")`); `maps("forms.submit")` stays false.

Appendix C's `forms.submit` row ("none on the wire, B3 (host submit handler)") is met by Task 2's `ActionSubmitHandler`, not by a route. `NativeEngine::has("forms.submit")` must stay false so `perform.rs` never prefers a document home that doesn't exist; a test pins it.

- [ ] **Step 1: Write the failing translation tests**

Append to `crates/epdf-viewer-host/tests/translate.rs`:

```rust
use epdf_viewer_host::translate::maps;

#[test]
fn actions_read_becomes_its_wire_kind() {
    let call = translate("d1", "actions.read", None, &[], 11).unwrap();
    assert_eq!(call.request, json!({"kind": "actions.read", "jobId": 11, "docId": "d1"}));
    assert!(matches!(call.decode, Decode::Result));
}

#[test]
fn the_actions_namespace_is_present() {
    assert!(maps("actions"));
    assert!(maps("actions.read"));
}

#[test]
fn forms_submit_stays_unmapped_so_submits_reach_the_handler() {
    assert!(!maps("forms.submit"));
}

#[test]
fn a_namespace_is_not_a_prefix_match() {
    assert!(!maps("act"));
    assert!(!maps("actions.write"));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p epdf-viewer-host --test translate actions`
Expected: FAIL: `actions_read_becomes_its_wire_kind` panics on `unwrap()` of an `unsupported` error, and `the_actions_namespace_is_present` fails its first assertion.

- [ ] **Step 3: Map the path and list it**

In `crates/epdf-viewer-host/src/translate.rs`, add to `route`'s match, after the `search.query` arm:

```rust
        "actions.read" => ("actions.read", Decode::Result),
```

and to `PATHS` (umbrella Task 0e), one line in plan order:

```rust
    "actions.read",
```

`maps` derives namespaces from `PATHS`: `maps("actions")` is true because `actions.read` is listed, and `maps("forms.submit")` stays false because no route or listed path is `forms.submit` or starts with `forms.submit.`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p epdf-viewer-host --test translate`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/epdf-viewer-host/src/translate.rs crates/epdf-viewer-host/tests/translate.rs
git commit -m "Map actions.read in the native engine"
```

### Task 4: Host wiring

**Files:**
- Modify: `crates/epdf-viewer-host/Cargo.toml`
- Modify: `crates/plugins/actions/src/ffi.rs` (the `ActionsConfig` record)
- Modify: `crates/epdf-viewer-host/src/api.rs`
- Modify: `crates/epdf-viewer-host/src/session.rs`
- Modify: `crates/epdf-viewer-host/src/lib.rs`
- Test: `crates/epdf-viewer-host/tests/viewer_host.rs`

**Interfaces:**
- Consumes: `ActionsHandle`; Task 3's `actions.read`.
- Produces: `epdf_plugin_actions::ffi::ActionsConfig { policy, triggers, open_sequence, utc_offset_minutes }` with `fn into_plugin_config(self, observer_error: Option<Rc<dyn Fn(&str, &Error)>>) -> crate::ActionsConfig`; `ViewerConfig.actions: Option<ActionsConfig>`; `ViewerHost::actions(document_id) -> Result<Arc<ActionsHandle>, ViewerError>`. Task 5 adds `javascript` to `ActionsConfig`.

- [ ] **Step 1: Write the failing host tests**

Append to `crates/epdf-viewer-host/tests/viewer_host.rs`:

```rust
use epdf_plugin_actions::ffi::{ActionTreeSource, ActionTrigger};
use epdf_plugin_actions::{ActionNodeStatus, DocumentTriggerEvent, OpenSequence};

const DOCUMENT_ACTIONS: &str = "tooling/engine-conformance/fixtures/document_actions.pdf";

fn with_actions() -> Arc<ViewerHost> {
    let config = ViewerConfig {
        actions: Some(epdf_plugin_actions::ffi::ActionsConfig {
            policy: None,
            triggers: None,
            open_sequence: Some(OpenSequence::Off),
            utc_offset_minutes: None,
        }),
        ..ViewerConfig::default()
    };
    let host = ViewerHost::new(config, Arc::new(Delegate)).unwrap();
    block_on(host.start()).unwrap();
    host
}

#[test]
fn the_actions_plugin_reads_the_catalog_through_the_native_engine() {
    let host = with_actions();
    let doc = block_on(host.open_document(test_support::fixture(DOCUMENT_ACTIONS), options())).unwrap();
    let actions = block_on(host.actions(doc.id)).unwrap();
    let open = block_on(actions.action_tree(ActionTreeSource::Document { event: None })).unwrap();
    assert_eq!(open.and_then(|t| t.root_type()).as_deref(), Some("javascript"));
    assert!(!block_on(actions.is_scripting_enabled()).unwrap());
    // Scripting is off, so the will-close script is recognized and inert.
    let result = block_on(actions.dispatch(ActionTrigger::Document { event: DocumentTriggerEvent::WillClose })).unwrap();
    assert_eq!(result.steps.len(), 1);
    assert_eq!(result.steps[0].result.nodes[0].status, ActionNodeStatus::Inert);
}

#[test]
fn without_its_config_the_actions_plugin_is_not_installed() {
    let host = started();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options())).unwrap();
    assert!(block_on(host.actions(doc.id)).is_err());
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p epdf-viewer-host --test viewer_host actions`
Expected: FAIL to compile with "unresolved import `epdf_plugin_actions`".

- [ ] **Step 3: Write the config record**

Append to `crates/plugins/actions/src/ffi.rs`:

```rust
use std::rc::Rc;

use crate::contract::{OpenSequence, TriggerGates};

/// `actionsPlugin(config)` as Swift configures it. Task 5 adds JavaScript.
#[derive(Clone, Debug, Default, uniffi::Record)]
pub struct ActionsConfig {
    /// Row-wise overrides of the default policy.
    #[uniffi(default = None)]
    pub policy: Option<ActionPolicyPatch>,
    /// Trigger-family gates, default all on.
    #[uniffi(default = None)]
    pub triggers: Option<TriggerGates>,
    /// When the document-open sequence fires. Default `auto`: when a UI
    /// adapter installs or on the first user dispatch.
    #[uniffi(default = None)]
    pub open_sequence: Option<OpenSequence>,
    /// Local minus UTC, in minutes, for script dates. Fixed for the
    /// viewer's lifetime. Default zero.
    #[uniffi(default = None)]
    pub utc_offset_minutes: Option<i32>,
}

impl ActionsConfig {
    /// `observer_error` is where a throwing observer on one of the five hooks
    /// goes.
    pub fn into_plugin_config(
        self,
        observer_error: Option<Rc<dyn Fn(&str, &Error)>>,
    ) -> crate::ActionsConfig {
        crate::ActionsConfig {
            policy: self.policy,
            triggers: self.triggers,
            open_sequence: self.open_sequence,
            javascript: None,
            observer_error,
            host_utc_offset_minutes: self
                .utc_offset_minutes
                .map(|minutes| Rc::new(move |_now: f64| f64::from(minutes)) as Rc<dyn Fn(f64) -> f64>),
        }
    }
}
```

Move the two new `use` lines to the top of the file with the others.

- [ ] **Step 4: Wire the plugin in**

`crates/epdf-viewer-host/Cargo.toml`, alphabetically:

```toml
epdf-plugin-actions = { path = "../plugins/actions", default-features = false, features = ["uniffi"] }
```

`crates/epdf-viewer-host/src/api.rs`, with the plugin imports `use epdf_plugin_actions::ffi::ActionsConfig;`, and at the end of `ViewerConfig` in plan order:

```rust
    /// Install `actionsPlugin(config)`: the PDF action engine, a dispatcher
    /// per document.
    #[uniffi(default = None)]
    pub actions: Option<ActionsConfig>,
```

`crates/epdf-viewer-host/src/session.rs`, after the other plugin blocks:

```rust
        if let Some(actions) = config.actions {
            let observer = delegate.clone();
            let observer_error: Rc<dyn Fn(&str, &Error)> = Rc::new(move |_hook, error| {
                observer.report(ViewerError::from(error.clone()));
            });
            plugins.push(epdf_plugin_actions::actions_plugin(
                actions.into_plugin_config(Some(observer_error)),
            ));
        }
```

`delegate` is still available here: `reporter` is a clone taken for `KernelOptions.report`, and `delegate` itself moves only into the store subscription below.

`crates/epdf-viewer-host/src/lib.rs`: `use epdf_plugin_actions::ffi::ActionsHandle;` and, in plan order among the accessors:

```rust
    /// The action engine of an open document. Needs `ViewerConfig.actions`.
    pub async fn actions(&self, document_id: String) -> Result<Arc<ActionsHandle>, ViewerError> {
        let post = self.post.clone();
        self.call(move |s| {
            let cap = s.kernel.capability(&epdf_plugin_actions::ACTIONS, Some(&document_id))?;
            Ok(ActionsHandle::new(cap, post))
        })
        .await
    }
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p epdf-ffi -p epdf-viewer-host`
Expected: PASS, including the two new tests.

- [ ] **Step 6: Commit**

```bash
git add crates/plugins/actions/src/ffi.rs crates/epdf-viewer-host
git commit -m "Install the actions plugin from ViewerConfig and expose ViewerHost.actions"
```

### Task 5: The JavaScript sandbox

**Files:**
- Modify: `crates/plugins/actions/src/ffi/sandbox.rs`
- Modify: `crates/plugins/actions/src/ffi.rs` (`ActionsConfig.javascript`)
- Test: `crates/plugins/actions/tests/ffi_sandbox.rs`
- Test: `crates/epdf-viewer-host/tests/viewer_host.rs`

**Interfaces:**
- Consumes: `epdf_acrojs::{ScriptSandbox, ScriptSandboxFactory, ScriptInput, ScriptOutput, ScriptBudget, DEFAULT_SCRIPT_BUDGET, PRELUDE_SOURCE}`; `epdf_ffi::HostError`.
- Produces: foreign traits `ScriptEngine` (`evaluate(program: String, time_limit_ms: u32) -> ScriptEvaluation`, `dispose()`) and `ScriptEngineFactory` (`create() -> Result<Arc<dyn ScriptEngine>, HostError>`); `ScriptEvaluation { Value { json }, Threw { detail }, TimedOut }`; `ActionsScriptBudget`; `ActionsJavaScriptConfig { enabled, engine, budget, max_script_nodes_per_dispatch }`; `ActionsConfig.javascript: Option<ActionsJavaScriptConfig>`; `pub fn sandbox_factory(engine: Arc<dyn ScriptEngineFactory>) -> ScriptSandboxFactory`; exported `check_script_engine(factory, source) -> Result<ScriptCheck, ViewerError>` with `ScriptCheck { alerts: Vec<String>, error: Option<ScriptExecutionError>, disposed: bool }`.

The program the sandbox hands the engine is the one `QuickJsSandbox.ts` `invoke` evaluates, wrapped in `JSON.stringify` so the engine only has to return a string: `JSON.stringify(globalThis.__acrojsRun(<source as JSON>,<input as JSON>,{"maxEffects":N}))`. The validation after it follows that file line by line: a thrown evaluation is `invalid-output` (or `budget` when it timed out), output that doesn't decode as `ScriptOutput` is `invalid-output`, a script error naming a resource limit is `budget`, and output past `maxOutputBytes` is `budget`. Each of those poisons the realm.

- [ ] **Step 1: Write the failing sandbox tests**

Create `crates/plugins/actions/tests/ffi_sandbox.rs`. Copy `named`, `page`, `spec` and `user` from `tests/ffi.rs`, then:

```rust
#![cfg(feature = "uniffi")]

use std::sync::{Arc, Mutex};

use epdf_acrojs::{
    DEFAULT_SCRIPT_BUDGET, ScriptDocumentInput, ScriptEnvironment, ScriptEventInput,
    ScriptEventKind, ScriptExecutionErrorKind, ScriptIdentity, ScriptInput,
};
use epdf_ffi::{HostError, KernelThread};
use epdf_plugin_actions::ffi::ports::{
    ActionAlertOptions, ActionUiHandler, ActionUiSupport, ActionUriOptions,
};
use epdf_plugin_actions::ffi::sandbox::{
    ScriptEngine, ScriptEngineFactory, ScriptEvaluation, sandbox_factory,
};
use epdf_plugin_actions::ffi::{ActionContext, ActionSource, ActionsHandle, PdfActionTree};
use epdf_plugin_actions::{
    ACTIONS, ActionNodeStatus, ActionOrigin, ActionTriggerEvent, ActionUiContext, ActionsConfig,
    JavaScriptConfig, OpenSequence, actions_plugin,
};
use futures::executor::block_on;
use plugin_conformance::test_kernel::{DOCUMENT, Reply, TestKernel, TestSpec};
use serde_json::{Value, json};

// named, page, spec, user: copied from tests/ffi.rs

const PROBE: &str = "\"function\"";

fn output(ui: Value) -> String {
    json!({
        "event": { "rc": true, "value": null, "change": "", "selStart": 0, "selEnd": 0 },
        "formEffects": [], "annotEffects": [], "uiEffects": ui, "diagnostics": []
    })
    .to_string()
}

/// A scripted engine: answers the prelude probe, then whatever `run` says.
struct Engine {
    run: Box<dyn Fn(&str) -> ScriptEvaluation + Send + Sync>,
    programs: Mutex<Vec<String>>,
    disposed: Mutex<bool>,
}

impl ScriptEngine for Engine {
    fn evaluate(&self, program: String, _time_limit_ms: u32) -> ScriptEvaluation {
        self.programs.lock().unwrap().push(program.clone());
        if program.contains("typeof globalThis.__acrojsRun") {
            return ScriptEvaluation::Value { json: PROBE.into() };
        }
        (self.run)(&program)
    }
    fn dispose(&self) {
        *self.disposed.lock().unwrap() = true;
    }
}

struct Factory(Arc<Engine>);

impl ScriptEngineFactory for Factory {
    fn create(&self) -> Result<Arc<dyn ScriptEngine>, HostError> {
        Ok(self.0.clone())
    }
}

fn engine(run: impl Fn(&str) -> ScriptEvaluation + Send + Sync + 'static) -> Arc<Engine> {
    Arc::new(Engine { run: Box::new(run), programs: Mutex::default(), disposed: Mutex::new(false) })
}

fn input() -> ScriptInput {
    let mut event = ScriptEventInput::of(ScriptEventKind::WidgetActivate);
    event.event_type = Some("Doc".into());
    event.name = Some("Open".into());
    ScriptInput {
        document: ScriptDocumentInput { id: "d".into(), file_name: "d.pdf".into(), page_count: 1.0, page_number: 0.0 },
        identity: ScriptIdentity::default(),
        environment: ScriptEnvironment { now_ms: 0.0, utc_offset_minutes: 0.0, random_seed: 1.0 },
        fields: vec![],
        annots: None,
        annot_pages: None,
        annots_cover_document: None,
        event,
    }
}

/// The sandbox the factory builds, on this thread: its promise is already
/// settled.
fn sandbox(engine: Arc<Engine>) -> std::rc::Rc<dyn epdf_acrojs::ScriptSandbox> {
    let made = sandbox_factory(Arc::new(Factory(engine)))();
    made.peek().expect("settled").expect("a sandbox")
}

#[test]
fn the_program_is_the_quickjs_sandbox_call_wrapped_in_stringify() {
    let engine = engine(|_| ScriptEvaluation::Value { json: output(json!([])) });
    let sandbox = sandbox(engine.clone());
    let out = sandbox.run("app.alert('hi')", &input(), None).unwrap();
    assert!(out.event.rc);
    let programs = engine.programs.lock().unwrap();
    let run = programs.last().unwrap();
    assert!(run.starts_with("JSON.stringify(globalThis.__acrojsRun(\"app.alert('hi')\","), "{run}");
    assert!(run.ends_with(",{\"maxEffects\":256}))"), "{run}");
}

#[test]
fn a_second_boot_throws() {
    let sandbox = sandbox(engine(|_| ScriptEvaluation::Value { json: output(json!([])) }));
    sandbox.boot(&[], &input(), None).unwrap();
    assert!(sandbox.boot(&[], &input(), None).is_err());
}

#[test]
fn a_timed_out_evaluation_is_a_budget_failure_and_poisons() {
    let engine = engine(|_| ScriptEvaluation::TimedOut);
    let sandbox = sandbox(engine.clone());
    let out = sandbox.run("while (true) {}", &input(), None).unwrap();
    assert!(!out.event.rc);
    assert_eq!(out.error.unwrap().kind, ScriptExecutionErrorKind::Budget);
    assert!(sandbox.disposed());
    assert!(*engine.disposed.lock().unwrap());
    assert!(sandbox.run("1", &input(), None).is_err());
}

#[test]
fn malformed_output_is_invalid_output() {
    for answer in [
        ScriptEvaluation::Value { json: "{\"event\":{}}".into() },
        ScriptEvaluation::Value { json: "not json".into() },
        ScriptEvaluation::Threw { detail: "SyntaxError".into() },
    ] {
        let answer = Mutex::new(Some(answer));
        let sandbox = sandbox(engine(move |_| answer.lock().unwrap().take().unwrap()));
        let out = sandbox.run("x", &input(), None).unwrap();
        assert_eq!(out.error.unwrap().kind, ScriptExecutionErrorKind::InvalidOutput);
        assert!(sandbox.disposed());
    }
}

#[test]
fn oversized_output_is_a_budget_failure() {
    let big = "x".repeat(2048);
    let sandbox = sandbox(engine(move |_| {
        ScriptEvaluation::Value { json: output(json!([{ "kind": "alert", "message": big, "icon": 0 }])) }
    }));
    let budget = epdf_acrojs::ScriptBudget { max_output_bytes: 1024.0, ..DEFAULT_SCRIPT_BUDGET };
    let out = sandbox.run("x", &input(), Some(&budget)).unwrap();
    let error = out.error.unwrap();
    assert_eq!(error.kind, ScriptExecutionErrorKind::Budget);
    assert!(error.message.contains("limit is 1024"), "{}", error.message);
}

#[test]
fn a_jsc_stack_overflow_counts_as_a_resource_limit() {
    let overflowed = json!({
        "event": { "rc": false, "value": null, "change": "", "selStart": 0, "selEnd": 0 },
        "formEffects": [], "annotEffects": [], "uiEffects": [], "diagnostics": [],
        "error": { "kind": "exception", "message": "RangeError: Maximum call stack size exceeded." }
    })
    .to_string();
    let sandbox = sandbox(engine(move |_| ScriptEvaluation::Value { json: overflowed.clone() }));
    let out = sandbox.run("f()", &input(), None).unwrap();
    assert_eq!(out.error.unwrap().kind, ScriptExecutionErrorKind::Budget);
}

#[test]
fn a_budget_that_is_not_a_positive_integer_throws() {
    let sandbox = sandbox(engine(|_| ScriptEvaluation::Value { json: output(json!([])) }));
    let budget = epdf_acrojs::ScriptBudget { max_effects: 0.5, ..DEFAULT_SCRIPT_BUDGET };
    assert!(sandbox.run("x", &input(), Some(&budget)).is_err());
}

#[test]
fn a_prelude_that_fails_rejects_the_factory() {
    struct Broken;
    impl ScriptEngine for Broken {
        fn evaluate(&self, _program: String, _limit: u32) -> ScriptEvaluation {
            ScriptEvaluation::Threw { detail: "no globalThis".into() }
        }
        fn dispose(&self) {}
    }
    struct BrokenFactory;
    impl ScriptEngineFactory for BrokenFactory {
        fn create(&self) -> Result<Arc<dyn ScriptEngine>, HostError> {
            Ok(Arc::new(Broken))
        }
    }
    let made = sandbox_factory(Arc::new(BrokenFactory))();
    let error = made.peek().expect("settled").err().expect("a rejection");
    assert!(error.message.contains("no globalThis"), "{error:?}");
}

#[derive(Default)]
struct Alerts(Mutex<Vec<String>>);

impl ActionUiHandler for Alerts {
    fn open_uri(&self, _uri: String, _options: ActionUriOptions) -> Result<(), HostError> {
        Ok(())
    }
    fn print(&self, _context: Option<ActionUiContext>) -> Result<(), HostError> {
        Ok(())
    }
    fn alert(&self, message: String, _options: ActionAlertOptions) -> Result<(), HostError> {
        self.0.lock().unwrap().push(message);
        Ok(())
    }
    fn goto_page(&self, _page: f64, _context: ActionUiContext) -> Result<(), HostError> {
        Ok(())
    }
}

#[test]
fn a_javascript_node_runs_in_the_foreign_engine_and_alerts() {
    let engine = engine(|_| {
        ScriptEvaluation::Value { json: output(json!([{ "kind": "alert", "message": "hi", "icon": 0 }])) }
    });
    // `sandbox_factory` returns an `Rc`, so it's built on the kernel thread.
    let shared = engine.clone();
    let thread = KernelThread::spawn(move |_| {
        let factory = sandbox_factory(Arc::new(Factory(shared)));
        let config = ActionsConfig {
            open_sequence: Some(OpenSequence::Off),
            javascript: Some(JavaScriptConfig {
                enabled: true,
                sandbox_factory: Some(factory),
                ..Default::default()
            }),
            ..Default::default()
        };
        TestKernel::open(vec![actions_plugin(config)], spec())
    })
    .unwrap();
    let post = thread.post();
    let actions = block_on(thread.call(move |k| ActionsHandle::new(k.capability(&ACTIONS).unwrap(), post))).unwrap();
    assert!(block_on(actions.is_scripting_enabled()).unwrap());
    let alerts = Arc::new(Alerts::default());
    let _ui = block_on(actions.set_ui_adapter(Some(alerts.clone()), ActionUiSupport { alert: true, goto_page: true })).unwrap();
    let script = PdfActionTree::from_json(
        json!({
            "root": { "type": "javascript", "subtype": "JavaScript", "script": "app.alert('hi')", "next": [] },
            "incomplete": false, "warningFlags": 0, "warnings": []
        })
        .to_string(),
    )
    .unwrap();
    let result = block_on(actions.execute(script, user())).unwrap();
    assert_eq!(result.nodes[0].status, ActionNodeStatus::Executed, "{result:?}");
    assert_eq!(*alerts.0.lock().unwrap(), ["hi"]);
    let programs = engine.programs.lock().unwrap();
    assert!(programs.iter().any(|p| p.contains("__acrojsBoot")), "{programs:?}");
    assert!(programs.iter().any(|p| p.contains("__acrojsRun(\"app.alert('hi')\"")), "{programs:?}");
}
```

`JavaScriptConfig` and `ScriptSandbox`'s `peek` are public (`crates/plugins/actions/src/contract.rs`, `crates/epdf-kernel/src/rt.rs`). The `Promise` from `sandbox_factory` is created settled, so `peek` works without running the kernel runtime.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p epdf-plugin-actions --features uniffi --test ffi_sandbox`
Expected: FAIL to compile with "unresolved imports `epdf_plugin_actions::ffi::sandbox::ScriptEngine`".

- [ ] **Step 3: Write `crates/plugins/actions/src/ffi/sandbox.rs`**

```rust
//! JavaScript through a foreign engine. `ForeignSandbox` implements
//! `epdf_acrojs::ScriptSandbox` the way `packages/core/js-sandbox/src/
//! QuickJsSandbox.ts` does: it builds the `__acrojsBoot`/`__acrojsRun` call,
//! hands it to a `ScriptEngine` that only evaluates JavaScript, and checks
//! the answer. The engine never sees anything but program text.

use std::cell::Cell;
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

use epdf_acrojs::{
    DEFAULT_SCRIPT_BUDGET, PRELUDE_SOURCE, ScriptBudget, ScriptDocumentInput, ScriptEnvironment,
    ScriptEventInput, ScriptEventKind, ScriptEventOutput, ScriptExecutionError,
    ScriptExecutionErrorKind, ScriptIdentity, ScriptInput, ScriptOutput, ScriptSandbox,
    ScriptSandboxFactory, ScriptUiEffect,
};
use epdf_ffi::{HostError, ViewerError};
use epdf_kernel::Error;
use epdf_kernel::rt::Promise;
use serde_json::Value;

/// What one evaluation came to.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ScriptEvaluation {
    /// The program's completion value, which was a string.
    Value { json: String },
    /// It threw, or completed with something that isn't a string.
    Threw { detail: String },
    /// It ran past `time_limit_ms`. The engine is unusable afterwards.
    TimedOut,
}

/// One JavaScript realm: persistent globals across evaluations. Called on
/// the kernel thread, which waits for the answer, so give up at
/// `time_limit_ms`.
#[uniffi::export(with_foreign)]
pub trait ScriptEngine: Send + Sync {
    fn evaluate(&self, program: String, time_limit_ms: u32) -> ScriptEvaluation;
    /// Release the realm. Idempotent.
    fn dispose(&self);
}

/// Makes one realm per document (and per detached realm).
#[uniffi::export(with_foreign)]
pub trait ScriptEngineFactory: Send + Sync {
    fn create(&self) -> Result<Arc<dyn ScriptEngine>, HostError>;
}

/// `ScriptBudget` with whole-number fields. A JavaScriptCore engine enforces
/// only `max_execution_ms`; memory and stack limits are JSC's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ActionsScriptBudget {
    pub max_execution_ms: u32,
    pub max_memory_bytes: u64,
    pub max_stack_bytes: u64,
    pub max_effects: u32,
    pub max_output_bytes: u64,
}

impl From<ActionsScriptBudget> for ScriptBudget {
    fn from(b: ActionsScriptBudget) -> Self {
        ScriptBudget {
            max_execution_ms: f64::from(b.max_execution_ms),
            max_memory_bytes: b.max_memory_bytes as f64,
            max_stack_bytes: b.max_stack_bytes as f64,
            max_effects: f64::from(b.max_effects),
            max_output_bytes: b.max_output_bytes as f64,
        }
    }
}

/// `ActionsConfig.javascript`. `enabled` with no `engine` runs as the
/// TypeScript does with scripting disabled.
#[derive(Clone, uniffi::Record)]
pub struct ActionsJavaScriptConfig {
    pub enabled: bool,
    #[uniffi(default = None)]
    pub engine: Option<Arc<dyn ScriptEngineFactory>>,
    #[uniffi(default = None)]
    pub budget: Option<ActionsScriptBudget>,
    /// The most JS nodes one dispatch may run. Default 16.
    #[uniffi(default = None)]
    pub max_script_nodes_per_dispatch: Option<u32>,
}

impl fmt::Debug for ActionsJavaScriptConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ActionsJavaScriptConfig")
            .field("enabled", &self.enabled)
            .field("engine", &self.engine.is_some())
            .field("budget", &self.budget)
            .field("max_script_nodes_per_dispatch", &self.max_script_nodes_per_dispatch)
            .finish()
    }
}

impl From<ActionsJavaScriptConfig> for crate::JavaScriptConfig {
    fn from(js: ActionsJavaScriptConfig) -> Self {
        crate::JavaScriptConfig {
            enabled: js.enabled,
            sandbox_factory: js.engine.map(sandbox_factory),
            budget: js.budget.map(Into::into),
            max_script_nodes_per_dispatch: js.max_script_nodes_per_dispatch.map(f64::from),
            ..Default::default()
        }
    }
}

/// The prelude is about 48 KB; loading it is not a script run, so it gets
/// its own limit.
const PRELUDE_LIMIT_MS: u32 = 1_000;

/// Each realm loads the prelude, then proves `__acrojsRun` is installed.
pub fn sandbox_factory(engine: Arc<dyn ScriptEngineFactory>) -> ScriptSandboxFactory {
    Rc::new(move || {
        let engine = match engine.create() {
            Ok(engine) => engine,
            Err(error) => return Promise::rejected(error.into()),
        };
        let program = format!("{PRELUDE_SOURCE}\nJSON.stringify(typeof globalThis.__acrojsRun)");
        let failed = |detail: String| {
            engine.dispose();
            Promise::rejected(Error::new(format!("the AcroJS prelude failed to load: {detail}")))
        };
        match engine.evaluate(program, PRELUDE_LIMIT_MS) {
            ScriptEvaluation::Value { json } if json == "\"function\"" => {
                Promise::resolved(Rc::new(ForeignSandbox::new(engine.clone())) as Rc<dyn ScriptSandbox>)
            }
            ScriptEvaluation::Value { json } => failed(format!("__acrojsRun is {json}")),
            ScriptEvaluation::Threw { detail } => failed(detail),
            ScriptEvaluation::TimedOut => failed(format!("it ran past {PRELUDE_LIMIT_MS} ms")),
        }
    })
}

struct ForeignSandbox {
    engine: Arc<dyn ScriptEngine>,
    booted: Cell<bool>,
    disposed: Cell<bool>,
}

impl ForeignSandbox {
    fn new(engine: Arc<dyn ScriptEngine>) -> Self {
        ForeignSandbox { engine, booted: Cell::new(false), disposed: Cell::new(false) }
    }

    fn active(&self) -> Result<(), Error> {
        if self.disposed.get() {
            return Err(Error::new("The script sandbox has been disposed"));
        }
        Ok(())
    }

    /// `poison()`: dispose, keep the output.
    fn poisoned(&self, output: ScriptOutput) -> ScriptOutput {
        self.dispose();
        output
    }

    fn invoke(
        &self,
        method: &str,
        first: String,
        input: &ScriptInput,
        budget: Option<&ScriptBudget>,
    ) -> Result<ScriptOutput, Error> {
        let budget = check_budget(budget.unwrap_or(&DEFAULT_SCRIPT_BUDGET))?;
        let input_json = serde_json::to_string(input)
            .map_err(|e| Error::new(format!("[acrojs] input does not serialize: {e}")))?;
        let program = format!(
            "JSON.stringify(globalThis.{method}({first},{input_json},{{\"maxEffects\":{}}}))",
            budget.max_effects
        );
        let json = match self.engine.evaluate(program, budget.max_execution_ms as u32) {
            ScriptEvaluation::TimedOut => {
                return Ok(self.poisoned(failed_output(
                    input,
                    ScriptExecutionErrorKind::Budget,
                    "Script execution exceeded its time budget".into(),
                )));
            }
            ScriptEvaluation::Threw { detail } => {
                return Ok(self.poisoned(failed_output(
                    input,
                    ScriptExecutionErrorKind::InvalidOutput,
                    detail,
                )));
            }
            ScriptEvaluation::Value { json } => json,
        };
        let Ok(output) = serde_json::from_str::<ScriptOutput>(&json) else {
            return Ok(self.poisoned(failed_output(
                input,
                ScriptExecutionErrorKind::InvalidOutput,
                "AcroJS returned an invalid output".into(),
            )));
        };
        if let Some(error) = output.error.as_ref().filter(|e| is_resource_error(e)) {
            let message = error.message.clone();
            return Ok(self.poisoned(failed_output(input, ScriptExecutionErrorKind::Budget, message)));
        }
        if json.len() as f64 > budget.max_output_bytes {
            let message = format!(
                "Script output used {} bytes; limit is {}",
                json.len(),
                budget.max_output_bytes
            );
            return Ok(self.poisoned(failed_output(input, ScriptExecutionErrorKind::Budget, message)));
        }
        Ok(output)
    }
}

impl ScriptSandbox for ForeignSandbox {
    fn disposed(&self) -> bool {
        self.disposed.get()
    }

    fn boot(
        &self,
        sources: &[String],
        input: &ScriptInput,
        budget: Option<&ScriptBudget>,
    ) -> Result<ScriptOutput, Error> {
        self.active()?;
        if self.booted.replace(true) {
            return Err(Error::new("The document JavaScript name tree has already booted"));
        }
        let sources = serde_json::to_string(sources).expect("strings serialize");
        self.invoke("__acrojsBoot", sources, input, budget)
    }

    fn run(
        &self,
        source: &str,
        input: &ScriptInput,
        budget: Option<&ScriptBudget>,
    ) -> Result<ScriptOutput, Error> {
        self.active()?;
        let source = serde_json::to_string(source).expect("a string serializes");
        self.invoke("__acrojsRun", source, input, budget)
    }

    fn dispose(&self) {
        if !self.disposed.replace(true) {
            self.engine.dispose();
        }
    }
}

/// `assertBudget`: every field a positive safe integer.
fn check_budget(budget: &ScriptBudget) -> Result<&ScriptBudget, Error> {
    let fields = [
        ("maxExecutionMs", budget.max_execution_ms),
        ("maxMemoryBytes", budget.max_memory_bytes),
        ("maxStackBytes", budget.max_stack_bytes),
        ("maxEffects", budget.max_effects),
        ("maxOutputBytes", budget.max_output_bytes),
    ];
    for (name, value) in fields {
        if !(value > 0.0 && value.fract() == 0.0 && value <= 9_007_199_254_740_991.0) {
            return Err(Error::named(
                "RangeError",
                format!("ScriptBudget.{name} must be a positive safe integer"),
            ));
        }
    }
    Ok(budget)
}

/// `isResourceError`, plus JavaScriptCore's spelling of a stack overflow.
fn is_resource_error(error: &ScriptExecutionError) -> bool {
    let message = error.message.to_lowercase();
    ["interrupted", "out of memory", "stack overflow", "maximum call stack size exceeded"]
        .iter()
        .any(|needle| message.contains(needle))
}

/// `failedOutput(input, kind, message)`: `rc` false, the event's value (or
/// the target field's) kept.
fn failed_output(input: &ScriptInput, kind: ScriptExecutionErrorKind, message: String) -> ScriptOutput {
    let event = &input.event;
    let target_value = event.target.as_ref().and_then(|target| {
        input.fields.iter().find(|field| same_field(&field.field_ref, target))?.value.clone()
    });
    let present = |v: &Option<Value>| v.clone().filter(|v| !v.is_null());
    ScriptOutput {
        event: ScriptEventOutput {
            rc: false,
            value: present(&event.value).or_else(|| present(&target_value)).unwrap_or(Value::Null),
            change: event.change.clone().unwrap_or_default(),
            sel_start: event.sel_start.unwrap_or(0.0),
            sel_end: event.sel_end.unwrap_or(0.0),
        },
        form_effects: Vec::new(),
        annot_effects: Vec::new(),
        ui_effects: Vec::new(),
        diagnostics: Vec::new(),
        error: Some(ScriptExecutionError { kind, message, stack: None }),
    }
}

fn same_field(a: &Value, b: &Value) -> bool {
    let kind = |v: &Value| v.get("kind").and_then(Value::as_str).map(str::to_string);
    match (kind(a).as_deref(), kind(b).as_deref()) {
        (Some("objectNumber"), Some("objectNumber")) => {
            a.get("fieldObjectNumber") == b.get("fieldObjectNumber")
        }
        (Some("fqn"), Some("fqn")) => a.get("name") == b.get("name"),
        _ => false,
    }
}

/// What `check_script_engine` saw.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ScriptCheck {
    /// `app.alert` messages, in order.
    pub alerts: Vec<String>,
    pub error: Option<super::ScriptExecutionError>,
    /// Whether the run poisoned the realm.
    pub disposed: bool,
}

/// Run `source` once in a fresh realm from `factory`, outside any document,
/// with the default budget: a host's check that its engine works. Runs on
/// the calling thread; call it off the main thread.
#[uniffi::export]
pub fn check_script_engine(
    factory: Arc<dyn ScriptEngineFactory>,
    source: String,
) -> Result<ScriptCheck, ViewerError> {
    let sandbox = match sandbox_factory(factory)().peek() {
        Some(Ok(sandbox)) => sandbox,
        Some(Err(error)) => return Err(error.into()),
        None => unreachable!("sandbox_factory settles at once"),
    };
    let mut event = ScriptEventInput::of(ScriptEventKind::WidgetActivate);
    event.event_type = Some("Doc".into());
    event.name = Some("Open".into());
    let input = ScriptInput {
        document: ScriptDocumentInput {
            id: "check".into(),
            file_name: "check.pdf".into(),
            page_count: 1.0,
            page_number: 0.0,
        },
        identity: ScriptIdentity::default(),
        environment: ScriptEnvironment { now_ms: 0.0, utc_offset_minutes: 0.0, random_seed: 1.0 },
        fields: Vec::new(),
        annots: None,
        annot_pages: None,
        annots_cover_document: None,
        event,
    };
    sandbox.boot(&[], &input, None)?;
    let output = sandbox.run(&source, &input, None)?;
    let alerts = output
        .ui_effects
        .iter()
        .filter_map(|effect| match effect {
            ScriptUiEffect::Alert { message, .. } => Some(message.clone()),
            _ => None,
        })
        .collect();
    let check = ScriptCheck {
        alerts,
        error: output.error.as_ref().map(Into::into),
        disposed: sandbox.disposed(),
    };
    sandbox.dispose();
    Ok(check)
}
```

`ScriptEventOutput`'s fields (`rc`, `value`, `change`, `sel_start`, `sel_end`) are the ones `ScriptOutput::empty` fills in `crates/plugins/acrojs/src/types.rs`; `ScriptEventInput`'s `target`, `value`, `change`, `sel_start` and `sel_end` are `Option`s there.

- [ ] **Step 4: Add `javascript` to `ActionsConfig`**

In `crates/plugins/actions/src/ffi.rs`, add the field to `ActionsConfig` after `open_sequence`:

```rust
    /// PDF JavaScript. `None`, or `enabled` without an engine, leaves
    /// scripting off.
    #[uniffi(default = None)]
    pub javascript: Option<sandbox::ActionsJavaScriptConfig>,
```

`ActionsConfig` derives `Debug` and `Default`, which still hold: `ActionsJavaScriptConfig` has a manual `Debug`, and `Option` defaults to `None`. In `into_plugin_config`, replace `javascript: None,` with `javascript: self.javascript.map(Into::into),`.

In `crates/epdf-viewer-host/tests/viewer_host.rs`, add `javascript: None,` to the `ActionsConfig` literal in `with_actions`.

- [ ] **Step 5: Add a host test that scripting turns on**

Append to `crates/epdf-viewer-host/tests/viewer_host.rs`:

```rust
use epdf_plugin_actions::ffi::sandbox::{
    ActionsJavaScriptConfig, ScriptEngine, ScriptEngineFactory, ScriptEvaluation,
};

struct ProbeOnly;

impl ScriptEngine for ProbeOnly {
    fn evaluate(&self, _program: String, _limit: u32) -> ScriptEvaluation {
        ScriptEvaluation::Value { json: "\"function\"".into() }
    }
    fn dispose(&self) {}
}

impl ScriptEngineFactory for ProbeOnly {
    fn create(&self) -> Result<Arc<dyn ScriptEngine>, epdf_ffi::HostError> {
        Ok(Arc::new(ProbeOnly))
    }
}

#[test]
fn an_engine_turns_scripting_on() {
    let config = ViewerConfig {
        actions: Some(epdf_plugin_actions::ffi::ActionsConfig {
            policy: None,
            triggers: None,
            open_sequence: Some(OpenSequence::Off),
            javascript: Some(ActionsJavaScriptConfig {
                enabled: true,
                engine: Some(Arc::new(ProbeOnly)),
                budget: None,
                max_script_nodes_per_dispatch: None,
            }),
            utc_offset_minutes: None,
        }),
        ..ViewerConfig::default()
    };
    let host = ViewerHost::new(config, Arc::new(Delegate)).unwrap();
    block_on(host.start()).unwrap();
    let doc = block_on(host.open_document(test_support::fixture(DOCUMENT_ACTIONS), options())).unwrap();
    let actions = block_on(host.actions(doc.id)).unwrap();
    assert!(block_on(actions.is_scripting_enabled()).unwrap());
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p epdf-plugin-actions --features uniffi --test ffi_sandbox`
Expected: PASS, 9 tests.

Run: `cargo test -p epdf-plugin-actions --features uniffi && cargo test -p epdf-ffi -p epdf-viewer-host`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/plugins/actions crates/epdf-viewer-host/tests/viewer_host.rs
git commit -m "Run PDF JavaScript through a foreign ScriptEngine behind a Rust sandbox"
```

### Task 6: Swift capability

**Files:**
- Create: `platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Actions.swift`
- Create: `platforms/apple/EmbedPDF/Tests/EmbedPDFTests/ActionsTests.swift`
- Modify: `platforms/apple/build-xcframework.sh` (one `FIXTURES` line)

**Interfaces:**
- Consumes: the generated handle, objects, records and port protocols.
- Produces: Swift `ActionsCapability` (one method per handle member), `PDFViewer.actions(for:)`, `PdfActionTree.decode(json:)`, `ClosureDocumentVerb` (internal), `ActionsConfig.forCurrentTimeZone(...)`.

- [ ] **Step 1: Add the fixture**

In `platforms/apple/build-xcframework.sh`, add to the `FIXTURES` list (umbrella Task 0f), after the last plan's lines:

```bash
  tooling/engine-conformance/fixtures/document_actions.pdf
```

- [ ] **Step 2: Write the failing Swift tests**

`platforms/apple/EmbedPDF/Tests/EmbedPDFTests/ActionsTests.swift`:

```swift
import CustomDump
import EmbedPDF
import Foundation
import Testing

extension BaseSuite {
  @Suite struct ActionsTests {
    func opened(_ fixture: String = "document_actions") async throws -> (PDFViewer, PDFDocument, ActionsCapability) {
      let viewer = try PDFViewer(ViewerConfig(actions: ActionsConfig(openSequence: .off)))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data(fixture))
      return (viewer, document, try await viewer.actions(for: document))
    }

    static let user = ActionContext(origin: .user, source: .api, event: .activate)

    static func uriTree(_ uri: String) throws -> PdfActionTree {
      try PdfActionTree.decode(
        json: #"{"root":{"type":"uri","subtype":"URI","uri":"\#(uri)","isMap":false,"next":[]},"incomplete":false,"warningFlags":0,"warnings":[]}"#
      )
    }

    @Test func theCatalogsOpenTreeIsJavaScript() async throws {
      let (_, _, actions) = try await opened()
      let open = try await actions.actionTree(for: .document(event: nil))
      expectNoDifference(open?.rootType(), "javascript")
      let scripting = try await actions.isScriptingEnabled()
      expectNoDifference(scripting, false)
    }

    @Test func aUserUriReachesTheHandlerAndAFileUriDoesNot() async throws {
      let (_, _, actions) = try await opened()
      let ui = RecordingUI()
      let registration = try await actions.setUIHandler(ui)
      _ = try await actions.execute(Self.uriTree("https://a.test/"), context: Self.user)
      let blocked = try await actions.execute(Self.uriTree("file:///etc/passwd"), context: Self.user)
      expectNoDifference(blocked.nodes.map(\.status), [.blocked])
      expectNoDifference(ui.opened, ["https://a.test/"])
      registration.unregister()
    }

    @Test func aSwiftExecutorRunsItsNodes() async throws {
      let (_, _, actions) = try await opened()
      let executor = RecordingExecutor()
      _ = try await actions.registerExecutor(for: "goto", executor)
      let goto = try PdfActionTree.decode(
        json: #"{"root":{"type":"goto","subtype":"GoTo","destination":{"kind":"fit","page":{"kind":"objectNumber","pageObjectNumber":1}},"next":[]},"incomplete":false,"warningFlags":0,"warnings":[]}"#
      )
      let result = try await actions.execute(goto, context: Self.user)
      expectNoDifference(result.nodes.map(\.status), [.executed])
      expectNoDifference(executor.types, ["goto"])
    }

    @Test func prepareCloseFindsTheWillCloseScript() async throws {
      let (_, _, actions) = try await opened()
      let result = try await actions.prepareClose()
      expectNoDifference(result.steps.count, 1)
      expectNoDifference(result.steps.first?.result.nodes.map(\.status), [.inert])
    }

    @Test func aDocumentVerbRunsItsOperation() async throws {
      let (_, _, actions) = try await opened()
      let ran = RecordingExecutor()
      try await actions.runDocumentVerb(.save) {
        try await Task.sleep(for: .milliseconds(5))
        ran.record("save")
      }
      expectNoDifference(ran.types, ["save"])
    }

    @Test func aClosedDocumentsCapabilityThrows() async throws {
      let (viewer, document, actions) = try await opened()
      try await viewer.close(document)
      await #expect {
        _ = try await actions.policy()
      } throws: { error in
        let error = error as? PDFError
        return error?.code == "instance-closed" && error?.name == "PluginError"
      }
    }

    @Test func droppingTheStreamUnsubscribes() async throws {
      let (_, _, actions) = try await opened()
      let task = Task { for await _ in actions.events {} }
      try await Task.sleep(for: .milliseconds(20))
      task.cancel()
      _ = await task.value
      _ = try await actions.prepareClose()
    }
  }
}

final class RecordingUI: ActionUiHandler, @unchecked Sendable {
  private let lock = NSLock()
  private var _opened: [String] = []
  var opened: [String] { lock.withLock { _opened } }

  func openUri(uri: String, options: ActionUriOptions) throws {
    lock.withLock { _opened.append(uri) }
  }
  func print(context: ActionUiContext?) throws {}
  func alert(message: String, options: ActionAlertOptions) throws {}
  func gotoPage(page: Double, context: ActionUiContext) throws {}
}

final class RecordingExecutor: ActionExecutor, @unchecked Sendable {
  private let lock = NSLock()
  private var _types: [String] = []
  var types: [String] { lock.withLock { _types } }
  func record(_ type: String) { lock.withLock { _types.append(type) } }

  func execute(node: PdfActionNode, context: ActionContext, completion: ActionExecutorCompletion) {
    record(node.nodeType())
    completion.executed()
  }
}
```

No member takes a `CancelToken` (Decisions), so there's no cancellation test.

- [ ] **Step 3: Rebuild and run the tests to verify they fail**

```bash
bash platforms/apple/build-xcframework.sh
cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17' -only-testing:EmbedPDFTests/BaseSuite/ActionsTests
```

Expected: FAIL to compile with "cannot find type 'ActionsCapability' in scope".

- [ ] **Step 4: Write `Plugins/Actions.swift`**

```swift
public import EmbedPDFBindings
import Foundation

public typealias ActionsConfig = EmbedPDFBindings.ActionsConfig
public typealias ActionsJavaScriptConfig = EmbedPDFBindings.ActionsJavaScriptConfig
public typealias ActionsScriptBudget = EmbedPDFBindings.ActionsScriptBudget
public typealias ActionPolicy = EmbedPDFBindings.ActionPolicy
public typealias ActionPolicyRow = EmbedPDFBindings.ActionPolicyRow
public typealias ActionPolicyPatch = EmbedPDFBindings.ActionPolicyPatch
public typealias PartialActionPolicyRow = EmbedPDFBindings.PartialActionPolicyRow
public typealias ActionPolicyDecision = EmbedPDFBindings.ActionPolicyDecision
public typealias TriggerGates = EmbedPDFBindings.TriggerGates
public typealias OpenSequence = EmbedPDFBindings.OpenSequence
public typealias PdfActionTree = EmbedPDFBindings.PdfActionTree
public typealias PdfActionNode = EmbedPDFBindings.PdfActionNode
public typealias PdfObjectRef = EmbedPDFBindings.PdfObjectRef
public typealias ActionOrigin = EmbedPDFBindings.ActionOrigin
public typealias ActionSource = EmbedPDFBindings.ActionSource
public typealias ActionContext = EmbedPDFBindings.ActionContext
public typealias PartialActionContext = EmbedPDFBindings.PartialActionContext
public typealias ActionTriggerEvent = EmbedPDFBindings.ActionTriggerEvent
public typealias ActionTrigger = EmbedPDFBindings.ActionTrigger
public typealias ActionTreeSource = EmbedPDFBindings.ActionTreeSource
public typealias ActionAnnotationTreeEvent = EmbedPDFBindings.ActionAnnotationTreeEvent
public typealias DocumentTriggerEvent = EmbedPDFBindings.DocumentTriggerEvent
public typealias PdfAnnotationEventKind = EmbedPDFBindings.PdfAnnotationEventKind
public typealias PageEventKind = EmbedPDFBindings.PageEventKind
public typealias PdfFieldEventKind = EmbedPDFBindings.PdfFieldEventKind
public typealias ActionNodeStatus = EmbedPDFBindings.ActionNodeStatus
public typealias ActionNodeResult = EmbedPDFBindings.ActionNodeResult
public typealias ActionResultStatus = EmbedPDFBindings.ActionResultStatus
public typealias ActionDispatchResult = EmbedPDFBindings.ActionDispatchResult
public typealias ActionStepResult = EmbedPDFBindings.ActionStepResult
public typealias ActionTriggerResult = EmbedPDFBindings.ActionTriggerResult
public typealias ActionDiagnostic = EmbedPDFBindings.ActionDiagnostic
public typealias ActionDiagnosticCode = EmbedPDFBindings.ActionDiagnosticCode
public typealias ActionDispatchEvent = EmbedPDFBindings.ActionDispatchEvent
public typealias OpenSequenceCompleted = EmbedPDFBindings.OpenSequenceCompleted
public typealias ScriptDiagnostic = EmbedPDFBindings.ScriptDiagnostic
public typealias ScriptDiagnosticCode = EmbedPDFBindings.ScriptDiagnosticCode
public typealias ScriptExecutionError = EmbedPDFBindings.ScriptExecutionError
public typealias ScriptErrorKind = EmbedPDFBindings.ScriptErrorKind
public typealias ActionsEvent = EmbedPDFBindings.ActionsEvent
public typealias ActionExecutor = EmbedPDFBindings.ActionExecutor
public typealias ActionExecutorCompletion = EmbedPDFBindings.ActionExecutorCompletion
public typealias ActionUiHandler = EmbedPDFBindings.ActionUiHandler
public typealias ActionUiSupport = EmbedPDFBindings.ActionUiSupport
public typealias ActionUiContext = EmbedPDFBindings.ActionUiContext
public typealias ActionUriOptions = EmbedPDFBindings.ActionUriOptions
public typealias ActionAlertOptions = EmbedPDFBindings.ActionAlertOptions
public typealias ScriptPhase = EmbedPDFBindings.ScriptPhase
public typealias ActionSubmitHandler = EmbedPDFBindings.ActionSubmitHandler
public typealias ActionSubmitRequest = EmbedPDFBindings.ActionSubmitRequest
public typealias FormSubmissionEntry = EmbedPDFBindings.FormSubmissionEntry
public typealias FormSubmissionValue = EmbedPDFBindings.FormSubmissionValue
public typealias DocumentVerb = EmbedPDFBindings.DocumentVerb

extension PdfActionTree {
  /// A tree in the engine's shape: `{root, incomplete, warningFlags, warnings}`.
  public static func decode(json: String) throws(PDFError) -> PdfActionTree {
    do {
      return try PdfActionTree.fromJson(json: json)
    } catch {
      throw PDFError(error)
    }
  }
}

extension ActionsConfig {
  /// The config with `utcOffsetMinutes` set from the current time zone. The
  /// offset is fixed for the viewer's lifetime.
  public static func forCurrentTimeZone(
    policy: ActionPolicyPatch? = nil,
    triggers: TriggerGates? = nil,
    openSequence: OpenSequence? = nil,
    javascript: ActionsJavaScriptConfig? = nil
  ) -> ActionsConfig {
    ActionsConfig(
      policy: policy, triggers: triggers, openSequence: openSequence, javascript: javascript,
      utcOffsetMinutes: Int32(TimeZone.current.secondsFromGMT() / 60)
    )
  }
}

final class ClosureDocumentVerb: DocumentVerbOperation, Sendable {
  let body: @Sendable () async throws -> Void

  init(_ body: @escaping @Sendable () async throws -> Void) {
    self.body = body
  }

  func run(completion: DocumentVerbCompletion) {
    let body = body
    Task {
      do {
        try await body()
        completion.finish()
      } catch {
        completion.fail(detail: "\(error)")
      }
    }
  }
}

/// A document's PDF actions: policy, the dispatcher and its ports. Get one
/// with `PDFViewer.actions(for:)`; the viewer needs
/// `ViewerConfig(actions: ActionsConfig())`. It keeps the viewer alive.
/// After its document closes, every call throws `instance-closed`. No call
/// is cancellable: a dispatch is a transaction on the document's queue.
public struct ActionsCapability: Sendable {
  let handle: ActionsHandle
  let viewer: PDFViewer

  public func policy() async throws(PDFError) -> ActionPolicy {
    try await bridged { try await handle.policy() }
  }

  public func updatePolicy(_ patch: ActionPolicyPatch) async throws(PDFError) {
    try await bridged { try await handle.updatePolicy(patch: patch) }
  }

  public func isScriptingEnabled() async throws(PDFError) -> Bool {
    try await bridged { try await handle.isScriptingEnabled() }
  }

  public func execute(_ tree: PdfActionTree, context: ActionContext) async throws(PDFError) -> ActionDispatchResult {
    try await bridged { try await handle.execute(tree: tree, context: context) }
  }

  public func canExecute(_ tree: PdfActionTree, context: ActionContext) async throws(PDFError) -> Bool {
    try await bridged { try await handle.canExecute(tree: tree, context: context) }
  }

  public func executeNamed(_ name: String, context: PartialActionContext? = nil)
    async throws(PDFError) -> ActionDispatchResult
  {
    try await bridged { try await handle.executeNamed(name: name, context: context) }
  }

  public func actionTree(for source: ActionTreeSource) async throws(PDFError) -> PdfActionTree? {
    try await bridged { try await handle.actionTree(source: source) }
  }

  public func dispatch(_ trigger: ActionTrigger) async throws(PDFError) -> ActionTriggerResult {
    try await bridged { try await handle.dispatch(trigger: trigger) }
  }

  public func canDispatch(_ trigger: ActionTrigger) async throws(PDFError) -> Bool {
    try await bridged { try await handle.canDispatch(trigger: trigger) }
  }

  /// Runs the document's will-close trees. Call before `PDFViewer.close`.
  public func prepareClose() async throws(PDFError) -> ActionTriggerResult {
    try await bridged { try await handle.prepareClose() }
  }

  /// Last wins per node type.
  public func registerExecutor(for actionType: String, _ executor: any ActionExecutor)
    async throws(PDFError) -> Registration
  {
    Registration(handle: try await bridged { try await handle.registerExecutor(actionType: actionType, executor: executor) })
  }

  /// `nil` uninstalls. Installing one releases an `auto` open sequence.
  public func setUIHandler(_ handler: (any ActionUiHandler)?, support: ActionUiSupport = ActionUiSupport())
    async throws(PDFError) -> Registration
  {
    Registration(handle: try await bridged { try await handle.setUiAdapter(handler: handler, support: support) })
  }

  public func setSubmitHandler(_ handler: (any ActionSubmitHandler)?) async throws(PDFError) -> Registration {
    Registration(handle: try await bridged { try await handle.setSubmitHandler(handler: handler) })
  }

  /// The document's will- trees, then `operation`, then its did- trees.
  public func runDocumentVerb(_ verb: DocumentVerb, operation: @escaping @Sendable () async throws -> Void)
    async throws(PDFError)
  {
    try await bridged { try await handle.runDocumentVerb(verb: verb, operation: ClosureDocumentVerb(operation)) }
  }

  /// Finishes when the document closes (at once if it already has) or the
  /// viewer is destroyed.
  public var events: AsyncStream<ActionsEvent> {
    viewer.registry.stream { handle.subscribe(listener: $0) }
  }
}

extension EventRelay: ActionsEventListener where Event == ActionsEvent {
  func onEvent(event: ActionsEvent) {
    continuation.yield(event)
  }

  func onEnd() {
    continuation.finish()
  }
}

extension PDFViewer {
  public func actions(for document: PDFDocument) async throws(PDFError) -> ActionsCapability {
    let handle = try await bridged { try await host.actions(documentId: document.id) }
    return ActionsCapability(handle: handle, viewer: self)
  }
}
```

If the generated protocol spellings differ (`openUri(uri:options:)`, `gotoPage(page:context:)`, `nodeType()`), match `Sources/EmbedPDFBindings/epdf_plugin_actions.swift`.

- [ ] **Step 5: Rebuild and run the tests to verify they pass**

```bash
bash platforms/apple/build-xcframework.sh
cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17' -only-testing:EmbedPDFTests/BaseSuite/ActionsTests
```

Expected: PASS, 7 tests.

- [ ] **Step 6: Commit**

```bash
git add platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Actions.swift platforms/apple/EmbedPDF/Tests/EmbedPDFTests
git commit -m "Add EmbedPDF's ActionsCapability"
```

### Task 7: `JavaScriptCoreEngine`

**Files:**
- Create: `platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/JavaScriptCoreEngine.swift`
- Create: `platforms/apple/EmbedPDF/Tests/EmbedPDFTests/JavaScriptCoreEngineTests.swift`

**Interfaces:**
- Consumes: generated `ScriptEngine`, `ScriptEngineFactory`, `ScriptEvaluation`, `checkScriptEngine(factory:source:)`, `ScriptCheck`.
- Produces: `JavaScriptCoreEngineFactory` (public, `ScriptEngineFactory`), `ScriptEngineCheck.run(_:source:)` (a `throws(PDFError)` wrapper over `checkScriptEngine`).

- [ ] **Step 1: Write the failing engine tests**

`platforms/apple/EmbedPDF/Tests/EmbedPDFTests/JavaScriptCoreEngineTests.swift`:

```swift
import CustomDump
import EmbedPDF
import Testing

extension BaseSuite {
  @Suite struct JavaScriptCoreEngineTests {
    @Test func anAlertComesBackAsAnEffect() async throws {
      let check = try await ScriptEngineCheck.run(JavaScriptCoreEngineFactory(), source: "app.alert('hi')")
      expectNoDifference(check.alerts, ["hi"])
      expectNoDifference(check.error, nil)
      expectNoDifference(check.disposed, false)
    }

    @Test func aThrowIsAnExceptionAndKeepsTheRealm() async throws {
      let check = try await ScriptEngineCheck.run(JavaScriptCoreEngineFactory(), source: "throw new Error('boom')")
      expectNoDifference(check.error?.kind, .exception)
      expectNoDifference(check.disposed, false)
    }

    @Test func aRunawayScriptTimesOut() async throws {
      let clock = ContinuousClock()
      let start = clock.now
      let check = try await ScriptEngineCheck.run(JavaScriptCoreEngineFactory(), source: "while (true) {}")
      expectNoDifference(check.error?.kind, .budget)
      expectNoDifference(check.disposed, true)
      // The default budget is 50 ms; allow scheduling slack, not a hang.
      #expect(clock.now - start < .seconds(2))
    }

    @Test func aDeepRecursionIsABudgetFailure() async throws {
      let check = try await ScriptEngineCheck.run(
        JavaScriptCoreEngineFactory(), source: "function f() { return f() + 1 } f()"
      )
      expectNoDifference(check.error?.kind, .budget)
    }
  }
}
```

`aDeepRecursionIsABudgetFailure` relies on JSC's "Maximum call stack size exceeded." reaching the prelude's catch and Task 5's `is_resource_error`. If JSC exhausts the 50 ms first on the simulator, it still lands on `.budget`, through the timeout instead.

- [ ] **Step 2: Rebuild and run the tests to verify they fail**

```bash
bash platforms/apple/build-xcframework.sh
cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17' -only-testing:EmbedPDFTests/BaseSuite/JavaScriptCoreEngineTests
```

Expected: FAIL to compile with "cannot find 'ScriptEngineCheck' in scope".

- [ ] **Step 3: Write `Plugins/JavaScriptCoreEngine.swift`**

```swift
public import EmbedPDFBindings
import Foundation
import JavaScriptCore

public typealias ScriptEngine = EmbedPDFBindings.ScriptEngine
public typealias ScriptEngineFactory = EmbedPDFBindings.ScriptEngineFactory
public typealias ScriptEvaluation = EmbedPDFBindings.ScriptEvaluation
public typealias ScriptCheck = EmbedPDFBindings.ScriptCheck

/// PDF JavaScript in JavaScriptCore, one `JSContext` per realm. Pass it as
/// `ActionsJavaScriptConfig(enabled: true, engine: JavaScriptCoreEngineFactory())`.
///
/// JavaScriptCore has no public execution time limit, so a realm that runs
/// past its budget is abandoned: the call returns `timedOut`, the realm is
/// poisoned, and the runaway script keeps its thread busy until the process
/// exits. Memory and stack budgets aren't enforced; JavaScriptCore's own
/// stack limit applies.
public final class JavaScriptCoreEngineFactory: ScriptEngineFactory, Sendable {
  public init() {}

  public func create() throws -> any ScriptEngine {
    JavaScriptCoreEngine()
  }
}

final class JavaScriptCoreEngine: ScriptEngine, @unchecked Sendable {
  private let queue = DispatchQueue(label: "com.embedpdf.acrojs")
  private let lock = NSLock()
  private var abandoned = false
  /// Touched only on `queue`.
  private var context: JSContext?

  init() {
    queue.sync { context = JSContext() }
  }

  func evaluate(program: String, timeLimitMs: UInt32) -> ScriptEvaluation {
    if lock.withLock({ abandoned }) {
      return .threw(detail: "the realm was abandoned after a timeout")
    }
    let done = DispatchSemaphore(value: 0)
    let answer = Answer()
    queue.async { [weak self] in
      defer { done.signal() }
      guard let context = self?.context else {
        answer.set(.threw(detail: "the realm is disposed"))
        return
      }
      context.exception = nil
      let value = context.evaluateScript(program)
      if let exception = context.exception {
        answer.set(.threw(detail: exception.toString() ?? "an exception"))
      } else if let value, value.isString, let json = value.toString() {
        answer.set(.value(json: json))
      } else {
        answer.set(.threw(detail: "the program did not evaluate to a string"))
      }
    }
    if done.wait(timeout: .now() + .milliseconds(Int(timeLimitMs))) == .timedOut {
      lock.withLock { abandoned = true }
      return .timedOut
    }
    return answer.get() ?? .threw(detail: "no answer")
  }

  func dispose() {
    lock.withLock { abandoned = true }
    queue.async { [weak self] in self?.context = nil }
  }

  private final class Answer: @unchecked Sendable {
    private let lock = NSLock()
    private var value: ScriptEvaluation?
    func set(_ value: ScriptEvaluation) { lock.withLock { self.value = value } }
    func get() -> ScriptEvaluation? { lock.withLock { value } }
  }
}

public enum ScriptEngineCheck {
  /// Runs `source` once in a fresh realm from `factory`, outside any
  /// document, with the default budget.
  public static func run(_ factory: any ScriptEngineFactory, source: String) async throws(PDFError) -> ScriptCheck {
    // The generated protocol may not be `Sendable`; the factory is only
    // used on the detached task.
    nonisolated(unsafe) let factory = factory
    let task = Task.detached { () throws -> ScriptCheck in
      try checkScriptEngine(factory: factory, source: source)
    }
    do {
      return try await task.value
    } catch {
      throw PDFError(error)
    }
  }
}
```

`checkScriptEngine` is synchronous and blocks for up to the budget, so `run` moves it off the caller's executor. `dispose()` marks the engine abandoned before clearing the context, so a dispose that follows a timeout (Task 5's `poisoned`) returns at once even though the queue is still busy.

- [ ] **Step 4: Rebuild and run the tests to verify they pass**

```bash
bash platforms/apple/build-xcframework.sh
cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17' -only-testing:EmbedPDFTests/BaseSuite/JavaScriptCoreEngineTests
```

Expected: PASS, 4 tests.

- [ ] **Step 5: Commit**

```bash
git add platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/JavaScriptCoreEngine.swift platforms/apple/EmbedPDF/Tests/EmbedPDFTests/JavaScriptCoreEngineTests.swift
git commit -m "Add a JavaScriptCore ScriptEngine for PDF JavaScript"
```

### Task 8: Docs, CI, merge and cleanup

**Files:**
- Modify: `.github/workflows/plugin-rust.yml`
- Modify: `platforms/apple/README.md`
- Modify: `docs/superpowers/plans/2026-10-01-swift-package.md`

- [ ] **Step 1: Add the crate to CI**

Append `actions` to the `crates=( ... )` list in `.github/workflows/plugin-rust.yml`, in plan order.

- [ ] **Step 2: Add the README row and the caveat**

```markdown
| actions | `ActionsCapability`, `JavaScriptCoreEngineFactory` | `actions: .forCurrentTimeZone(javascript: ActionsJavaScriptConfig(enabled: true, engine: JavaScriptCoreEngineFactory()))` | `2026-10-06-swift-plugins/15-actions.md` |
```

Under the table:

```markdown
> Note: `JavaScriptCoreEngine` enforces a script's time budget by abandoning the realm, because
> JavaScriptCore has no public execution limit. A runaway script keeps one thread busy until the
> app exits. Memory and stack budgets aren't enforced. JavaScript on a real document also needs
> the form and annotation plugins (plans 05 and 06), whose engine paths the script world reads.
```

- [ ] **Step 3: Mark the spec entry delivered**

In `docs/superpowers/plans/2026-10-01-swift-package.md` Appendix A §A.4, change `**actions** (D).` to `**actions** (D), delivered by 2026-10-06-swift-plugins/15-actions.md (application members only; see its Decisions).` and leave the rest alone. In Appendix C, leave the rows as they are; plan 15's Task 3 is the receipt for `actions.read`.

- [ ] **Step 4: Run the definition of done**

```bash
cargo test -p epdf-plugin-actions --features uniffi
cargo test -p epdf-ffi -p epdf-viewer-host
cargo clippy -p epdf-plugin-actions -p epdf-viewer-host --all-targets --features epdf-plugin-actions/uniffi -- -D warnings
cargo test -p epdf-plugin-actions
bash platforms/apple/build-xcframework.sh
(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17')
cargo build -p epdf-viewer-host -p uniffi-bindgen
./target/debug/uniffi-bindgen generate --library target/debug/libepdf_viewer_host.dylib --language kotlin --no-format --out-dir "$TMPDIR/epdf-kotlin-check"
stat -f%z target/aarch64-apple-ios-sim/release/libepdf_viewer_host.a
```

Expected: every command passes. Kotlin generation is where a field named like a Kotlin keyword fails; `ScriptExecutionError.message` is a record field, not an exception's, so it generates.

- [ ] **Step 5: Commit, with the archive sizes**

```bash
git add .github/workflows/plugin-rust.yml platforms/apple/README.md docs/superpowers/plans/2026-10-01-swift-package.md
git commit -m "Test the actions uniffi exports in CI and document the Swift capability

libepdf_viewer_host.a (aarch64-apple-ios-sim, release): <before> -> <after> bytes"
```

- [ ] **Step 6: Merge into the integration branch**

From `.claude/worktrees/swift-plugins-integration`:

```bash
git merge --no-ff research/swift-actions -m "Merge the actions Swift wrapper"
```

Resolve shared-file conflicts by keeping both blocks in plan-number order. If plan 10 (page-edit) merged first, its `translate.rs` route arms and this plan's `actions.read` arm are adjacent additions; keep both, plan order.

- [ ] **Step 7: Rerun on the integration branch**

```bash
bash platforms/apple/build-xcframework.sh
(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17')
cargo build -p epdf-viewer-host -p uniffi-bindgen
./target/debug/uniffi-bindgen generate --library target/debug/libepdf_viewer_host.dylib --language kotlin --no-format --out-dir "$TMPDIR/epdf-kotlin-check"
```

- [ ] **Step 8: Remove the worktree and branch**

```bash
git worktree remove .claude/worktrees/swift-actions
git branch -d research/swift-actions
git worktree prune
```

No `--force`. Then tick `15 actions` in the umbrella.
