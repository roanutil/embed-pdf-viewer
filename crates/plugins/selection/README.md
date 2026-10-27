# epdf-plugin-selection

`packages/plugin/selection` in Rust, on the v3 controller contract: text selection over the engine's
text geometry, the pointer handler it registers with the interaction hub, selection handles, copy,
and the `canSelect` / `canCopy` twins. Every case in the three vector files replays identically
(`CARGO_BUILD_JOBS=6 cargo test -p epdf-plugin-selection`):

| File | Cases | Covers |
|---|---|---|
| `vectors/plugins/selection.json` | 34 | the handler through the hub, the gesture verbs, `select`, `selectPage`, `selectAll`, `readText` and `readTextInRange` with their batching and abort checks, the change, commit and clear events, registry and content invalidation, permissions, the `dragThreshold` setting |
| `vectors/plugins/selection-geometry.json` | 288 | `pageGeometryOf`, `pdfPointOf`, `pageQuadOf`, `pageTextSegmentOf`, the page-space text layout (with sweeps over hit tests, ranges, line merges, char maps and frame handedness), `sliceText`, the model's transitions |
| `vectors/plugins/selection-handles.json` | 43 | `selectionHandleGeom`, the handle drag session, the handle constants |

Geometry and text reach the plugin from the fake engine's response table: `page.text.layout`
answers a `$textLayout` marker holding a page-space `PageGeometrySnapshot` (Ruling 13.2), which
the plugin builds into its own `TextLayout`, and `page.text.get` answers a `PageTextSnapshot` with a
char map.

## Coverage

Stale since Task 13.15 re-ported the controller (refreshed with the coverage metadata in Task 14).
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

Annotation markup, redaction and the viewer host use the public contract and `/contract/host`.
Here those are one type, `SelectionCapability`, resolved with `SELECTION`: the settings
(`get_settings`, `update_settings`, `reset_settings`, `on_settings_changed`), the public methods
(`select`, `get_snapshot`, `is_selecting`, `read_text`, `on_changed`, `list_segments`, ...) and the
host ones (`ensure_loaded`, `begin_gesture_at`, `end_gesture`, `set_highlight_visible`, ...) sit
side by side. `SelectionHostCapability` is an alias for it. A public verb takes a `PageRef` or an
index (`impl Into<PageTarget>`), as the TypeScript's `PageRef | number`. Segments, endpoints and
anchors carry named quads (`epdf_geometry::Quad`) and page-space `Rect`s. `text_layout` is public
for search and redaction, which build the same layout (Ruling13.15). `harness::Selection` is public
so a downstream crate's tests can register it.

### Migrating from the pre-13.15 API

For annotation (Task 13.17), search (13.20), redaction (13.23), and the viewer host and `epdf-web`
(13.33).

| Old | New |
|---|---|
| engine path `page.geometry.read` (PDF space, `charStart`, `looseBox`, `flags`) | `page.text.layout`, answering `{"$textLayout": PageGeometrySnapshot<Page>}` or the bare page-space snapshot. 13.33 must route it in `translate.rs` and `JsDocumentHandle` |
| engine path `page.text.read` | `page.text.get` |
| `text_layout::{UprightGeometryRun, RotatedGeometryRun, PageGeometryGlyph{loose_box, flags, tight_box}}` | `PageGeometryRun<C>::{Upright{rect,start,font_size,glyphs}, Rotated{rect,start,rotation,ascent_flip,font_size,glyphs}}`, glyphs `{space, empty, loose, tight}`, `C` = `PageCoordinates` (default) or `PdfCoordinates` |
| `build_page_text_layout` + `text_glyph_at`/`expand_*`/`text_glyph_quad`/`text_segments_for_range` on PDF space | `create_text_layout(PageGeometrySnapshot) -> TextLayout` with `char_at`, `word_at`/`line_at` (`TextAt` point or index, `-> Option<text_layout::TextRange{start,count}>`), `segments`, `char_quad`. The y-up functions stay public on `PageTextLayout` |
| `slice_text_by_chars(snap, from, to)` | `slice_text(snap, start, count)` |
| `geometry::{SelectionPageGeometry, build_selection_page_geometry, content_point_to_pdf, to_content_segment, to_content_text_quad}` | removed. `text_layout::{page_geometry_of, pdf_point_of, page_text_segment_of}` |
| `SelectionSegment{quad: TextQuad, rect, advance}` | `PageTextSegment{quad: epdf_geometry::Quad, rect: Rect, advance: i32}` |
| `SelectionEndpoint.glyph_quad: TextQuad`, `SelectionHandleEndpoint.glyph_quad: TextQuad` | `Quad` (`upper_left`..`lower_right`) |
| `SnapshotPage.segments: Vec<_>` | `Rc<Vec<_>>` (iterates the same) |
| `page_ref_json` on contract types | kernel `PageRef` serde |
| `SelectionConfig { drag_threshold: Option<f64> }` | `{ drag_threshold, color: Option<Option<String>>, handles: Option<SelectionHandleConfig> }`, `to_json`/`from_json`. New `SelectionSettings`, `selection_defaults()`, `SelectionSettingsChanged`, and `get_settings`/`update_settings`/`reset_settings`/`on_settings_changed` |
| `ChangeOrigin`, `ChangeTrigger`, `.origin` on changed and cleared events | removed. `SelectionChangedEvent{range, pages}`, `SelectionClearedEvent{}` |
| `is_gesture_active()` | `is_selecting()` |
| `select_page`, `select_word_at`, `select_line_at`, `extend_to`, `list_segments`, `list_rects` take `PageRef` | take `impl Into<PageTarget>` (`PageRef` or `usize`; `PageTarget::Index(f64)`) |
| `GlyphPointer`, `SelectionAction`, `reduce_selection`, `SelectionState.{loaded, gesture_active}` | `GlyphPosition`; `model::{set_selection, clear_selection, set_highlight_hidden, set_selecting, content_changed_pages_of}`; `SelectionState{selection, segments, highlight_hidden, selecting}` |
| `create_selection_controller` (pub) | crate-private |
| `TextSelectHandlerOptions{drag_threshold: Option<f64>}` | `Option<Rc<dyn Fn() -> f64>>` |
| new | `selection_state`/`SelectionStateView`, `SelectionHandleEndpoints`, `selection_handle_endpoints_of`, `arm_selection_handle`/`ArmedSelectionHandle`, `SelectionHandleStage`/`selection_handle_view_of` |
| ffi `SelectionChangeTrigger`, `.trigger`, `SelectionClearedEvent`, `is_gesture_active` | removed; `SelectionEvent::Cleared` is a unit variant; `is_selecting`. Quads map onto `epdf_ffi::TextQuad` through `quad()` until 13.33 renames the mirror |

