# epdf-plugin-redaction

`packages/plugin/redaction` in Rust, on the v3 controller shape. All 50 cases in two vector files
replay identically (`cargo test -p epdf-plugin-redaction`):

| File | Kind | Cases | Covers |
|---|---|---|---|
| `vectors/plugins/redaction.json` | scenario | 22 | the twins, marking from the text selection, by area, by page and from search matches, the configured overlay look, the redact tool through the suite's lens, the pending view and its reads, collateral, `unmark` and `clearPending`, labels and a refused label edit, `apply`, `applyPages` and `applyAll` over the engine's `redaction` service (and an engine without it), refusals, remote applies, `onPendingChanged`, a throwing `onApplied` listener |
| `vectors/plugins/redaction-functions.json` | function | 28 | the reducer, and `estimateCollateral` over a stub annotation plane: touching edges, slivers, tilted quads, `NaN` geometry, ref filters of every kind |

The scenario registry runs the real interaction, selection, annotation and search adapters beside
this one, so marks are real annotations and search marks come from a real search session.

## The API

One token, `REDACTION`, and one type, `RedactionCapability`, with the TypeScript's members in snake
case: the reads `list_pending`, `get_pending`, `get_pending_count` and `estimate_collateral`;
marking with `mark_selection`, `mark_area`, `mark_page`, `mark_matches`, `unmark`, `clear_pending`
and `update_label`; applying with `apply`, `apply_pages` and `apply_all`; and `can_mark`,
`can_apply`, `is_applying`, `get_last_result`, `on_applied` and `on_pending_changed`.
`redaction_plugin(RedactionConfig)` is `redactionPlugin(config)`. The files follow the TypeScript's
`read/`, `write/`, `sync/` and `services/` split, each an `impl` block on the one capability.

`mark_area`, `mark_page` and `clear_pending` return `Result<Promise<_>, Error>`, because the
TypeScript's are plain functions that throw where they are called (`markPage` of an unknown page,
`markArea` without create authority). The `async` verbs reject instead. `apply`, `apply_pages` and
`apply_all` are plain functions too, but they refuse with `Promise.reject`, so they return a
`Promise`.

Apply results, `redaction.applied` events and collateral refs stay `serde_json::Value`: the plugin
never reads inside them.

## Host input

The TypeScript reports a throwing event listener to `console.error('[redaction] listener failed:',
error)`. That comes in as `RedactionConfig::host.console_error`. The vectors log it as a
`console.error` callback, the same shim the annotation suites use.

## The `redaction-tool` lens

v2's `enableRedact`, `toggleRedact` and `isRedactActive` left the plugin: the `redact` tool is the
annotation plugin's, armed through the interaction hub. The generator keeps the old cases readable
with a suite-local `redaction-tool` factory (`redaction-test/tool`) over the hub, and `harness.rs`
builds the same lens. `toggleRedact` goes back through `activateDefaultTool`, not to `pointer`.

## Where the port differs in shape

The TypeScript memoizes each page's marks on the identity of `annotation.list({page, subtype})`,
and the document-wide list on the identity of the per-page results. The annotation plane builds a
fresh array per call, so the per-page memo never hits (`read/pending.ts:41` is unreached). The port
rebuilds the marks on every read. Nothing observable changes.

`originOf(event)` for a remote apply is computed locally in `sync/mod.rs`. The kernel's `origin_of`
answers `sessionId: null` for an origin without one, where the TypeScript's literal carries
`undefined`, which canonical JSON leaves out (`remote-applies-arrive-as-events`).

`apply` with no argument throws `TypeError: Cannot read properties of undefined (reading 'map')` in
the TypeScript before any promise exists. The harness produces that throw, since the Rust signature
takes a slice.

## Coverage

The generators reach 731 of the package's 738 statements and 167 of its 172 branches
(`vectors/coverage.json`). What is left cannot be reached from the plugin:

- `controller.ts:53-59`, `createRedactionCapability`, the unit tests' helper. The plugin builds
  through `createRedactionController`.
- `read/pending.ts:41`, the memo hit, for the reason above.
- `read/pending.ts:44`, a `redact` record without a `redact` raw DTO. The annotation plane only
  lists confirmed entries, and those carry their DTO.
- `services/store.ts:20`, `requireDoc` without a handle, and `store.ts:31`, `pages()` without a
  document. The plugin is document-scoped and a controller is built after the handle is set.
- `services/store.ts:33`, the `-1` page index. A mark's page comes from the page filter or the
  document's page list, and hydration drops annotations on pages the document does not list.

## Mutation testing

The pre-v3 runs covered `capability.rs`, which is gone. The reducer moved unchanged to `model.rs`;
its pre-v3 result (3 caught, 1 equivalent miss: `initial_redaction_state` as `Default::default()`)
is pre-v3, rerun pending.
