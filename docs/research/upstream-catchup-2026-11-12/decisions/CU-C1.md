# CU-C1 decisions

Decisions made while preparing the dev container, merging upstream runtime `aa59ce554` into the
runtime fork and building the PDFium libraries. Each one is committed in the same commit as this
file on `research/catchup-2026-11-12-c1`.

## CU-C1-D1. Keep the fork's Android secure-random branch

Decision: keep `3c6e02297` ("Give Android a secure random source") in the merge.
Reason: upstream `aa59ce554` still has no Android branch in `core/fxcrt/epdf_secure_random.cpp`.
Its platform list is `IS_WIN`, `IS_APPLE`, `IS_ASMJS`, `IS_LINUX`, then `#error`, the same as
`4346c5131`. The brief says to prefer upstream's branch only once it has one.
Cost: the fork keeps a 4-line change in an upstream file.
Reverse: when upstream adds an Android branch, take upstream's side of that file in the next merge.

## CU-C1-D2. Clone the viewer from a bundle of the host repository

Decision: `/workspaces/embed-pdf-viewer` comes from `git bundle create --branches --remotes` of
`/Users/andrew/repos/embed-pdf-viewer`. Local branches map to local branches, `fork/*` to `fork/*`,
and the host's `origin/*` (embedpdf) to `upstream/*`. Remotes `fork` and `upstream` point at GitHub.
Reason: impl `e4c17f48d` and its parent are local only; GitHub has neither.
Cost: the clone's remote-tracking refs are as fresh as the host's last fetch, not GitHub's.
Reverse: `git fetch fork` and `git fetch upstream` inside the container.

## CU-C1-D3. linux-arm64 PDFium from upstream's release, not a fork build

Decision: use `libembedpdf-runtime-linux-arm64-aa59ce554.tar.gz` from upstream release
`runtime-aa59ce554aab55879b728dbb86714e13e5cb327f`.
Reason: the release exists, and the fork's delta from upstream is build scripts, patches, tests,
docs and the Android branch. None of it changes a linux-arm64 library. This is what the 2026-11-07
catch-up did for `4346c5131`.
Cost: the linux-arm64 library is built from `aa59ce554`, not from the merge `17d8073d4`. The C++ sources
that differ between them are `epdf_secure_random.cpp` (the Android branch) and
`fpdfsdk/epdf_layer_embeddertest.cpp` (a test); the rest of the 27 files are build scripts,
patches, `skia/BUILD.gn`, args goldens and docs.
Reverse: build linux-arm64 from `17d8073d4` with the fork's `build-target.sh`.

## CU-C1-D4. Helper scripts: impl's `prep.sh`, the brief's Cargo settings, a pin file for wasm32-eh

Decision: the scripts in `scripts/` start from `docs/research/upstream-catchup-2026-11-07/` on impl
`e4c17f48d`, not plugins `7990560d6`. Impl's `prep.sh` (`bc2b75c83`) is newer than the plugins
copy: it puts the clang shim on `PATH` and builds every workspace package. The other scripts match
on both branches. Changes: `prep.sh` and `suite.sh` export `CARGO_INCREMENTAL=0`,
`CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0` and use `CARGO_BUILD_JOBS=4`
(`suite.sh` had 12). `prep.sh` fetches wasm32-eh through `/workspaces/wasm32-eh-pin.json`.
`build-rust-payload.sh` is new: the 2026-11-07 copy lived only on the deleted
`embedpdf-catchup1107` volume.
Reason: the brief's Cargo rules, and later jobs need wasm32-eh without editing a pin.
Cost: `cargo test` in `suite.sh` runs with 4 jobs, slower than 12.
Reverse: edit `/workspaces/suite.sh` in the container.

## CU-C1-D5. wasm32-eh build environment

Decision: container `embedpdf-runtime-build-c1112` from `debian:trixie` (linux/amd64), volume
`embedpdf-runtime-build-c1112-work`, depot_tools from `chromium.googlesource.com`, emsdk 4.0.0
cloned into `third_party/emsdk`. The runtime is a blobless clone of the fork's
`fix/android-secure-random` plus the merge commit from a bundle. The script is
`scripts/build-wasm32-eh.sh`.
Reason: the brief names the container, and the merge is not pushed. Two attempts failed before the
build ran: `debian:trixie` has no `patch`, and `gn` needs depot_tools bootstrapped first
(`ensure_bootstrap`). Both are now steps in the script.
Cost: none for later jobs; the container and volume are removed.
Reverse: rerun the script in a fresh container.

## CU-C1-D6. No fork ledger or sync note for this merge

Decision: the runtime merge commit adds no `docs/embedpdf/syncs/` note and leaves
`docs/embedpdf/fork-ledger.md` as is.
Reason: the merge had no conflicts and the fork delta is byte-identical before and after it. The
previous clean merge, `786765b80`, added none either. The ledger says rules change only with
review.
Cost: the fork's own docs do not mention this merge; the merge commit message and this report do.
Reverse: add a note in a later commit on `catchup/2026-11-12-runtime`.

## CU-C1-D7. Leave the untagged Android blocks reported by `check-fork-tags.py`

Decision: no `EmbedPDF(<rule>)` tag added to `epdf_secure_random.cpp`.
Reason: the two untagged blocks (lines 16 and 41) are `3c6e02297`'s, already present before the
merge. A tag needs a rule in the ledger, and the ledger changes only with review. The check exits 0.
Cost: the tag check keeps reporting 2 untagged blocks.
Reverse: add a ledger rule and tag the blocks in a reviewed runtime commit.

## CU-C1-D8. wasm-bindgen-cli 0.2.127

Decision: install `wasm-bindgen-cli` 0.2.127 with `cargo install --locked`.
Reason: plugins `7990560d6` `Cargo.lock` pins `wasm-bindgen` 0.2.127; impl `e4c17f48d` has no
`wasm-bindgen` in its lock file. Plugins is where the Rust port runs.
Cost: if CU-C2/C3 move the lock to another version, reinstall.
Reverse: `cargo install wasm-bindgen-cli --version <v> --locked --force`.
