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
