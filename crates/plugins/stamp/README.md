# epdf-plugin-stamp

`packages/plugin/stamp` in Rust, at the TypeScript of upstream `b456ee4fd` plus the reviewed
migration (`b029e3d78`): workspace stamp libraries, each a canonical PDF whose pages are its
stamps; marks authored into pages; dynamic stamps evaluated in a detached script realm; placement
through the annotation plugin; and the persistence helpers. All 106 cases in five vector files
replay with no member-order allowance and no outcome exception (`cargo test -p epdf-plugin-stamp`):

| File | Kind | Cases | Covers |
|---|---|---|---|
| `vectors/plugins/stamp-library.json` | scenario | 23 | `importLibrary`: catalog and page PieceInfo, `/Title`, the named-page registry, plain PDFs, the asset engine settings and their refusals, ids, previews, persistence |
| `vectors/plugins/stamp-assets.json` | scenario | 20 | `createAsset` from rasters and PDFs, appends and removals, `deleteLibrary`, the per-library queue, a queued mutation after destroy |
| `vectors/plugins/stamp-placement.json` | scenario | 22 | `armAsset` through the real annotation plugin, ghost renders, materialization in a detached realm, sandbox failures, atomic flatten, the file picker, permission refusals |
| `vectors/plugins/stamp-api.json` | scenario | 21 | library and asset writes, marks, registered libraries, moves and copies, assets from annotations, `placeAsset` and `placeAssetOnPages`, the armed asset, persistence |
| `vectors/plugins/stamp-functions.json` | function | 20 | the `model.ts` transitions |

`tests/behavior.rs` pins what no vector reaches, because no vector passes a signal or changes a
setting: the TypeScript-measured turns of ten verbs (`verbs_settle_on_the_typescripts_turns`), a
queued mutation cancelled before it starts, a cancel after the rewrite that keeps nothing, one
queue per library, work in flight at teardown that completes where a cancel rejects, ghost renders
cached per width, a library's assets going with it, a setting change that re-resolves the asset
engine, and an unconfigured import on a local kernel engine.
`the_settings_verbs_replay_and_the_host_is_restored` in `tests/conformance.rs` drives
`onSettingsChanged`, `updateSettings`, `getSettings` and `resetSettings` through the adapter and
checks the case's console and random are put back when its kernel goes.

## Shape

The files follow the TypeScript package. `contract.rs`, `token.rs`, `model.rs`, `convention.rs` and
`blank_library.rs` are the vocabulary, the token, the session state and its transitions, the key
and PieceInfo rules and the seed PDF. `controller.rs` composes the services (`services/`: events,
binaries and the per-library queue, the asset engine port, ghosts, the target document, settings)
and the areas (`read/catalog.rs`, `write/{libraries,marks,assets,placement}.rs`), each an `impl`
block on the one `StampCapability`. `persistence.rs` is the store port and its two helpers.
`stamp_plugin(config)` is workspace-scoped, with the annotation and actions tokens optional.

The verbs take the TypeScript's options as structs whose `signal` is the trailing
`OperationOptions` member. A placing verb names its document in `StampDocumentOptions` or
`StampArmOptions`; left out, the active document. The kernel has no `inScope`, so a host fills the
document in itself. A write resolves the library or asset it made, where the TypeScript wraps it in
`{ library }` or `{ asset }`.

The state is `StampState` with `libraries`, `libraryOrder`, `assets` and `armed`. The TypeScript
builds a library with two literals whose member order differs and adds a member later by
assignment, so `StampLibrary`, `StampAsset` and `StampState` each carry their member order
(`Members`) and serialize in it. The records are maps in `Object.keys` order: integer-like keys
first, then insertion order (`model::assign`).

## Host inputs

`console.rs` holds the globals the TypeScript reads. `set_sink` takes the `[stamp]` lines written
to `console.warn` and `console.error`; the default writes to stderr. `set_random` is
`Math.random`, which `customStampName` draws an identifier from; the default is a clock-seeded
xorshift. The harness installs both per case, as `stamp-current-fixtures.ts` does.

`uid` numbers session ids from a thread-local counter, the module-level `idCounter` of
`services/asset-engine.ts`. The generator builds each case over a private copy of the controller
module, so the harness calls `reset_id_sequence` when it builds the plugin.

## Where the port differs

