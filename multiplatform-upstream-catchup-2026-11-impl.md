# November 2026 catch-up: impl

This is the record of bringing `research/multi-platform-rust-core-impl` up to upstream `main`. It is
Task 3 of `multiplatform-upstream-catchup-2026-11.md` (committed on `research/catchup-2026-11-plan`
at `867c1c036`), plus the part of D6 that belongs on impl. Plugins is C4 and follows from here. C4
runs two plan tasks: Task 4 is the plugins merge, and Task 5 is the port work.

## What was merged

Impl was at `52f10e027`. We merged upstream `main` at `a40d63a19`, 7 commits past the `74afd5575`
the plan measured. The extra 7 are viewer examples and docs, plus `07a39d018`, which only regenerates
an API contract. None of them touch `packages/engine` or the runtime pin. The merge is `50786828f`.

It conflicted in the same three places as the C1 probe (`e35924682`), and we resolved them the same
way:

| Path | Resolution |
|---|---|
| `.prettierignore` | Union: upstream's generated-font entries plus impl's conformance and ts-rs entries |
| `packages/engine/runtime/engine-runtime-build.local.json` | Upstream's `wasm32` and `darwin-arm64` entries plus impl's `ios-arm64`, `ios-sim-arm64`, `android-arm64` and `android-x64` |
| `packages/engine/runtime/runtime-src` | Pinned to `8b1e69abd` on `roanutil/embed-pdf-runtime` (fork `926c126b2` merged with upstream `cff0077cf`, per D1) |

`.gitmodules` keeps impl's URL. Four files merged cleanly with changes from both sides:
`WorkerHost.ts`, `wasm-runtime.ts`, `packages/engine/main/src/index.ts` and
`.github/workflows/build-engine-runtime.yml`. Each of them, diffed against upstream, carries exactly
impl's own hunks from `b456ee4fd` and nothing else. So does `pnpm-lock.yaml`.

## Commits on top

- `77bcc82e1` adds `epdf_ppo.h` to `crates/pdfium-sys/wrapper.h`. The runtime ships it now, and the
  wrapper includes every public header.
- `1109eeea1` passes the new `tier` argument to `EPDFFont_AddFallbackFont`, which is what stopped
  the build. It also threads the tier through `FontRegistry` the way `FontRegistrar.ts` does:
  `add_fallback` keeps the `FALLBACK_TIERS.own` default, `add_fallback_in_tier` takes one, re-adding
  a key changes only its tier, and `fallback_order` reports the chain by tier, then by the order
  added. `StartupFont` gains `tier`, and `fonts.addFallback` reads an optional `tier`.
- `cbb538fee` binds the rest of the font surface that upstream's TS reads through the runtime, in two
  new modules, `crates/pdfium/src/font_program.rs` and `crates/pdfium/src/document_fonts.rs`.
- `798f7b229` makes a wire `tier` coerce the way the TS on the WASM runtime does, below.
- `2e5a14a81` narrows the two `GetProgramObjNum` results with `u32::try_from`, a number past `u32`
  reading as the fork's 0 (no program), and reserves the fallback-id buffer with
  `try_reserve_exact` like the others in `document_fonts.rs`.
- `332394f16` allows clippy's `useless_conversion` at the three `c_ulong` conversions in
  `document_fonts.rs`, which are identities where `c_ulong` is 32-bit.
- `18f7ce2b7` answers `fonts.addFallback` with no payload, as `WorkerHost` does. The plugins merge
  below says why.
- `7bf5f760d` corrects two of those lint comments: `c_ulong` is 64-bit on LP64 targets and 32-bit
  on Windows, wasm32 and other 32-bit targets, not "64-bit on Unix".
- `7e5b519bf` answers `fonts.clearFallbacks` and `fonts.clear` with no payload too.

## The tier at the FFI boundary

The TS never checks `tier`. `WorkerHost` passes `req.tier` to `FontRegistrar.addFallback`, which
passes it to `EPDFFont_AddFallbackFont`. `createWasmFunctions`
(`packages/engine/runtime/src/wasm/wasm-runtime.ts`) changes only `Ptr` bigints, and the slot's
`cwrap` type is `number` (`pdf-functions.generated.ts:1383`). Emscripten's `cwrap` returns the raw
export for an all-numeric signature when ASSERTIONS is off, and `ccall` converts only `string` and
`array` arguments when it is on (`src/library_ccall.js:65` and `:134-145` in emscripten 3.1.72). So
the JavaScript value reaches the wasm `i32` as it is, and the WebAssembly JS API applies
`ToInt32(ToNumber(v))`. An absent tier never gets that far: the default parameter makes it
`FALLBACK_TIERS.own`. `null` is not absent, and ToNumber makes it 0.

