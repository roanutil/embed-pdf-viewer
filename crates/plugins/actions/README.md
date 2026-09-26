# epdf-plugin-actions

`packages/plugin/actions` in Rust, at the v3 contract: the PDF action engine. All 159 cases replay
identically (`cargo test -p epdf-plugin-actions`): 36 function cases in `vectors/plugins/actions.json`
and 123 scenarios across `actions-dispatcher.json` (30), `actions-submit.json` (10),
`actions-triggers.json` (15), `actions-lifecycle.json` (27), `actions-script.json` (31) and
`actions-hover.json` (10).

The plugin is document-scoped and built through `create`, so its controller exists from bring-up. A
`headless` open sequence therefore reads the catalog while the document opens, as the TypeScript
does. Triggers, sources, page-state reports and hover targets carry a `PageRef`; engine DTOs (action
trees and nodes, annotation and field refs, form effects) stay `serde_json::Value`, because the
dispatcher echoes trees back in results and events and must print them exactly as it received them.
Engine calls go through the kernel's guarded handle, so an engine rejection reaches a caller as a
`PluginError` with capability `actions` (`hide-forms-read-rejects`).

## Layout

The TypeScript builds each area as a closure over the services it declares and composes one object
from their slices. Rust keeps one shared `Inner` in `controller.rs` and gives each area its own
`impl ActionsCapability` block in the module named after its file: `services/` (policy, ports,
catalog, events, authority), `dispatch/` (dispatcher, run, triggers, fold), `lifecycle/` (open
sequence, page lifecycle, document events), `scripting/` (environment, realm, executor, world,
surface) and `submit/` (intent, perform). Every verb still has one home.

## The two lenses

The TypeScript splits the capability into a public lens (`/contract`) and a host lens
(`/contract/host`) over one runtime object. Rust has one `ActionsCapability` behind one token,
`ACTIONS`, and `ActionsHostCapability` is an alias. The public surface is `execute`, `canExecute`,
`executeNamed`, `getActionTree`, `getPolicy`, `updatePolicy`, `isScriptingEnabled`, `dispatch`,
`canDispatch`, `setUiAdapter`, `runDocumentVerb`, `prepareClose`, `setSubmitHandler`,
`registerExecutor` and the five hooks (`onExecuted`, `onDiagnostic`, `onScriptDiagnostic`,
`onScriptError`, `onOpenSequenceCompleted`). The host lens adds `register_annot_commit_sink`,
`register_form_commit_sink`, `register_submit_resolver`, `report_page_state`,
`surface_script_result`, `surface_script_commit`, `script_realm` and
`create_detached_script_realm`.

The two realm members are optional in the TypeScript: they exist only with scripting on. In Rust
they return `Option`, and `None` is the absent member. `script_realm()` is a `ScriptRealmPort`
(`transaction` plus `budget`); `create_detached_script_realm(target)` mints a `DetachedScriptRealm`
for another document under this document's environment. It gets a fresh sandbox from the same
factory, the target's id and metadata, the owner's identity, a seed from the target id, the target's
boot sources and the configured budget (`detached-script-realm`). The caller owns it and calls
`dispose`. A result surfaced with `realm: detached` reaches the adapter only as an alert; print,
`gotoPage` and `submitForm` are suppressed with `ui-effect-suppressed` (`surface-a-detached-realm`).

Policy is live. `config.policy` and `updatePolicy` merge row-wise, so a row names only the origins it
changes, and `updatePolicy` bumps `policyRevision` in the slice even for an empty patch
(`policy-rows-merge-and-update-live`).

## Host inputs

The TypeScript reads three ambient values, and each is an input here:

- The five hooks report a throwing listener to `console.error`. Rust takes that sink as
  `ActionsConfig::observer_error`, called with the same message and error; with no sink the report
  is dropped. The vectors route both sides into the trace as a `console.error` callback.
- `Date.now()` is `rt::now()`, or `javascript.now` when configured (it also stamps `clientTimeMs` on
  submits, with scripting on or off).
- `-new Date(nowMs).getTimezoneOffset()` is `host_utc_offset_minutes`, default zero. The generator
  pins `TZ=UTC`, so the vectors record zero.

## Scripting without a sandbox

JavaScript runs only when `javascript.enabled` is set and the host supplies
`javascript.sandbox_factory`. Where the TypeScript lazily imports `@embedpdf/core-js-sandbox` as a
default, we have none, and a host with no sandbox runs exactly as the TypeScript does with scripting
disabled: javascript nodes are inert ("no executor registered (scripting unavailable)"),
`is_scripting_enabled()` is false, and both realm members are absent. The `no-sandbox-host` case pins
that configuration against the TypeScript with `enabled: false`.

## Where the port has to be careful

