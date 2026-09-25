# epdf-plugin-actions

`packages/plugin/actions` in Rust: the PDF action engine. All 150 cases replay identically
(`cargo test -p epdf-plugin-actions`): 36 function cases in `vectors/plugins/actions.json` and 114
scenarios across `actions-dispatcher.json` (27), `actions-submit.json` (10),
`actions-triggers.json` (15), `actions-lifecycle.json` (26), `actions-script.json` (26) and
`actions-hover.json` (10).

The TypeScript splits the capability into a public lens (`/contract`) and a host lens
(`/contract/host`). That split is API visibility over one runtime object, so Rust has one
`ActionsCapability` behind one token, `ACTIONS`, and marks the host members in their docs. Sibling
crates (form, link, annotation, stamp) use `register_executor`, `register_annot_commit_sink`,
`register_form_commit_sink`, `register_submit_resolver`, `report_page_state`,
`surface_script_result` and `script_transaction`, plus the public `dispatch`, `execute` and hooks.
Engine DTOs (action trees and nodes, refs, form effects) stay `serde_json::Value`, because the
dispatcher echoes trees back in results and events and must print them exactly as it received them.

## Host inputs

The TypeScript reads three ambient values, and each is an input here:

- The four observer hooks report a throwing listener to `globalThis.console.error`. Rust takes that
  sink as `ActionsPluginConfig::observer_error`, called with the same message and error; with no
  sink the report is dropped. The vectors route both sides into the trace as a `console.error`
  callback.
- `Date.now()` is `rt::now()`, or `javascript.now` when configured (it also stamps `clientTimeMs` on
  submits, with scripting on or off).
- `-new Date(nowMs).getTimezoneOffset()` is `host_utc_offset_minutes`, default zero. The generator
  pins `TZ=UTC`, so the vectors record zero.

`capability.ts` has no `window` read; `globalThis.console` is the only global it touches.

## Scripting without a sandbox

JavaScript runs only when `javascript.enabled` is set and the host supplies
`javascript.sandbox_factory`. Where the TypeScript lazily imports `@embedpdf/core-js-sandbox` as a
default, we have none, and a host with no sandbox runs exactly as the TypeScript does with scripting
disabled: javascript nodes are inert ("no executor registered (scripting unavailable)") and
`has_script_transaction()` is false. The `no-sandbox-host` case pins that configuration against the
TypeScript with `enabled: false`.

## Where the port has to be careful

Turn counts are behaviour here, because the trace records how concurrent dispatches, hover pairs
and detached submits interleave. Three awaits cost more than one turn, and `capability.rs` spells
each out: an engine call returns an `AbortablePromise`, which `await` and `Promise.resolve` adopt as
a thenable (`adopt_engine`); an `async` function that returns a promise unawaited resolves through
the same adoption (`adopt`); and the serial queue adopts each operation's promise, which needed a
fix in `epdf_kernel::SerialQueue` (commit `3dbd19149`). `Promise.allSettled` in the hover pump
awaits each dispatch rather than observing it, so its reactions keep their registration order.

Event names in an `ActionContext` are Rust enums, so a type-violating name (`'bogus'`) can't be
expressed. The hover pump compares an index ref's `revision` by value; the TypeScript's `===` is
reference identity for a `RevisionToken` object, which a feed holding one token object means the
same way.

## Coverage

`capability.ts` is at 1604/1615 statements and 573/601 branches, `hover-pump.ts` at 99/99 and
38/38, and every other file is fully covered except `reducer.ts:9`, the default arm of a one-action
reducer. What remains in `capability.ts` is unreachable from the public surface or doesn't port:

- `capability.ts:328-332`, the lazy QuickJS default. Rust has no default sandbox.
- The `String(error)` arm of every `error instanceof Error ? ... : String(error)` (`:272`, `:291`,
  `:298`, `:310`, `:902`, `:980`, `:1230`, `:1377`, `:1430`). Scripted callbacks throw and reject
  with `Error`s, and Rust has only `Error`.
- The `ctx.doc` and `ctx.documentId` null arms (`:182`, `:336`, `:344-345`, `:351`, `:640`,
  `:1069`, `:1177`) and the `meta`/`document()` nullish arms (`:338`, `:454`, `:511`). The plugin is
  document-scoped, and every call runs against a ready document.
- `:182`'s `?? true`: `DocumentsToken` always resolves.
- `:559`, `:561`, `:562`, the fallbacks for an event name outside the declared union.
- `:638`, the javascript executor's non-javascript guard: it is only ever called for javascript
  nodes.
- `:782`'s `: null`, guarded by the enclosing `if`.
- `:1326-1332`, `releaseBarrier`'s `catch`: nothing inside the `try` can throw.

`cargo mutants -p epdf-plugin-actions --test-package epdf-plugin-actions` catches 274 of 283 viable
mutants (262 caught, 12 timeouts, 115 unviable). The timeouts are all in `hover_pump.rs`: a
`same_target` that never matches re-delivers a target on every settle and never stalls, which the
replay reports as a hang. The nine survivors are equivalent:

- `capability.rs:125` and `:126`, `same_number`'s numeric arm and its `==`: every ref member the
  vectors can write is an integer, for which `Value` equality and `f64` equality agree.
- `capability.rs:786` and `:794`, `page_index_of` returning `1` instead of `-1`: a page that isn't
  in the document makes `doc.page(pon)` throw right after, so the index is never used.
- `capability.rs:1118`, dropping the boot world's `event`: `ScriptHost` replaces it with
  `name-tree-boot` before the sandbox sees it.
- `capability.rs:1477`, `&&` to `||` in the lifecycle filter: an annotation with actions but no
  lifecycle tree then sits in the cache, and `plan_page_steps` filters it out again.
- `capability.rs:2027` and `:2219`, the `(None, None)` arms of the adapter and handler disposers:
  clearing an empty slot and leaving it empty are the same.
- `reducer.rs:6`, `initial_actions_state` as `Default::default()`, the same `{seq: 0}`.