Rust now does what the WASM runtime does, and only that runtime. The Node native addon converts
differently, as noted below. `crates/epdf-engine/src/js_number.rs` implements
`ToNumber` over every JSON value (strings by the `StringNumericLiteral` grammar, arrays through
their `join`) and `ToInt32`.
The host decodes `tier` with it and no longer refuses a value as `WireFormat`. The registry passes
`ToInt32` of the tier to PDFium: 1.7 is tier 1, -0.5 is 0, `null`, `"own"` and `{}` are 0, `"1"`
is 1, 2^32 + 1 is 1, and 2^31 wraps to -2^31. `crates/epdf-engine/tests/font_fallback_tier_wire.rs`
checks each against the chain PDFium actually tries.

The registrar also keeps the raw tier and reports `fallbackKeys` sorted by `left.tier -
right.tier`, so the reported order uses the number itself, and 1 comes before 1.7 although PDFium
tries both as tier 1. Rust's `fallback_order` does the same. One case can't match exactly, and it is
an accepted (2026-11-02) Rust-only exception. When a tier's ToNumber is NaN (a word,
an object), `Array.prototype.sort` counts the difference as equal. The comparator is then not an
order, and the TS answer depends on the JavaScript engine's sort algorithm, so V8, JavaScriptCore
and SpiderMonkey can disagree. Rust answers what a stable insertion sort makes of the same
comparator, because `sort_by` may panic on a comparator that isn't total. This affects only the
reported order. PDFium's chain matches exactly. Nothing on impl reports the order to a TS caller
yet. `fonts.list` does once C4 (plan Task 5) ports it, and C4 has to carry the exception to plugins
with it.

A separate observation, not part of the exception: TS's native runtime converts a non-number tier
its own way. The Node addon's generated wrapper reads an `int32` argument as `int32_t x;` followed by
`napi_get_value_int32(env, argv[i], &x)`, and ignores the status
(`packages/engine/runtime/build/generate-napi-binding.mjs:65`). For a non-number tier (`null`, a
string, an object) the call fails with `napi_number_expected` and leaves `x` unwritten, so PDFium
gets an indeterminate int. A number converts the way `ToInt32` does, so for numbers the two runtimes
and the Rust agree. The Rust mirrors the WASM runtime, and we didn't change any behavior for the
native one.

## The font port, split between impl and plugins

Impl has the pdfium-layer bindings for the 45 font functions upstream's TS calls: a registered font's
program facts and the last-resort font (`FontRegistrar.ts`), a document's faces, fingerprint,
display programs and `EPDFDoc_ResolveFonts` (`DocumentFontReader.ts`), the missing-character list
(`DocumentSession.ts`), and the two `/DA` calls that name a document's own program. Each binding
answers the facts as the fork reports them, and `crates/pdfium/tests/document_fonts.rs` checks them
against Roboto and `embedded_orientations.pdf`.

Everything above that layer is left for plugins: the `FontRegistry` port of `FontRegistrar`
(identities, program facts, `engineFontFacts`, `programFor`, `keyForFace`, the last resort
registered at construction and again after `clear`), a `DocumentFontReader` port, the five font
request kinds (`document.fonts.list`, `document.fonts.resolve`, `document.fonts.program`,
`fonts.list`, `fonts.program`), `packages/core/text` and `packages/plugin/fonts`. The reason is that
plugins already rewrote this code and impl doesn't have it. `7566552a2` changed the `fonts.*` wire
answers and the `italic` decoding, and plugins' `crates/pdfium/src/fonts.rs` has `font_identity`,
`font_licence` and `authorize_font_editing`. An impl port would duplicate those and conflict with
them. The cost is that the engine-level font port, the larger half of D6, all lands in C4 (plan
Task 5).

For the plugins merge (plan Task 4), `git merge-tree --write-tree HEAD
private-fork/research/multi-platform-rust-core-impl-plugins` at `061adf19f`, against plugins
`3dec6b60b`, reports five conflicted paths: six text hunks plus the submodule conflict. The commit that records this
changes only this file, so the result holds at the branch tip. Two paths are the runtime pin:
`packages/engine/runtime/engine-runtime-build.local.json` (one hunk, the `darwin-arm64` artifact's
`url` and `sha256`) and the `runtime-src` submodule (`8b1e69abd` against `926c126b2`). The
resolution takes plugins' target entries in `local.json`, as plan Task 4 says, and keeps
`runtime-src` at `8b1e69abd`, which leaves a `darwin-arm64-926c126b2` artifact beside a
`8b1e69abd` gitlink for C5's payload and pin refresh to replace. The other three are Rust, all in
the font code:

