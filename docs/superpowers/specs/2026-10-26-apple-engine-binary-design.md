# A downloadable engine binary for the Swift package

Today I opened `platforms/apple/EmbedPDF` in Xcode and got a wall of missing-symbol errors. The cause turned out to be a stale Tuist project next to `Package.swift`, but the conversation that followed asked a better question. Why does building the Swift package need a local PDFium fork build and a Rust toolchain at all?

It shouldn't. The wasm32 engine already ships as a release, `runtime-payloads-ef318e5cb82aac78` on [`roanutil/embed-pdf-runtime`](https://github.com/roanutil/embed-pdf-runtime/releases/tag/runtime-payloads-ef318e5cb82aac78), published by `scripts/publish-engine-payload.mjs`. iOS has no equivalent. `Package.swift` points at `.binaryTarget(path: "EmbedPDFFFI.xcframework")`, a gitignored file that `platforms/apple/build-xcframework.sh` assembles from local fork slices and a local cargo build.

So the goal is one downloadable thing. A fresh checkout of this branch opens `Package.swift`, SwiftPM fetches the engine binary, and `EmbedPDF` builds. No fork toolchain, no cargo, no generated files in git.

## What "the engine binary" has to contain

My first instinct was that the binary is the XCFramework `build-xcframework.sh` already makes: the Rust core (`epdf-viewer-host` and every plugin crate it links) with the fork's `libembedpdf.a` folded in, for `ios-arm64` and `ios-arm64-simulator`.

But that binary exports C functions. Nothing in `EmbedPDF` calls those. It calls the Swift types uniffi generates into `Sources/EmbedPDFBindings/` (`PdfDocument`, the error enums, the callback protocols), and those wrappers are welded to the binary they came from. They call C symbols by exact name and check uniffi's API checksums at load. Wrappers from another build fail to compile or fail at runtime with a checksum mismatch.

And SwiftPM can download binaries, not source files.

