# epdf-plugin-stamp

`packages/plugin/stamp` in Rust at the v3 contract: workspace stamp libraries, each a PDF whose
pages are its stamps, over an asset engine; marks authored into assets; dynamic stamps evaluated in
a detached script realm; placement through the annotation plugin; and the persistence helpers. All
100 cases in five vector files replay identically (`cargo test -p epdf-plugin-stamp`):

| File | Kind | Cases | Covers |
|---|---|---|---|
| `vectors/plugins/stamp-library.json` | scenario | 21 | `importLibrary` of a plain PDF: catalog and page PieceInfo, `/Title`, registration of every page, source errors, missing services, engine failures mid-import and at close, URL renders, id allocation, concurrent imports, every asset-engine configuration and its teardown |
| `vectors/plugins/stamp-assets.json` | scenario | 20 | `createAsset` from rasters and PDFs, appends to and deletes from a library, `deleteLibrary`, the per-library mutation queue, a mutation that outlives the kernel |
| `vectors/plugins/stamp-placement.json` | scenario | 19 | `armAsset` and `disarm` through the real annotation plugin, the skipped cases, dynamic materialization in a detached realm, its refusals, flatten and close failures |
| `vectors/plugins/stamp-api.json` | scenario | 21 | library create, update, export and delete, registered (`/Names /Pages`) libraries, marks, asset edits, moves and copies, assets from annotations, filtered reads, rendered previews, `placeAsset` and `placeAssetOnPages`, the armed asset, the eight change events, persistence |
| `vectors/plugins/stamp-functions.json` | function | 19 | the reducer |

The scenario registry runs the real interaction, actions, annotation and form adapters beside this
one. The first three files are the v2 suites reaching v3 through renamed calls; `stamp-api` is
where the v3 surface is pinned.

## Shape

The v3 plugin is a different design from v2's canonical-or-loose libraries, so the crate was
rewritten along the TypeScript's split rather than adapted. `contract.rs`, `model.rs`,
`convention.rs` and `blank_library.rs` are the vocabulary, the reducer, the key and PieceInfo rules
and the seed PDF. `controller.rs` composes the services (events, the binary sidecar with the
per-library mutation queue, the asset engine port, the ghost renderer) and the areas: `read/catalog`
and `write/{libraries,marks,assets,placement}`, each an `impl` block on the one `StampCapability`.
`persistence.rs` is the store port and its two helpers. `stamp_plugin` declares `create` and is
workspace-scoped with the annotation and actions tokens optional.

The methods are the TypeScript's in snake case. Options objects become structs:
`CreateLibraryOptions`, `LibraryPatch`, `AssetPatch` (whose `subject` is `Some(None)` for `null`),
`StampLibraryFilter`, `StampAssetFilter`, `AddAssetInput` with a typed `MarkSource`.
`place_asset_on_pages` takes the pages as `Option<Vec<PageRef>>`, `None` for `'all'`.

Dynamic stamps no longer take scripting options. The target document's actions plugin mints a
detached realm (`create_detached_script_realm`), the form plugin's controller rides it, and the
result surfaces through `surface_script_commit` with `realm: Detached`. No actions plugin, scripting
off, or `dynamic: Some(false)` arms the template.

Engine answers stay `serde_json::Value`. Bytes the plugin keeps are `Option<Vec<u8>>`: `None` is an
engine that answered a save, an extraction or a render with no bytes, which the TypeScript stores as
`undefined` and later treats as missing.

## Host inputs

`StampHost` carries what the TypeScript reads from globals: `console_error` for a throwing change
listener, `console_warn` for ignored registrations and a failed ghost render, and `random` for
`Math.random`, which `customStampName` draws a minted identifier from. The persistence helpers take
their `console.warn` sink as an argument. The vectors log both consoles as callbacks and answer
`random` with a fixed sequence (see `stamp-fixtures.ts`).

## Ids

`services/asset-engine.ts` numbers session ids from a module-level counter:
`${prefix}-${Date.now().toString(36)}-${seq.toString(36)}`. The Rust counter is thread-local, and
`Date.now()` is `rt::now()`. The generator's `stamp` factory builds each case over a private copy of
the stamp source (`src/loader/instance-hook.mjs`), so its counter starts at zero, and the harness
calls `services::asset_engine::reset_id_sequence` when it builds the plugin.

## Turn order

Each `await` and `.then` is mirrored in `src/promise.rs`. An engine call awaited resumes three turns
after it settles, an `async` function that returns a promise settles two turns after it, and
`mutateLibrary`'s `result.finally(...)` settles three turns after `result`. The queue's ordering is
pinned by `removals-are-serialized-per-library`, the `*-queued-*` cases and
`edits-queued-behind-removals`, where a deferred `open` holds one mutation while others line up
behind it. A page handle without `pieceInfo` costs the `await undefined` turn the TypeScript's
`pieceInfo?.read(...)` does.

## Where the port differs in shape

The state's `libraries` and `assets` are `BTreeMap`s; the vectors compare the TypeScript's records
unordered. A few malformed engine answers make the TypeScript throw a `TypeError` where the Rust
carries on, and no vector sends one: a page layout without a `ref`, a registry entry pointing at a
page the layout lacks, a created annotation without a `ref`. The default `random` is a clock-seeded
xorshift, where the TypeScript uses `Math.random`.

## Harness changes

Two came with this port. The Rust fake engine now answers `DocumentHandle::has` for every
`pages.*` member (present unless `lacks` names it) and every `page.annotations.*` member (present),
as the TypeScript proxies already did (commit `beee89103`). And `stamp-fixtures.ts` gained a
`console.warn` shim, a `Math.random` shim and the `stamp-persistence` driver, each mirrored in
`src/harness.rs`.

## Coverage

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
minted `#...` name, so a moved stamp's `/Name` changes (`edit-move-and-copy-assets`).
`placeAssetOnPages(..., 'all', ...)` on a document that is not open resolves with nothing applied
rather than rejecting. A supplied preview whose MIME type is `''` keeps it (`??`, not `||`).

## Mutation testing

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