| Path | Hunk | Impl | Plugins |
|---|---|---|---|
| `crates/epdf-engine/src/features/fonts.rs` | 1 of 2 | The `fallback_tier` module | `FontEmbeddingPermission` and `FontIdentityInfo` at the same spot |
| `crates/epdf-engine/src/features/fonts.rs` | 2 of 2 | `self.fallbacks.clear()` in `clear()` | `IDENTITIES` cleared with a `fallback_order: Vec<String>` field in `clear()` |
| `crates/epdf-engine/src/host.rs` | 1 of 2 | The `fonts.addFallback` arm decodes `tier` through `js_number` and calls `add_fallback_in_tier` | The arm reads `string_field(body, ..)` from a borrowed body and calls `add_fallback` |
| `crates/epdf-engine/src/host.rs` | 2 of 2 | `fonts.clearFallbacks` and `fonts.clear` answer `Null`, each with a comment naming its `WorkerHost` handler | The same `Null` answers, with the `fonts.authorizeEditing` arm between them |
| `crates/epdf-engine/tests/font_registry.rs` | 1 of 1 | Four tier tests and the NaN test appended at the end | `a_registered_face_maps_back_to_its_key_and_clear_forgets_it` appended at the end |

The rest of `fonts.rs` merges on its own, and it takes impl's side: the `fallbacks: Vec<(String,
f64)>` field, `add_fallback_in_tier` and the insertion-sort `fallback_order() -> Vec<String>`.
Plugins' `fallback_order` field and its `fallback_order() -> &[String]` survive only in the
`clear()` hunk. The resolution keeps plugins' identity code (`FontEmbeddingPermission`,
`FontIdentityInfo`, `IDENTITIES` cleared in `clear()`), impl's tier module and tier decoding, and
impl's `fallbacks` field, and drops plugins' `fallback_order` field. In `host.rs` it takes plugins'
`body` and impl's tier decoding. In `font_registry.rs` it keeps both sides' tests. Keeping both
sides in `fonts.rs` hunk 1 and `font_registry.rs` hunk 1 needs a closing brace added between them,
since both sides share the one trailing brace after the marker. Nothing on plugins
needs adapting to the `Vec<String>`: `git grep fallback_order()` on plugins finds it only in
`font_registry.rs`, where the tests compare it to arrays and call `is_empty`, and both compile
against a `Vec<String>`. Plugins' only caller of `pdfium::add_fallback_font` is
`FontRegistry::add_fallback`, inside the conflicted `fonts.rs`, so it takes impl's tiered form with
the rest. The cost is that plugins' simpler order (insertion only) goes, and its registry tests
now run against the tier sort, which agrees with insertion order when every tier is the default.

The `host.rs` conflict used to be semantic as well. Impl answered `fonts.addFallback` with
`{ "fallbacks": [...] }`, a divergence from TypeScript that predates the catch-up (it is on
`52f10e027`). `WorkerHost.handleFontsAddFallback` returns `wirePack({ tag: 'fonts.addFallback' })`
(`packages/engine/services/src/worker-host/WorkerHost.ts:2410`), which carries no result, and
plugins answers `serde_json::Value::Null` since `7566552a2`. So the merge would have resolved it in
TypeScript's favor anyway, but impl was wrong against TypeScript until then. `18f7ce2b7` fixes it on
impl, and `the_answer_carries_no_payload` in `crates/epdf-engine/tests/font_fallback_tier_wire.rs`
checks that the envelope is `{ "result": null }`. Both sides now answer `Null`, and the conflict is
only textual. `fonts.clearFallbacks` and `fonts.clear` diverged the same way on impl, answering
`{ "fallbacks": [] }` and `{ "registered": 0 }` where `handleFontsClearFallbacks` and
`handleFontsClear` return an empty `wirePack` (`WorkerHost.ts:2417` and `:2445`). `7e5b519bf`
makes both answer `Null`, with a test per kind in the same file. Nothing outside `packages/` and
`api/` read the old keys.

Those two arms are the second `host.rs` hunk. Both sides answer `Null`, so it's textual, and the
resolution is plugins' side: keep the `fonts.authorizeEditing` arm and its neighbours' `Null`
answers. Dropping the arm would turn the kind into an unknown-kind error, because plugins'
`RequestEffect::of` still lists it as `RuntimeWrite` and `PageEffect::of` as `Runtime`, while
`route_wire` would pass it on to `route_session`, whose last arm answers `unknown_kind`. Deleting
impl's two comments doesn't remove the hunk. We tried it on a scratch commit and merge-tree still
reported two `host.rs` hunks: impl's `Null` on the `clearFallbacks` line touches the lines where
plugins inserts the arm, so the conflict comes from the fix itself, and we kept the comments.

The merge resolves two older divergences in the same arms, both in plugins' favor, so we left them
alone on impl. Both are on `52f10e027`, so they predate the catch-up. Impl answers `fonts.register` with
`{ fontKey, registered: <id> }` (`crates/epdf-engine/src/host.rs:269`), where
`handleFontsRegister` answers `{ tag, fontKey, identity: this.fonts.describe(fontKey) }`
(`WorkerHost.ts:2369-2373`); plugins answers the identity since `7566552a2`. Impl also has no
`fonts.authorizeEditing` arm, though `WorkerHost` routes the kind (`WorkerHost.ts:787`) to
`handleFontsAuthorizeEditing` (`:2376-2382`); plugins added the arm in `86e20c356`. The
`fonts.register` arm merges cleanly to plugins' identity answer, and the `fonts.authorizeEditing`
arm is kept in the second `host.rs` hunk.

`crates/epdf-engine/tests/font_fallback_tier_wire.rs` needs two adaptations in the merge, and all
seven of its tests fail without both. We checked this on a scratch merge of `b64099fb9` with
plugins `3dec6b60b`, resolved as above.

The first is the job envelope. Since `de296ac32`, plugins' `JobEnvelope::parse`
(`crates/epdf-engine/src/host.rs`, about `:406-430` there) requires a `jobId` on every kind, an
`effect` on every kind `RequestEffect::of` lists, and an `opId` on a write. The file's `call` helper
(`:20`) sends none of them, so every test fails with `WireFormat: the 'fonts.register' request has
no 'jobId'`. The fix is one line at the top of `call`, `host.fill_envelope(&mut request);`, with the
`request` parameter made `mut`. Plugins' helper is `pub fn fill_envelope(&self, request: &mut
serde_json::Value)` on `Host`, and it fills only the fields the request leaves out. The `mut` takes
the `call` signature past the line width, so run `cargo fmt` on the file afterwards.

The second is `italic`. The file sends `"italic": null` in all three of its `fonts.register` calls:
the `chain` helper (`:41`), `the_answer_carries_no_payload` (`:146`) and `answer_after_a_fallback`
(`:170`). Impl's host decodes `italic` as an optional bool. Plugins' decodes an `i32` and refuses
the `null` with `'italic' is not the shape this operation takes: invalid type: null, expected i32`,
so each becomes `-1`, the C API's "infer". The comment at `:37-38` that explains the `null` goes
stale with it. Its replacement reads: "`italic: -1` is the C API's infer value, as the TypeScript
sends it."

With only the envelope fix, all seven still fail, on `italic`. With both, `cargo test -p epdf-engine
--test font_fallback_tier_wire --test font_registry` exits 0, with 7 and 19 passed. Impl's other
new tests don't go through the wire: `crates/pdfium/tests/document_fonts.rs` calls the `pdfium`
layer, `font_registry.rs` drives `FontRegistry` directly, and `js_number`'s are unit tests. The full
run below finds no other failure in them.

### What else the merge breaks

`cargo test --workspace --no-fail-fast` on that scratch merge, with both adaptations, exits 101:
2847 passed and 3 failed across 290 test runs, and 22 more test binaries don't start. None of impl's
99 fails there, because plugins carries the readers and vectors they need. Everything that fails is
new from the merge:

| Failure | Cause | Class |
|---|---|---|
| `error::tests::codes_are_byte_identical_to_the_typescript_enum` (`crates/epdf-engine/src/error.rs:293`) | Plugins' test reads upstream's `EngineErrorCode.ts`, which now has `GrantPassphraseRequired` for `SharePasswordRequired`, no `StaleBase`, and adds `LayerDeleted` and `VisitorIdentityRequired` | C4 Task 5 parity work |
| `error::tests::codes_serialize_as_their_own_names` | The same drift | C4 Task 5 parity work |
| `every_kind_states_the_effect_its_interface_does` (`crates/epdf-engine/tests/wire_contract.rs:380`) | `worker-protocol.ts` states an effect for 91 kinds against the test's 86: the five font kinds | C4 Task 5 parity work |
| 22 test binaries in `epdf-ffi`, `epdf-web` and `epdf-manual-bridge` | `error while loading shared libraries: libembedpdf.so`. With `LD_LIBRARY_PATH=/opt/pdfium/linux-arm64/lib`, `cargo test -p epdf-ffi -p epdf-manual-bridge -p epdf-web --no-fail-fast` exits 0, with 72 passed | Environmental |

Impl's own error-code test can't see the drift. `TYPESCRIPT_CODES` in `crates/epdf-engine/src/error.rs`
(`:181`) is a hand list of 19, and the test passes on impl today. Upstream's `EngineErrorCode.ts` at
`b64099fb9` has 30 codes, so impl's enum already diverges from TypeScript: it has
`SharePasswordRequired`, `InvalidReference` and `WeakAnnotationSessionConflict`, which TypeScript
doesn't, and lacks 14 that it has. Most of that is older than the catch-up (TypeScript had 29 on
`52f10e027`), and the merge adds the rename, the removal and the two new codes. Plugins' enum has the 29 TypeScript
had before, and its test reads `EngineErrorCode.ts` at run time, which is why it goes red. Bringing impl's enum to TypeScript's is C4
(plan Task 5) work on plugins, not a one-line rename, so we didn't port it on impl.

## Known gap: `EPDFAnnot_GetStructurePosition`

Upstream pins the runtime in two places, and on `a40d63a19` they disagree. The submodule gitlink is
`cff0077cf`. `engine-runtime-build.json`, which decides the artifacts the TS build fetches, names
upstream runtime `28b8b808f` (`3830bfb61`). `28b8b808f` exports `EPDFAnnot_GetStructurePosition`.
`cff0077cf` doesn't, and neither does `8b1e69abd`, which is built on `cff0077cf`.

Upstream's TS calls it from
`packages/engine/services/src/features/annotations/internal/read/annotationReadPrimitives.ts:491`,
and every annotation read now carries `structurePosition` (`6d4d4352d`). After this merge impl's
`engine-runtime-build.json` is upstream's, so a TS build that fetches upstream artifacts has the
export. Anything built on the fork runtime doesn't: the Linux library the containers mount, the
Rust core, and the wasm payload Task 6 publishes. The Rust doesn't call the function yet, so nothing
fails today. A TS oracle run on a fork payload would fail on annotation reads, and vectors
regenerated on upstream artifacts will carry a `structurePosition` the Rust can't read. The fix is
for the fork to merge `28b8b808f` or whatever upstream pins next, which is D1's option 2 or 3. We
haven't patched around it.

## Known gap: eight non-font calls without a binding

Upstream's TS calls 54 `fn.EPDF*` functions on `a40d63a19` that it didn't call on `b456ee4fd`: the
45 font functions above, `EPDFAnnot_GetStructurePosition`, and eight more that this catch-up didn't
port. The fork runtime exports all eight (they are in the `linux-arm64` headers the containers
mount), but nothing under `crates/` names any of them, on impl or on plugins
(`private-fork/research/multi-platform-rust-core-impl-plugins`). They are C4 (plan Task 5) work,
with the TS feature each one serves:

| Function | TS caller, under `packages/engine/services/src/features/` | Feature |
|---|---|---|
| `EPDFDoc_ImportPages` | `pages/PagesInserter.ts`, `pages/PagesExtractor.ts` | Page insert and export: copies pages with the form fields of their widgets |
| `EPDFDoc_GetImportFieldRoots` | `pages/internal/keepFieldNamesApart.ts` | An insert renames the inserted file's top-level fields that clash (`name_2`) |
| `EPDFForm_GetFieldPartialName` | `pages/internal/keepFieldNamesApart.ts` | The same rename |
| `EPDFDoc_GetPageLabelSectionCount` | `pages/internal/pageLabels.ts` | Reading `/PageLabels` (`PagesReader.ts`, `changes/internal/pageChanges.ts`) |
| `EPDFDoc_GetPageLabelSectionAt` | `pages/internal/pageLabels.ts` | The same read |
| `EPDFDoc_SetPageLabelSections` | `pages/internal/pageLabels.ts` | Rewriting `/PageLabels` after a page change (`pageChanges.ts`) |
| `EPDFCatalog_GetMetadata` | `metadata/internal/xmp/xmpPacket.ts` | Reading the catalog's XMP packet, for `syncXmp.ts` |
| `EPDFCatalog_SetMetadata` | `metadata/internal/xmp/xmpPacket.ts` | Writing it back after a metadata change (`MetadataMutator.ts`) |

### C4: the eight calls ported

C4 (`c4/runtime-calls`) binds all eight in `crates/pdfium` and uses them where upstream's TS does:
`pages.insert` and `pages.extract` copy with `EPDFDoc_ImportPages` and keep clashing field names
apart, `pages.insert` takes `objectNumbers`, the new `pages.copy` op runs, page lists carry
`labels` and every page op moves the sections, and `metadata.update` and `metadata.updateCustom`
keep the XMP packet in step. `pages.copy` is a change op inside `document.apply`, not a new
request kind, so the 94-kind count doesn't move.

One test shim is temporary: `test_support::without_unrecorded_labels`, used by
`crates/epdf-engine/tests/pages_corpus.rs` and `pages_mutate.rs`. It drops an empty `labels` list
from the Rust answer when the recorded layout predates the field. Remove it when C5 regenerates
the pages/metadata vectors with the new wasm payload.

Two divergences are proposed, not decided. An XML character reference to a surrogate code
(`&#xD800;`) decodes to U+FFFD, where the TS makes a lone surrogate a Rust string can't hold; it
only matters inside a packet no tool writes. A label section that
`EPDFDoc_GetPageLabelSectionAt` refuses is skipped, where the TS would push one with stale
values; no index below the count is refused.

