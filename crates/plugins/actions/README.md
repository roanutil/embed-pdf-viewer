# epdf-plugin-actions

`packages/plugin/actions` in Rust, at the current TypeScript contract (Task 13.12): the PDF action
engine. All 160 schema-2 cases replay identically (`cargo test -p epdf-plugin-actions`): 36 function
cases in `vectors/plugins/actions.json` and 124 scenarios across `actions-dispatcher.json` (30),
`actions-submit.json` (10), `actions-triggers.json` (15), `actions-lifecycle.json` (27),
`actions-script.json` (32) and `actions-hover.json` (10). One member-order allowance remains,
`actions-submit.json` `a-handler-beats-the-home`: the resolver hands back a request whose `charSet`
it appended last, and the typed `ActionSubmitRequest` prints it in field order.

The plugin is document-scoped, stateless (its slice is `undefined`) and built through `create`, so
its controller exists from bring-up. A
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
`ACTIONS`, and `ActionsHostCapability` is an alias. The public surface is the four settings calls
(`getSettings`, `updateSettings`, `resetSettings`, `onSettingsChanged`), `execute`, `canExecute`,
`executeNamed`, `canExecuteNamed`, `getActionTree`, `isScriptingEnabled`, `dispatch`,
`canDispatch`, `setUiAdapter`, `runDocumentVerb`, `prepareClose`, `setSubmitHandler`,
`registerExecutor` and the five events (`onExecuted`, `onDiagnosticReported`,
`onScriptDiagnosticReported`, `onScriptFailed`, `onOpenSequenceCompleted`). The verbs that take
`OperationOptions` in the TypeScript take a trailing `Option<AbortSignal>`. The host lens adds
`register_annot_commit_sink`, `register_form_commit_sink`, `register_form_reader`,
`register_submit_resolver`, `report_page_state`,
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

The rules are settings, kept in the kernel's store for the whole registration: a change reaches
every open document. `policy` rows merge origin by origin (`policy-rows-merge-and-update-live`).
`javascript.enabled` is read when a document opens, so it reaches only documents opened after the
change. `javascript.identity` may be a function, which JSON can't hold: `settings.rs` stores it as a
handle, `{"$identity": n}`, into a table the registration owns, so the same function reads back as
the same `Rc` (`a_function_identity_keeps_its_identity_through_the_store` in `tests/behavior.rs`).

Rust ships no default UI adapter, so the dispatcher does the URI check the TypeScript's default
adapter does. `sanitize_external_uri` holds a link's scheme to http, https, mailto and tel before
`open_uri`, and any other scheme ends the node `blocked` with `uri: scheme not allowed`. The cost:
an embedder cannot receive a custom scheme through `open_uri`, where a TypeScript override adapter
can. No vector covers it, since every vector link is https.

## Host inputs

The TypeScript reads two ambient values, and each is an input here. A throwing event listener is no
longer one: it goes to the kernel's reporter, as every `ctx.events` source's does, and
`observer_error` is gone.

- `Date.now()` is `rt::now()`, or `javascript.now` when configured (it also stamps a submit's
  `sentAt`, with scripting on or off).
- `-new Date(nowMs).getTimezoneOffset()` is `javascript.host_utc_offset_minutes`, default zero. The
  generator pins `TZ=UTC`, so the vectors record zero.

Print requests go to the kernel's print when the host gave the kernel a printer
(`KernelOptions::printer`): `DocumentPrint` reads `can_print()` and `print()` on the documents
capability as this document's scope sees it, and `around_print` runs `/WP` and `/DP` around every
kernel print, as `controller.ts` does with `ctx.aroundPrint`. Without a printer every print falls back
to the UI adapter. The printer branches (user prints, the M14 non-user refusal, the form pipeline's
print effect, the wrap holding the print latch) are tested in `lifecycle/document_events.rs`; no
vector configures a printer.

## Scripting without a sandbox

With `javascript.enabled` on and no `javascript.sandbox_factory`, the TypeScript lazily imports
QuickJS from `@embedpdf/core-js-sandbox`. Rust has no JavaScript engine. The realm still exists, as
the TypeScript's does: `is_scripting_enabled()` is true and both realm members are present, which is
what `no-sandbox-host` replays (it closes the document before any run). Every sandbox build then
rejects with `NO_DEFAULT_SANDBOX`, so a javascript node fails with that message
(`scripting_on_without_a_factory_has_a_realm_whose_runs_fail`). Before Task 13.12 such a host ran as
if scripting were off; the vector now records the realm, so Rust follows it.

