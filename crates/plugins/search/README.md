# epdf-plugin-search

`packages/plugin/search` in Rust, ported to the current TypeScript (Task 13.20): `create()` builds
the controller, `connect` subscribes the mutation rerun, the session runs on the kernel's `latest`
lane, every slice is asked for at low priority, and the session moves through `model.ts`'s named
transitions. All 76 cases in four vector files replay (`cargo test -p epdf-plugin-search`), with no
member-order allowance:

| File | Kind | Cases | Covers |
|---|---|---|---|
| `vectors/plugins/search.json` | scenario | 39 | the session, snippets degrading to none, stale-cursor restarts, errors, hit geometry, `canSearch`, navigation, the rerun, `cancel`, the seven events, the page reads, close |
| `vectors/plugins/search-stage.json` | scenario | 6 | viewport-first scans, reveal-on-navigate and `revealActiveHit` through `epdf-plugin-stage` |
| `vectors/plugins/search-find-all.json` | scenario | 10 | `findAll`: the cursor chain, pinned and degraded snippets, restarts, errors, its abort signal |
| `vectors/plugins/search-reducer.json` | function | 21 | `initialSearchState` and the seven named transitions, with `changed` as `next !== state` |

`tests/behavior.rs` covers what the vectors don't print: a refusal that rejects before the session
changes, snippets denied under one start, a superseded scan that lands nothing, an empty scan, the
reveal of a hit's page, `findAll`'s order beside a Stage, the low priority (Ruling13.16), and the
turns each chain takes, counted from the TypeScript.

## The public API

`search_plugin(config)` builds the plugin; `SEARCH` is its token and `SearchCapability` its
capability, with the TypeScript's members in snake case: the settings API (`get_settings`,
`update_settings`, `reset_settings`, `on_settings_changed`), `can_search`, `search`, `refresh`,
`cancel`, `clear`, `next_hit`, `previous_hit`, `go_to_hit`, `reveal_active_hit`, the reads,
`find_all` and the seven `on_*` hooks. `search_state` is `state.ts`. The engine DTOs
(`SearchQuery`, `SearchRequest`, `SearchSlice`, `SearchMatch`, `SearchSnippet`, `TextSegment`)
are in `contract.rs`; a match's segments arrive in page space, so the crate builds no text layout
and has no `epdf-engine` dependency at all. Ruling13.15 lists search among the layout's consumers,
but the current TypeScript reads none.