The asset engine is typed. `StampAssetEngine` is a `LocalEngine`, which is the TypeScript's
`LOCAL_ENGINE_BRAND` made a type, or a factory that makes one. With nothing configured the kernel's
engine is used when `Engine::is_local` says it carries the brand (Ruling13.24, `7557d8a45`), as
`isLocalEngine` reads it at `services/asset-engine.ts:56`; an unbranded engine, the cloud one or
the conformance fake, is refused before any open. The native host's engine is local (Task 13.33).
`an_unconfigured_plugin_imports_on_a_local_kernel_engine` pins both cases. The web host's
`JsEngine` (`crates/epdf-web/src/engine.rs`) forwards the bridge's `isLocal` since Task 14.1
(`9a2ff3217`), so a web viewer over the local WASM engine imports with no `assetEngine` too.

The settings live in the kernel's JSON store. The asset engine is stored as a handle,
`{"$assetEngine": n}`, into a table the registration owns, as the actions plugin stores a function
identity. `get_settings()` decodes it; `settings_json()` shows the handle where the TypeScript shows
the engine.

`mutate_library` (`services/binaries.rs`) rides the kernel's `SerialQueue::run_with`, which is
`serial-queue.ts`'s `{ signal }` option: a mutation whose signal fired while it waited never starts.

A render's bytes the host holds (`$hostBytes`) are read out and the render released
(`release_host`), as before this port. A page ref with no object number is the TypeScript's
`TypeError`; one the kernel's `PageRef` can't hold otherwise is a `TypeError` with the port's own
message, where the TypeScript calls the engine with it (`js::page_ref`). A sibling plugin whose
lookup throws reads as absent in `can_place`, `disarm` and materialization. A library or asset id
that `Object.prototype` holds (`constructor`, `toString`) is an unknown id here, where the
TypeScript's `state.libraries[id]` finds the inherited function: `getAsset('toString')` returns it
and `setLibrary` takes the id as known. Rust keeps the plain lookup, the choice Ruling13.7 made for
kind names. No vector reaches any of these.

The default QuickJS execution has no Rust counterpart: with scripting on and no sandbox factory,
every sandbox build is refused (`NO_DEFAULT_SANDBOX`, `crates/plugins/actions/README.md`), so a
dynamic stamp's arm fails where the TypeScript would load QuickJS. `no-sandbox-host` replays the
case with no actions plugin, and no vector runs the default VM.

## The ffi

`src/ffi.rs` and `tests/ffi.rs` were migrated by hand in Task 13.24, and are compiled and tested
since Task 13.33. The handle answers ids as before. Placements take `center` and
answer the placed annotation's ref, `persist` lost its debounce, `ArmChanged` carries the asset, and
`can_create_from_annotations` is new. A host whose engine reports `is_local` needs no asset
engine; `StampConfig::into_core` still takes one.

## Harness changes

Task 13.24 needed two outside this crate, both dump-time only. The annotation records mirror strips
its `$v` identity tag when the store is dumped (`records_json`,
`crates/plugins/annotation/src/sync/records.rs`), and the form harness's diagnostics plugins have no
state slice, as the generator's `definePlugin` declares none. The stamp vectors are the first to
dump the whole state with those plugins installed.

## Coverage

Stale since Task 13.24: the counts and lines below are the pre-13.24 port's, over code that no
longer exists. Task 14 regenerates them.

Across the whole corpus the generators reach 2,286 of the package's 2,305 statements and 633 of
its 651 branches (`vectors/coverage.json`). The 19 statements left:

- `write/assets.ts` 215 to 219 and `write/marks.ts` 89 to 93: an engine without
  `annotations.exportAppearance`. The fake engine's page annotation service always has it.
- `write/placement.ts` 78 to 82: a target with an actions plugin but no document record. A document
  plugin resolves only for an open document.
- `write/placement.ts` 98 to 100: an asset without a page. Every v3 asset is a page.
- `write/placement.ts` 107: the loose-asset refusal, which needs a library whose save answered
  nothing and whose extracted page lacks the asset's page. No case builds that.

The unreached branches are the same guards, `opts.kind ?? DEFAULT_LIBRARY_KIND` (every caller passes
a kind), `libraries[id] ?? null` right after the dispatch that adds it, `removeAsset`'s missing
library for an existing asset, the `bin.preview` absent arm of a move or copy (every asset has a
preview), `result.error?.message ?? ...` (a failed commit always carries an error), the boot-source
arms for an asset document without `actions`, and `placeAssetOnPages`' `?? []`.

## Pinned TypeScript behaviour

`moveAsset` and `duplicateAsset` copy the page without its identifier: the copy gets a freshly
minted `#...` name, so a moved stamp's `/Name` changes (`edit-move-and-copy-assets`). A supplied
preview whose MIME type is `''` reads as `image/png`, since `resolveBinarySource` drops an empty
type.