So the decision (the user's) is that the published XCFramework contains a compiled `EmbedPDFBindings` Swift module alongside the Rust and PDFium code. The Swift wrapper ships as part of the binary.

(The other two options were committing the 21k lines of generated Swift next to a URL binary target, or a fetch script that drops the generated Swift into place. Both keep generated code somewhere a human has to think about it. The README already argues against reviewing uniffi output in a diff, and I agree with it.)

## Shape of the published artifact

Each slice of the XCFramework becomes a static framework, `EmbedPDFBindings.framework`, holding:

- the merged static archive (Rust core, PDFium, and the compiled Swift wrapper objects),
- `Modules/EmbedPDFBindings.swiftmodule` with a `.swiftinterface` for the slice's target triple,
- whatever the interface needs to resolve its own imports.

That last item is where I expect most of the debugging. The generated Swift does `#if canImport(epdf_ffiFFI) import epdf_ffiFFI`, and so on for each of the 22 `*FFI` clang modules. If those imports land in the public `.swiftinterface`, every client has to find the C headers too. Either the headers and a modulemap ship inside the framework, or the build rewrites those imports to `internal import` (or `@_implementationOnly import`) so they never appear in the interface. I'd try the access-level rewrite first, since it keeps the C surface private, but I haven't verified that uniffi's generated code compiles with it under library evolution. That's for the builder to find out.

The wrapper has to be compiled with library evolution (`-enable-library-evolution`, which is what `BUILD_LIBRARY_FOR_DISTRIBUTION` sets) so the `.swiftinterface` exists and a newer Swift compiler can still read it. The cost is that `EmbedPDF` can't inline across that boundary. I think that's noise next to the cost of an FFI call, but nobody has measured it.

The binary target is named `EmbedPDFBindings` so `public import EmbedPDFBindings` in `Sources/EmbedPDF` keeps working untouched. The linker settings currently on the source target (`c++`, `CoreFoundation`, `CoreGraphics`) need a home. A framework can't declare them for a static archive, so either a thin source target wraps the binary and carries them, or `EmbedPDF` declares them itself. Builder's call.

## Where iOS PDFium comes from

`buildFork` in `perf-compare/scripts/build-arm.mjs` builds the fork at runtime-src's recorded commit, cached under `PERF_WORK_DIR/fork/<sha>`. It only knows `wasm32-eh`. It hard-codes `FORK_TARGET`, `PDF_RUNTIME_TARGET_OS_LIST=emscripten` and the emsdk setup.

The fork itself already supports both iOS targets. `scripts/embedpdf-runtime/target-args.sh` at `7c7b600b1` has `ios-arm64` and `ios-sim-arm64` cases with `ios_deployment_target="17.0"`.

So `buildFork` grows a target parameter, and the publish script calls it for `ios-arm64` and `ios-sim-arm64`. The cached fork at `7c7b600b1` has never synced iOS dependencies, so the first iOS build will mean a gclient sync with `target_os=ios` and a cold PDFium compile per slice. I'd budget several hours for that first run and minutes after it.

The existing wasm32 path, its stamp file and `FORK_RECIPE` have to keep working exactly as they do now, because the perf harness depends on them.

## A separate hash and a separate release

The wasm payload hash (`runtime-payload-hash.sh`) covers the crates `epdf-wasm` links, the fork commit and `build-arm.mjs`'s constants. The XCFramework links a much bigger set: `epdf-viewer-host`, uniffi, every plugin's ffi crate. If both shared one tag, a plugin change would leave the hash alone and publish nothing, and every checkout would download a stale binary with no warning.

So the Apple artifact gets its own hash and its own release, `apple-engine-<hash>`. The hash is over the workspace crates `epdf-viewer-host` links (normal and build deps), `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, the fork commit, `build-xcframework.sh`, and whatever new script compiles the Swift wrapper. Like `RUST_CRATES` today, the crate list is checked against `cargo metadata` before publishing, so a new dependency fails loudly instead of going unhashed.

`publish-engine-payload.mjs` publishes both. Each half is skipped when its release already exists, so a Rust change that only touches a plugin publishes only the Apple release. A `--dry-run` builds and verifies both without creating anything. That's my call. The user asked for one script, not one release.

## How `Package.swift` consumes it

After uploading the zip, the publish script rewrites the binary target in `Package.swift` to:

```swift
.binaryTarget(
  name: "EmbedPDFBindings",
  url: "https://github.com/roanutil/embed-pdf-runtime/releases/download/apple-engine-<hash>/EmbedPDFBindings.xcframework.zip",
  checksum: "<swift package compute-checksum output>"
),
```

That edit leaves the tree dirty on purpose. The release notes record the viewer commit the binary was built from, and the `Package.swift` bump is a follow-up commit the user makes. That's my call too. It keeps the publish script's "clean tree, release names HEAD" rule intact.

Local Rust work still needs the old path. With `EMBEDPDF_LOCAL_ENGINE=1` set at manifest evaluation, `Package.swift` goes back to exactly today's layout: the C-only `EmbedPDFFFI.xcframework` from `build-xcframework.sh` as a path binary target, and `EmbedPDFBindings` as a source target over the generated Swift. That's the same mechanism `EMBEDPDF_EXPERIMENTAL_BRIDGE` already uses to switch the manifest. (I first wrote that both modes should see an identical prebuilt framework. The reviewer pointed out that this breaks the smoke tests in both modes, so that's gone.)

So the modes differ on purpose. `build-xcframework.sh` keeps producing what it produces now, and a new packaging step, run only by the publish script, compiles the generated Swift into the framework shape above. `EmbedPDF` sees a module named `EmbedPDFBindings` either way.

The cost is that the published shape is only exercised when someone publishes. The publish script should build `EmbedPDF-Package` against the freshly packaged framework (through a local path override, before uploading) so a broken framework never reaches the release.

Two things then only work in local mode, and that's fine. `EmbedPDFBindingsSmokeTests` uses `@testable import EmbedPDFBindings`, which needs the source target. The experimental manual bridge (`EmbedPDFManualBridge`, `EmbedPDFBridgeBenchmarks`) imports `EpdfManualFFI` from the C xcframework and relies on the manual bridge sharing `epdf-viewer-host`'s staticlib. Both keep today's shape untouched, and the experimental flag implies local mode.

## Catching a stale pin

The pin in `Package.swift` is a snapshot. After a plugin change, and before anyone publishes and commits the bump, a checkout builds against a binary whose API no longer matches the Rust source. Nothing says so.

The user chose a script check over a manifest-time warning. A new `platforms/apple/check-apple-engine.sh` computes the current tree's Apple hash, reads the `apple-engine-<hash>` out of the URL in `Package.swift`, and exits non-zero with both values when they differ. It also says whether the current hash is already published, because "the release exists but nobody committed the bump" needs a different fix (rerun the publish script to rewrite `Package.swift`) than "nothing is published yet".

The publish script runs the same check first. When the pin already matches, it skips the Apple half without touching anything.

A manifest-time warning would catch more, since Xcode evaluates the manifest constantly. But it would hash the tree on every load, and I'd rather the manifest stay boring.

## What stays out

Test fixtures. `build-xcframework.sh` copies them into `Tests/*/Fixtures`, and the test targets need them. Running the tests from a fresh checkout still needs that copy step. Splitting it into its own small script is reasonable but not part of this.

CI. No `.github/**` changes on this branch.

Signing. The XCFramework isn't code-signed. SwiftPM's checksum is the integrity check.

## Done looks like

On a machine that has never built the fork or the Rust workspace, `git clone`, check out the branch, `xcodebuild build -scheme EmbedPDF-Package -destination 'generic/platform=iOS Simulator'` succeeds. And `EmbedPDFTests` passes with the downloaded binary once fixtures are in place.

The first publish will be slow. After that, I expect the slow part of iOS work on this branch to be the Swift, which is how it should be.