## Tests

All of these ran in the `embedpdf-c3-impl` container against the `linux-arm64` library built from
`6dfc03697` (same core source as `8b1e69abd`).

| Command | Commit | Exit | Result |
|---|---|---|---|
| `cargo test --workspace --no-fail-fast` | `52f10e027`, old `1461b0c51` library | 101 | 541 passed, 99 failed, 73 binaries |
| `cargo test --workspace --no-fail-fast` | `1109eeea1` | 101 | 545 passed, 99 failed, 74 binaries |
| `cargo test --workspace --no-fail-fast` | `cbb538fee` | 101 | 556 passed, 99 failed, 75 binaries |
| `cargo test -p pdfium --test document_fonts` | `cbb538fee` | 0 | 11 passed |
| `cargo test -p epdf-engine --test font_registry --test font_fallback_tier_wire` | `1109eeea1` | 0 | 16 and 1 passed |
| `cargo clippy -p pdfium -p pdfium-sys -p epdf-engine --all-targets` | `cbb538fee` | 0 | No lint warnings |
| `cargo test --workspace --no-fail-fast` | `798f7b229` | 101 | 565 passed, 99 failed (the same 99), 75 binaries |
| `cargo test -p epdf-engine --lib js_number` | `798f7b229` | 0 | 4 passed |
| `cargo test -p epdf-engine --test font_registry --test font_fallback_tier_wire` | `798f7b229` | 0 | 18 and 4 passed |
| `cargo test -p pdfium --test document_fonts` | `798f7b229` | 0 | 11 passed |
| `cargo clippy -p pdfium -p pdfium-sys -p epdf-engine --all-targets` | `798f7b229` | 0 | No lint warnings |
| `pnpm install --frozen-lockfile` | `cbb538fee` | 0 | Installed |
| `pnpm exec turbo run build --filter=@embedpdf/engine... --cache-dir=target/turbo-cache` | `cbb538fee` | 1 | 5 of 6 tasks; `@embedpdf/engine#build` can't resolve `@embedpdf/engine-runtime-wasm32` |
| `pnpm --filter @embedpdf/tooling-engine-conformance run operations:check` | `cbb538fee` | 1 | The five font kinds have no row; 94 kinds against the 89 documented |
| `cargo test -p pdfium --test document_fonts` | `2e5a14a81` | 0 | 11 passed |
| `cargo clippy -p pdfium --all-targets` | `2e5a14a81` | 0 | No lint warnings |
| `pnpm run -s actions:check` in `tooling/engine-conformance` | `91d208467` | 0 | Current |
| `pnpm run -s operations:check` in `tooling/engine-conformance` | `91d208467` | 1 | 94 request kinds against the 89 documented |
| `cargo clippy -p pdfium --all-targets -- -D warnings` | `332394f16` | 0 | No lint warnings |
| `cargo test -p pdfium --test document_fonts` | `332394f16` | 0 | 11 passed |
| `EMBEDPDF_FORK_TARGET=linux-arm64 cargo clippy -p pdfium --target wasm32-unknown-emscripten -- -D warnings` | `332394f16` | 101 | Only `unnecessary_cast` at `crates/pdfium/src/file_access.rs:112`, which predates the catch-up; `document_fonts.rs` is clean |
| `cargo clippy -p pdfium -p epdf-engine --all-targets -- -D warnings` | `7bf5f760d` | 0 | No lint warnings |
| `cargo test -p epdf-engine --test font_registry --test font_fallback_tier_wire` | `7bf5f760d` | 0 | 18 and 5 passed |
| `cargo test -p pdfium --test document_fonts` | `7bf5f760d` | 0 | 11 passed |
| `cargo test -p epdf-engine --lib js_number` | `7bf5f760d` | 0 | 4 passed |
| `cargo test -p epdf-engine --test font_registry --test font_fallback_tier_wire` | `7e5b519bf` | 0 | 18 and 7 passed |
| `cargo clippy -p pdfium -p epdf-engine --all-targets -- -D warnings` | `7e5b519bf` | 0 | No lint warnings |
| `cargo test --workspace --no-fail-fast` | `7e5b519bf` | 101 | 568 passed, 99 failed (the same 99, by binary and count), 75 binaries |
| `cargo test -p epdf-engine --test font_registry --test font_fallback_tier_wire` | `061adf19f` | 0 | 18 and 7 passed |
| `cargo clippy -p epdf-engine --all-targets -- -D warnings` | `061adf19f` | 0 | No lint warnings; ts-rs prints its four `deserialize_with` parse notes |
| `cargo test -p epdf-engine --test font_fallback_tier_wire --test font_registry` | Scratch merge of `b64099fb9` and plugins `3dec6b60b`, no adaptations | 101 | 0 of 7 passed: no `jobId` |
| The same | The same merge, envelope fix only | 101 | 0 of 7 passed: `italic` is `null` |
| The same | The same merge, both adaptations | 0 | 7 and 19 passed |
| `cargo test --workspace --no-fail-fast` | The same merge, both adaptations | 101 | 2847 passed, 3 failed, 22 binaries don't load `libembedpdf.so` |
| `LD_LIBRARY_PATH=/opt/pdfium/linux-arm64/lib cargo test -p epdf-ffi -p epdf-manual-bridge -p epdf-web --no-fail-fast` | The same merge | 0 | 72 passed |
| `cargo test --workspace --no-fail-fast` | `b64099fb9` | 101 | 568 passed, 99 failed, 75 binaries |

