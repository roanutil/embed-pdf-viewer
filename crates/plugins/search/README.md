# epdf-plugin-search

`packages/plugin/search` in Rust, on the v3 contract: `create()` builds the controller, `connect`
subscribes the mutation rerun, the session runs on the kernel's `latest` lane, pages are addressed
by `PageRef`, and engine failures arrive as `PluginError`s through the guarded handle. All 77 cases
in four vector files replay identically (`cargo test -p epdf-plugin-search`):

| File | Kind | Cases | Covers |
|---|---|---|---|
| `vectors/plugins/search.json` | scenario | 39 | the session, `'full'` to `'rects'` degradation, stale-cursor restarts, errors, hit geometry, `canSearch`, navigation, the rerun, `cancel`, the seven events, the page reads, close |
| `vectors/plugins/search-stage.json` | scenario | 6 | viewport-first start pages, reveal-on-navigate and `revealActiveHit` through `epdf-plugin-stage` |
| `vectors/plugins/search-find-all.json` | scenario | 10 | `findAll`: the cursor chain, pinned and degraded modes, restarts, errors, its abort signal |
| `vectors/plugins/search-reducer.json` | function | 22 | `initialSearchState` and `reduceSearch` |

The generators (`tooling/plugin-conformance/src/suites/search.ts` and `search-reducer.ts`) reach all
678 of the package's statements and 159 of its 162 branches, counted over the search vector files
alone. The three missing branches are under Coverage.

## The public API

`search_plugin(config)` builds the plugin; `SEARCH` is its token and `SearchCapability` its
capability, with the TypeScript's members in snake case (`can_search`, `search`, `refresh`,
`cancel`, `clear`, `next_hit`, `go_to_hit`, `reveal_active_hit`, `list_hits`, `find_all`,
`on_started`, ...). The engine's search DTOs (`SearchQuery`, `SearchRequest`, `SearchSlice`,
`SearchMatch`, `PdfTextSegment`, `SearchSnippet`) are typed in `contract.rs`. `list_hits(None)`
returns the state's `Rc<Vec<SearchHit>>`, and `list_hits(Some(page))` that page's bucket, each shared
until new hits land there, as the TypeScript's spreads share the arrays.

`search` and `refresh` return a `Promise<SearchResult>`: `complete`, `superseded` or `cancelled`,
rejecting only on a real failure. `next_hit`, `previous_hit`, `go_to_hit` and `reveal_active_hit`
return a `Result`, whose `Err` is the TypeScript's throw from `ctx.tryGet(StageToken)` when the
Stage's constructor fails, which no vector reaches. `find_all` takes a kernel `AbortSignal` in
`SearchFindAllOptions`; aborting it aborts the in-flight slice with the signal's reason and rejects
`operation-cancelled`.

`SearchRevealOptions` has two options per member, because the TypeScript merges arrival options with
object spread, and a spread copies a key that is present as `undefined`. The outer `None` is an
absent key, `Some(None)` a key present as `undefined`, which overrides the default away:
`goToHit(2, {anchor: undefined})` on a hit with no bounds is a bare reveal. A `null` anchor is
present, so the Stage positions the reveal with no axis set; that's `Some(Some(RevealAnchor::default()))`.

## Harness shim

`findAll`'s `signal` can't cross as JSON. `tooling/plugin-conformance/src/suites/search.ts` and
`src/harness.rs` both read an option `signal: {"$abortSignal": name}` as the signal of a controller
named `name`, which the `search-signals` test plugin's `abort(name, reason?)` aborts. With no reason
it is the platform's `AbortError` DOMException (`This operation was aborted`), as in `render.ts`.
Only `findAll` gets the shim; `search`'s own `signal` goes to the lane, which the kernel's vectors
cover.

## Where the port differs in shape

The engine sees an abort reason as the TypeScript passes it. A lane aborts its run with a string
(`'superseded'`, `'cancelled'`, `'cleared'`, `'instance-closed'`), and the kernel's `AbortSignal`
carries it as an error whose message is that string, so the session hands the engine the message.
`findAll`'s own controller is aborted with the caller's reason, an error, and hands the engine its
canonical form.

The hit index is an `isize`, so `goToHit(1.5)` and `goToHit(NaN)` can't be expressed. In the
TypeScript those read `hits[1.5]`, which is `undefined`. A signal aborted with a non-`Error` reason
(a string) isn't ported either: the kernel's `AbortSignal` carries an `Error`, so the reason logged
on the engine's abort would print as `{"$error": ...}` instead of the bare string.

The pre-v3 README recorded that no vector pinned the turn counts. Under v3 that is narrower but
still true. `collect` awaits each slice through `await_engine`, which spends the turns `await` takes
to adopt an `AbortablePromise`, and `rt::returned` around `collect`. Two new cases,
`close-races-the-session-and-find-all` and `close-races-find-all-first`, settle a session and a
`findAll` in one step and pin their order: `findAll` settles first whichever started first, because
the session's chain runs through the lane and its catch. That pins the difference between the
chains, not each count. Dropping `await_engine`'s adoption turn, or the `rt::returned` turn in
`find_all`, still replays every file. The turns stay so the port is right when two chains are
closer.

Not ported: inputs outside the TypeScript's types, which crash it. A query without `text`, a slice
missing `matches`, and a `nextCursor` that is absent rather than `null` (the TypeScript loops on it
forever; serde reads it as `null`).

## Coverage

The three unreached branches can't be reached from the public surface:

- `controller.ts:96`, `boundsOfRects(...) ?? undefined`. It runs only when there are segments, and
  then it never returns `null`.
- `controller.ts:165`, the signal aborted after a slice resolved but before the loop resumed. An
  abort of a settled slice does nothing, and nothing can run in the turns between the engine's
  answer and the loop's next line.
- `controller.ts:223`, `details?.reason ?? 'cancelled'`. The lane attaches `details` to every
  `operation-cancelled` it raises.

Two TypeScript behaviours are pinned that look unintended. The lifetime abort reaches the engine as
the kernel's `CancelledError` `[kernel] cancelled: closed while opening: a`, though the document had
finished opening (`close-races-*`). And the `a-user-unit-of-zero-still-converts` case records that
hits now convert through `ctx.geometry.forPage`, which reads only the crop, so a page whose
`userUnit` is 0 no longer fails the search as it did before v3.

## Mutation testing

`scripts/mutants.sh -p epdf-plugin-search --test-package epdf-plugin-search --file <file>`, file by
file over every module but `harness.rs`, catches 89 of 90 viable mutants (84 caught, 5 timeouts;
42 unviable). The timeouts are `&&` to `||` in the fallback and restart guards of `collect`, which
retry a refused request forever. One caught mutant is killed by the new case
`search-signal-already-aborted`, checked by applying it by hand: `&&` to `||` in `search`'s
cancelled branch (`controller.rs:465`), which would dispatch `CANCELLED` for a run the lane refused
before it started. Reaching it meant extending the harness shim to `search`'s `signal` on both
sides.

The one survivor is in `src/controller.rs:495`: `refresh`'s match guard
`state.status != SearchStatus::Idle` replaced with `true`. It is unreachable. Only the initial
state is `idle`, and its query is `null`, so the `Some(query)` arm never sees an idle status. The
TypeScript's `status === 'idle'` test is the same dead check.
