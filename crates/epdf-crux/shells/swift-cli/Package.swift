// swift-tools-version: 5.9
//
// A command-line Crux shell over epdf-crux, to prove the generated Swift
// types and the uniffi FFI meet in a real process. Run `./generate.sh` first:
// everything under Sources/ except Shell/ is generated and ignored.
import PackageDescription

let root = Context.packageDirectory + "/../../../.."

let package = Package(
    name: "EpdfCruxShell",
    platforms: [.macOS(.v13)],
    targets: [
        // Crux's typegen: the events, view model and effects, plus bincode.
        .target(name: "Serde", path: "Sources/Serde"),
        .target(name: "EmbedPdfCrux", dependencies: ["Serde"], path: "Sources/EmbedPdfCrux"),
        // uniffi's bindings for CoreFfi and the CruxShell callback.
        .target(name: "epdf_cruxFFI", path: "Sources/epdf_cruxFFI"),
        .target(name: "EpdfCruxCore", dependencies: ["epdf_cruxFFI"], path: "Sources/EpdfCruxCore"),
        .executableTarget(
            name: "Shell",
            dependencies: ["EmbedPdfCrux", "EpdfCruxCore"],
            path: "Sources/Shell",
            linkerSettings: [
                .unsafeFlags([
                    "-L", root + "/target/debug",
                    "-lepdf_crux",
                    "-Xlinker", "-rpath", "-Xlinker", root + "/target/debug",
                ]),
            ]
        ),
    ]
)
