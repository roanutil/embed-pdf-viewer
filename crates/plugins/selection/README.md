# epdf-plugin-selection

`packages/plugin/selection` in Rust, on the v3 controller contract: text selection over the engine's
text geometry, the pointer handler it registers with the interaction hub, selection handles, copy,
and the `canSelect` / `canCopy` twins. Every case in the three vector files replays identically
(`CARGO_BUILD_JOBS=6 cargo test -p epdf-plugin-selection`):

| File | Cases | Covers |
|---|---|---|
| `vectors/plugins/selection.json` | 33 | the handler through the hub, the gesture verbs, `select`, `selectPage`, `selectAll`, `readText` and `readTextInRange` with their batching and abort checks, the change, commit and clear events with their origins, registry and content invalidation, permissions, the `dragThreshold` option |
| `vectors/plugins/selection-geometry.json` | 281 | the coordinate seam, the engine-core text layout (with sweeps over hit tests, ranges, line merges, char maps and frame handedness), `sliceTextByChars`, the reducer |
| `vectors/plugins/selection-handles.json` | 43 | `selectionHandleGeom`, the handle drag session, the handle constants |

Geometry and text reach the plugin from the fake engine's response table in the `engine-core`
shapes (`PageGeometrySnapshot` with upright and rotated runs, `PageTextSnapshot` with a char map).

## Coverage

The generators reach 1,446 of the package's 1,447 statements and 287 of its 296 branches. None of
the 9 missing branches, nor the one statement, is reachable from the public surface:

- `model.ts:68` and `:69`, the `default` arm for an action outside the union.
- `controller.ts:229` and `:475`, a `ponAtIndex` miss inside a span. The span's bounds are the
  `index` fields of pages the registry holds, and no registry names an index past its page count.
- `controller.ts:379`, the same miss for `select`'s focus page, which `assertPageRef` just checked.
- `controller.ts:255`, `segments[pon]?.length ?? 0` over keys taken from `segments` itself.
- `controller.ts:316`, an endpoint whose page has segments but no geometry. Segments are only
  written from geometry, and both are dropped together.
- `controller.ts:391`, `:507` and `:602`, the `ctx.document()` null arms. A document plugin's
  controller only exists while its document does.

## The downstream API

Annotation markup and redaction use the `/contract/host` and `/internal` entries. Here those are
one type, `SelectionCapability`, resolved with `SELECTION`: the public methods (`select`,
`get_snapshot`, `read_text`, `on_changed`, `on_committed`, `list_selected_pages`, `list_segments`,
...) and the host ones (`ensure_loaded`, `begin_gesture`, `begin_gesture_at`, `extend_to`,
`end_gesture`, `set_highlight_visible`, ...) sit side by side. `SelectionHostCapability` is an alias
for it. `create_selection_controller`, `create_text_select_handler`, the model and the seam
(`geometry`) are public, as `/internal` exports them. `harness::Selection` is public so a
downstream crate's tests can register it. Those two downstream crates are not on v3 yet and do not
build against this one.

## Where the port differs in shape

`text_layout` is a second transcription of `engine-core`'s `text/layout.ts` and the
`sliceTextByChars` half of `text/charmap.ts`. That is still true under v3, and for the same reason:
`crates/epdf-engine` has the first copy in `src/features/text/layout.rs`, and that crate depends on
`pdfium` unconditionally, which the plugin layer must not link. The cost is two Rust copies of one
algorithm to keep in step. `selection-geometry.json` pins this one against the TypeScript,
including every run variant (rotated, mirrored, sheared, all-empty, RTL). Glyph indices and range
bounds are `i64`, because `selectAll` parks its open end at `Number.MAX_SAFE_INTEGER` and `select`
accepts a negative start.

`read_text_in_range` slices UTF-16 code units, as `String.prototype.slice` does, so a
supplementary-plane character behind a char map comes out whole. Its page reads mirror the
TypeScript's turn count: `Promise.resolve` adopts an `AbortablePromise` as a thenable, which costs
one turn more than awaiting it, and `Promise.all` settles one turn after the last read.

The selection writes return `Result`. `select_word_at`, `select_line_at`, `extend_to` and
`select_page` throw `permission-denied` in the TypeScript, and the handler only calls them after
`can_select()` has answered true, so it drops those `Result`s. The `SelectionHandleTarget` impl on
the capability drops `extend_to`'s for the same reason: `begin_gesture_at` answers false first.

A page whose crop box has no area has no selection geometry here. The TypeScript's `pageGeometry`
throws on it, and the throw would surface from `isOverText`, a hover, or `select`. No vector opens
such a page.

The two TypeScript tokens (the public one from the package root, the host one from
`/contract/host`) are the same runtime object with two static types. Rust has one token and one
type.

## Mutation testing

Not rerun for the v3 port. The last run, before it, caught 678 of 722 viable mutants with 6 timeouts.
`capability.rs`, `reducer.rs` and the old handler are gone, so the survivors recorded there no
longer describe anything. What survived in code that is still here, pre-v3 and rerun pending:

- `text_layout.rs`, equivalent, 16: `:124` (`1 ^ 2` is `1 | 2`), `:228:77` (two ascents within half
  a degree never have a zero dot product), `:439` (restarting the forward line walk at the anchor
  re-reads the anchor itself), `:523` twice (with finite boxes `left` is infinite exactly when
  nothing accumulated), `:551` and `:563` (the `hi < lo` guard and an empty `s..=e` loop catch what
  the mutated condition lets through), `:555` twice (`hi` is clamped per run by `run_end`), `:576`
  three times and `:578` (`prev_right` is finite whenever a glyph has accumulated, and a zero
  average width never splits), `:582` (doubling the centre instead of halving it keeps
  `last_x >= first_x`), `:742` twice (text offsets are never negative).
- The run-rect prefilter in `textGlyphAt`, 4 (`text_layout.rs:350` to `:353`). The per-glyph test
  that follows repeats it with boxes inside the run rect, so loosening it only costs time.
- Sign-only arithmetic in `frame_for_run`, 7 (`text_layout.rs:223` to `:228`). These survived
  because no fixture sat where a mutated product flips sign. The `handedness-*` cases in
  `selection-geometry.json` are that fixture now: sixteen sheared cells whose ascent edge is within
  8 degrees of the baseline, at baselines of 30, 100, 200 and 300 degrees and both handednesses,
  where the two terms of the cross product have opposite signs. Whether they kill all 7 waits on the
  rerun.
- `text_layout.rs:709`, the binary search midpoint, 1. `lo * hi >> 1` still lands on the right
  anchor for maps of up to four anchors, which is all the vectors carry.
- `text_layout.rs:693`, 1: `min(top) / max(bottom)` for the overlap height happens to agree with
  the subtraction on every fixture pair's side of the 0.5 and 0.8 thresholds.
- `handles.rs:131`, 1, `rotation.abs() < 0.05` against `<=`: no fixture tilts by exactly 0.05°.
