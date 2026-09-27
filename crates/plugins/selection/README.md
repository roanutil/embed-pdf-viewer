# epdf-plugin-selection

`packages/plugin/selection` in Rust, on the v3 controller contract: text selection over the engine's
text geometry, the pointer handler it registers with the interaction hub, selection handles, copy,
and the `canSelect` / `canCopy` twins. Every case in the three vector files replays identically
(`CARGO_BUILD_JOBS=6 cargo test -p epdf-plugin-selection`):

| File | Cases | Covers |
|---|---|---|
| `vectors/plugins/selection.json` | 34 | the handler through the hub, the gesture verbs, `select`, `selectPage`, `selectAll`, `readText` and `readTextInRange` with their batching and abort checks, the change, commit and clear events with their origins, registry and content invalidation, permissions, the `dragThreshold` option |
| `vectors/plugins/selection-geometry.json` | 290 | the coordinate seam, the engine-core text layout (with sweeps over hit tests, ranges, line merges, char maps and frame handedness), `sliceTextByChars`, the reducer |
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

`scripts/mutants.sh -p epdf-plugin-selection --test-package epdf-plugin-selection --file <file>`,
file by file over every module but `harness.rs`, catches 699 of 716 viable mutants (693 caught, 6
timeouts; 79 unviable). Sixteen of those are killed by new cases, each checked by applying the
mutant by hand; one of them, `(lo + hi)` to `(lo * hi)` in `anchor_at_or_before`, now loops forever
instead of passing.

- `vertical-baselines`: `*` to `/` in `frame_for_run`'s handedness dot (`text_layout.rs:228:66`).
  At exactly 90 and 270 degrees each ascent has `y = 0`, so the mutant divides zero by zero.
- `on-the-left-tolerance-edge` and `on-the-right-tolerance-edge`: the three boundary mutants of the
  run-rect prefilter in `text_glyph_at` (`:350` twice, `:351`). `run-rect-prefilters-the-glyph`,
  a run rect that misses its own glyph, kills the `||` to `&&` at `:353`.
- `emojiRow` in the char-map sweep: five anchors, so the binary search takes a third step (`:709`).
- `negative-text-offset`: both mutants of `len + v` in `slice_utf16` (`:742`).
- `zeroLead` in the layout sweep: `avg > 0.0` to `>=` (`:578`).
- `char-start-past-the-glyphs`: both mutants of the `len - 1` clamp in `text_segments_for_range`
  (`:555`), which then index past the last glyph.
- `rtl-near-the-float-limit`: `/ 2.0` to `* 2.0` for a glyph centre (`:582`), which overflows both
  centres to infinity and flips the advance.
- `a-crop-change-re-derives-the-seam`: `==` to `!=` in the seam memo (`controller.rs:301`).
- An empty `readTextInRange` on a page whose text was never read: `||` to `&&` at
  `controller.rs:808`, which reads the page anyway.
- `handler-priority` now listens to `onGestureStarted`: both replacements of the handler's `id`
  (`handler.rs:71`).

The old README hoped the rotated handedness fixture would kill the seven sign-only mutants in
`frame_for_run`. It killed four. Of the three the run left, `vertical-baselines` kills one and the
other two are equivalent (below).

The 17 survivors:

`src/text_layout.rs`, 12:

- `:124`, `|` to `^` in `is_boundary`: `1 ^ 2` is `1 | 2`.
- `:225:41` and `:225:63`, `* sign` to `/ sign` for the ascent. `sign` is 1 or -1, and dividing by
  either is exact.
- `:228:77`, `> 0.0` to `>=` in the handedness dot. It only counts when the baselines are within
  the frame tolerance of parallel, so the ascents are near parallel or antiparallel and their dot
  is near 1 or -1, never 0.
- `:439`, `ri + 1` to `ri * 1`: the forward line walk restarts at the anchor. A zero anchor is
  skipped, a zero-height one breaks at once just as it breaks on the next run, and any other
  overlaps itself and sets `to` to what it already was.
- `:523` twice, `>` to `>=` and `&&` to `||` in `flush`. With finite boxes `left` is infinite
  exactly when nothing accumulated. JSON cannot carry an infinite coordinate, and the snapshot
  decoder reads plain JSON numbers.
- `:551`, `||` to `&&`, and `:563`, `||` to `&&`. What the mutated guard lets through meets the
  `hi < lo` return or an empty `s..=e` loop.
- `:576` three times: `char_count > 0` to `>=`, `&&` to `||`, and `prev_right > -inf` to `>=`.
  `prev_right` is `-inf` exactly when nothing accumulated or the last glyph's right edge is `-inf`;
  in that second case the width sum is `-inf` or NaN, so `avg > 0.0` fails and nothing splits.

`src/controller.rs`, 3:

- `:209:73`, the `-1` revision default to `1`. `connect` runs during bring-up, after the kernel
  staged the document's meta at revision 0, so `revision()` is never `None` there.
- `:427:86` twice, `len - 1` to `len + 1` or `len / 1` for an interior page's last glyph. The count
  it feeds is clamped to the last glyph inside `text_segments_for_range`.

`src/handles.rs:131`, `rotation.abs() < 0.05` to `<=`. The rotation is `degrees - 90` with the
degrees near 90, so it is a multiple of 2^-46, and 0.05 is not; the two comparisons never differ.

`src/model.rs:78`, `initial_selection_state()` replaced with `Default::default()`. Its body is
`SelectionState::default()`.
