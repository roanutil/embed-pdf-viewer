# EpdfOps

A Swift package wrapping the uniffi bindings generated from the Rust
`epdf-uniffi` crate: `EpdfEngine`, `EpdfDocument`, and the value types they
pass across the boundary. `Sources/EpdfOps/Epdf.swift` adds one convenience
on top of the generated API, `EpdfDocument.renderPageCGImage(index:scale:)`,
so a SwiftUI view can show a page without knowing about BGRA pixel layout.

## Build in two steps

```
bash build/build-swift.sh
bash build/test-swift.sh
```

Run from the `pdfium-multiplatform-prototype` root. The first step builds
`libepdf_uniffi` and `libembedpdf` for `macos-arm64`, stages both dylibs
under `Frameworks/macos-arm64/`, and runs `uniffi-bindgen` to generate
`Sources/EpdfOps/Generated/epdf_uniffi.swift` plus the C header and
modulemap under `Sources/EpdfOpsFFI/`. The second step exports
`LIBRARY_PATH` so the linker can find the staged dylibs, then runs
`swift test` with `-Xlinker -rpath -Xlinker <path to Frameworks/macos-arm64>`
so the test binary can find them again at load time. That flag exists
because `swift test` runs the compiled bundle through
`swiftpm-testing-helper`, an Apple-signed binary with the hardened runtime
and no `allow-dyld-environment-variables` entitlement, so macOS strips
`DYLD_LIBRARY_PATH` before it reaches the test process. Baking the path
into the binary's rpath survives that strip; setting the environment
variable does not.

`swift build` fails on a fresh clone, before `build-swift.sh` has run: the
`EpdfOpsFFI` system-library target points at a directory
(`Sources/EpdfOpsFFI`) that does not exist yet, and there is no generated
`epdf_uniffi.swift` for `EpdfOps` to compile against.

## Mobile verification and packaging

The iOS app rebuilt and displayed a rendered page, extracted text, and search
results in the iOS 26.5 simulator during the [September 8 audit](../../../../docs/research/platform-gap-audit.md).
Use the prototype's [local artifact pin](../../../README.md#building-with-local-artifacts)
until the fork publishes mobile artifacts. The mobile build now stages static archives and links Rust/PDFium into the app;
the simulator was rebuilt and rendered without dynamic Rust/PDFium dependencies.
Physical-device distribution still requires signing/provisioning and a device
test. macOS continues using dylibs. Moving the checkout requires rebuilding
generated binaries.

## Why the generated files aren't tracked

`Frameworks/`, `Sources/EpdfOps/Generated/`, and `Sources/EpdfOpsFFI/` are
all gitignored. Every file in them is a direct, byte-for-byte output of
`build-swift.sh`: the dylibs come from `cargo build`, and the Swift
bindings, header, and modulemap come from `uniffi-bindgen` reading those
dylibs. Tracking them alongside the script that produces them would give
this package two sources of truth for the same content, one committed and
one regenerated, and nothing would catch them drifting apart. The cost is
the one paid here: a fresh clone cannot `swift build` this package on its
own, and anyone building it has to know to run `build-swift.sh` first. See
`Package.swift`, where the `EpdfOpsFFI` system-library target and the
`.linkedLibrary` calls in `EpdfOps`'s linker settings depend on that first
step having already run.

## Tests

`Tests/EpdfOpsTests/VectorsTests.swift` reads the same frozen parity file
the Rust suites read, `vectors/report-page4.json`, and the golden render
it points to, then asserts `EpdfDocument`'s answers against both: page
count, page size, extracted text, search hit count, and a rendered bitmap
compared byte-for-byte with `meanAbsDiffTolerance` and
`maxAbsDiffTolerance` from that file. No expected value is hardcoded in
the test; every one comes from the vectors file, so a Swift binding that
drifts from the Rust core fails here instead of only looking fine on
screen.

The suite also asserts UTF-16 text length and exact interior DeviceRGB colors from
`fixtures/colors.pdf` / `vectors/colors.json`, independently of the grayscale golden.
