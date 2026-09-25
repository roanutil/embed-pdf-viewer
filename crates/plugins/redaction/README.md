# epdf-plugin-redaction

`packages/plugin/redaction` in Rust: the destructive apply stage over `redact` annotation marks.
All 45 cases in two vector files replay identically (`cargo test -p epdf-plugin-redaction`):

| File | Kind | Cases | Covers |
|---|---|---|---|
| `vectors/plugins/redaction.json` | scenario | 14 | the twins, the redact tool verbs, marking from the text selection and by area drag, the pending projection, collateral, `setLabel`, `apply` and `applyAll` over the engine's `redaction` service (and an engine without it), refusals, remote applies, a throwing `onApplied` listener |
| `vectors/plugins/redaction-functions.json` | function | 31 | the reducer, and `estimateCollateral` and `getPending` over a stub context: touching edges, slivers, tilted quads, `NaN` geometry, id filters, every ref kind |

The scenario registry runs the real interaction, selection and annotation adapters beside this one,
so marks are real annotations and the apply flow sees the annotation plane's own reaction to a
`redaction.applied` event.

## The API

One token, `REDACTION`, and one type, `RedactionCapability`, with the TypeScript's methods in snake
case: `can_mark`, `can_apply`, `is_applying`, `last_result`, `enable_redact`, `toggle_redact`,
`is_redact_active`, `queue_current_selection`, `prepare_pending`, `get_pending`, `pending_count`,
`estimate_collateral`, `set_label`, `apply`, `apply_all` and `on_applied`. `redaction_plugin(host)`
is `redactionPlugin()`. The reducer, `create_redaction_capability`, and the pure halves of the
capability (`collect_pending`, `estimate_collateral`, `id_of`, `regions_of`, `intersects`) are
public too.

`canMark` is `annotation.can_create()`, not a re-derivation. `canApply` is engine support plus all
three of `doc.redact`, `doc.pages.modify` and `doc.annotate.modify` (`APPLY_CAPABILITIES`), as
`permissions.md` requires.

Apply results and `redaction.applied` events stay `serde_json::Value`: the plugin never reads
inside them. So does a pending item's `ref`, which is the DTO's own and goes back to the engine
verbatim.

## Host input

The TypeScript reports a throwing `onApplied` listener to `console.error` and carries on with the
next one. That comes in as `RedactionHost::console_error(message, error)`; `None` drops it. The
vectors log it as a `console.error` callback, the same shim the annotation suites use.

## Turn order

`apply` and `apply_all` are `async` functions that `return runApply(...)`, so their promise settles
two turns after `runApply`'s (the resolve-thenable job, then the reaction). `runApply` awaits the
engine call, which costs the three turns `epdf_plugin_annotation::promise::await_engine` models.
`prepare_pending` is `await Promise.all(...)` over `ensurePage`'s `undefined`s: one turn for the
`all`, one for the `await`.

## Where the port differs in shape

`toggle_redact`, `enable_redact` and `is_redact_active` return `Result`, because `ctx.try_get` can
fail when a capability constructor throws. The TypeScript throws the same error from the same call.

A `redaction.applied` event with no `origin` makes the TypeScript listener throw a `TypeError` from
inside `emit`. The Rust listener can't throw, so it ignores the event. No engine sends one.

`get_pending`'s `id` is the TypeScript's template literal over whatever the ref carries, including
`undefined` members of an unknown ref kind, so it's computed from the JSON rather than through
`AnnotationRef`, which would refuse such a ref.

## Coverage

The generators reach all 253 statements in `capability.ts` and every statement in the other five
files, and 98 of its 101 branches (`vectors/coverage.json`). The three left are unreachable from the
public surface:

- `capability.ts:67`, `requireDoc` with no `ctx.doc`. The plugin is document-scoped, so its context
  always has a handle.
- `capability.ts:160`, `toggleRedact` without an interaction plugin. The annotation plugin requires
  one, and this plugin requires the annotation plugin.
- `capability.ts:173`, a selected page with no segments. The selection plugin's `snapshot` filters
  those pages out before it returns.

## Mutation testing

Complete, file by file through `scripts/mutants.sh`:

| File | Caught | Missed | Timeout | Unviable |
|---|---|---|---|---|
| `capability.rs` | 87 | 0 | 1 | 38 |
| `reducer.rs` | 3 | 1 | 0 | 0 |
| `lib.rs` | 0 | 0 | 0 | 1 |

The timeout is `index *= 1` in the listener walk, which never ends, so cargo-mutants catches it by
its timeout. The one miss is equivalent: `initial_redaction_state` replaced with
`Default::default()` builds the same state. `types.rs` has no functions to mutate, and `harness.rs`
is the adapter, which the vectors exercise but nobody mutates.

The first `capability.rs` run missed 7. Two were guards for an `undefined` ref member, which
canonical JSON never carries, so they're gone and a function case pins how an array-valued member
joins. The other five were the listener set's re-entry bookkeeping. No vector reaches it (a `$fn`
callback can't unsubscribe while `onApplied` listeners are firing), so three unit tests in
`capability.rs` pin JavaScript's `Set` iteration rules instead. That's the cost of those tests: they
check the port against the language semantics, not against the TypeScript oracle.
