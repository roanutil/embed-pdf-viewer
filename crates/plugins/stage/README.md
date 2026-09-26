# epdf-plugin-stage

`packages/plugin/stage` in Rust: the viewport lens. Camera, zoom, scroll, spreads, per-document
layout, motion over a host frame seam, responsive settings, PDF destinations, the six change hooks,
and the pan handler for the interaction hub. It follows the v3 contract: `create()` builds the
controller, `connect` registers the navigation executors, pages are addressed by `PageRef`, and the
verbs carry the v3 names. All 190 cases in six vector files replay identically (`cargo test -p
epdf-plugin-stage`):

| File | Kind | Cases | Covers |
|---|---|---|---|
| `vectors/plugins/stage-destination.json` | function | 29 | `destinationToReveal` for every `/XYZ` and `/Fit*` kind, null and absent members, offset crops, rotation, `userUnit` (0 throws, absent is NaN) |
| `vectors/plugins/stage-pure.json` | function | 87 | `settingsEqual`, the registry constants, the motion laws, `boxOf`, `matchesQuery`, `mergeSettings`, `eqSetting`, `resolveResponsive`, `initialStageState`, `stageReducer` |
| `vectors/plugins/stage-capability.json` | scenario | 48 | placement and initial-view providers (withdrawn ones included), every read, the setters by effect class, zoom intents, `zoomTo` with a bare level, `zoomBy`, fit-all, view rotation, wrapped grids, paged flow, spreads, RTL, navigation by index and by `PageRef`, bare and positioned reveal, `revealRect`, `resetSettings`, the scroller, gestures, the double-tap ladder, responsive rules, two lenses, the page-less document |
| `vectors/plugins/stage-motion.json` | scenario | 15 | the rest detector, the navigation and zoom-anchored tweens, caught tweens, fling glide and bounce, the elastic release spring, all driven by `advance` |
| `vectors/plugins/stage-feeds.json` | scenario | 8 | the page-registry refit, the `goto` and `named` executors through the real actions plugin, the page-state feed (a cursor past the last page included), and the pan handler through the real interaction hub |
| `vectors/plugins/stage-events.json` | scenario | 3 | the six change hooks behind the diffing dispatch, `onMotionEnded` for tweens, flings and `stopMotion`, and the camera hook's object identity on an unbounded landing |

`tests/api.rs` covers what the vectors can't: lens identity (`same`), frames that fire after their
document closed, and the `RuleWhen` and `PdfDestination` helpers.

The generators (`tooling/plugin-conformance/src/suites/stage-pure.ts` and `stage-plugin.ts`) reach
3,536 of the package's 3,542 statements and 774 of its 804 branches, counted over the stage vector
files alone. What they can't reach is listed under Coverage.

## The public API

`stage_plugin(StagePluginOptions)` builds the plugin. `STAGE` is the main lens's token and
`StageCapability` its capability, with the TypeScript's members in snake case (`get_camera`,
`list_visible_pages`, `get_page_frame`, `go_to_page`, `go_to_page_index`, `reveal`, `reveal_index`,
`zoom_to`, `update_settings`, `on_page_changed`, ...). The TypeScript types the same object twice,
as `StageCapability` and the wider `StageHostCapability`; Rust has one type, and the host-only
members (`set_viewport_size`, `begin_gesture`, `fling`, `zoom_around`, `get_scroll_metrics`,
`provide_initial_view`, `refit`, `get_lens_id`, ...) sit on it with the rest. A second lens is
`StagePluginOptions { id, token, .. }`, as `stagePlugin({id, token})` is. `list_visible_pages()`
returns an `Rc<Vec<VisiblePage>>` that stays the same `Rc` until the camera, scene, viewport, ratio
or rest state changes, so a layer that compared it with `Object.is` can use `Rc::ptr_eq`.

