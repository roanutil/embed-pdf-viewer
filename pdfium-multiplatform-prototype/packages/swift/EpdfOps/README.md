# EpdfOps Swift package

This package wraps the UniFFI bindings generated from
[`epdf-uniffi`](../../../crates/epdf-uniffi/src/lib.rs): `EpdfEngine`,
`EpdfDocument`, and their value types.
[`Epdf.swift`](Sources/EpdfOps/Epdf.swift) adds
`EpdfDocument.renderPageCGImage(index:scale:)` to convert rendered BGRA bytes
into a Core Graphics image.

## Build and test

Run from the prototype root, using Bash 4 or newer:

```bash
bash build/build-swift.sh
bash build/test-swift.sh
```

The default build targets ARM64 macOS. It builds `libepdf_uniffi`, fetches
`libembedpdf`, and stages the dynamic libraries under `Frameworks/macos-arm64/`.
It also generates Swift code under `Sources/EpdfOps/Generated/` and the C header
and module map under `Sources/EpdfOpsFFI/`.

Those directories are ignored by Git. A fresh checkout needs the build script
before `swift build` or `swift test` can use this package. The test script sets
the link-time library search path and embeds a runtime search path in the test
binary so it can load the staged libraries. Rebuild after moving the checkout.

## iOS app

Use the prototype's [local artifact pin](../../../README.md#building-with-local-artifacts)
for targets marked pending in the committed pin. From the prototype root:

```bash
bash build/build-swift.sh ios-sim-arm64
bash build/build-ios-app.sh 'platform=iOS Simulator,name=iPhone 17'
```

Choose an installed simulator. The app script requires Xcode and XcodeGen.
Use `ios-arm64` and a matching local-pin entry to stage device libraries.
The script stages static Rust and PDFium archives for iOS; macOS uses dynamic
libraries. Binding generation still reads the Rust dynamic-library build output.

The [September 8 audit](../../../../docs/research/platform-gap-audit.md) records
a rebuilt iOS simulator app displaying a page, text, and search results.
It does not verify physical-device execution or distribution signing.

## Tests

[`VectorsTests.swift`](Tests/EpdfOpsTests/VectorsTests.swift) reads the report
fixture's frozen expectations. It checks page count, page size within 0.001
point, text, UTF-16 length, total search-hit count, bitmap dimensions, and pixel
differences within the configured tolerances. It does not compare every search
hit or require byte-identical rendering.

Separate tests check an out-of-range page, Core Graphics image dimensions,
channel conversion using synthetic colored pixels, and exact interior colors
from the shared color PDF. These checks cover more than the grayscale report
bitmap alone.
