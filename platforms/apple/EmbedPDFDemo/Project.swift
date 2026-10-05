import ProjectDescription

let project = Project(
  name: "EmbedPDFDemo",
  packages: [
    .package(path: "../EmbedPDF"),
  ],
  targets: [
    .target(
      name: "EmbedPDFDemo",
      destinations: .iOS,
      product: .app,
      bundleId: "com.embedpdf.demo",
      deploymentTargets: .iOS("17.0"),
      infoPlist: .extendingDefault(with: [
        "CFBundleDisplayName": "EmbedPDF Demo",
        "CFBundleShortVersionString": "0.0.0",
        "UILaunchScreen": [:],
      ]),
      sources: ["EmbedPDFDemo/**"],
      resources: [
        "../../../pdfium-multiplatform-prototype/fixtures/report.pdf",
        "../../../pdfium-multiplatform-prototype/fixtures/encrypted-hello.pdf",
      ],
      dependencies: [
        .package(product: "EmbedPDF"),
        .package(product: "EmbedPDFUI"),
      ],
      settings: .settings(base: [
        "SWIFT_VERSION": "6.0",
        // No Apple developer team is configured. Ad-hoc signing is enough for
        // the simulator; a device run needs a team set here.
        "CODE_SIGN_IDENTITY": "-",
        "CODE_SIGN_STYLE": "Manual",
        "DEVELOPMENT_TEAM": "",
      ])
    ),
  ]
)