`PageInfo` is the kernel's registry row as JSON (`serde_json::Value`), because the kernel keeps the
page registry that way. `get_current_page`, `list_current_item_pages` and the page hook hand those
rows back. `zoom_to` takes a `ZoomTarget`, the TypeScript's `number | ZoomSpec`. The hooks return
the kernel's `Unsubscribe`, and so does `provide_initial_view`.

The `/destination` entry is `destination_to_reveal(&PdfDestination, &PageLayout, Option<Rect>)`,
returning a `DestinationReveal { page_index, options }` for `reveal_index`. It returns
`Err(NonInvertible)` where the TypeScript throws, for a layout whose `userUnit` is 0. The contract
types link and search need are exported from the crate root: `RevealOptions`, `RevealAnchor`,
`RevealZoom`, `ScrollBehaviorKind`, `StageState`, `StageAction`, `StageSettings`,
`PartialSettings`, `StageViewState`, `Viewpoint`, `VisiblePage`. `create_scroll_handler(stage,
hub, options)` returns the `InteractionHandler` the surface binding registers, scoped to
`stage.get_lens_id()`.

Where the TypeScript throws, the verb returns `Err`. The only throw the capability can reach is a
`TypeError` (`Cannot read properties of undefined (reading 'x')`) from every verb that clamps
against the paged slice of a document with no pages: `set_camera`, `pan_by`, `zoom_around`,
`zoom_in`, `zoom_out`, `zoom_by`, `fling`, `end_gesture` after an elastic gesture, and
`update_settings` plus every setter built on it when the change re-clamps. The cost is a `Result`
on those methods for one degenerate document. The page-less case in `stage-capability.json` pins
seven of those throws.

## Host inputs

`services/scheduler.ts` reads two globals: `globalThis.requestAnimationFrame` and
`globalThis.cancelAnimationFrame`. They are the `Scheduler` trait here (`raf(callback) -> handle`,
`caf(handle)`), passed as `StageConfig::scheduler`. Rust has no ambient frame clock, so without one
the lens is frameless, which is what the TypeScript is under Node: navigation jumps, `fling` only
snaps into bounds, and the rest detector never runs. A handle is never 0; the capability reads 0 as
"nothing scheduled".

The viewport size and device pixel ratio were already inputs in the TypeScript (`setViewportSize`,
`setDevicePixelRatio`) and stay that way.

## Harness shims

`src/harness.rs` mirrors four shims in `stage-plugin.ts`. A config with `frames: true` gets
`FrameScheduler`: 16 ms timers on the runtime clock, stamped with `rt::now()`, handles from 1, the
same as the generator's scheduler over `@sinonjs/fake-timers`. `lens: name` registers the plugin
under that id with a token of that name. A responsive rule's `when` may be a `$fn` spec, a scripted
predicate that logs the box it is asked about. The `stage-scroll` factory is the surface binding's
half of interaction: a document plugin that registers the scroll handler with the hub, as the React
and Angular stages do. In `stage-pure.ts`, `resolveResponsive` rules may carry `when: {"$returns":
v}` for the predicate `() => v`, and `stageReducer` reports `{state, same}` so the vectors pin the
reference identity the store notifies on (`None` in Rust).

A `VisiblePage` prints its transform's six methods as `$ref`s, registered in member order, so the
ref numbering matches the TypeScript's. The cases were authored against the v2 names, and
`stageStep` in `stage-plugin.ts` emits them against the v3 surface (`goToPage` with an index becomes
`goToPageIndex`, a per-setting read becomes `getSettings`, `pages` reads the kernel's
`documents.listPages`), so the vectors only ever call v3 members. A registry row is printed with its
`undefined` members dropped, as JavaScript reads it.

## Where the port differs in shape

The TypeScript controller is a composition root over areas (`services/`, `read/`, `camera/`,
`navigation/`, `settings/settings.ts`, `view/`), each a closure that returns an `api` slice for
`composeApi`. The areas call each other in a cycle: settings changes re-place through arrivals,
arrivals read the scene, the view lifecycle drives both. Here each area is an `impl
StageCapability` block in a file of the same name over one shared `Inner`, so the cycle needs no
late binding. `settings/settings.ts` is `settings/area.rs`, because `settings::settings` would trip
clippy's `module_inception`.

