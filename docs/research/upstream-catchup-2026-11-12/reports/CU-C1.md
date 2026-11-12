# CU-C1: dev container and runtime fork merge

The dev container `embed-pdf-catchup1112` is up with both clones, the helper scripts, linux-arm64
and wasm32-eh PDFium, emsdk 4.0.0 and `wasm-bindgen-cli` 0.2.127. The runtime fork branch
`catchup/2026-11-12-runtime` (`17d8073d49bab5c29d956568345652bf893b459c`) merges upstream runtime
`aa59ce554` into `3c6e02297` with no conflicts. It is a local branch in
`/Users/andrew/repos/embed-pdf-runtime` and is not pushed. Upstream added 5 exports and removed
none. Decisions are in [`../decisions/CU-C1.md`](../decisions/CU-C1.md).

## Commits

| Repository | Branch | Commit | Summary |
|---|---|---|---|
| runtime fork | `catchup/2026-11-12-runtime` | `17d8073d4` | Merge upstream `aa59ce554` into the fork |
| viewer | `research/catchup-2026-11-12-c1` | this commit | CU-C1 report, decisions and the container helper scripts |

No viewer code, vectors or packages changed, so there are no test counts before or after and no
regenerated vector groups.

## Container layout

Disk before starting: 546 GiB free on `/System/Volumes/Data`; `docker system df` showed 53.41 GB of
images, 110.2 GB of local volumes. Both are in `container-state.log` (evidence below), taken after
the build at 536 GiB free.

Container `embed-pdf-catchup1112`, image `embedpdf-dev:latest` (arm64, `30b97dec3bd8`), volume
`embedpdf-catchup1112` at `/workspaces`, started with `--init` and `sleep infinity`. Git identity is
set globally for user `node`. Use `docker exec embed-pdf-catchup1112 bash -c '...'`.

| Path | Contents |
|---|---|
| `/workspaces/embed-pdf-viewer` | Viewer clone, detached at impl `e4c17f48d`. Local branches from the host, `fork/*`, and `upstream/*` (upstream `main` = `ed064e0c8`). Has `7990560d6` and `fork/research/catchup-2026-11-06` (`a9c2a4157`). |
| `/workspaces/runtime` | Runtime fork, blobless clone. Remotes `fork` (roanutil/embed-pdf-runtime) and `upstream` (embedpdf/runtime). Branch `catchup/2026-11-12-runtime` checked out. |
| `/workspaces/wt/` | Job worktrees, `/workspaces/wt/<JOB-ID>`. Empty. |
| `/workspaces/logs/`, `/workspaces/bundles/` | Logs, git bundles. |
| `/workspaces/pdfium/archives/` | `libembedpdf-runtime-linux-arm64-aa59ce554.tar.gz`, `libembedpdf-runtime-wasm32-eh-17d8073d4.tar.gz` |
| `/workspaces/pdfium/lib/{linux-arm64,wasm32-eh}` | The two archives extracted (a check that they unpack; checkouts fetch their own copy) |
| `/workspaces/wasm32-eh-pin.json` | Pin file for the wasm32-eh archive |
| `/workspaces/emsdk4` | emsdk 4.0.0 (emsdk `708dcbcc9`), for wasm32-eh links. The image's own emsdk at `/opt/emsdk` stays 3.1.72. |
| `/workspaces/bin/clang` | Clang shim (`scripts/clang-shim.sh`) |
| `/workspaces/{prep,prep-extra,suite,regen-engine,build-rust-payload}.sh`, `jdiff.py` | Helper scripts, identical to `scripts/` here (sha256 compared) |

`wasm-bindgen` 0.2.127 is in `/home/node/.cargo/bin`, which is in the container's root filesystem,
not the volume: recreating the container loses it.

## Runtime merge

`git merge --no-ff aa59ce554` on `3c6e02297` had no conflicts. Upstream brought 38 commits (35
non-merge) since `4346c5131`: unit tests, Skia and DIB fixes, dependency rolls, a `DEPS` move to
PDFium `6688c576f`, and the raw viewport API (`55cc10b20`). The fork's diff against upstream is the
same 27 files, 1,306 insertions and 112 deletions, before and after the merge: the diff of
`4346c5131..3c6e02297` and of `aa59ce554..17d8073d4` match line for line once `index` and `@@`
lines are dropped.

Upstream `aa59ce554` still has no Android branch in `core/fxcrt/epdf_secure_random.cpp`, so the
fork's `3c6e02297` stays (CU-C1-D1).

The fork's script tests pass on the merge: `tests/emsdk-version.test.sh` and
`tests/target-args.test.sh`. `check-fork-tags.py --base aa59ce554` exits 0 and reports 2 untagged
blocks, both the Android lines from `3c6e02297` (CU-C1-D7).

### Export diff, `4346c5131` to `aa59ce554`

