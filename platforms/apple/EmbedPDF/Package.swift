// swift-tools-version: 6.2

import PackageDescription

// Where the engine comes from. Written by the engine release scripts: the
// release tag on embed-pdf-runtime and the SHA-256 of its
// EmbedPDFBindings.xcframework.zip. Both are "unpublished" until the first
// publish.
let engineRelease = "apple-engine-03b4f75a0a9a8493"
let engineChecksum = "2e29c8dd7c5986a1a8f0643f0d450a4af14b609c3d823b5814e36ef3f45bc8fa"

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
        .copy("Locales"),
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

// The targets above build the engine from source: the generated Swift
// bindings over EmbedPDFFFI.xcframework, both from build-xcframework.sh. That
// is the layout for EMBEDPDF_LOCAL_ENGINE=1, which also covers the
// experimental bridge below. Every other mode swaps those two targets for one
// EmbedPDFBindings binary, which already holds the compiled Swift, Rust core
// and PDFium. EMBEDPDF_PREBUILT_ENGINE=local reads the package-engine-framework.sh
// output beside this manifest; otherwise SwiftPM downloads the published
// release. EmbedPDFBindingsSmokeTests uses `@testable import`, which a
// library-evolution binary does not allow, so it exists only in local mode.
let environment = Context.environment
let usesLocalEngine = environment["EMBEDPDF_LOCAL_ENGINE"] == "1"
  || environment["EMBEDPDF_EXPERIMENTAL_BRIDGE"] == "1"
if !usesLocalEngine {
  package.targets.removeAll {
    ["EmbedPDFFFI", "EmbedPDFBindings", "EmbedPDFBindingsSmokeTests"].contains($0.name)
  }
  if environment["EMBEDPDF_PREBUILT_ENGINE"] == "local" {
    package.targets.append(
      .binaryTarget(name: "EmbedPDFBindings", path: "EmbedPDFBindings.xcframework")
    )
  } else {
    package.targets.append(
      .binaryTarget(
        name: "EmbedPDFBindings",
        url: "https://github.com/roanutil/embed-pdf-runtime/releases/download/\(engineRelease)/EmbedPDFBindings.xcframework.zip",
        checksum: engineChecksum
      )
    )
  }
  for target in package.targets where target.name == "EmbedPDF" {
    target.linkerSettings = (target.linkerSettings ?? []) + [
      .linkedLibrary("c++"),
      .linkedFramework("CoreFoundation"),
      .linkedFramework("CoreGraphics"),
    ]
  }
}

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