## Mutation testing

Stale since Task 13.24: the counts and lines below are the pre-13.24 port's, over code that no
longer exists. Task 14 regenerates them.

`scripts/mutants.sh -p epdf-plugin-stamp --test-package epdf-plugin-stamp --file <file>`, file by
file over every module but `harness.rs`, catches 239 of 251 viable mutants (228 caught, 11
timeouts; 174 unviable). The run was against `afb842b48`; line numbers below are that run's. It
found 35 survivors. 21 of them are killed by new cases and one unit test, each checked by applying
the mutant by hand:

- `stop-flushes-a-write-still-pending-behind-another` (`stamp-api`) changes library `a`, then `b`
  50 ms later, lets `a`'s write land and stops. Keeping only the firing library's timer in
  `Persisting::write` (`persistence.rs:122`) loses `b`'s flush. Finding a timer by `!=`
  (`persistence.rs:192`) overwrites `a`'s slot with `b`'s timer, so the stop flushes nothing.
- `a-render-with-no-image-throws` (`stamp-library`) answers a render with `null` and with a `null`
  source. `missing` as `false` and its `||` as `&&` (`services/asset_engine.rs:72`) both reach the
  URL-render refusal instead of the `TypeError`.
- `a-cloud-kernel-engine-names-the-fix` now reads `canImport` before and after the cloud refusal,
  which kills `can_import` as `true` (`services/asset_engine.rs:232`).
- `marks-that-are-refused` adds the colours `#12345` and `#zzzzzz`, which kill `||` as `&&` in
  `hex_color` (`write/marks.rs:28`), and a PDF mark at page `-1`, which kills `&&` as `||` in
  `extract_page` (`write/marks.rs:168`): `-1 as usize` saturates to page 0.
- `the-target-width-sizes-the-ghost-and-a-template-ghost-is-cached` (`stamp-placement`) reads the
  hover ghost after `armAsset` with a target width of 120, which kills dropping `target_width` in
  `arm_asset` (`write/placement.rs:259`). It then asks for the ghost at one width, disarms, arms
  again and asks again. With the `!` deleted from the cache key (`write/placement.rs:89`) the second
  ask tries to open a second copy.
- `the_default_random_spreads_over_the_unit_interval` in `controller.rs` draws 10,000 values from
  the default `random` (now `clock_random`, a free function, so it can be tested): all in [0, 1),
  mean within 0.45 to 0.55, every tenth of the interval holding more than 800. The vectors always
  answer `random`, so nothing else reaches it. It kills 12 of the 15 `StampCapability::random`
  mutants (`controller.rs:143` to `148`).

Two more survivors were a port bug. `create_asset_from_annotations` built its input with `mark:
None, size: None`, where the TypeScript's `addAsset({ ...input, source })` carries both. A `mark`
is legal in `Omit<AddAssetInput, 'source' | 'size'>`, and the TypeScript then rejects `InvalidArg`
for a source and a mark; the Rust dropped the mark and created the asset. The fix is
`AddAssetInput { source: Some(exported), ..input }`, which is the `mark` mutant itself, and the
`size` field went with it. `an-asset-from-annotations` now passes a mark, and the old code fails
it. Both are left out of the 251.

The 12 that remain:

`controller.rs`, in the default xorshift. `^` as `|` when mixing in the clock (143), and `<<` as
`>>` in the first and third steps (144, 146). Each still yields a spread of values in [0, 1), and
the unit test passes them. `Math.random` promises nothing more than that range, so pinning the
exact sequence would test the generator rather than the contract.

`convention.rs:39`, `BASE62.len() - 1` as `+ 1` and as `/ 1` in the clamp of `custom_stamp_name`.
The index is `floor(random() * 62)`, which is at most 61 for any `random` in [0, 1). The clamp only
acts on a host that breaks that range, where the TypeScript appends `undefined`.

`model.rs:41`, `initial_stamp_state` as `Default::default()`, and `persistence.rs:39`,
`memory_stamp_store` as `Default::default()`. Both bodies are already `Default::default()`.

`write/libraries.rs:60`, deleting the `"stamp"` arm of `kind_from_pdf_name`. The only caller falls
back to `Stamp` on `None`, as the TypeScript's `?? 'stamp'` does, so the result is the same.

`write/marks.rs:49`, `52`, `55` and `58`, `<` as `<=` and `>` as `>=` in `ink_bounds`. On a tie the
mutant assigns an equal value. The one tie that differs is `0` against `-0`, and a vector cannot
carry `-0`: `JSON.stringify(-0)` is `"0"`.
