# epdf-plugin-link

`packages/plugin/link` in Rust, on the v3 controller shape. All 27 cases replay identically
(`cargo test -p epdf-plugin-link`):

| File | Kind | Cases | Covers |
|---|---|---|---|
| `vectors/plugins/link.json` | scenario | 18 | the stand-alone source (per-page reads, joined in-flight loads, retries after a refused read, malformed lists, refetch on annotation events, `onLoaded`), the annotation-backed source, hit testing and lookup, `listAllLinks` and `isLoaded`, `resolve` and `getLabel`, the `link-nav` behavior and `isNavigationEngaged`, and `activate` and `activateAt` with no stage, with a stage, and through the action engine |
| `vectors/plugins/link-functions.json` | function | 9 | `linkReducer` and `initialLinkState` |

The scenarios run the real interaction, annotation, actions and stage adapters beside this one, in
the combinations a deployment ships: viewer-only, annotation-backed, with a stage, and all of them.

## The API

`link_plugin(LinkConfig)` builds the plugin, `LINK` is its token and `LinkCapability` its
capability, which is also `LinkHostCapability`. The members are the TypeScript's in snake case:
`list_links`, `get_link`, `get_link_at`, `list_all_links`, `ensure_loaded`, `is_loaded`, `resolve`,
`activate`, `activate_at`, `get_label`, `on_activated`, `on_loaded` and `is_navigation_engaged`.
`LinkNavBehavior` is the behavior `connect` registers with the annotation plugin, and
`load_links_page` and `project_links` are the stand-alone source.

`activate` and `activate_at` take a `LinkInput`, a bare `PdfLinkTarget` or a whole `Link`, where the
TypeScript tests `'target' in value && 'bounds' in value`. `LinkActivateContext` keeps each member
as present-or-absent JSON, because activation spreads the context over the link's own `activate`
and `ref` and a present `null` wins (`activate-dispatches-through-actions`). `get_label` reads the
target's `kind` as JSON, so any shape gets a label.

`resolve`, `activate` and `activate_at` return `Err` where the TypeScript throws: a goto to a page
whose `userUnit` is 0 (`Mat2D.invert: non-invertible matrix (det = 0)`). `activate` also errs on a
`Link` whose `target` does not decode as a `PdfLinkTarget`.

## Host input and the harness shims

A throwing `onActivated` or `onLoaded` listener goes to `console.error('[link] event listener
failed:', error)` and no longer escapes `activate`. That comes in as `LinkConfig::host`. The
vectors log it as a `console.error` callback (`a-throwing-listener-throws-out-of-activate`, whose
name predates v3).

v2 took `onActivate` as config. The generator's `link` factory subscribes the scripted callback to
`onActivated` as soon as the controller is built, before `connect`, and logs a dispatched
activation's promise as `[promise]`. `harness.rs` does the same through the crate-private
`build_link_plugin(config, on_created)`.

## Turn order

`loadLinksPage` awaits the engine read (the three turns of
`epdf_plugin_annotation::promise::await_engine`), and `ensureLoaded` returns its `.then` derivative,
one turn later. The refetch chains `.then` too. `listAllLinks` is `await Promise.all(...)`.

A list the loop throws on (not iterable, a `null` entry, a link without `flags`, `ref` or `rect`)
rejects `ensureLoaded` with the JavaScript `TypeError` text (`viewer-only-malformed-lists`), and the
rejected load stays in the in-flight map, as it does in the TypeScript.

## Where the port differs in shape

The cache is a `BTreeMap<u32, Vec<Link>>`. The TypeScript's `Record<number, ...>` keys are strings;
every page now arrives as a `PageRef`, so the keys never meet a string.

`anno()` reads a `try_get` error as absence. The kernel only errs there when a capability's
constructor threw, which fails the open before any link call can run.

A malformed `ref` on an engine DTO still differs, and no vector feeds it: `Link.ref_` is a typed
`AnnotationRef`, so a ref that does not decode is dropped where the TypeScript copies it verbatim.

## Coverage

The generators reach 460 of the package's 461 statements and 128 of its 132 branches
(`vectors/coverage.json`). What is left cannot be reached from the public surface:

- `controller.ts:121-122`, the `reported` arm of a `reveal` resolution. Only a `goto` resolves to a
  reveal.
- `controller.ts:174`, `listAllLinks` without a document. The plugin is document-scoped.
- `controller.ts:190` and `link.plugin.ts:41`, the `?? false` after `getActiveTool()?.`. The hub's
  `getActiveTool` always returns a tool, falling back to a neutral one for an unknown id.

The function fixture `item()` in `suites/link.ts` still built v2 items with `rect`; it builds
`bounds` now, so `link-functions.json` carries the v3 `Link` shape.

## Mutation testing

The pre-v3 run covered `capability.rs`, `effects.rs` and `source.rs`, which are rewritten or gone.
The reducer moved to `model.rs` with the v3 action shape; its pre-v3 result (one equivalent
survivor, `initial_link_state` as `Default::default()`) is pre-v3, rerun pending.