A page argument is `PageTarget` (the Stage's): a `PageRef` or a display index, as
`PageRef | number`. `go_to_hit` takes a `HitTarget`, a hit or an index. `list_hits` returns the
state's `Rc<Vec<Rc<SearchHit>>>`, and per page that page's list, each shared until hits land there.
`get_progress` returns the state's `Rc<SearchProgress>`; `onProgressChanged` fires when that `Rc`
changes, which is the TypeScript's identity test.

`SearchRevealOptions` has two options per member, because the TypeScript merges arrival options with
object spread over the `reveal` setting, and a spread copies a key that is present as `undefined`.
The outer `None` is an absent key, `Some(None)` a key present as `undefined`, which overrides the
setting away. A `null` anchor, in the setting or a call, positions the reveal with no axis set
(`RevealAnchor::default()`), as the TypeScript Stage reads a present `null`.

## Migrating from the pre-13.20 API

For redaction (Task 13.23), the viewer host, `epdf-manual-bridge` and `epdf-web` (13.33).

| Old | New |
|---|---|
| `SearchHit.{char_start, char_count}` | `start`, `count` |
| `SearchSnippet { text, match_start, match_length }` | `{ before, matched, after }` (JSON `match`) |
| `SearchProgress { scanned, total }` | `{ pages_searched, page_count }`, read as `Rc` |
| `SearchMode`, `can_search(mode)`, `SearchFindAllOptions.mode` | `snippets: Option<bool>` |
| `SearchOptions.start_page: Option<PageRef>` | `from: Option<PageTarget>`; a page not in the document rejects `not-found` |
| `PdfTextSegment`, `PdfRect`, the y-flip in the plugin | `TextSegment { quad: Quad, rect: Rect, advance }`, page space from the engine |
| `SearchAction`, `reduce_search` | `start_session`, `append_hits`, `complete_session`, `cancel_session`, `fail_session`, `set_active_hit`, `clear_session` |
| `SearchConfig { reveal }` | settings: `SearchConfig { reveal, highlight }` over `search_defaults()` |
| `on_progress`, events with `operation_id` | `on_progress_changed`; no event carries an operation id |
| `SearchActiveHitChangedEvent.index: usize` | `isize`, `-1` when a new search or `clear` drops the active hit |
| `go_to_hit(index: isize, ..)`, hits as `SearchHit` | `go_to_hit(impl Into<HitTarget>, ..)`, hits as `Rc<SearchHit>` |
| ffi `start_search(.., start_page)`, `start_find_all(.., mode)` | `start_search(.., from)`, `start_find_all(.., snippets)` |

`src/ffi.rs` and `tests/ffi.rs` are migrated by hand and not compiled until `epdf-ffi` builds again
(13.33). They map the named quad corners onto `epdf_ffi::TextQuad`, drop `PluginErrorInfo`'s
`permission`, and bridge no settings yet.

## Harness shim

`findAll`'s and `search`'s `signal` can't cross as JSON. `tooling/plugin-conformance/src/suites/search.ts`
and `src/harness.rs` both read an option `signal: {"$abortSignal": name}` as the signal of a
controller named `name`, which the `search-signals` test plugin's `abort(name, reason?)` aborts.
With no reason it is the platform's `AbortError` DOMException (`This operation was aborted`).

## Where the port differs in shape

The engine sees an abort reason as the TypeScript passes it: `collect` uses the kernel's
`cancellable`, which hands the engine the signal's JavaScript value (`AbortSignal::reason_value`), a
lane's string such as `"superseded"` or a caller's error (Ruling13.20, `4ad664efd`).
Closing the document aborts an in-flight slice with the kernel's `CancelledError`
(`[kernel] cancelled: closed while opening: a`), as `kernel.ts:560` does even though the document
had finished opening; the `close-races-*` cases pin it.

The hit index is an `isize`, so `goToHit(1.5)` and `goToHit(NaN)` can't be expressed; the
TypeScript reads `hits[1.5]`, which is `undefined`. `validateSearchQuery` and
`validateSearchRegex`, which `contract.ts` re-exports from engine-core, live in
`crates/epdf-engine/src/features/search/mod.rs`, which links PDFium, so this crate doesn't
re-export them.

Not ported: inputs outside the TypeScript's types, which crash it. A query without `text`, a slice
missing `matches`, and a `nextCursor` that is absent rather than `null` (the TypeScript loops on it
forever; serde reads it as `null`).

A request's query members go out in `SearchQuery`'s field order (`text`, `regex`, `matchCase`,
`wholeWord`, `matchDiacritics`, `ignoreWhitespace`), where the TypeScript's `{...query}` keeps the
caller's key order. Every vector writes `text` first and the flags in that order, so the strict
replay can't tell; a caller that orders the flags otherwise sends the same request in another
member order.

## Turns

Each TypeScript `await` is a plain `.await`, since the kernel's promises take JavaScript's turns.
Awaiting the `async` `collect` is `rt::returned`, and `search`'s `return session.run(...).catch(...)`
is `adopt`, the resolve-thenable job and its reaction. `scans_settle_on_the_typescripts_turns` pins
the counts with an immediate answer: `findAll` settles on turn 5 without a signal and 6 with one;
`search` commits its slice on 5, completes on 6 and settles on 9. Dropping the `rt::returned` in
`find_all` fails it. The two `close-races-*` vector cases pin the order a session and a `findAll`
settle in at close.

## Coverage and mutation testing

The statement, branch and mutant counts that stood here were measured on the pre-catch-up port and
are stale. Task 14 re-measures them.