`services/events.ts` wraps `ctx.dispatch` so every write compares the state before and after and
fires the hooks. `StageCapability::dispatch` does the same. The camera hook fires on
`before.camera !== after.camera`, an object comparison, and the port keeps that: a CAMERA write
always carries a new camera except `setCam(cam())` on an unbounded stage, where `clampCamera`
returns its input. `set_cam_current` marks that write, and no camera event fires for it. The
`an-unbounded-landing-keeps-the-camera` case pins it. The viewport hook fires on every VP write for
the same reason.

The settings registry is the `SettingKey` enum in `SETTING_KEYS` order, and `for_each_setting!`
expands one arm per field wherever the TypeScript loops over the keys (merge, diff, effect lookup,
equality). A setting added to `StageSettings` without a row fails to compile, which is the
TypeScript's `Record<keyof StageSettings, ...>` guarantee. `eqSetting`'s one level of own-key
equality is `PartialEq` on the typed values, `NaN` unequal to itself included.

The two watchers in `sync/watchers.ts` are written against `subscribe` rather than
`EffectCtx::watch`, because the page-state feed's selector resolves the capability and throws while
the document closes. The TypeScript reports that throw and leaves the watch's previous value alone,
and so does the port: `navigation-executors` in `stage-feeds.json` records the report on its close
step.

Engine JSON reaches the stage carrying the kernel's `{"$undefined": true}` marker (a page without
`userUnit`, say). `without_undefined` drops those members before the page registry and destinations
deserialize, as a JavaScript read sees them.

A frame scheduled before its document closed still fires. Under v3 the lease answers the last state
after close, so the frame reads it, as the TypeScript's does, and its dispatches are dropped with
one `instance-closed` report. The pre-v3 port returned early instead.

## Not ported

Page indexes are integers. `goToPageIndex` and `revealIndex` take any number in the TypeScript, and
a fractional index lands as a fractional cursor. Here they take `isize` and the cursor is `usize`,
so a restored view with a negative cursor does not decode.

`undefined` and `null` settings are absent. `stagePlugin({padding: undefined})` spreads the
`undefined` over the default and the state loses its padding; the typed config keeps the default.
`mergeSettings(s, {zoom: null})` lands a null zoom, and `settingsEqual` and `eqSetting` compare
nulls; none of those values is a setting a viewer can hold, so the cases were dropped from the
suite. For the same reason a zoom spec carrying two keys (`{level, mode}`) decodes as its first,
and `eqSetting` on nested objects is not ported.

`RevealOptions.anchor: null` reads as absent (a bare reveal); the TypeScript counts a present null
as positioned. `GoToOptions.arrivalAlign: {y: undefined}` reads as absent; the TypeScript spreads
the `undefined` and places at NaN. A goto node whose destination has an unknown `kind` fails the
executor here, where the TypeScript's switch falls through to a bare smooth reveal.

## Coverage

Six statements and 30 branches are unreached. Each is unreachable from the public surface or
doesn't port:

- `services/scheduler.ts:18` (and branch `17:43`), the `globalThis.requestAnimationFrame` default.
  Node has none; Rust takes the `Scheduler` explicitly.
- The `ctx.document()` absent arms: `services/scene.ts:34`, `:36`, `:105`, `:180`,
  `read/pages.ts:157`, `:193`, `navigation/arrive.ts:99`, `:180`, `:181`, `view/lifecycle.ts:60`,
  `sync/watchers.ts:31`. The plugin is document-scoped, and the kernel keeps serving the staged meta
  while the document closes.
- `services/scene.ts:167`, the `JSON.stringify` arm of the scene key: every object-valued scene
  setting has its own key function.
