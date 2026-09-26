# epdf-plugin-stamp

`packages/plugin/stamp` in Rust: workspace stamp libraries over an asset engine, dynamic stamps
evaluated through form scripting, and placement through the annotation plugin. All 74 cases in four
vector files replay identically (`cargo test -p epdf-plugin-stamp`):

| File | Kind | Cases | Covers |
|---|---|---|---|
| `vectors/plugins/stamp-library.json` | scenario | 20 | `importLibraryPdf`: catalog and page `/PieceInfo` reads and writes, options over metadata, source errors, missing services, engine failures mid-import and at close, URL renders, id allocation and collisions, concurrent imports, every asset-engine configuration and its teardown |
| `vectors/plugins/stamp-assets.json` | scenario | 20 | loose raster and PDF assets, appends to and deletes from a canonical library, `removeLibrary`, the per-library mutation queue, loose and canonical libraries replacing each other under one id, a mutation that outlives the kernel |
| `vectors/plugins/stamp-placement.json` | scenario | 19 | `armAsset` and `disarm` through the real annotation plugin, the skipped cases (scripting off, no sandbox, raster, no field on the page), materialization, its refusals, the observers, flatten and close failures |
| `vectors/plugins/stamp-functions.json` | function | 15 | the reducer |

The scenario registry runs the real interaction, annotation and form adapters beside this one, and
dynamic stamps drive the form plugin's standalone controller over the scripted sandbox from
`epdf_acrojs::harness`.

## The API

One token, `STAMP`, and one type, `StampCapability`, with the TypeScript's methods in snake case:
`libraries`, `library`, `assets`, `asset`, `asset_preview`, `asset_bytes`, `library_bytes`,
`import_library_pdf`, `add_asset`, `remove_asset`, `remove_library`, `arm_asset` and `disarm`.
`stamp_plugin(config)` is `stampPlugin(config)`, and `create_stamp_capability`, the reducer and the
descriptor types are public too.

`StampConfig` carries the TypeScript's three members. `asset_engine` is `AssetEngine::Instance` or
`AssetEngine::Factory`, and a factory answers `AssetEngineRef::Ready` or `Pending`, which is
`Engine | Promise<Engine>`. `StampScriptingOptions` is the TypeScript's `FormScriptingOptions`
extension spelled as composition: `enabled`, `form` (the `epdf_plugin_form::FormScriptingOptions`
the controller takes) and the three observers, each returning `Result` so a throw can be reported.

`FormScriptingOptions` requires a sandbox factory, since Rust has no QuickJS default. A host
without a sandbox passes `scripting: None`, and the plugin then arms every asset's template bytes,
which is what the TypeScript does with scripting disabled (`no-sandbox-host`).

Engine answers stay `serde_json::Value`. Bytes the plugin keeps are `Option<Vec<u8>>`: `None` is an
engine that answered a save, an extraction or a render with no bytes, which the TypeScript stores as
`undefined` and later treats as missing.

## Host input

The TypeScript reports a throwing scripting observer to `console.error`. That comes in as
`StampHost::console_error(message, error)` on `StampConfig::host`; `None` drops it. The vectors log
it as a `console.error` callback.

## Ids

`capability.ts` numbers session ids from a module-level counter:
`${prefix}-${Date.now().toString(36)}-${seq.toString(36)}`. The Rust counter is thread-local, since
a kernel and its plugins live on one thread, and `Date.now()` is `rt::now()`.

A module-level counter makes every vector depend on the cases that ran before it in the same
process. So the generator's `stamp` factory builds each case over its own import of
`capability.ts?instance=<n>`, whose counter starts at zero, and the harness calls
`capability::reset_id_sequence` when it builds the plugin. The cost is that the TypeScript
`stamp` factory restates `stamp.plugin.ts` rather than calling it. One case,
`the-plugin-factory-defaults`, runs the real `stampPlugin()` so that file is covered too, and the
fixture refuses a second one.

## Turn order

