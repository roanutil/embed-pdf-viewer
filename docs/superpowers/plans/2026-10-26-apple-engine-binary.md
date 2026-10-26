# Apple Engine Binary Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `publish-engine-payload.mjs` also builds and publishes an iOS `EmbedPDFBindings.xcframework` (Rust core, fork PDFium, compiled uniffi Swift wrapper), and `Package.swift` downloads it by default.

**Architecture:** `buildFork` learns the two iOS fork targets. `build-xcframework.sh` keeps producing today's local layout; a new `package-engine-framework.sh` compiles the generated Swift into a library-evolution static framework per slice and zips the XCFramework. A separate `apple-engine-<hash>` release, with its own hash script and a stale-pin check, feeds a `.binaryTarget(url:checksum:)` in `Package.swift`.

**Tech Stack:** Node 22 ESM scripts, bash, cargo/uniffi, `swiftc`, `xcodebuild -create-xcframework`, SwiftPM 6.2, `gh`.

**Spec:** `docs/superpowers/specs/2026-10-26-apple-engine-binary-design.md`

## Global Constraints

- Branch `research/apple-engine-binary` (off `research/multi-platform-rust-core-impl-plugins` at `dfd2937d4`), worktree `.claude/worktrees/apple-engine`. Commit with explicit paths. Never push; the user merges into plugins.
- No `.github/**` changes. No TypeScript behavior changes under `packages/**`.
- Release repo `roanutil/embed-pdf-runtime` (public). Tag `apple-engine-<16 hex>`. Asset `EmbedPDFBindings.xcframework.zip`. Prerelease, `--target` the fork commit, as the wasm release does.
- Slices: Rust `aarch64-apple-ios` with fork `ios-arm64`; Rust `aarch64-apple-ios-sim` with fork `ios-sim-arm64`. Deployment target iOS 17.0.
- The wasm32 path of `buildFork`, its `wasm32-eh.json` stamp and `FORK_RECIPE = 3` behave exactly as before. The perf harness depends on them.
- Binary target and module name: `EmbedPDFBindings`. `Sources/EmbedPDF` keeps `public import EmbedPDFBindings` unchanged.
- `EMBEDPDF_LOCAL_ENGINE=1` gives today's manifest layout (C `EmbedPDFFFI.xcframework` path target plus source `EmbedPDFBindings`). `EMBEDPDF_EXPERIMENTAL_BRIDGE=1` implies local mode.
- Fork builds and emsdk cache under `PERF_WORK_DIR` (default `perf-compare/.work`). Use `PERF_WORK_DIR=/Users/andrew/repos/embed-pdf-viewer/perf-compare/.work` from this worktree.
- Rust format with `cargo fmt -p <crate>` only; revert hunks outside your lines.

## Review Focus

1. The cached fork checkout switching between `emscripten` and `ios` dependency sets: a wasm build after an iOS build must not trigger a full gclient resync, and an iOS build on a seeded checkout must fetch iOS deps rather than fail under `PDF_RUNTIME_SYNC=skip`. Pinned in Task 2.
2. A `.swiftinterface` that names a `*FFI` clang module: clients without the C headers fail with "no such module". Pinned in Task 3 by building a client package that has no C headers on its path.
3. Xcode newer than the one that built the framework loading the `.swiftinterface`: must compile from the interface, not fail on the binary module. Pinned in Task 3 by deleting the `.swiftmodule` binaries and keeping only the interface.
4. `Package.swift` pinned to a release whose zip checksum differs (a re-upload): SwiftPM fails with a checksum error. Pinned in Task 6 by the publish script refusing to upload to an existing tag.
5. A crate added to `epdf-viewer-host`'s graph without updating the hash list: must fail before publishing. Pinned in Task 1.

---

### Task 1: Apple engine hash and its crate guard

**Files:**
- Create: `platforms/apple/apple-engine-hash.sh`
- Create: `scripts/lib/linked-crates.mjs`
- Create: `scripts/lib/linked-crates.test.mjs`
- Modify: `scripts/publish-engine-payload.mjs` (replace the inline crate walk, lines that build `linked`/`local`, with the helper)

**Interfaces:**
- Produces: `bash platforms/apple/apple-engine-hash.sh` prints 16 lowercase hex chars and nothing else. Its crate list is a line `APPLE_CRATES=(...)`, same format as `RUST_CRATES`.
- Produces: `linkedLocalCrates(metadata: object, root: string): string[]`, sorted crate directory basenames of every workspace crate (`!p.source`) reachable from package `root` over normal and build deps.
- Produces: `listedCrates(script: string, variable: string): string[]`, sorted entries of the `variable=(...)` line.

