# epdf-plugin-link

`packages/plugin/link` in Rust: the navigation plane of link annotations. All 23 cases replay
identically (`cargo test -p epdf-plugin-link`):

| File | Kind | Cases | Covers |
|---|---|---|---|
| `vectors/plugins/link.json` | scenario | 14 | the engine-backed page cache and its refetch on annotation events, malformed lists, the annotation-backed source, the `link-nav` behavior and `engaged()`, and `activate` with no stage, with a stage, and through the action engine |
| `vectors/plugins/link-functions.json` | function | 9 | `linkReducer` and `initialLinkState` |

The scenarios run the real interaction, annotation, actions and stage adapters beside this one, in
the four combinations a deployment ships: viewer-only, annotation-backed, with a stage, and all of
them together.

## The public API

`link_plugin(LinkPluginConfig)` builds the plugin, `LINK` is its token and `LinkCapability` its
capability. The four members are the TypeScript's in snake case:

- `links_on(pon)` returns the page's `LinkNavItem`s, from the annotation plugin's
  `link_items_on` when that plugin is registered, else from this plugin's cache.
- `ensure_page(pon)` defers to the annotation plugin, or lists the page once through
  `page.annotations.list`.
- `engaged()` is whether the active tool enables `link-nav`.
- `activate(&PdfLinkTarget, Option<&LinkActivateContext>)` returns a `LinkActivation`.

`LinkActivation::Dispatched` carries the action engine's `Promise<ActionDispatchResult>`, so the
enum isn't `Serialize`; `to_json_with` prints it with the promise spelled however the caller wants.
`LinkNavBehavior` is the behavior `init` registers, and `load_links_page` and `project_links` are
the engine-backed source. `LinkNavItem`, `PdfLinkTarget` and `PdfDestination` are re-exported from
the annotation crates, as `types.ts` re-exports them.

Three members return `Result` where the TypeScript can't throw in practice: `links_on`,
`ensure_page` and `engaged` propagate the kernel's `try_get` error. `activate` returns `Err` in the
two places the TypeScript throws: a goto to a page whose `userUnit` is 0 (the stage's
`NonInvertible`, printed as `Mat2D.invert: non-invertible matrix (det = 0)`), and an `on_activate`
listener that throws after the built-in handling ran. `ensure_page` also returns `Err` if the
document's page list names a page the engine doesn't have, where `doc.page(pon)` throws.

## Host inputs

The plugin reads no globals. `onActivate` is `LinkPluginConfig::on_activate`, a listener that
returns `Err` to throw.

The generator's `link` factory takes `onActivate` as a scripted callback spec. A callback's logged
arguments can't print a promise, so the shim logs a dispatched activation as `{outcome:
'dispatched', dispatch: '[promise]'}`, and `harness.rs` does the same.

## Turn order

`loadLinksPage` chains `.then(onFulfilled, () => {})` on the engine call, which reacts one turn
after the call settles. The port uses `epdf_plugin_annotation::promise::then`, which costs the same
turn. Nothing else here awaits. A throw inside the fulfilled handler (a list without
`annotations`, a `null` entry, a link without `flags`, `ref` or `rect`) rejects a promise nothing
observes; `project_links` returns `None` for exactly those and nothing is dispatched
(`viewer-only-malformed-lists`).

## Where the port differs in shape

The cache is a `BTreeMap<u32, Vec<LinkNavItem>>`. The TypeScript's `Record<number, ...>` keys are
strings, and the refetch effect tests `pon in pages`, so the effect spells the event's
`pageObjectNumber` as a property key before comparing it with the cached pages.

`LinkNavItem` needed `Deserialize` to live in slice state, which commit `060c21ee6` added in the
annotation crate. `behaviorFor` in the annotation harness printed a behavior it hadn't scripted as
`{id}` alone; commit `c1bca998a` prints it as the `{id, matches, engaged}` literal the link and form
plugins write, so `link-nav-behavior` can call both functions.

## Not ported

These inputs answer differently, and no vector feeds them:

- A malformed `ref` on an engine DTO. `LinkNavItem.ref_` is a typed `AnnotationRef`, so a ref that
  doesn't decode (an `objectNumber` ref without `annotObjectNumber`) is dropped, where the
  TypeScript copies it verbatim. Its id is still built from the raw JSON, so `obj:undefined` would
  match.
- A `PdfLinkTarget` or `LinkActivateContext` that doesn't match its type fails to decode in the
  harness. In particular a goto destination of an unknown `kind` can't be expressed, where the
  TypeScript's `destinationToReveal` falls through to a bare smooth reveal.
- A string `pageObjectNumber` in an annotation event. The TypeScript's `pon in pages` matches
  `"1"`, then `loadLinksPage` finds no page `=== "1"` and does nothing; the port refetches page 1.

## Coverage

The generators reach every statement of the package (352 of 352) and 81 of its 84 branches
(`vectors/coverage.json`). The three branches left are unreachable from the public surface:

- `capability.ts:96` and `link.plugin.ts:43`, the `?? false` after `activeTool()?.`. The hub's
  `activeTool` always returns a tool, falling back to a default for an unknown id.
- `effects.ts:15`, `if (!doc) return`. The plugin is document-scoped, so its effect context always
  has a handle.

## Mutation testing

`scripts/mutants.sh -p epdf-plugin-link --test-package epdf-plugin-link --exclude
crates/plugins/link/src/harness.rs` catches 62 of 63 viable mutants (10 unviable). Two earlier
misses were closed: the smooth reveal behavior got `activate-reveals-smoothly` (commit
`86594ae1c`), and `js_string`'s guard for the `undefined` marker was dead code and went (commit
`473e6d227`). The one survivor is equivalent: `reducer.rs:6`, `initial_link_state` as
`Default::default()`, which is the same empty cache.