Turn counts are behaviour here, because the trace records how concurrent dispatches, hover pairs and
detached submits interleave. The guarded handle already spends the turns an engine call's thenable
adoption costs, so a call is awaited directly; wrapping it again adds a turn the TypeScript doesn't
have, which is why the catalog read awaits the call inside its `catch` rather than through a second
promise. An `async` function that returns a promise unawaited resolves through the adoption turn
(`adopt` in `document_events.rs`). `Promise.allSettled` in the hover pump awaits each dispatch rather
than observing it, so its reactions keep their registration order.

Event names in an `ActionContext` are Rust enums, so a type-violating name (`'bogus'`) can't be
expressed. The hover pump compares an index ref's `revision` by value; the TypeScript's `===` is
reference identity for a `RevisionToken` object, which a feed holding one token object means the
same way.

## Coverage

The generators reach 3,184 of the package's 3,195 statements and 754 of its 780 branches. This port
added nine scenarios to `src/suites/actions.ts` (`policy-rows-merge-and-update-live`,
`execute-named`, `get-action-tree`, `open-sequence-completed-event`, `detached-script-realm`,
`detached-script-realm-without-metadata`, `no-detached-realm-without-scripting`,
`surface-a-detached-realm`, `surface-script-commit`), one more step in `scripting-disabled`, and the
`detached-realm` driver, a sibling that mints a realm the way the stamp plugin does. The 11
statements and 26 branches left are unreachable from the public surface or don't port:

- `scripting/environment.ts:42-46`, the lazy QuickJS default and its `??` arm (4 statements, 2
  branches). Rust has no default sandbox.
- `lifecycle/page-lifecycle.ts:101-107`, `releaseBarrier`'s `catch` (6 statements, 1 branch):
  nothing inside the `try` can throw.
- `model.ts:20-21`, the reducer's `default` arm (1 statement, 1 branch). Rust's action is an enum.
- The `String(error)` arm of every `error instanceof Error ? ... : String(error)`: `dispatcher.ts:155`,
  `run.ts:207` and `:285`, `document-events.ts:68`, `open-sequence.ts:80`, `perform.ts:60`, `:79`, `:86`
  and `:98` (9 branches). Scripted callbacks throw and reject with `Error`s, and Rust has only `Error`.
- The `ctx.doc` null guards, `dispatcher.ts:101` and `:189`, `triggers.ts:57` and `executor.ts:89` (4
  branches). A controller's `ctx.doc` is the guarded handle, which is never null.
- `executor.ts:87`, the javascript executor's non-javascript guard: it is only called for javascript
  nodes.
- `run.ts:87`'s `: null`, guarded by the enclosing `if`.
- `world.ts:25` and `:82`, the `ctx.document()` nullish arms, and `:130`, `:132`, `:133`, the fallbacks
  for an event name outside the declared unions (5 branches).
- `authority.ts:8`, the `tryGet(DocumentsToken)` null arm and its `?? true` (2 branches):
  `DocumentsToken` always resolves.

## TypeScript behaviour pinned as is

- `updatePolicy({})` dispatches `ACTIONS_POLICY_CHANGED` and bumps `policyRevision` although nothing
  changed (`policy-rows-merge-and-update-live`).
- A blocked Print verb reports its node type, not its policy row: `executeNamed('Print', {origin:
  'hover'})` diagnoses "named: policy 'block' for origin 'hover'" while the decision came from the
  `print` row (`execute-named`).

## Mutation testing

Pre-v3, rerun pending. The pre-v3 run of `cargo mutants -p epdf-plugin-actions --test-package
epdf-plugin-actions` caught 274 of 283 viable mutants (262 caught, 12 timeouts, 115 unviable). The
timeouts were all in `hover_pump.rs`: a `same_target` that never matches re-delivers a target on
every settle and never stalls, which the replay reports as a hang. The nine survivors were
equivalent, and the code behind each still exists, now in the area modules:

- `same_number`'s numeric arm and its `==` (now `services/mod.rs`): every ref member the vectors can
  write is an integer, for which `Value` equality and `f64` equality agree.
- `page_index_of` returning `1` instead of `-1` (now `scripting/world.rs`): a page that isn't in the
  document makes `doc.page(pon)` throw right after, so the index is never used.
- Dropping the boot world's `event` (now `scripting/executor.rs`): `ScriptHost` replaces it with
  `name-tree-boot` before the sandbox sees it.
- `&&` to `||` in the lifecycle filter (now `dispatch/triggers.rs`): an annotation with actions but no
  lifecycle tree then sits in the cache, and `plan_page_steps` filters it out again.
- The `(None, None)` arms of the adapter disposer (`lifecycle/open_sequence.rs`) and of the handler
  disposer, which is now a no-op for a `None` install (`services/ports.rs`): clearing an empty slot
  and leaving it empty are the same.
- `initial_actions_state` as `Default::default()` (`model.rs`), the same zeroed state.
