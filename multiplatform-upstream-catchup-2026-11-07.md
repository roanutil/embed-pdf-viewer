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

The `wasm32-eh` PDFium (which no release publishes, and which `epdf-wasm` links) is being built
from `786765b80` in a separate container, `embedpdf-runtime-build-c7`, into
`/Users/andrew/repos/embed-pdf-runtime-builds/c7-4346c5131/` on the host, following the recipe of
the previous build in `c2-cff0077cf/` beside it. Until it exists, every checkout here builds its
`wasm32` payload with `EMBEDPDF_RUST_CORE=0` (PDFium alone). That is enough for the TS oracle and
the vector generators. The `packages/core-wasm` suites wait for it.

The branch is not pushed yet. Pushing from the container fails because the host's git credential
forwarder (port 18081) is not running.

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

Pending.

### Test suites

`/workspaces/suite.sh` (committed as `docs/research/upstream-catchup-2026-11-07/suite.sh`, beside `prep.sh`) takes `<checkout> <out-dir> [cargo engconf plugconf ts]` runs `cargo test
--workspace --no-fail-fast`, every `:check` script in `tooling/engine-conformance`, the
plugin-conformance `vectors:check`, `coverage:check`, `test:harness` and `typecheck`, and
`turbo run test --continue` over `packages/**`. Failing names land in `<out-dir>/*.failed`.
`LD_LIBRARY_PATH` points at the checkout's `packages/engine/runtime/build/libpdfium/linux-arm64/lib`.