`EPDF*` functions declared in `public/*.h`: 623 before, 628 after. No `FPDF*` changes.

| Added | Header |
|---|---|
| `EPDFPage_AddViewportRaw` | `public/epdf_measure.h` |
| `EPDFPage_CountViewportsRaw` | `public/epdf_measure.h` |
| `EPDFPage_GetViewportRaw` | `public/epdf_measure.h` |
| `EPDFPage_RemoveViewportRaw` | `public/epdf_measure.h` |
| `EPDFViewport_Close` | `public/epdf_measure.h` |

Removed: none. Nothing for Rust or TS loses a symbol. Upstream viewer `ed064e0c8` uses the new
functions in `packages/engine/services/src/features/measure/MeasureMutator.ts` and
`internal/readViewports.ts`, and lists them in `pdf-functions.generated.ts`. Plugins `7990560d6`
has no reference to them in `crates/` or `packages/`, so CU-C4 (`pdfium`, `epdf-engine`) needs
bindings for these 5 if the engine port follows the measure reads.

## PDFium libraries

| Target | Source | Archive sha256 | Container | Host copy |
|---|---|---|---|---|
| `linux-arm64` | Upstream release `runtime-aa59ce554aab55879b728dbb86714e13e5cb327f` | `da0ec9f9440ab73e9d3cfd3cee83b1152deaaf4317bcf078f923f35aae05faa7` (matches upstream `engine-runtime-build.json` at `ed064e0c8`) | `/workspaces/pdfium/archives/libembedpdf-runtime-linux-arm64-aa59ce554.tar.gz` | none needed: the release is public |
| `wasm32-eh` | Fork build of `17d8073d4`, emsdk 4.0.0 | `f93c5cf92f367116c062d9d7a0ae53806410a5c8bd74dd92fcaf2eb37fe2e2a5` | `/workspaces/pdfium/archives/libembedpdf-runtime-wasm32-eh-17d8073d4.tar.gz` | `/Users/andrew/repos/embed-pdf-runtime-builds/c1112-aa59ce554/` (with `SHA256SUMS`, build script and logs) |

The wasm32-eh archive's `BUILD-METADATA.json` names `17d8073d49bab5c29d956568345652bf893b459c`
and `emsdkVersion` `4.0.0`. Its `lib/libembedpdf.a` has sha256
`315bb221a6f1baea8485881e1fd7d22fb49926c9054afeefd8e1f504ba1aef36`. It differs from the
`3c6e02297` build (`7a160f61…`), as expected after 35 upstream commits.

The build ran in `embedpdf-runtime-build-c1112` (`debian:trixie`, linux/amd64, volume
`embedpdf-runtime-build-c1112-work`): 108 s dependency sync, 163 s build. Attempts 1 and 2 failed
before compiling (no `patch` in the image; depot_tools not bootstrapped), see CU-C1-D5. Their logs
are in the host copy's `logs/attempt1` and `logs/attempt2`.

### How later jobs fetch the libraries

In a checkout whose `packages/engine/runtime/engine-runtime-build.json` pins `aa59ce554` (after
CU-C2), `prep.sh` does both:

```sh
bash packages/engine/runtime/scripts/fetch-libpdfium.sh linux-arm64
PDF_RUNTIME_BUILD_FILE=/workspaces/wasm32-eh-pin.json \
  bash packages/engine/runtime/scripts/fetch-libpdfium.sh wasm32-eh
```

The first downloads upstream's release; the second copies the local archive (`file://` URL) and
checks its sha256. `/workspaces/build-rust-payload.sh <checkout>` then builds the wasm32 payload
with the Rust core: emsdk from `/workspaces/emsdk4`, the pin file above, and
`packages/engine/runtime/scripts/build-target.sh wasm32-eh`. It has not been run yet: impl has no
`epdf-wasm` crate, and plugins has not been caught up.

## Tool versions

| Tool | Version | Where |
|---|---|---|
| emsdk (image) | 3.1.72 | `/opt/emsdk` |
| emsdk (wasm32-eh) | 4.0.0 | `/workspaces/emsdk4` |
| `wasm-bindgen-cli` | 0.2.127 | `/home/node/.cargo/bin` |
| cargo | 1.98.0 | image |
| node | 24.20.0 | image |
| pnpm | 10.34.0 | image |

## Evidence

Viewer branch base `e4c17f48d7e7216b51193e48497056f900e720e9`; head is the commit that adds this
file. Runtime: base `3c6e0229738d19c8f724251bb9c479aecbd18a00`, upstream
`aa59ce554aab55879b728dbb86714e13e5cb327f`, head `17d8073d49bab5c29d956568345652bf893b459c`.
Logs are in `/tmp/cu1112/logs/CU-C1/` on the host, hashed in its `SHA256SUMS`.

