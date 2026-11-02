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
`pdfium::add_fallback_font` need the tier: the first keeps compiling, the second takes `(id, 0)`.

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
| `pnpm install --frozen-lockfile` | `cbb538fee` | 0 | Installed |
| `pnpm exec turbo run build --filter=@embedpdf/engine... --cache-dir=target/turbo-cache` | `cbb538fee` | 1 | 5 of 6 tasks; `@embedpdf/engine#build` can't resolve `@embedpdf/engine-runtime-wasm32` |
| `pnpm --filter @embedpdf/tooling-engine-conformance run operations:check` | `cbb538fee` | 1 | The five font kinds have no row; 94 kinds against the 89 documented |

The 99 failures predate the catch-up. The same 99 tests fail on `52f10e027` for the same reasons,
compared message by message. Most of them are committed vectors that later TS regenerations moved
past the Rust readers. For example, `vectors/annotation/create.json` has no `weakRefsInvalidated`,
`vectors/text/layout.json` has no `p1`, and `wire/requests.json` lists 89 kinds against the Rust's
65. The rest are refusals of form writes "not ported to the runtime 1461b0c51 API", plus
`scope_conformance`, `schema_parity` and `epdf-wasm`'s `borrowed_page`. `cargo fmt --check` still
reports diffs in 11 files we didn't touch. They were unformatted before the merge.

Two failures are new, and the merge causes both. `operations:check` reads upstream's
`worker-protocol.ts`, which now has five kinds `operations.ts` has no row for:
`document.fonts.list`, `document.fonts.program`, `document.fonts.resolve`, `fonts.list` and
`fonts.program`. A row names a port group, an ownership stage and a cancellation case, and the plan
and port order have to change with it, so it belongs with the engine-level font port in Task 5.
The engine build fails for want of the `wasm32` payload package, the same gap as below. It fails
the same way on `52f10e027`, whose payload hash, `ac356131f67e5d3f`, isn't published either.

The TS `:check` scripts in `tooling/engine-conformance` didn't run. They need a `wasm32` payload for
this tree's payload hash, `facdbea7f80d8cef`. Neither `embedpdf/runtime-payloads` nor
`roanutil/embed-pdf-runtime` publishes one, and building it is Task 6.
