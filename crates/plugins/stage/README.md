# epdf-plugin-stage

`packages/plugin/stage` in Rust: the viewport lens. Camera, zoom, scroll, spreads, per-document
layout, motion over a host frame seam, responsive settings, PDF destinations, and the pan handler for
the interaction hub. All 182 cases in five vector files replay identically (`cargo test -p
epdf-plugin-stage`):

| File | Kind | Cases | Covers |
|---|---|---|---|
| `vectors/plugins/stage-destination.json` | function | 29 | `destinationToReveal` for every `/XYZ` and `/Fit*` kind, null and absent members, offset crops, rotation, `userUnit` (0 throws, absent is NaN) |
| `vectors/plugins/stage-pure.json` | function | 87 | `settingsEqual`, the registry constants, the motion laws, `boxOf`, `matchesQuery`, `mergeSettings`, `eqSetting`, `resolveResponsive`, `initialStageState`, `stageReducer` |
| `vectors/plugins/stage-capability.json` | scenario | 44 | placement and initial-view providers, every selector, the setters by effect class, zoom intents, fit-all, view rotation, wrapped grids, paged flow, spreads, RTL, navigation, bare and positioned reveal, the scroller, gestures, the double-tap ladder, responsive rules, two lenses, the page-less document |
| `vectors/plugins/stage-motion.json` | scenario | 15 | the rest detector, the navigation and zoom-anchored tweens, caught tweens, fling glide and bounce, the elastic release spring, all driven by `advance` |
| `vectors/plugins/stage-feeds.json` | scenario | 7 | the page-registry refit, the `goto` and `named` executors through the real actions plugin, the page-state feed, and the pan handler through the real interaction hub |

`tests/api.rs` covers what the vectors can't: lens identity (`same`), a frame that fires after its
document closed, and the `RuleWhen` and `PdfDestination` helpers.

The generators (`tooling/plugin-conformance/src/suites/stage-pure.ts` and `stage-plugin.ts`) reach
2,984 of the package's 2,990 statements and 722 of its 750 branches. What they can't reach is listed
under Coverage.

## The public API

`stage_plugin(StagePluginOptions)` builds the plugin. `STAGE` is the main lens's token and
`StageCapability` its capability, with the TypeScript's members in snake case (`visible_pages`,
`page_rect`, `go_to_page`, `reveal`, `scroll_to`, `zoom_around`, `double_tap_zoom`, ...). A second
lens is `StagePluginOptions { id, token, .. }`, as `stagePlugin({id, token})` is. `visible_pages()`
returns an `Rc<Vec<VisiblePage>>` that stays the same `Rc` until the camera, scene, viewport, ratio
or rest state changes, so a layer that compared it with `Object.is` can use `Rc::ptr_eq`.

The `/destination` entry is `destination_to_reveal(&PdfDestination, &PageLayout, Option<Rect>)`,
returning a `DestinationReveal { page_index, options }` for `reveal`. It returns
`Err(NonInvertible)` where the TypeScript throws, for a layout whose `userUnit` is 0. The contract
types link and search need are exported from the crate root: `RevealOptions`, `RevealAnchor`,
`RevealZoom`, `ScrollBehaviorKind`, `StageState`, `StageAction`, `StageSettings`,
`PartialSettings`, `StageViewState`, `Viewpoint`, `VisiblePage`. `create_scroll_handler(stage,
hub, options)` returns the `InteractionHandler` the surface binding registers, scoped to
`stage.lens_id()`.

Where the TypeScript throws, the verb returns `Err`. The only throw the capability can reach is a
`TypeError` (`Cannot read properties of undefined (reading 'x')`) from every verb that clamps
against the paged slice of a document with no pages: `set_camera`, `pan_by`, `zoom_around`,
`zoom_in`, `zoom_out`, `fling`, `end_gesture` after an elastic gesture, and `update` plus every
setter built on it when the change re-clamps. The cost is a `Result` on those methods for one
degenerate document. The page-less case in `stage-capability.json` pins seven of those throws.

## Host inputs

`capability.ts` reads two globals: `globalThis.requestAnimationFrame` and
`globalThis.cancelAnimationFrame`. They are the `Scheduler` trait here (`raf(callback) -> handle`,
`caf(handle)`), passed as `StageConfig::scheduler`. Rust has no ambient frame clock, so without one
the lens is frameless, which is what the TypeScript is under Node: navigation jumps, `fling` only
snaps into bounds, and the rest detector never runs. A handle is never 0; the capability reads 0 as
"nothing scheduled".

The viewport size and device pixel ratio were already inputs in the TypeScript (`setViewport`,
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
ref numbering matches the TypeScript's.

## Where the port differs in shape

The settings registry is the `SettingKey` enum in `SETTING_KEYS` order, and `for_each_setting!`
expands one arm per field wherever the TypeScript loops over the keys (merge, diff, effect lookup,
equality). A setting added to `StageSettings` without a row fails to compile, which is the
TypeScript's `Record<keyof StageSettings, ...>` guarantee. `eqSetting`'s one level of own-key
equality is `PartialEq` on the typed values, `NaN` unequal to itself included.

The two watches in `effects.ts` are written against `subscribe` rather than `EffectCtx::watch`,
because the page-state feed's selector resolves the capability and throws while the document
closes. The TypeScript reports that throw and leaves the watch's previous value alone, and so does
the port: `navigation-executors` in `stage-feeds.json` records the report on its close step.

Engine JSON reaches the stage carrying the kernel's `{"$undefined": true}` marker (a page without
`userUnit`, say). `without_undefined` drops those members before the page registry and destinations
deserialize, as a JavaScript read sees them.

A scheduled frame that fires after its document closed returns without reading state. The
TypeScript would read the removed slice; no vector closes a document mid-motion.

## Not ported

Page indexes are integers. `goToPage` and `reveal` take any number in the TypeScript, and a
fractional index lands as a fractional cursor. Here they take `isize` and the cursor is `usize`, so
a restored view with a negative cursor does not decode.

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

What the generators don't reach is unreachable from the public surface or doesn't port:

- `capability.ts:81-82`, the `globalThis.requestAnimationFrame` default. Node has none; Rust takes
  the `Scheduler` explicitly.
- The `ctx.document()` absent arms (`:102`, `:104`, `:173`, `:248`, `:1093`, `:1101`, `:1109`,
  `:1206`, `effects.ts:29`). The plugin is document-scoped, and the kernel keeps serving the staged
  meta while the document closes.
- `:235`, the `JSON.stringify` arm of the scene key: every object-valued scene setting has its own
  key function.
- `:280` and `:295`, `?? it.pages[0]`; `:804`, `:1112`, `:1367`, `:1414`, `:1448`, `:1482`,
  `:1491`, an empty scene after a non-empty page count; `:870`, `:876-878`, `:888-889`, `:1114`, a
  laid-out box without a registry row. The scene is built from the same registry and grouping, so
  each target is in its item and each box has its row.
- `effects.ts:59`, a null snapshot in the feed handler: `placed` is a one-way latch.
- `stage.plugin.ts:70` and `:85`, the executors' node-type guards, and `:87`, a missing stage in the
  named executor. The dispatcher calls an executor only for its own type, and the stage lives as
  long as its document.
- `reducer.ts:58-59`, the `default` arm. `StageAction` is an enum here.

## Mutation testing

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