- [ ] **Step 1: Write the failing tests** in `scripts/lib/linked-crates.test.mjs` (node:test, as `scripts/resolve-server-recovery.test.mjs` does):
  - `linkedLocalCrates follows normal and build deps, not dev`: a 4-package fixture metadata where `a` depends on `b` (normal), `c` (build), `d` (dev); expect `['a','b','c']`.
  - `linkedLocalCrates drops registry crates`: a dep with `source: 'registry+...'` is absent.
  - `listedCrates reads the array line`: `'X=1\nRUST_CRATES=(b a)\n'` gives `['a','b']`.
- [ ] **Step 2:** `node --test scripts/lib/linked-crates.test.mjs`. Expected: FAIL, module not found.
- [ ] **Step 3: Implement both functions** in `scripts/lib/linked-crates.mjs`, moving the walk out of `publish-engine-payload.mjs` and calling it there for `epdf-wasm` against `RUST_CRATES`.
- [ ] **Step 4:** `node --test scripts/lib/linked-crates.test.mjs`. Expected: PASS.
- [ ] **Step 5: Write `apple-engine-hash.sh`** on the pattern of `packages/engine/runtime/scripts/runtime-payload-hash.sh`: inputs are `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, the git-tracked files of each `APPLE_CRATES` crate (same exclusions: `tests|benches|examples`, `*.md`), `platforms/apple/build-xcframework.sh`, `platforms/apple/package-engine-framework.sh`, and the runtime-src gitlink sha. Fill `APPLE_CRATES` from `node -e` over `linkedLocalCrates(cargo metadata, 'epdf-viewer-host')`.
- [ ] **Step 6: Verify:** `bash platforms/apple/apple-engine-hash.sh` prints 16 hex chars, and the same value twice. Adding a comment line to `crates/epdf-viewer-host/src/lib.rs` changes it; reverting restores it. A missing input file contributes `<path>:-`, as in `runtime-payload-hash.sh`, so the hash works before Task 3 creates `package-engine-framework.sh`.
- [ ] **Step 7:** `node scripts/publish-engine-payload.mjs --dry-run` still reaches "building payload" (stop it there with Ctrl-C; the refactor only has to keep the wasm guard passing).
- [ ] **Step 8: Commit** the four files.

### Task 2: `buildFork` builds the iOS targets

**Files:**
- Modify: `perf-compare/scripts/build-arm.mjs:29-140`

**Interfaces:**
- Produces: `buildFork(dir: string, target: 'wasm32-eh' | 'ios-arm64' | 'ios-sim-arm64' = 'wasm32-eh'): { recipe, sha, seeded, staging, libSha256 }`. `staging/lib/libembedpdf.a` and `staging/include/` exist on return. Stamp file `<root>/<target>.json`.

- [ ] **Step 1: Implement the target parameter.** Per target: wasm32-eh keeps its emsdk setup and env exactly. iOS targets skip emsdk and source nothing. Every target passes the same `PDF_RUNTIME_TARGET_OS_LIST='emscripten ios'`, so the fork's `deps-sync.stamp` stays valid across targets (the wasm path's `PDF_RUNTIME_SYNC` stays `skip` when seeded; iOS uses `auto` when `third_party` lacks the iOS deps, `skip` otherwise; check with the fork's `ensure-deps.sh` `required_paths_exist` list). Clear only `out/embedpdf-runtime/<target>` before building.
- [ ] **Step 2: Verify wasm is untouched:** `git diff` shows no change to the wasm32-eh code path's commands or env; `node -e "import('./perf-compare/scripts/build-arm.mjs').then(m=>console.log(m.buildFork(process.cwd()).libSha256))"` with the cached fork returns `2bc254d7f1a4ea947e831ffe307200cfa1025242ddbeed6502a7dfbe8efb2c0f` immediately (stamp hit).
- [ ] **Step 3: Build iOS (long run, several hours cold):** the same one-liner with `'ios-sim-arm64'`, then `'ios-arm64'`, in the background. Expected: both return; `lipo -info <staging>/lib/libembedpdf.a` reports arm64; `otool -l` on one member shows `LC_BUILD_VERSION` platform 7 (simulator) and 2 (iOS) respectively.
- [ ] **Step 4: Review Focus 1:** rerun the wasm32-eh call. Expected: stamp hit, no gclient output.
- [ ] **Step 5: Commit** `perf-compare/scripts/build-arm.mjs`.

### Task 3: Package the generated Swift into a binary framework

**Files:**
- Modify: `platforms/apple/build-xcframework.sh:40` (`LIB_ROOT="${LIB_ROOT:-$ROOT/packages/engine/runtime/build/libpdfium}"`)
- Create: `platforms/apple/package-engine-framework.sh`
- Modify: `platforms/apple/.gitignore` (add `EmbedPDF/EmbedPDFBindings.xcframework/`)

**Interfaces:**
- Consumes: `platforms/apple/.build/{ios-arm64,ios-sim-arm64}/libEmbedPDFFFI.a`, `.build/generated/*.swift`, `.build/headers/` as `build-xcframework.sh` leaves them.
- Produces: `bash platforms/apple/package-engine-framework.sh` writes `platforms/apple/.build/publish/EmbedPDFBindings.xcframework.zip`, prints its `swift package compute-checksum` value on the last line, and copies the unzipped framework to `platforms/apple/EmbedPDF/EmbedPDFBindings.xcframework` for local verification.

- [ ] **Step 1: Implement the script.** Per slice: copy the generated Swift, rewrite each `import <name>FFI` inside `#if canImport(...)` to `internal import <name>FFI`; compile with `swiftc -parse-as-library -module-name EmbedPDFBindings -swift-version 5 -enable-library-evolution -emit-module-interface -emit-module -emit-library -static -target arm64-apple-ios17.0[-simulator] -sdk <iphoneos|iphonesimulator> -I .build/headers -O -wmo`; `libtool -static` the Swift objects with that slice's `libEmbedPDFFFI.a` into `EmbedPDFBindings.framework/EmbedPDFBindings`; put the `.swiftmodule` directory (interface, `.swiftdoc`, binary module) under `Modules/`; write a minimal `Info.plist`. Then `xcodebuild -create-xcframework -framework ... -framework ...`, `ditto -c -k --keepParent`.
- [ ] **Step 2: Run it:** `LIB_ROOT=<fork root>/runtime-src/out/embedpdf-runtime-staging bash platforms/apple/build-xcframework.sh && bash platforms/apple/package-engine-framework.sh`. Expected: zip exists, last line is 64 hex chars.
- [ ] **Step 3: Review Focus 2:** `grep -l 'FFI' $(find platforms/apple/EmbedPDF/EmbedPDFBindings.xcframework -name '*.swiftinterface')` prints nothing.
- [ ] **Step 4: Review Focus 3:** in a scratch copy of the xcframework, delete every `*.swiftmodule` binary file (keep `.swiftinterface`), point a scratch two-file client package at it (`import EmbedPDFBindings`, call one free function), build for `generic/platform=iOS Simulator`. Expected: BUILD SUCCEEDED.
- [ ] **Step 5: Commit** the three files.

### Task 4: Manifest modes

**Files:**
- Modify: `platforms/apple/EmbedPDF/Package.swift`

**Interfaces:**
- Produces: two top-level constants the scripts rewrite and read, each on its own line: `let engineRelease = "apple-engine-<hash>"` and `let engineChecksum = "<64 hex>"`. Until the first publish both hold the placeholder `"unpublished"`.
- Produces: modes chosen at manifest evaluation: `EMBEDPDF_LOCAL_ENGINE=1` or `EMBEDPDF_EXPERIMENTAL_BRIDGE=1` → today's layout; `EMBEDPDF_PREBUILT_ENGINE=local` → `.binaryTarget(name: "EmbedPDFBindings", path: "EmbedPDFBindings.xcframework")`; otherwise `.binaryTarget(name: "EmbedPDFBindings", url: "https://github.com/roanutil/embed-pdf-runtime/releases/download/\(engineRelease)/EmbedPDFBindings.xcframework.zip", checksum: engineChecksum)`. In both binary modes `EmbedPDF` carries the `c++`, `CoreFoundation`, `CoreGraphics` linker settings and `EmbedPDFBindingsSmokeTests` is absent.

- [ ] **Step 1: Implement the modes.** Keep the experimental block as is; it already rebuilds from the local layout.
- [ ] **Step 2: Verify local mode:** `EMBEDPDF_LOCAL_ENGINE=1 xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17'` in `platforms/apple/EmbedPDF`. Expected: same pass count as before the change (record it first from `HEAD~`).
- [ ] **Step 3: Verify prebuilt mode** with Task 3's framework in place: `EMBEDPDF_PREBUILT_ENGINE=local xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17'`. Expected: all `EmbedPDFTests` and `EmbedPDFUITests` pass.
- [ ] **Step 4: Commit** `Package.swift`.

### Task 5: Stale-pin check

**Files:**
- Create: `platforms/apple/check-apple-engine.sh`

**Interfaces:**
- Consumes: `apple-engine-hash.sh` (Task 1), `engineRelease` line (Task 4).
- Produces: `bash platforms/apple/check-apple-engine.sh [--manifest <path>]`. Exit 0 and print `current apple-engine-<hash>` when the pin matches. Exit 1 when it doesn't and the tree's release isn't published ("publish it: node scripts/publish-engine-payload.mjs --only apple"). Exit 2 when it doesn't but the release exists ("commit the bump: rerun the publish script to rewrite Package.swift"). Both mismatch messages print the pinned and current values.

- [ ] **Step 1: Implement.** `gh release view <tag> --repo roanutil/embed-pdf-runtime` decides published.
- [ ] **Step 2: Verify two exits** against scratch manifests passed with `--manifest`: `apple-engine-0000000000000000` gives exit 1. The tree's own tag (from the hash script) also gives exit 1 while unpublished. Exit 0 and exit 2 need a real published tag, so Task 7 Step 2 checks them.
- [ ] **Step 3:** `shellcheck platforms/apple/*.sh` clean.
- [ ] **Step 4: Commit.**

### Task 6: Publish the Apple half

**Files:**
- Modify: `scripts/publish-engine-payload.mjs`
- Modify: `platforms/apple/README.md` (a section on the three manifest modes, the check script and publishing)

**Interfaces:**
- Consumes: `buildFork(dir, target)` (Task 2), `package-engine-framework.sh` (Task 3), manifest constants (Task 4), `check-apple-engine.sh` (Task 5).
- Produces: `node scripts/publish-engine-payload.mjs [--dry-run] [--only wasm|apple]`.

- [ ] **Step 1: Implement.** Order: clean-tree and runtime-src checks (shared); wasm half unchanged unless `--only apple`; Apple half unless `--only wasm`: skip if `check-apple-engine.sh` exits 0; refuse if the tag already exists and the pin differs only by commit (print the exit-2 advice, don't re-upload: Review Focus 4); `buildFork` for both iOS targets; `build-xcframework.sh` with `LIB_ROOT` at the fork staging root; `package-engine-framework.sh`; `EMBEDPDF_PREBUILT_ENGINE=local xcodebuild test` (as Task 4 Step 3); `gh release create apple-engine-<hash>` with the zip and a `.sha256`, notes listing hash, viewer commit, fork commit, both `libembedpdf.a` sha256s, `rustc --version`, `xcodebuild -version` first line, and the SwiftPM checksum; then rewrite `engineRelease` and `engineChecksum` in `Package.swift` and print "commit platforms/apple/EmbedPDF/Package.swift". `--dry-run` does everything except `gh release create` and the rewrite.
- [ ] **Step 2: Verify:** `PERF_WORK_DIR=... node scripts/publish-engine-payload.mjs --dry-run --only apple`. Expected: ends with the `gh release create apple-engine-...` line it would run and the checksum, tests passed, `Package.swift` untouched.
- [ ] **Step 3: Commit** both files.

### Task 7: First publish and the default switch

Needs the user's go-ahead: it creates a public release.

- [ ] **Step 1:** Ask the user. On yes: `PERF_WORK_DIR=... node scripts/publish-engine-payload.mjs --only apple`.
- [ ] **Step 2:** `bash platforms/apple/check-apple-engine.sh` exits 0 after the rewrite. Save `git show HEAD:platforms/apple/EmbedPDF/Package.swift` (still `unpublished`) to a scratch file and pass it with `--manifest`: with the tree's tag now published, it exits 2.
- [ ] **Step 3: Fresh-checkout proof:** `git clone --branch research/apple-engine-binary <this repo path> <scratch>`, then in `<scratch>/platforms/apple/EmbedPDF` with no env vars: `xcodebuild build -scheme EmbedPDF-Package -destination 'generic/platform=iOS Simulator'`. Expected: SwiftPM downloads the zip, BUILD SUCCEEDED, and no cargo or fork step ran.
- [ ] **Step 4: Commit** `Package.swift` with the release in the message.
