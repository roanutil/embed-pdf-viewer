// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "EpdfOps",
    platforms: [.macOS(.v14), .iOS(.v17)],
    products: [
        .library(name: "EpdfOps", targets: ["EpdfOps"])
    ],
    targets: [
        // The C header and modulemap uniffi emits, staged here by
        // build/build-swift.sh.
        .systemLibrary(name: "EpdfOpsFFI", path: "Sources/EpdfOpsFFI"),
        .target(
            name: "EpdfOps",
            dependencies: ["EpdfOpsFFI"],
            linkerSettings: [
                // No -L here on purpose: build/test-swift.sh exports
                // LIBRARY_PATH for link time and passes -Xlinker -rpath for
                // load time, and the Xcode project sets its own search
                // paths. Hardcoding a relative path would depend on the
                // directory `swift build` was run from.
                .linkedLibrary("epdf_uniffi"),
                .linkedLibrary("embedpdf"),
                .linkedLibrary("c++", .when(platforms: [.iOS])),
            ]
        ),
        .testTarget(name: "EpdfOpsTests", dependencies: ["EpdfOps"]),
    ]
)
