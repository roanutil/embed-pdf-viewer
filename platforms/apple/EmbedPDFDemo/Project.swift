import ProjectDescription

// No Apple developer team is configured. Ad-hoc signing is enough for the
// simulator; a device run needs a team set here.
let signing: Settings = .settings(base: [
  "SWIFT_VERSION": "6.0",
  "CODE_SIGN_IDENTITY": "-",
  "CODE_SIGN_STYLE": "Manual",
  "DEVELOPMENT_TEAM": "",
])

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
      settings: signing
    ),
    // Drives a scroll-and-search scenario. Run it on its own, or with
    // TEST_RUNNER_EMBEDPDF_ATTACH=1 against an app Instruments launched
    // (see platforms/apple/README.md, "Profiling the demo app").
    .target(
      name: "EmbedPDFDemoUITests",
      destinations: .iOS,
      product: .uiTests,
      bundleId: "com.embedpdf.demo.uitests",
      deploymentTargets: .iOS("17.0"),
      sources: ["EmbedPDFDemoUITests/**"],
      dependencies: [
        .target(name: "EmbedPDFDemo"),
      ],
      settings: signing
    ),
  ]
)
