// swift-tools-version: 5.9
//
// A Swift SDK facade over epdf-crux, plus a command-line consumer of it. Run
// `./generate.sh` first: everything under Sources/ except EmbedPDFEngine/ and
// Shell/ is generated and ignored.
import PackageDescription

let root = Context.packageDirectory + "/../../../.."

let package = Package(
    name: "EpdfCruxShell",
    platforms: [.macOS(.v13)],
    products: [
        .library(name: "EmbedPDFEngine", targets: ["EmbedPDFEngine"]),
    ],
    targets: [
        // Crux's typegen: the events, view model and effects, plus bincode.
        .target(name: "Serde", path: "Sources/Serde"),
        .target(name: "EmbedPdfCrux", dependencies: ["Serde"], path: "Sources/EmbedPdfCrux"),
        // uniffi's bindings for CoreFfi and the CruxShell callback.
        .target(name: "epdf_cruxFFI", path: "Sources/epdf_cruxFFI"),
        .target(name: "EpdfCruxCore", dependencies: ["epdf_cruxFFI"], path: "Sources/EpdfCruxCore"),
        // The handwritten facade: the only module a consumer imports.
        .target(
            name: "EmbedPDFEngine",
            dependencies: ["EmbedPdfCrux", "EpdfCruxCore"],
            path: "Sources/EmbedPDFEngine",
            linkerSettings: [
                .unsafeFlags([
                    "-L", root + "/target/debug",
                    "-lepdf_crux",
                    "-Xlinker", "-rpath", "-Xlinker", root + "/target/debug",
                ]),
            ]
        ),
        .executableTarget(name: "Shell", dependencies: ["EmbedPDFEngine"], path: "Sources/Shell"),
        .testTarget(
            name: "EmbedPDFEngineTests",
            dependencies: ["EmbedPDFEngine"],
            path: "Tests/EmbedPDFEngineTests"
        ),
    ]
)