Known call sites: `annotation/src/tools/markup_bridge.rs:43-46` and `annotation/src/write/markup.rs:102-130` (segment quads become `Quad`), `redaction/src/write/marks.rs:78-97` (only `has_selection`, unchanged), `epdf-viewer-host/tests/translate.rs:2657` (uses the removed `UprightGeometryRun` and `char_start`), `epdf-viewer-host/src/lib.rs:440` and the ffi handle, and `epdf-web/src/adapters.rs:26`. Search (13.20) and redaction (13.23) build the same layout from `epdf_plugin_selection::text_layout` (Ruling13.15).

## Where the port differs in shape

`text_layout` is a second transcription of `engine-core`'s `text/layout.ts`, its page-space
wrapper `createTextLayout` (`pageSpace/text.ts`) and `sliceText` (`text/charmap.ts`). The first is
`crates/epdf-engine`'s (`src/features/text/layout.rs`, `src/page_space/text.rs`), and that crate
links PDFium, which the plugins' wasm32 build must not (Ruling13.15, the same call as
`annotation-core`'s `src/engine` and `form-core`). The cost is two copies of one algorithm to keep
in step. Two things hold them there: `selection-geometry.json` pins this copy against the
TypeScript, and `tests/engine_cross_check.rs` runs every page-space layout the selection vectors
carry, plus a shared-edge tie, empty text and rotated and mirrored runs, through both copies on
native hosts and asserts they agree within the vectors' one part in 10^12. The shared-edge
tie-break (`dx + dy < best - EDGE_EPSILON`, `text/layout.ts:362`) is in both. The engine is a
dev-dependency gated to non-wasm32 targets, so
`cargo tree -p epdf-web --target wasm32-unknown-unknown -e normal -i pdfium-sys` still prints
nothing.

The copies differ in one place, on input PDFium never produces. Glyph indices and range bounds
are `i64` here, because `selectAll` parks its open end at `Number.MAX_SAFE_INTEGER` and a caller
can start a range before the page. Runs whose `start`s don't tile the glyphs are read without
indexing past them in both copies (S2 here, the engine's since Task 14.3), where the TypeScript
reads `undefined` and throws a `TypeError`. The cross-check compares the vector layouts
`rotated-all-empty` and `char-start-past-the-glyphs` like the rest and pins that they are the two
that don't tile.

`read_text_in_range` slices UTF-16 code units, as `String.prototype.slice` does, so a
supplementary-plane character behind a char map comes out whole. Its page reads mirror the
TypeScript's turn count: `Promise.resolve` adopts an `AbortablePromise` as a thenable, which costs
one turn more than awaiting it, and `Promise.all` settles one turn after the last read.

The selection writes return `Result`. `select_word_at`, `select_line_at`, `extend_to` and
`select_page` throw `permission-denied` in the TypeScript, and the handler only calls them after
`can_select()` has answered true, so it drops those `Result`s. The `SelectionHandleTarget` impl on
the capability drops `extend_to`'s for the same reason: `begin_gesture_at` answers false first.

A `page.text.layout` answer that is neither a `$textLayout` marker nor a page-space snapshot reads
as a page with no geometry: the page is loaded and nothing on it hits. The TypeScript would hold
whatever the engine answered and throw when a method is called on it. No vector answers one.

The typed settings API only writes well-typed members, but a host can write any JSON through the
kernel's settings store (`kernel.settings_of`). A member of the wrong type reads as its default in
`get_settings` and `on_settings_changed`, where the TypeScript hands the value back, and the
change listener's event reports `[selection] malformed settings` once its listeners have run
(`a_malformed_setting_reads_as_its_default_and_is_reported` in `tests/api.rs`).

The two TypeScript tokens (the public one from the package root, the host one from
`/contract/host`) are the same runtime object with two static types. Rust has one token and one
type.

## Mutation testing

Stale since Task 13.15 rewrote `controller.rs`, `model.rs` and `text_layout.rs`; Task 14 reruns it.
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