## Where the port has to be careful

Turn counts are behaviour here, because the trace records how concurrent dispatches, hover pairs and
detached submits interleave. The guarded handle already spends the turns an engine call's thenable
adoption costs, so a call is awaited directly; wrapping it again adds a turn the TypeScript doesn't
have, which is why the catalog read awaits the call inside its `catch` rather than through a second
promise. An `async` function that returns a promise unawaited resolves through the adoption turn
(`adopt` in `document_events.rs`). `Promise.allSettled` in the hover pump awaits each dispatch rather
than observing it, so its reactions keep their registration order.

Event names in an `ActionContext` are Rust enums, so a type-violating name (`'bogus'`) can't be
expressed. The hover pump compares targets by page and `annotationKey`, as the TypeScript now does.

`can_dispatch` used to answer `false` where the TypeScript threw `not-ready` (L-A4 in
`docs/research/rust-plugins-branch-review.md`). The current TypeScript reads only the `triggers`
setting there (`dispatch/dispatcher.ts`, `canDispatch`), and so does Rust: that note no longer
applies.

## Coverage

Stale since Task 13.12; Task 14 did not regenerate it (not re-run since 172f5369d). The generators reach 3,184 of the package's 3,195 statements and 754 of its 780 branches. This port
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

- A blocked Print verb reports its node type, not its policy row: `executeNamed('Print', {origin:
  'hover'})` diagnoses "named: policy 'block' for origin 'hover'" while the decision came from the
  `print` row (`execute-named`).

## Mutation testing

Stale since Task 13.12 (the re-port moved most lines and removed `model.rs`); Task 14 did not re-measure it (not re-run since 172f5369d).
`scripts/mutants.sh -p epdf-plugin-actions --test-package epdf-plugin-actions --file <file>`, file
by file over every module but `harness.rs`, catches 307 of 314 viable mutants (295 caught, 12
timeouts; 142 unviable). The timeouts are all in `hover_pump.rs`: a `same_target` that never
matches re-delivers a target on every settle, which the replay reports as a hang.

Four of the caught are killed by new steps in existing cases, each checked by applying the mutant by
hand:

- `get-action-tree` asks for field 101 by object number. With `&&` to `||` at
  `dispatch/dispatcher.rs:304`, any numbered field matches, so the mutant answers field 100's
  keystroke tree.
- `get-action-tree` gives page 1 `visible` and `invisible` trees and asks for both. Deleting either
  arm at `dispatcher.rs:318` or `:319` answers the page's open tree.
- `surface-script-commit` surfaces a commit with diagnostics and an error but no effects. With `==`
  to `!=` at `scripting/surface.rs:126` the empty user phase is skipped and both are lost.

The 7 survivors:

- `src/services/mod.rs:108`, deleting `same_number`'s numeric arm, and `:109`, `==` to `!=` in its
  fallback. The fallback compares two JSON values when either is not a number. Every member it is
  given is a ref's object number, page or index: typed on the caller's side and a number in every
  engine DTO, so the fallback never runs. With two numbers, `Value` equality and `f64` equality
  only disagree for `5` against `5.0`, and TypeScript never writes `5.0`.
- `src/scripting/world.rs:76`, `-1` to `1` in `page_index_of`. A page the document lacks makes
  `doc.page_index(pon)?` fail on the next line, so the index is never used.
- `src/scripting/executor.rs:214`, dropping the boot world's `event`. `boot_now` in
  `crates/plugins/acrojs/src/host.rs` replaces it with a fresh `name-tree-boot` event before the
  sandbox sees it.
- `src/dispatch/triggers.rs:90`, `&&` to `||` in the lifecycle filter. An annotation with actions
  but no lifecycle tree then sits in the cache, and `plan_page_steps` drops it again because it has
  no root for the event's key.
- `src/lifecycle/open_sequence.rs:155`, deleting the adapter disposer's `(None, None)` arm. The
  slot is already empty, and clearing it or leaving it is the same.
- `src/model.rs:26`, `initial_actions_state` as `Default::default()`. `ActionsState` derives
  `Default`, the same zeroed state.