### The 99 cargo failures

All 99 predate the catch-up. The same 99 tests fail on `52f10e027` against the old `1461b0c51`
library, and we compared them message by message: the reasons match too. They fall into three
groups. Counts are tests, and paths are under `crates/epdf-engine/tests/` unless they say
otherwise.

Committed vectors that a later TS regeneration moved past the Rust readers (75 tests). The vector
carries a field, or a shape, the Rust type doesn't:

| Binary | Tests | Reason |
|---|---|---|
| `text_layout.rs` | 13 | `vectors/text/layout.json`: missing field `p1` |
| `annotation_create.rs` | 11 | `create.json`: missing field `weakRefsInvalidated` |
| `annotation_appearance.rs` | 10 | `appearances.json`: missing field `left` |
| `search_dialect.rs` | 8 | `fold.json`, `regex-dialect.json`, `literal.json`: missing field `length`; `parse` and `snippet.json`: missing field `text` |
| `annotation_reader.rs` | 7 | `read.json`: missing field `pageState` |
| `annotation_update.rs` | 6 | `update.json`: missing field `weakRefsInvalidated` |
| `annotation_move.rs` | 5 | `move.json`: missing field `weakRefsInvalidated` |
| `pages_mutate.rs` | 4 | `mutate.json`: a map where a `u32` was expected |
| `annotation_write.rs` | 3 | `write.json`: missing field `mutations` |
| `annotation_delete.rs` | 2 | `delete.json`: missing field `weakRefsInvalidated` |
| `annotation_vocabulary.rs` | 2 | `vocabulary.json` has no `stampName` map; `freeTextIntent` decodes differently |
| `form_effects.rs` | 1 | `vectors/forms/effects.json` doesn't exist |
| `wire_contract.rs` | 1 | `wire/requests.json` lists 89 kinds, the Rust 65 |
| `search_slice_protocol.rs` | 1 | `text` |
| `geometry_corpus.rs` | 1 | `flags is a number` |

