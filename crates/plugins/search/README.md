# epdf-plugin-search

`packages/plugin/search` in Rust. All 67 cases in four vector files replay identically (`cargo test
-p epdf-plugin-search`):

| File | Kind | Cases | Covers |
|---|---|---|---|
| `vectors/plugins/search.json` | scenario | 32 | the session, `'full'` to `'rects'` degradation, stale-cursor restarts, errors, hit geometry, `canSearch`, navigation, the rerun effect, close |
| `vectors/plugins/search-stage.json` | scenario | 5 | viewport-first start pages and reveal-on-navigate through `epdf-plugin-stage` |
| `vectors/plugins/search-find-all.json` | scenario | 10 | `findAll`: the cursor chain, pinned and degraded modes, restarts, errors, its abort signal |
| `vectors/plugins/search-reducer.json` | function | 20 | `initialSearchState` and `searchReducer` |

## The public API

`search_plugin(config)` builds the plugin; `SEARCH` is its token and `SearchCapability` its
capability, with the TypeScript's methods in snake case (`can_search`, `search`, `rerun`, `clear`,
`next`, `prev`, `go_to`, `find_all`, `hits_for_page`, ...). The engine's search DTOs
(`SearchQuery`, `SearchRequest`, `SearchSlice`, `SearchMatch`, `PdfTextSegment`, `SearchSnippet`)
are typed in `types.rs`. `hits()` returns the state's `Rc<Vec<SearchHit>>`, shared until an `APPEND`
replaces it, as the TypeScript's spreads share the array.

`next`, `prev` and `go_to` return `Result<Option<SearchHit>, Error>`. The `Err` is the TypeScript's
throw from `ctx.tryGet(StageToken)` when the Stage's constructor fails, which no vector reaches.
`find_all` takes a kernel `AbortSignal` in `SearchFindAllOptions`; aborting it aborts the in-flight
slice with the signal's reason and rejects with that reason.

`SearchRevealOptions` has two options per member, because the TypeScript merges arrival options with
object spread, and a spread copies a key that is present as `undefined`. The outer `None` is an
absent key, `Some(None)` a key present as `undefined`, which overrides the default away:
`goTo(2, {anchor: undefined})` on a hit with no bounds is a bare reveal. A `null` anchor is present,
so the Stage positions the reveal with no axis set; that's `Some(Some(RevealAnchor::default()))`.

## Harness shim

`findAll`'s `signal` can't cross as JSON. `tooling/plugin-conformance/src/suites/search.ts` and
`src/harness.rs` both read an option `signal: {"$abortSignal": name}` as the signal of a controller
named `name`, which the `search-signals` test plugin's `abort(name, reason?)` aborts. With no reason
it is the platform's `AbortError` DOMException (`This operation was aborted`), as in `render.ts`.

## Where the port differs in shape

The hit index is an `isize`, so `goTo(1.5)`, `goTo(NaN)` and a stored fractional `activeIndex` can't
be expressed. In the TypeScript those read `hits[1.5]`, which is `undefined`: `goTo` then returns
`undefined` without a Stage and throws a `TypeError` with one, and `activeHit()` returns `undefined`
rather than `null`. A signal aborted with a non-`Error` reason (a string) isn't ported either: the
kernel's `AbortSignal` carries an `Error`, so the reason logged on the engine's abort would print as
`{"$error": ...}` instead of the bare string.

No vector pins the turn counts. `collect` awaits each slice through `await_engine`, which spends the
three turns `await` takes to adopt an `AbortablePromise`, and `rt::returned` around `collect`. Only
one chain is ever resumed per step, so a missing turn wouldn't show in a trace; the turns are there so
the port stays right when a second chain can race it.

Not ported: inputs outside the TypeScript's types, which crash it. A query without `text`, a slice
missing `matches`, a page layout missing its crop, and a `nextCursor` that is absent rather than
`null` (the TypeScript loops on it forever; serde reads it as `null`).

## Coverage

The generators reach 631 of 634 statements and 138 of 147 branches of the package. None of the rest
is reachable from the public surface:

- `capability.ts:144`, `:202-205`, `:265` and `:288`, and `effects.ts:18`, the `ctx.doc` null arms.
  The plugin is document-scoped, so its context always carries a handle.
- `capability.ts:110`, `boundsOfRects(...) ?? undefined`. It runs only when there are segments, and
  then it never returns `null`.
- `capability.ts:228`, a superseded session reaching the `catch`. `collect` checks cancellation
  before every throw it can make; the only later throw is `hitsFromSlice`, which runs right after that
  check with no turn in between.
- `capability.ts:229`, `String(err)` for a non-`Error` rejection. The engine rejects with `Error`s.
- `capability.ts:312`, `signal.reason ?? new Error('findAll aborted')`. An aborted signal always has
  a reason.

## Mutation testing

`cargo mutants -p epdf-plugin-search --test-package epdf-plugin-search` tests 128 mutants: 98 caught, 5 timeouts, 22 unviable, 3 missed. The five timeouts are `&&` to `||`
in the fallback and restart guards (`capability.rs:286-288` and `:295`), which retry a refused
request forever. Every miss is equivalent:

- `capability.rs:137`, the session sink's `is_cancelled` to `false`. A superseded loop's slice is
  always aborted, so its `await` rejects with an `AbortError`, which is neither a refusal nor an
  `InvalidArg`; the loop throws, and `run_session` drops the error because the generation moved.
- `capability.rs:382`, `complete && generation == ...` to `||`. `complete` is false only when the
  loop saw a new generation, so both sides are false together.
- `capability.rs:547`, the `!indices.is_empty()` guard to `true`. The reducer never stores an empty
  page bucket.
