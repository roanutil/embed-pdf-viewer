// swift-tools-version: 6.2

import PackageDescription

let package = Package(
  name: "EmbedPDF",
  platforms: [
    .iOS(.v17),
  ],
  products: [
    .library(name: "EmbedPDF", targets: ["EmbedPDF"]),
    .library(name: "EmbedPDFUI", targets: ["EmbedPDFUI"]),
  ],
  dependencies: [
    .package(url: "https://github.com/pointfreeco/swift-custom-dump", from: "1.3.0"),
  ],
  targets: [
    .binaryTarget(name: "EmbedPDFFFI", path: "EmbedPDFFFI.xcframework"),
    .target(
      name: "EmbedPDFBindings",
      dependencies: [
        "EmbedPDFFFI",
      ],
      swiftSettings: [
        .swiftLanguageMode(.v5),
      ],
      linkerSettings: [
        .linkedLibrary("c++"),
        .linkedFramework("CoreFoundation"),
        .linkedFramework("CoreGraphics"),
      ]
    ),
    .target(
      name: "EmbedPDF",
      dependencies: [
        "EmbedPDFBindings",
      ]
    ),
    .target(
      name: "EmbedPDFUI",
      dependencies: [
        "EmbedPDF",
      ]
    ),
    .testTarget(
      name: "EmbedPDFBindingsSmokeTests",
      dependencies: [
        "EmbedPDFBindings",
      ],
      resources: [
        .copy("Fixtures"),
      ]
    ),
    .testTarget(
      name: "EmbedPDFTests",
      dependencies: [
        "EmbedPDF",
        .product(name: "CustomDump", package: "swift-custom-dump"),
      ],
      resources: [
        .copy("Fixtures"),
      ]
    ),
    .testTarget(
      name: "EmbedPDFUITests",
      dependencies: [
        "EmbedPDFUI",
        .product(name: "CustomDump", package: "swift-custom-dump"),
      ],
      resources: [
        .copy("Fixtures"),
      ]
    ),
  ]
)

// Task 11's experimental bridge benchmarks: a hand-written C bridge
// (EmbedPDFManualBridge) and the tests that time it against uniffi. Neither
// is a product. Both exist only when EMBEDPDF_EXPERIMENTAL_BRIDGE=1 is set at
// manifest evaluation, against an XCFramework built with
// `build-xcframework.sh --experimental-manual-bridge`, so a normal checkout
// never sees them. The benchmarks run in Release, where
// EmbedPDFBindingsSmokeTests' `@testable import` cannot build, so the other
// test targets are left out of an experimental build.
if Context.environment["EMBEDPDF_EXPERIMENTAL_BRIDGE"] == "1" {
  package.targets.removeAll { $0.type == .test }
  for target in package.targets where target.name == "EmbedPDF" {
    target.swiftSettings = (target.swiftSettings ?? []) + [.define("EMBEDPDF_EXPERIMENTAL_BRIDGE")]
  }
  package.targets += [
    .target(
      name: "EmbedPDFManualBridge",
      dependencies: [
        "EmbedPDF",
        "EmbedPDFFFI",
      ]
    ),
    .testTarget(
      name: "EmbedPDFBridgeBenchmarks",
      dependencies: [
        "EmbedPDF",
        "EmbedPDFManualBridge",
      ],
      resources: [
        .copy("Fixtures"),
      ]
    ),
  ]
}
