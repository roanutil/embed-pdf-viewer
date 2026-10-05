import ProjectDescription

// A copy of EmbedPDFDemo with Nutrient (PSPDFKit) underneath, for comparing
// the two under the same scenario. Same views, same accessibility
// identifiers, same signpost names. Ad-hoc signing; simulator only.
let signing: Settings = .settings(base: [
  "SWIFT_VERSION": "6.0",
  "CODE_SIGN_IDENTITY": "-",
  "CODE_SIGN_STYLE": "Manual",
  "DEVELOPMENT_TEAM": "",
])

let project = Project(
  name: "NutrientDemo",
  packages: [
    .remote(url: "https://github.com/PSPDFKit/PSPDFKit-SP", requirement: .exact("26.11.0")),
  ],
  targets: [
    .target(
      name: "NutrientDemo",
      destinations: .iOS,
      product: .app,
      bundleId: "com.embedpdf.nutrientdemo",
      deploymentTargets: .iOS("17.0"),
      infoPlist: .extendingDefault(with: [
        "CFBundleDisplayName": "Nutrient Demo",
        "CFBundleShortVersionString": "0.0.0",
        "UILaunchScreen": [:],
      ]),
      sources: ["NutrientDemo/**"],
      resources: [
        "../../../pdfium-multiplatform-prototype/fixtures/report.pdf",
        "../../../pdfium-multiplatform-prototype/fixtures/encrypted-hello.pdf",
      ],
      dependencies: [
        .package(product: "PSPDFKit"),
      ],
      settings: signing
    ),
    .target(
      name: "NutrientDemoUITests",
      destinations: .iOS,
      product: .uiTests,
      bundleId: "com.embedpdf.nutrientdemo.uitests",
      deploymentTargets: .iOS("17.0"),
      sources: ["NutrientDemoUITests/**"],
      dependencies: [
        .target(name: "NutrientDemo"),
      ],
      settings: signing
    ),
  ]
)
