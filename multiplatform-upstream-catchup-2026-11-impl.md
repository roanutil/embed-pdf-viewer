# November 2026 catch-up: impl

This is the record of bringing `research/multi-platform-rust-core-impl` up to upstream `main`. It is
Task 3 of `multiplatform-upstream-catchup-2026-11.md` (committed on `research/catchup-2026-11-plan`
at `867c1c036`), plus the part of D6 that belongs on impl. Plugins is Task 4 and follows from here.

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
- `798f7b229` makes a wire `tier` coerce the way the TS does, below.

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

Rust now does the same. `crates/epdf-engine/src/js_number.rs` implements `ToNumber` over every JSON
value (strings by the `StringNumericLiteral` grammar, arrays through their `join`) and `ToInt32`.
The host decodes `tier` with it and no longer refuses a value as `WireFormat`. The registry passes
`ToInt32` of the tier to PDFium: 1.7 is tier 1, -0.5 is 0, `null`, `"own"` and `{}` are 0, `"1"`
is 1, 2^32 + 1 is 1, and 2^31 wraps to -2^31. `crates/epdf-engine/tests/font_fallback_tier_wire.rs`
checks each against the chain PDFium actually tries.

The registrar also keeps the raw tier and reports `fallbackKeys` sorted by `left.tier -
right.tier`, so the reported order uses the number itself, and 1 comes before 1.7 although PDFium
tries both as tier 1. Rust's `fallback_order` does the same. One case can't match exactly, and we
propose it as a Rust-only exception for the human to decide. When a tier's ToNumber is NaN (a word,
an object), `Array.prototype.sort` counts the difference as equal. The comparator is then not an
order, and the TS answer depends on the JavaScript engine's sort algorithm, so V8, JavaScriptCore
and SpiderMonkey can disagree. Rust answers what a stable insertion sort makes of the same
comparator, because `sort_by` may panic on a comparator that isn't total. This affects only the
reported order. PDFium's chain matches exactly. Nothing on impl reports the order to a TS caller
yet. `fonts.list` does once C4 ports it.

## The font port, split between impl and plugins

Impl has the pdfium-layer bindings for the 47 font calls in upstream's TS: a registered font's
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
them. The cost is that the engine-level font port, the larger half of D6, all lands in Task 4.

For Task 4, two details. `crates/epdf-engine/tests/font_fallback_tier_wire.rs` sends `"italic":
null` because impl's host still decodes `italic` as a bool, so it needs `-1` once plugins' decoding
wins the merge. And plugins' callers of `FontRegistry::add_fallback(key)` and
`pdfium::add_fallback_font` need the tier: the first keeps compiling, the second takes `(id, 0)`,
and `StartupFont` and `add_fallback_in_tier` take the tier as an `f64`.

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

`cargo fmt --check` still reports diffs in 11 files we didn't touch. They were unformatted before
the merge.

### `operations:check` is red on impl

`operations:check` stays red on impl until C4 adds the five font-kind rows on plugins. The merge
causes it: upstream's `worker-protocol.ts` now has 94 request kinds against the 89 `operations.ts`
documents, and the five new ones have no row:
`document.fonts.list`, `document.fonts.program`, `document.fonts.resolve`, `fonts.list` and
`fonts.program`. A row names a port group, an ownership stage and a cancellation case, and the plan
and port order have to change with it, so it belongs with the engine-level font port in Task 5.

The engine build fails for want of the `wasm32` payload package, the same gap as below. It fails
the same way on `52f10e027`, whose payload hash, `ac356131f67e5d3f`, isn't published either.

The TS `:check` scripts in `tooling/engine-conformance` didn't run. They need a `wasm32` payload for
this tree's payload hash, `facdbea7f80d8cef`. Neither `embedpdf/runtime-payloads` nor
`roanutil/embed-pdf-runtime` publishes one, and building it is Task 6.