Each `await` and `.then` is mirrored in `src/promise.rs`. An engine call awaited resumes three turns
after it settles, an `async` function that returns a promise settles two turns after it, and
`mutateLibrary`'s `result.finally(...)` settles three turns after `result`. The queue's ordering is
pinned by `removals-are-serialized-per-library`, `a-loose-append-finds-its-library-canonical` and the three `*-queued-*` cases, where a deferred
`open` holds one mutation while others line up behind it. `openAssetDocument` draws its
`stamp-import` id after awaiting the engine, as the argument evaluation order puts it, which the
concurrent cases would expose.

## Where the port differs in shape

The state's `libraries` and `assets` are `BTreeMap`s. The TypeScript records are plain objects, and
the vectors compare members unordered.

Prototype-named ids (`constructor`, `__proto__`) resolve through `Object.prototype` in the
TypeScript's record lookups. The Rust maps don't, so such an id is just unknown. The annotation port
made the same call.

A few malformed engine answers make the TypeScript throw a `TypeError` where the Rust carries on,
and no vector sends one: an insert result without `insertedPageObjectNumbers` (read as empty), a
page layout without `pageObjectNumber` (read as 0), a truthy non-bytes answer to a save or an
extraction (read as no bytes), and a form widget or selected page whose `pageObjectNumber` is not a
number (never a match, where the TypeScript's `===` matches two `undefined`s). An `assetName` callback that answers a non-string reads as no name,
where the TypeScript would store the value itself.

The selectors read the state with `Ctx::state()`, which panics once the kernel is destroyed, where
the TypeScript throws a `TypeError`. A caller can't reach the capability after destroy through the
kernel. The mutation paths, which can outlive it, read with `try_state()` and reject with the
TypeScript's `TypeError` (`a-queued-mutation-after-destroy`).

## Harness change

One came with this port (commit `ddf5fdcfb`): a fake document's `lacks` list now removes
`pages.flatten` and `pages.extract`, as it already did `forms.applyEffects`. Without it no vector
could reach the NotImplemented refusal for an engine that can't flatten.

## Coverage

The generators reach every statement and branch of `reducer.ts`, `stamp.plugin.ts` and `types.ts`,
and all 731 statements and 302 of the 304 branches of `capability.ts` (`vectors/coverage.json`).
The two branches left are unreachable from the public surface:

- `capability.ts:636`, the `'native form effect failed'` fallback. Every failed `FormCommitResult`
  the form controller returns carries an `error`, including the one for a native effect failure.
- `capability.ts:717`, `s.libraries[lid]?.assetIds ?? []` for an ordered id with no library.
  `LIBRARY_REMOVED` drops every order entry of the id it removes, so the order never names a
  missing library.

The missing-canonical-bytes refusals looked unreachable too, until `a-save-that-answers-nothing`:
an engine whose `download` answers nothing leaves the library's bytes `undefined`.

## Mutation testing

Complete, file by file through `scripts/mutants.sh`:

| File | Caught | Missed | Timeout | Unviable |
|---|---|---|---|---|
| `capability.rs` | 116 | 1 | 11 | 63 |
| `reducer.rs` | 4 | 1 | 0 | 0 |
| `promise.rs` | 0 | 0 | 0 | 6 |
| `lib.rs` | 0 | 0 | 0 | 1 |

The timeouts count as caught. Ten mutate `base36` or `uid` into a function that repeats an id,
and `allocate_id`'s draw-again loop then never ends; the eleventh is `allocate_id`'s `!` itself.
The two misses are equivalent:

- `capability.rs:115`, deleting the `"stamp"` arm of `kind_from_pdf_name`. `None` falls back to
  `StampAssetKind::Stamp`, the same kind.
- `reducer.rs:9`, `initial_stamp_state` as `Default::default()`: the same value.

The first `capability.rs` run missed 14 of 206. `a-loose-append-finds-its-library-canonical` and
four more flatten results in `a-flatten-that-fails-or-skips` kill the reachable ones, and two
helpers lost code no vector could tell apart: a guard in `entries_of` and a general `===` in widget
matching. `types.rs` has no functions to mutate, and `harness.rs` is the adapter.
