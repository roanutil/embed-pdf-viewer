# epdf-plugin-selection

`packages/plugin/selection` in Rust: text selection over the engine's text geometry, the pointer
handler it registers with the interaction hub, selection handles, copy, and the `canSelect` /
`canCopy` twins. Every case in the three vector files replays identically
(`cargo test -p epdf-plugin-selection`):

| File | Cases | Covers |
|---|---|---|
| `vectors/plugins/selection.json` | 27 | the handler through the hub, the gesture verbs, `select`, `selectAll`, `readText` and its batching, registry and content invalidation, permissions |
| `vectors/plugins/selection-geometry.json` | 263 | the coordinate seam, the engine-core text layout (with sweeps over hit tests, ranges, line merges and char maps), `sliceTextByChars`, the reducer |
| `vectors/plugins/selection-handles.json` | 43 | `selectionHandleGeom`, the handle drag session, the handle constants |

Geometry and text reach the plugin from the fake engine's response table in the `engine-core`
shapes (`PageGeometrySnapshot` with upright and rotated runs, `PageTextSnapshot` with a char map).

The generators reach 1,324 of the package's 1,325 statements and 260 of its 277 branches. None of
the 17 missing branches is reachable from the public surface:

- `reducer.ts:34`, the `default` arm for an action outside the union.
- `capability.ts:90`, `:91`, `:96`, `:184`, `:366`, `:385`, `:431` and `:455`, the null arms of
  `ctx.doc` and `ctx.document()`. The plugin is document-scoped, so while its capability can be
  resolved both are present.
- `capability.ts:197` and `:485`, `segments[pon]?.length ?? 0` over keys taken from `segments`
  itself.
- `capability.ts:269`, `:356`, `:400` and `:407`, lookups of a page index the registry just
  produced, or of an endpoint page the store listener has already cleared the selection for.
- `capability.ts:145`, an endpoint whose page has segments but no geometry. Segments are only
  written from geometry, and both are dropped together.
- `capability.ts:254`, `recompute` with no selection. Every caller checks first.

## The downstream API

Annotation markup and redaction use the `/contract/host` and `/internal` entries. Here those are
one type, `SelectionCapability`, resolved with `SELECTION`: the public methods (`select`,
`snapshot`, `read_text`, `on_change`, `on_commit`, `selected_pages`, `segments_for_page`, ...) and
the host ones (`ensure_page`, `begin_at`, `extend_to`, `end`, `set_highlight_visible`, ...) sit side
by side. `SelectionState`, `SelectionAction`, `create_selection_capability`,
`create_text_select_handler`, the reducer and the seam (`geometry`) are exported as `/internal`
exports them. `harness::Selection` is public so a downstream crate's tests can register it.

## Where the port differs in shape

`text_layout` is a second transcription of `engine-core`'s `text/layout.ts` and the
`sliceTextByChars` half of `text/charmap.ts`. `crates/epdf-engine` has the first, but that crate
links PDFium and the plugin layer doesn't. The cost is two Rust copies of one algorithm to keep in
step; `selection-geometry.json` pins this one against the TypeScript, including every run variant
(rotated, mirrored, sheared, all-empty, RTL). Glyph indices and range bounds are `i64`, because
`selectAll` parks its open end at `Number.MAX_SAFE_INTEGER` and `select` accepts a negative start.

`read_text` slices UTF-16 code units, as `String.prototype.slice` does, so a supplementary-plane
character behind a char map comes out whole. Its page reads mirror the TypeScript's turn count:
`Promise.resolve` adopts an `AbortablePromise` as a thenable, which costs one turn more than
awaiting it, and `Promise.all` settles one turn after the last read.

A page whose crop box has no area has no selection geometry here. The TypeScript's `pageGeometry`
throws on it, and the throw would surface from `isOverText`, a hover, or `select`. No vector opens
such a page.

The two TypeScript tokens (the public one from the package root, the host one from
`/contract/host`) are the same runtime object with two static types. Rust has one token and one
type.

## Mutation testing

`cargo mutants -p epdf-plugin-selection --test-package epdf-plugin-selection` catches 678 of 722
viable mutants, and 6 more time out (loops that stop terminating). The first run missed 119; the
sweeps in `selection-geometry.json` and five scenario cases (`drag-threshold`, `handler-priority`,
`backward-drag-on-one-page`, `endpoints-follow-their-segment`, an unknown start page) came out of
them, along with `tests/api.rs`, which drives a handle drag through the capability's
`SelectionHandleTarget` impl. The 38 that survive:

- Equivalent, 23. `capability.rs:136` (any initial revision other than 0 behaves the same),
  `:212` (a derived-geometry cache miss rebuilds the same geometry), `:309` (the interior `to`
  bound is clamped by `textSegmentsForRange` anyway), `handler.rs:58` (the hub never reads a
  handler's `id`), `reducer.rs:6` (`Default` builds the same state), `text_layout.rs:124`
  (`1 ^ 2` is `1 | 2`), `:228:77` (two ascents within half a degree never have a zero dot
  product), `:439` (restarting the forward line walk at the anchor re-reads the anchor itself),
  `:523` twice (with finite boxes `left` is infinite exactly when nothing accumulated), `:551` and
  `:563` (the `hi < lo` guard and an empty `s..=e` loop catch what the mutated condition lets
  through), `:555` twice (`hi` is clamped per run by `run_end`), `:576` three times and `:578` (`prev_right`
  is finite whenever a glyph has accumulated, and a zero average width never splits), `:582`
  (doubling the centre instead of halving it keeps `last_x >= first_x`), `:742` twice (text
  offsets are never negative).
- The run-rect prefilter in `textGlyphAt`, 4 (`text_layout.rs:350` to `:353`). The per-glyph test
  that follows repeats it with boxes inside the run rect, so loosening it only costs time.
- Sign-only arithmetic in `frame_for_run`, 7 (`text_layout.rs:223` to `:228`). The handedness and
  frame-match tests read only the sign of these products, and no fixture sits where a mutated
  product flips it. A rotated fixture close to the handedness boundary would pin them.
- `text_layout.rs:709`, the binary search midpoint, 1. `lo * hi >> 1` still lands on the right
  anchor for maps of up to four anchors, which is all the vectors carry.
- `text_layout.rs:693`, 1: `min(top) / max(bottom)` for the overlap height happens to agree with
  the subtraction on every fixture pair's side of the 0.5 and 0.8 thresholds.
- `capability.rs:590`, 1, the `si < 0 || ei < 0` guard in `read_text`, which is unreachable (see
  the coverage list above).
- `handles.rs:124`, 1, `rotation.abs() < 0.05` against `<=`: no fixture tilts by exactly 0.05°.
