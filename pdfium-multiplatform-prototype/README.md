# PDFium multiplatform prototype

This standalone prototype exposes PDFium operations through Rust to Node,
WebAssembly (Wasm), Swift, and Kotlin. It is an experiment for
[the Rust port plan](../docs/research/rust-core-port.md).

The committed [artifact pin](build/runtime-build.json) has download URLs and
checksums for nine targets. Five entries are marked `pending`: `ios-arm64`,
`ios-sim-arm64`, `android-arm64`, `android-x64`, and `wasm32-eh`.
The fetcher exits with status 2 for those entries unless you supply a local pin.
This describes the committed configuration, not the current contents of upstream
releases.

The [September 8 audit](../docs/research/platform-gap-audit.md) records local
Wasm testing and rendered pages, text, and search results in an iOS simulator
and an ARM64 Android emulator. Android used an existing APK; iOS was rebuilt.
Those records do not verify physical devices or every supported target.

## Start with Node

Run from `pdfium-multiplatform-prototype`:

```bash
bash build/build-node.sh
cargo test --workspace
node --test packages/node/*.test.mjs
```

The build script fetches the host's PDFium headers and library, builds the Rust
addon, and copies it beside `packages/node/index.mjs`. It rejects a target that
differs from the detected host. Its output-copy step handles macOS and Linux;
Windows builds are not implemented by this script.

Rust uses the toolchain in `rust-toolchain.toml`. JavaScript build helpers use
Node APIs such as `import.meta.dirname`; the recorded runs used Node 24.
Use Bash 4 or newer for scripts with associative arrays, including the Swift
build script. macOS's bundled Bash 3.2 is insufficient for those scripts.

## Building with local artifacts

Prepare archives from matching fork builds, then generate a separate local pin:

```bash
node build/local-pin.mjs \
  wasm32-eh=/absolute/path/to/libembedpdf-runtime-wasm32-eh.tar.gz \
  ios-sim-arm64=/absolute/path/to/libembedpdf-runtime-ios-sim-arm64.tar.gz \
  android-arm64=/absolute/path/to/libembedpdf-runtime-android-arm64.tar.gz \
  > build/runtime-build.local.json
export EPDF_SCAFFOLD_PIN_FILE="$PWD/build/runtime-build.local.json"
```

The helper calculates SHA-256 checksums, encodes local file URLs, and preserves
other entries. It does not verify where the archives came from. Archives must
contain matching headers and libraries in the layout expected by
[`build/fetch-libpdfium.sh`](build/fetch-libpdfium.sh). The committed pin is
unchanged, and `.gitignore` excludes the local pin.

The [audit](../docs/research/platform-gap-audit.md) records local mobile and
`wasm32-eh` artifacts from fork commit `7f0fa5dbe`. Use the fork's
[build script](../packages/engine/runtime/runtime-src/scripts/embedpdf-runtime/build-target.sh)
to build the target you need; downloading and compiling PDFium is a separate
prerequisite from building these adapters.

`EPDF_SCAFFOLD_LIB_DIR` can select an alternative library directory. The fetcher,
Rust build support, and Swift/Kotlin/Wasm scripts use that location. Generated
native artifacts can retain absolute library paths, so rebuild after moving the
checkout. If Rust tests retain an old fixture path, run
`cargo clean -p test-support` before rebuilding them.

## Build and test each adapter

Commands below run from the prototype root unless a subshell changes directory.

| Adapter | Build | Test |
| --- | --- | --- |
| Node | `bash build/build-node.sh` | `node --test packages/node/*.test.mjs` |
| Web | `bash build/link-wasm.sh` | `node --test packages/web/*.test.mjs` |
| Swift on ARM64 macOS | `bash build/build-swift.sh` | `bash build/test-swift.sh` |
| Kotlin on ARM64 macOS | `bash build/build-kotlin.sh` | `(cd packages/kotlin && ./gradlew :epdf-ops:test)` |

The web build requires the local `wasm32-eh` entry above and `em++` on `PATH`.
The recorded audit used Emscripten 3.1.72 with Binaryen 132, selected using
`EM_BINARYEN_ROOT=/absolute/path/to/binaryen` (the directory containing
`bin/wasm-opt`). That is a recorded working combination, not a guarantee for
other compiler versions.

