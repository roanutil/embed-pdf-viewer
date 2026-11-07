# Upstream catch-up, 2026-11-07

This is the ledger for bringing `research/multi-platform-rust-core-impl` ("impl") and
`research/multi-platform-rust-core-impl-plugins` ("plugins") up to upstream `main`. Upstream is
canonical. The Rust port, the conformance tooling and the `platforms/` wrappers follow it. The
method is the catch-up guide at `docs/research/upstream-catchup/CATCHUP-GUIDE.md` on fork branch
`research/catchup-2026-11-06` (a separate, earlier attempt; nothing else from that attempt is used
here).

## Pinned targets

| What | Commit |
|---|---|
| Upstream viewer `embedpdf/embed-pdf-viewer` `main` | `074a190c910a66181fe7294737f7de55e279b189` (2026-11-07 01:36 +0200, PR #1020) |
| Upstream runtime `embedpdf/runtime`, pinned by that `main` | `4346c5131cd009c4086fdce13bde7fd43fbd4ce0` |

These stay fixed for this catch-up. If upstream moves, that is a separate step.

## Starting points

| Branch | Commit | Runtime pin (`runtime-src`) |
|---|---|---|
| fork `research/multi-platform-rust-core-impl` | `5a2f25f063054c344ea514c2f6da788ad45052f3` | `8b1e69abd` |
| fork `research/multi-platform-rust-core-impl-plugins` | `8fa01cc1ec4d0739af535feec6928e6a81163fd9` | `82f2dfabe` |
| runtime fork, plugins' pin | `82f2dfabe2542208c783d49e210ed87f214ac0c3` | |

Impl is an ancestor of plugins. Both share merge base `a40d63a199f2d483fd27c9ad439f0d5efbc195aa`
with upstream `main`, which is 189 commits behind the target (63 non-merge commits under
`packages/`, 1,132 files changed there).

## Working branches

| Branch | Remote | Purpose |
|---|---|---|
| `catchup/2026-11-07-runtime` | `roanutil/embed-pdf-runtime` | Runtime fork merged with upstream runtime `4346c5131` |
| `research/catchup-2026-11-07-impl` | `fork` | Upstream `main` merged into impl, plus port work |
| `research/catchup-2026-11-07-plugins` | `fork` | The impl catch-up merged into plugins, plus port work |

The canonical branches move only after the user approves.

## Where the work runs

Everything runs in the dev container `embed-pdf-catchup1107` (image `embedpdf-dev:latest`, built
from `.devcontainer/Dockerfile`), with its own volume `embedpdf-catchup1107` mounted at
`/workspaces`. The `embed-pdf` container and `embedpdf-workspace` volume belong to the earlier
attempt and are not touched.

| Path in the container | Checkout |
|---|---|
| `/workspaces/embed-pdf-viewer` | Plugins baseline, detached at `8fa01cc1e` |
| `/workspaces/wt/impl-base` | Impl baseline, detached at `5a2f25f06` |
| `/workspaces/wt/impl` | `research/catchup-2026-11-07-impl` |
| `/workspaces/wt/plugins` | `research/catchup-2026-11-07-plugins` |
| `/workspaces/runtime` | Runtime fork (blobless clone) |

Each checkout is prepared with `/workspaces/prep.sh <dir>`: `pnpm install`, the `linux-arm64`
PDFium from `engine-runtime-build.json`, the `wasm32` payload (fetched, or built locally with
`build:target wasm32` when no release matches the checkout's payload hash), and the engine TS
builds. `CARGO_TARGET_DIR` is `<dir>/target`. Logs go to `/workspaces/logs/`.

## Progress

### Runtime

`catchup/2026-11-07-runtime` at `786765b80` merges upstream `4346c5131` into `82f2dfabe`. The
merge had no conflicts. Upstream's 42 commits add two exports, `EPDFAnnot_SetNoColor` and
`EPDFAnnot_SetSubtype`, remove none, and move `DEPS` (PDFium sync to `2b52a82ab`). The fork's
diff against upstream after the merge is build scripts, patches, args goldens,
`fpdfsdk/epdf_layer_embeddertest.cpp` and docs, 25 files.

The fork's `wasm32-eh` PDFium (which no release publishes, and which `epdf-wasm` links) was built
from `786765b80` in container `embedpdf-runtime-build-c7` (debian trixie, linux/amd64, volume
`embedpdf-runtime-build-c7-work`, emsdk 4.0.0, 263 s after a 127 s dependency sync). The archive
is `/Users/andrew/repos/embed-pdf-runtime-builds/c7-4346c5131/libembedpdf-runtime-wasm32-eh-786765b80.tar.gz`,
sha256 `2ae6869dcc29da304c950d0fc1a77f88332d88d7618c68d493d2fcdafa92bc8d`, and its
`BUILD-METADATA.json` names `786765b8048312ce1def201baf9680a4d0347fdd`. A copy sits at
`/workspaces/` in the dev container, with a pin file `/workspaces/wasm32-eh-pin.json` for
`PDF_RUNTIME_BUILD_FILE=/workspaces/wasm32-eh-pin.json fetch-libpdfium.sh wasm32-eh`. The fork's
`linux-arm64` and `wasm32` are not rebuilt: the checkouts use upstream's `runtime-4346c5131`
release for both, and the fork's diff from upstream is build-only.

Pushed to `roanutil/embed-pdf-runtime` on 2026-11-07. The host's git credential forwarder listens
on port 10895; pass `-e GIT_CREDENTIAL_FORWARDER_SERVER=host.docker.internal:10895` to
`docker exec` for pushes.

### Container environment notes

- Use `bash -c`, not `bash -lc`, with `docker exec`: a login shell drops emsdk from `PATH`.
- `packages/engine/runtime/build/generate-functions.mjs` parses the PDFium headers with the host
  `clang`. On Linux LP64, `uint64_t` resolves to `unsigned long`, which the generator classifies
  as `i32`; on macOS it is `unsigned long long` (`i64`). `EPDFAttachment_ExtractFile` and
  `EPDFAttachment_ExtractFileToOwnedBuffer` then fail `verify-wasm-abi.mjs` (5 arguments against
  the export's 6). The shim `/workspaces/bin/clang` (committed as `clang-shim.sh`) parses the AST
  against the emscripten sysroot; the regenerated `pdf-functions.generated.ts` then matches the
  committed file byte for byte. `prep.sh` puts it first on `PATH`. No repo code changed.
- `packages/rust-viewer-bundle` fails to build until the `wasm32` payload carries the Rust core.

### Impl merge

`825e12e8e` merges upstream `074a190c9` into impl. Three conflicts:

| Path | Resolution |
|---|---|
| `.agents/skills/embedpdf-conventions/SKILL.md` | Upstream's table (adds the `api-evolution.md` row) plus impl's `engine-lifecycle.md` row |
| `package.json` | Upstream's `ci:publish` (drops `--tag next`), impl's `test:viewer:*` scripts kept |
| `packages/engine/runtime/runtime-src` | `786765b80` |

`engine-runtime-build.json` is upstream's, pinned to upstream release `runtime-4346c5131`, whose
`linux-arm64` archive the Rust tests link. The clean merges in
`packages/engine/main/src/index.ts`, `WorkerHost.ts` and `.github/workflows/release.yml` carry only
impl's own hunks against upstream.

### Baselines

Results live in `/workspaces/results/<name>/` in the container (`cargo.failed`, `engconf.all`,
`plugconf.all`, `ts.log`).

Impl at `5a2f25f06` is red before anything changes: `cargo test --workspace` passes 568 and fails
99, all in `crates/epdf-engine/tests` and all on stale-shape vectors (`missing field ...`); 10 of 32
engine `:check` scripts fail (`scope`, `metadata`, `annotation-read`, `forms`, `annotation-create`,
`annotation-update`, `operations`, `pages-mutate`, `wire`, `form-apply`). The earlier catch-up
regenerated impl's vectors and left the Rust port to plugins, and this one follows that: impl gets
the merge, the generator fixes and the regenerated vectors; the Rust port happens on plugins.

Plugins at `8fa01cc1e`: `cargo test --workspace` passes 3,187 and fails none (1 ignored). Every
engine `:check` script passes. Every plugin-conformance check fails (`vectors:check`,
`coverage:check`, `test:harness`, `typecheck` with 70 errors): the generator imports
`packages/core/annotation/src/font-css`, removed by `595fa2bd5` before the start, and plugin models
renamed before the start. So the plugin vectors were already not regenerable at the start; the
committed ones still replay in Rust. TS suites: `@embedpdf/engine` native tests need the
`linux-arm64` addon (`build:target linux-arm64`, now in `prep-extra.sh`) and the viewer tests need
Playwright browsers; rerun after `prep-extra.sh`.

### Impl after the merge

The merged impl compiles and its cargo failures are the same 99 as its baseline. The generators had
9 type errors against upstream's TS, fixed in `dc4e31dc6`: `changed` -> `changedRefs` (26a686502),
form `family` -> `kind` (c4b286a69), `decodePdfBits`/`PdfBits` -> `permissionFlagsOf`/
`PdfPermissionFlags` (a780243ba), and `advisoryFromPdfBits` removed with no replacement (a780243ba),
whose cases now record the flags and what `materializePdfPermissions` grants on them. Every group was
regenerated in `7fe03c102` (deterministic over two runs); the commit message traces each change.
`wire:check` still fails as at the start (it expects 89 request kinds; the union has 94).

### Plugins

`702dacba8` merges the impl catch-up into plugins (only `runtime-src` conflicted), `f8958b69f`
merges impl's generator and vector commits (the scope generator conflicted; vectors taken from impl
and regenerated after). On plugins: `f55f8c35a` (`SignatureDTO` -> `Signature`, `contentsSize` ->
`contentsSizeBytes`), `1dbde8772` (the fonts generator sent `displayHash`, renamed `displaySha256` in
26a686502, so three program reads regenerated as `NotFound`; caught by a regenerated answer that
contradicted its case), `cc2d9490f` (every engine group regenerated; `operations:check` and
`wire:check` pass).

On `cc2d9490f`, `cargo test --workspace` passes 3,077 and fails 110, all in `epdf-engine` (vector
shape and behaviour changes above).

Phase 1 runs four workers in their own worktrees and branches, briefed by
`/workspaces/AGENT-BRIEF.md` (copied to `docs/research/upstream-catchup-2026-11-07/AGENT-BRIEF.md`):

| Worker | Branch | Area |
|---|---|---|
| e1 | `research/catchup-1107-e1` | `epdf-engine` annotations: annotation_*, attachments, actions, submit flags |
| e2 | `research/catchup-1107-e2` | `epdf-engine` everything else: forms, security, scope, signature, pages, text, save, fonts, error codes, wire |
| t1 | `research/catchup-1107-t1` | plugin-conformance shared sources and the annotation, form, fonts, stamp, signature, measurement, redaction and acrojs suites |
| t2 | `research/catchup-1107-t2` | plugin-conformance actions, attachment, commands, i18n, kernel, link, metadata, outline, page-edit, render, search, selection, shell, stage, ui-core and view-manager suites |

Phase 2, after integrating phase 1 on the plugins branch: regenerate every plugin vector and port
the Rust plugin crates to replay them; then `coverage:check` (new upstream TS paths need cases), the
`core-wasm` suites on a payload with the Rust core (the `wasm32-eh` build above), and the
`platforms/` wrappers.

### Test suites

`/workspaces/suite.sh` (committed as `docs/research/upstream-catchup-2026-11-07/suite.sh`, beside `prep.sh`) takes `<checkout> <out-dir> [cargo engconf plugconf ts]` runs `cargo test
--workspace --no-fail-fast`, every `:check` script in `tooling/engine-conformance`, the
plugin-conformance `vectors:check`, `coverage:check`, `test:harness` and `typecheck`, and
`turbo run test --continue` over `packages/**`. Failing names land in `<out-dir>/*.failed`.
`LD_LIBRARY_PATH` points at the checkout's `packages/engine/runtime/build/libpdfium/linux-arm64/lib`.

### Phase 1 and 2 results (2026-11-07)

Integrated on `research/catchup-2026-11-07-plugins` (each worker's branch is kept; reports and
open decisions are summarised in `docs/research/upstream-catchup-2026-11-07/decisions.md`):

| Worker | Branch tip | Result |
|---|---|---|
| t1, t2 | `5a3bab4af`, `cde185217` | plugin-conformance generator repaired (465 type errors to 0); every plugin vector regenerated, `bc47af9ac` |
| e2 | `d2621b12c` | engine: permission flags, form `kind`, units and hashes, page text `\n`/`charMap` (found a latent `CharMapAnchor` serialisation bug), error codes, `mimeType` |
| e1 | `b79bd6eba` | engine annotations: `changedRefs`, `md5`/`sizeBytes`, flat submit flags, rich-text comments, `color: null` via `EPDFAnnot_SetNoColor`, dash defaults, sibling kind changes via `EPDFAnnot_SetSubtype` |
| merge | `b98e3ecb7` | e1/e2 overlap resolved; `epdf-engine` 1,494 passed, 0 failed (110 failed at `cc2d9490f`) |
| pb1 | `23e81404f` | form-core text layout ported from `packages/core/text` (core-text); 146 cases that only recorded removed functions rewritten against the new API; 19/19 |
| pb2 | `a0b0507be` | acrojs, fonts, signature, signature-core, stamp, form plugin |
| pd | `b23655aa5` | kernel, plugin-conformance harness, stage (paged flow and page turning), stage-core, search, render settings, selection, view-manager, ui-core |
| pa | `4930f667d` | annotation-core and annotation plugin |
| integrator | `fa70c1bbc` | `epdf-capi` helper tests send `charMap` anchors as pairs |

On 2026-11-07 the host disk filled and the container's volume went read-only (`emergency_ro`) in
the middle of pa, pb2, pc and pd. Nothing was lost (pd's uncommitted work was intact; `git fsck`
clean). About 527 GB of `target/` caches were deleted, and every result from after the failure was
rerun on a cold build. Workers now build with `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0
CARGO_PROFILE_TEST_DEBUG=0`, at most two or three at a time, and delete `target/` when done.

`cargo test --workspace` on the integrated branch after pa's merge: 3,177 passed, 30 failed, the
failures in pc's crates (not yet merged) and the 23 annotation-core scene cases below.

The `wasm32` payload with the Rust core links as `wasm32-eh` with emsdk 4.0.0, installed beside the
container's 3.1.72 at `/workspaces/emsdk4`; `/workspaces/build-rust-payload.sh` builds it, then
`core-wasm` (needs `wasm-bindgen-cli` 0.2.127, installed). Before phase 2, `core-wasm` passed 75
of 91.

Phase 3: tx (shared text crate extracted from form-core, annotation-core redaction labels through
it, `text-fonts.ts`), pc's rerun and cross-crate renames, then the snapshot tool and turn gestures,
`coverage:check`, `core-wasm`, the FFI and `platforms/` rename pass, and the TS suites.