| Check | Command | Exit | Log | Log sha256 |
|---|---|---|---|---|
| Merge ancestry, fork delta, export diff, Android branch | script in `CU-C1-runtime-verify.log` (in `/workspaces/runtime`) | 0 | `CU-C1-runtime-verify.log` | `7da28f7c6f1435f0b2e2b3e3214e528e698592a0e30fb6eadf8c5080f92ec9ce` |
| emsdk pin test | `bash scripts/embedpdf-runtime/tests/emsdk-version.test.sh` | 0 | `CU-C1-emsdk-version.test.log` | `4e11ce45a8448fdb27d5fa86a205593efffad143a951fa836ec6473963f03c36` |
| target args test | `bash scripts/embedpdf-runtime/tests/target-args.test.sh` | 0 | `CU-C1-target-args.test.log` | `d485a1179d9c0901e8ddf3479e2cffdac88fb9f2613e13cd4e29c8776b7daecc` |
| Fork tag check | `python3 scripts/embedpdf-runtime/check-fork-tags.py --base aa59ce554` | 0 | `CU-C1-check-fork-tags.log` | `c17d0a54de6b7791f4b1eb3c1e1ad473be1bcee0b5bf7968cad1476aceeedfe8` |
| wasm32-eh build | `heavy-build.sh CU-C1-wasm32eh -- docker exec embedpdf-runtime-build-c1112 bash /work/in/build.sh` (attempt 3) | 0 | `wasm32-eh-run.out` | `b2e2fc76be312fb6e91780fdb12b9c35a781a579205ee59a978cbfcb547fede6` |
| wasm32-eh compile step | `build-target.sh wasm32-eh` | 0 | `wasm32-eh.log` | `9e27182ee3d51d90a4fc233c94e987399b2deb13801fa6acf20660dab84eda60` |
| wasm32-eh package step | `package-target.sh wasm32-eh` | 0 | `package.log` | `cdf0408da5bc398eee2c02ead46227093442ac9cf9966784231d455c40e5bb85` |
| wasm32-eh dependency sync | `ensure-deps.sh` (attempt 1) | 0 | `sync.attempt1.log` | `40223c5dad1434349a5f7427662d70cd1cfb4453bb847a003631d2b9effba285` |
| wasm32-eh attempts 1 and 2 | same as attempt 3 | 1, 1 | `wasm32-eh-run.attempt1.out`, `wasm32-eh-run.attempt2.out` | `af8af680…`, `282a67f2…` |
| emsdk 4.0.0 install | `./emsdk install 4.0.0 && ./emsdk activate 4.0.0` | 0 | `CU-C1-emsdk4.log` | `4092546b6b099540315503156635e137136df90a1ad0db8bda3f2414bf59edbe` |
| wasm-bindgen install | `heavy-build.sh CU-C1-wasmbindgen -- docker exec … cargo install wasm-bindgen-cli --version 0.2.127 --locked` | 0 | `CU-C1-wasm-bindgen-install.log` | `367b7f524691553e986d3d72bbb6cb50cc298b5a1d8f160e1a928d63954a309e` |
| Container state, disk, tool versions | `df`, `docker system df`, `docker ps`, `ls`, `--version` | 0 | `container-state.log` | `5af1e4ffe831b0756e41fdff06a1bb8a3913a02f5155ed1fca2094b6029c5d86` |
| wasm32-eh archive on host | `shasum -a 256 -c SHA256SUMS` in the host copy | 0 | (output `OK`) | |
| wasm32-eh fetch through the pin | `PDF_RUNTIME_BUILD_FILE=/workspaces/wasm32-eh-pin.json fetch-libpdfium.sh wasm32-eh` | 0 | (prints `/workspaces/pdfium/lib/wasm32-eh`) | |

## Known gaps

The runtime branch is local only (manager decision D-04): any `runtime-src` pin to `17d8073d4`
names a commit GitHub doesn't have until the human pushes `catchup/2026-11-12-runtime`.
linux-arm64 comes from upstream's `aa59ce554` release, not the fork merge (CU-C1-D3). No native
addon, payload or test suite has run against the new libraries yet; CU-C2 does the first. The
wasm32 (non-EH) library is not prepared here: checkouts fetch it from upstream's release through
their own pin.

## Cleanup

Removed: container `embedpdf-runtime-build-c1112` and volume `embedpdf-runtime-build-c1112-work`
(after `docker inspect` showed only that volume mounted, and the archive passed `shasum -c` on the
host). The temporary Cargo target `/workspaces/wt/CU-C1-cargo-install` was deleted after the
install. CU-C1 had no viewer worktree or `target/` in the container. Kept for later jobs: container
`embed-pdf-catchup1112` and volume `embedpdf-catchup1112`. Host files: `/tmp/cu1112/` (bundles,
logs) and `/Users/andrew/repos/embed-pdf-runtime-builds/c1112-aa59ce554/`.