`build/link-wasm.sh` enables `-fwasm-exceptions`. The matching PDFium archive
uses Wasm exception handling for `setjmp`/`longjmp`; see
[Emscripten's documentation](https://emscripten.org/docs/porting/setjmp-longjmp.html).
The ordinary `wasm32` archive is not the input selected by this script.

For mobile, stage the matching libraries before building the app:

```bash
bash build/build-swift.sh ios-sim-arm64
bash build/build-ios-app.sh 'platform=iOS Simulator,name=iPhone 17'
bash build/build-kotlin.sh android-arm64
```

Choose a simulator installed on your machine. The Apple app build requires
Xcode and XcodeGen. Use `ios-arm64` with a corresponding local-pin entry for a
device build. iOS stages static archives; macOS stages dynamic libraries.
Device signing and provisioning are separate from compiling the libraries.
See the [Swift package](packages/swift/EpdfOps/README.md) and
[Android app](apps/android/README.md) for details.

`npm test` runs the build-helper, vector, Node, web, CLI, and web color-conversion
tests listed in `package.json`; build the Node and web modules first.
`bash build/test-all.sh` additionally builds adapters and runs Rust, Swift,
Kotlin, and Android JVM tests. It assumes Apple tooling. A pending Wasm pin
causes it to skip the web tests, continue with other suites, and finish nonzero
if those suites complete. Other build or test failures stop the script.
It does not automate browser or mobile UI tests.

## API and implementation

The core has an engine constructor, an open operation, and six document methods:

```text
Engine::new()
Engine::open(bytes, password)
Document::page_count()
Document::page_size(index)
Document::render_page(index, scale)
Document::page_text(index)
Document::search(index, query, case_sensitive)
Document::close()
```

Page indices start at zero. Operations return owned values rather than raw
PDFium pointers. Rendering returns dimensions, row stride, and BGRA bytes
(blue, green, red, alpha). Node exports `openDocument(bytes, password)`. Web exposes that method on the
object returned by `await createEpdf()`. Both use camel-case document methods. Swift/Kotlin use generated `EpdfEngine` and
`EpdfDocument` types. Kotlin renames the explicit document close to `closeSync`;
see [cleanup and error handling](packages/kotlin/README.md).

| Crate | Responsibility |
| --- | --- |
| `build-support` | Resolve target libraries and emit linker settings |
| `pdfium-sys` | Generate Rust declarations from the fork's C headers |
| `pdfium` | Own and release PDFium library, document, page, text-page, and bitmap resources |
| `test-support` | Load fixtures and expected values for Rust tests |
| `epdf-ops` | Implement document operations and dispatch |
| `epdf-uniffi` | Adapt types for Swift/Kotlin generation |
| `epdf-node` | Provide the Node addon |
| `epdf-wasm` | Provide C-compatible exports for the Wasm module |
| `uniffi-bindgen` | Run the binding generator |

On native targets, each engine owns a dedicated PDFium thread. Calls queue work
on that thread and wait for the result. PDFium handles remain on the thread
that created them. This relies on the fork's per-thread native state; serializing
calls with a mutex alone would not enforce thread ownership.
See [`dispatch/threaded.rs`](crates/epdf-ops/src/dispatch/threaded.rs).

The Emscripten build executes operations directly on its single thread and
rejects reentrant access to its state. It catches unwinding Rust panics, but
that does not undo partial mutations or recover from a native crash. The
optional probe checks that a document remains usable after a deliberate panic:

```bash
EPDF_PANIC_PROBE=1 bash build/link-wasm.sh
node packages/web/panic-probe.mjs
bash build/link-wasm.sh
```

The final command restores the normal module without the test export.

## Expected results and their limits

[`vectors/report-page4.json`](vectors/report-page4.json) and its reference bitmap
were generated by [`build/generate-vectors.mjs`](build/generate-vectors.mjs)
using the built repository runtime. Tests read the committed files; they do not
regenerate them. The fixture has 14 pages. Page index 4 is 612 × 792 points;
its frozen text has 8,106 UTF-16 code units and the search has 120 hits.

At scale 0.25, the reference bitmap is 153 × 198 pixels, 121,176 bytes.
Render tests allow a mean absolute byte difference of 2 and a maximum of 16.
Passing those tests does not imply byte identity. The page is grayscale, so
swapping red and blue cannot be detected with this bitmap alone.
The [color fixture](fixtures/README.md) supplies exact interior pixel checks
for Node, web, Swift, and Kotlin.

The operation suites check page count and text exactly and page size within
0.001 point. Search coverage differs: the Rust operation and JavaScript suites
also check the first recorded hit indices/counts, while Swift/Kotlin check total
hit count. This is not a full comparison of every search rectangle.
Swift/Kotlin additionally compare extracted UTF-16 length with `charCount`.

See the [audit](../docs/research/platform-gap-audit.md) for historical execution
results. Physical devices, Windows, musl, and release Safari/Firefox were not
verified there. The prototype still lacks cancellation, limits on queued work,
an aggregate memory budget, and automatic engine replacement. Its per-render
256 MiB limit is not a total-process memory limit.
