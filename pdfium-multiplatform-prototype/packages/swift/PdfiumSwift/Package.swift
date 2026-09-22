// swift-tools-version: 6.4
import PackageDescription

let package = Package(
    name: "PdfiumSwift",
    platforms: [.macOS(.v14), .iOS(.v17)],
    products: [
        .library(name: "PdfiumSwift", targets: ["PdfiumSwift"]),
        .executable(name: "PdfiumSwiftBench", targets: ["PdfiumSwiftBench"]),
    ],
    targets: [
        .systemLibrary(name: "CPDFium"),
        .target(
            name: "PdfiumSwift",
            dependencies: ["CPDFium"],
            linkerSettings: [
                .linkedLibrary("embedpdf"),
                .linkedFramework("AppKit", .when(platforms: [.macOS])),
                .linkedFramework("CoreFoundation"),
                .linkedFramework("CoreGraphics"),
                .linkedFramework("Foundation"),
            ]
        ),
        .executableTarget(name: "PdfiumSwiftBench", dependencies: ["PdfiumSwift"]),
        .testTarget(name: "PdfiumSwiftTests", dependencies: ["PdfiumSwift"]),
    ]
)