Vectors that parse but whose values the Rust no longer matches (16 tests): `geometry_corpus.rs` (5
of 7 pages diverge), `attachment_reader.rs` (`tree`), `document_save.rs` (`annotations`),
`files_write.rs` (`piece-info-set-every-type`), `form_field.rs` (`create-text-bare`),
`form_reader.rs` (`families`), `form_write.rs` (`repair-plain`, and "the import list shrank"),
`metadata_corpus.rs` (`rich`), `metadata_write.rs` (two cases), `pages_corpus.rs` (`rich`:
`namedPages` absent), `piece_info_reader.rs` (`both-holders`), `redaction_apply.rs`
(`whole-page`), `security_state.rs` (`annotate-and-modify`), and
`crates/epdf-wasm/tests/borrowed_page.rs` (5 pages diverge).

Behavior the Rust refuses or decides differently (8 tests). Six are form writes in `form_write.rs`
and `form_field.rs`: three answer `NotImplemented` ("not ported to the runtime 1461b0c51 API"), and
three refuse differently (`radio-clear-refused`, `attach-off-state-refused`, "no payload is
refused").
`schema_parity.rs` has 9 of 22 `MetadataPatch` cases disagreeing with the zod schema, and
`scope_conformance.rs` expands the PDF permission bits without `doc.text.search` and
`doc.text.select`.

`cargo fmt --check` still reports diffs in 15 files we didn't touch, and lists the same 15 on
`52f10e027`: nine in `crates/epdf-engine`, two each in `crates/epdf-wasm` and `crates/pdfium`, and
one each in `crates/epdf-node` and `crates/epdf-uniffi`. They were unformatted before the merge.

### Three conformance checks are red from the merge

Twelve of the 32 `:check` scripts in `tooling/engine-conformance` run without a `wasm32` payload,
and the other 20 need it. We ran the twelve below with `pnpm run <script>` in that directory. Ten
ran on `52f10e027` in a temporary worktree and on `090fe3f55`, the same tree as the merge for these
scripts. `actions:check` and `operations:check` ran at `91d208467`, and on `52f10e027` in a
temporary worktree after building `@embedpdf/engine-runtime` there:

| Script | `52f10e027` exit | `090fe3f55` exit | Result at `090fe3f55` |
|---|---|---|---|
| `schema:check` | 0 | 0 | Current |
| `search:check` | 0 | 0 | Current |
| `scope:check` | 0 | 1 | `DIFFERS scope/parse.json`, `DIFFERS scope/resolve.json` |
| `security:check` | 0 | 0 | Current |
| `render:check` | 0 | 0 | Current |
| `charmap:check` | 0 | 0 | Current |
| `annotation:check` | 0 | 0 | Current |
| `annotation-write:check` | 0 | 0 | Current |
| `layout:check` | 0 | 0 | Current |
| `wire:check` | 0 | 1 | `expected 89 request kinds, the union has 94` |
| `actions:check` | 0 | 0 at `91d208467` | Current |
| `operations:check` | 0 | 1 at `91d208467` | `worker-protocol.ts has 94 request kinds; the plan documents 89` |

`operations:check` is the third red one. The other 20 boot the engine, and the engine doesn't build without the payload (`metadata:check` fails with `Cannot
find module .../packages/engine/main/workers/inline-transport.node.js`).

`scope:check` is red because upstream changed the scope grammar in five commits (`97871d404`,
`bd82ec1af`, `8638dc749`, `ddd26250d`, `3fa96a354`). Regenerating moves six vector lines. In
`parse.json`, `shape-uppercase` (`"Doc.Open"`) still fails, but the issue is now `invalid plugin
permission shape (expected <plugin id>.<action>)` instead of `invalid capability name shape`: a
dotted word whose first segment isn't a core namespace now parses as a plugin permission. In
`resolve.json`, five cases (`pdf-permissions`, `forms-submit-is-grant-only`,
`narrowing-from-bits-not-scope`, `duplicate-scopes`, `order-does-not-matter`) gain
`doc.render.thumbnail` in every expanded set, because `doc.render` now implies it. Nothing else in
either file changes. That count is impl's. On the merged tree with plugins, regenerating moves
seven `resolve.json` cases: those five plus plugins' `grant-minted-never-from-bits` and
`fields-with-pdf-permissions`. `parse.json` moves by the same single `shape-uppercase` line.

`wire:check` is red because `EXPECTED_KINDS` in `src/generate-wire-vectors.ts` is still 89 and the
`WorkerRequest` union has the five font kinds. Plan Task 5 already lists updating it.

`operations:check` stays red on impl until C4 (plan Task 5) adds the five font-kind rows on plugins.
The merge causes it: upstream's `worker-protocol.ts` now has 94 request kinds against the 89
`operations.ts` documents, and the five new ones have no row: `document.fonts.list`,
`document.fonts.program`, `document.fonts.resolve`, `fonts.list` and `fonts.program`. A row names a
port group, an ownership stage and a cancellation case, and the plan and port order have to change
with it, so it belongs with the engine-level font port.

### The Rust scope parser is behind the TS

C4 (plan Task 5) has to port the new scope grammar to `crates/epdf-engine/src/auth/scope/`. We
didn't, because it is TS-parity work and the vectors for it don't exist yet. Upstream's
`packages/engine/core/src/auth/scope/` now has:

- A `plugin` kind of `ParsedScope`: a dotted word outside `CORE_NAMESPACES` parses as
  `<plugin id>.<action>` against `PLUGIN_PERMISSION_SHAPE`, or fails with the new issue text.
  `pluginOfPermission`, `isPluginId`, `pluginPermissionsOf` and `checkPluginPermission` go with it.
- `doc.render.thumbnail` in the closed set, and the implication `doc.render` implies it.
- `scopeNotCovered` in the new `coverage.ts`, the rule for minting only what you hold.

The Rust has none of these. `parse_capability` refuses `protected-display.view` as `unknown
capability`, and `DocCapability` has no `doc.render.thumbnail`.

`scope_conformance.rs` doesn't catch this, and regenerating won't make it. We ran it against
regenerated vectors. `the_grammar_accepts_and_refuses_what_typescript_does` stays green: it
compares only whether a case parses, no parse case is a plugin permission, and both sides refuse
`"Doc.Open"`. `the_closed_capability_set_is_the_same_set` stays green because the vector's list is
`ALL_CAPABILITIES` in `tooling/engine-conformance/src/scope-cases.ts`, a hand list without
`doc.render.thumbnail` (or `doc.sign`, `doc.outline.modify` and the other capabilities it was
already missing). Only `resolution_agrees_with_typescript` moves: it already fails, and now
`doc.render.thumbnail` joins the missing capabilities. So the port also needs parse cases for plugin
permissions, and `ALL_CAPABILITIES` updated by hand, or the gate keeps passing over the drift. The
TS closed set, `KNOWN_CAPABILITIES` in `packages/engine/core/src/auth/scope/parser.ts:14`, is
module-private, and exporting it would be a TS change. The hand list lacks seven of its 28 entries:
`doc.annotate.import`, `doc.forms.import`, `doc.forms.script`, `doc.outline.modify`,
`doc.render.thumbnail`, `doc.sign` and `doc.sign.certify`. Only `doc.render.thumbnail` is new from
the merge. That gap is impl's. On plugins, `tooling/engine-conformance/src/scope-cases.ts` already
has six of the seven, and only `doc.render.thumbnail` is missing.

### The engine build

The engine build fails for want of the `wasm32` payload package. It fails the same way on
`52f10e027`, whose payload hash, `ac356131f67e5d3f`, isn't published either. This tree's payload
hash is `facdbea7f80d8cef`. Neither `embedpdf/runtime-payloads` nor `roanutil/embed-pdf-runtime`
publishes one, and building it is Task 6. That gap is what keeps the other 20 `:check` scripts from
running.