- A laid-out box without a registry row, or a row without its box: `read/pages.ts:30`, `:36-38`,
  `:49`, `:162`, `services/scene.ts:218`, `:233`. The scene is built from the same registry and
  grouping, so each target is in its item and each box has its row.
- An empty scene after a non-empty page count: `read/pages.ts:160`, `navigation/reveal.ts:94`,
  `:128`, `:162`, `:171`, `navigation/zoom.ts:65`.
- `sync/watchers.ts:64`, a null snapshot in the feed handler: `placed` is a one-way latch.
- `stage.plugin.ts:87` and `:102`, the executors' node-type guards, and `:104`, a missing stage in
  the named executor. The dispatcher calls an executor only for its own type, and the stage lives as
  long as its document.
- `model.ts:124-125`, the reducer's `default` arm. `StageAction` is an enum here.

Two TypeScript behaviours are pinned that look unintended. `applyViewState` writes the view's
cursor without clamping it, so a view saved on a longer document leaves the cursor past the last
page: `getCurrentPage` answers null, the page hook reports `page: null`, and the feed reports a null
current page (`a-cursor-past-the-last-page`, `change-hooks`). And the camera hook's object identity
means a bounded re-clamp that moves nothing still fires a camera event, while the same re-clamp on an
unbounded stage fires none.

## Mutation testing

Pre-v3, rerun pending. The numbers and line references below are from the pre-v3 port, whose
`capability.rs` is now split across the area files; the code each item describes still exists.

`cargo mutants -p epdf-plugin-stage --test-package epdf-plugin-stage -j 4` tests 973 mutants: 859
caught, 4 caught by timeout (a binary search in `item_index_of_page` or a watch that never settles),
78 unviable, 32 missed. Two rounds of new cases (commits `9855c1579` and `fd54cc2fe`) took the
misses from 102. Every remaining one is equivalent:

- `lib.rs:131`, `page_count - 1` to `+ 1` or `/ 1` for `LastPage`: `go_to_page` clamps both to the
  last page.
- `capability.rs:292`, `no_frame` always false: an all-zero frame divides to the same zeros, and
  the frame key only changes spelling while it stays constant.
- `capability.rs:327`, the `!falsy(px)` guard: a `{px}` gap of 0 gives 0 either way, and a NaN one
  can't be written into a vector, because the kernel prints an action through `serde_json`, which
  writes NaN as `null` where the TypeScript trace has `{"$number": "NaN"}`.
- `capability.rs:461`, the paged slice's `len - 1` bound: `item_index_of_page` never exceeds it.
- `capability.rs:554`, `>` to `>=` and `<` to `<=` in the nearest reveal: at equality both branches
  return the same camera.
- `capability.rs:890` and `:938`, `item_count > 0` to `>= 0` in the zoom-anchored tween: the
  frameless path only runs over a scene with items, and a registry change refits, which cancels the
  tween before a frame can meet an empty scene.
- `capability.rs:1036`, `> 1e-6` to `>= 1e-6`: exact equality of a float difference.
- `capability.rs:1231`, `:1236` and `:1237` (six mutants), the registry fallbacks in
  `pon_for_index` and `unrotated_points`, unreachable as their TypeScript lines are.
- `capability.rs:1412` (three) and `serde_str` at `:1435-1436` (three): the scene cache is
  invalidated on every write of a scene setting, and the scene key carries them all, so each is a
  second guard for the other.
- `capability.rs:1709` and `:1785` (three): `gesture_elastic` is only true while a gesture is open,
  and `gesture_raw` is only read by an elastic pan.
- `capability.rs:1781`, `after.item_count > 0` to `>= 0`: an empty scene after the zoom means an
  empty one before it, so there is no focal point.
- `capability.rs:1876` and `:1880` (three): the double-tap ladder's comparisons at exact equality of
  float products.
- `scroll_handler.rs:37` (two), the handler id: the hub never reads it.
