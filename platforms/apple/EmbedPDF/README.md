# EmbedPDF Swift package

Use `EmbedPDF` to open, render, search, and edit PDF documents. Add `EmbedPDFUI`
for SwiftUI reader surfaces and observable models. The application owns file
access, document lifetime, surrounding controls, and persistence.

The package declares iOS 17 or later, macOS 14 or later, and Swift tools 6.2.
The engine build supports arm64 iOS devices/simulators and native macOS on
Apple silicon and Intel. Use Xcode 27.1 or later with the pinned engine binary.
The existing iOS-only engine pin must be replaced by a verified iOS/macOS
release before downloaded macOS builds work; contributors can build the local
engine using the [Apple contributor notes](../README.md).

To test a prepared local engine on macOS from this directory:

```sh
bash ../copy-test-fixtures.sh
EMBEDPDF_LOCAL_ENGINE=1 swift test
```

After packaging the engine, use `EMBEDPDF_PREBUILT_ENGINE=local swift test`
with a separate scratch directory. `EmbedPDFUI.HapticFeedback` uses AppKit on
macOS; feedback depends on the input device and does not distinguish iOS impact
weights or notification outcomes. The reader retains its existing drag,
magnify, and double-tap gestures; desktop wheel/keyboard controls are host-owned.

## Read the documentation

Start with [Render your first PDF page](Sources/EmbedPDF/Documentation.docc/GettingStarted.md).
It explains local package installation and includes a complete first-page
rendering example. Continue with [Display a SwiftUI reader](Sources/EmbedPDFUI/Documentation.docc/DisplaySwiftUIReader.md)
for an on-screen document.

The [EmbedPDF catalog](Sources/EmbedPDF/Documentation.docc/EmbedPDF.md) groups
the core APIs by task. The [EmbedPDFUI catalog](Sources/EmbedPDFUI/Documentation.docc/EmbedPDFUI.md)
covers reader, image, tile, and search UI. Symbol contracts live in the Swift
source comments and are available through Xcode Quick Help.

## Build and open DocC documentation

From this package directory, run:

```sh
xcodebuild docbuild -scheme EmbedPDFUI \
  -destination 'generic/platform=iOS Simulator' \
  -derivedDataPath /tmp/embedpdf-swift-docs-build \
  ARCHS=arm64 ONLY_ACTIVE_ARCH=YES CODE_SIGNING_ALLOWED=NO
```

This builds both modules and writes their archives under
`/tmp/embedpdf-swift-docs-build/Build/Products/Debug-iphonesimulator/`:

```sh
open /tmp/embedpdf-swift-docs-build/Build/Products/Debug-iphonesimulator/EmbedPDF.doccarchive
open /tmp/embedpdf-swift-docs-build/Build/Products/Debug-iphonesimulator/EmbedPDFUI.doccarchive
```

You can also use **Product > Build Documentation** for the `EmbedPDFUI` scheme
in Xcode with an arm64 simulator destination. If Xcode tries to build x86_64,
select a concrete Apple silicon simulator or use the command above to match
the engine's supplied simulator architecture.

For source engine builds and the demo app, see the [Apple contributor notes](../README.md).
