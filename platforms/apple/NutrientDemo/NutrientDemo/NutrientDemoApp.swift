import PSPDFKit
import SwiftUI

@main
struct NutrientDemoApp: App {
  @State private var model = DocumentModel()

  init() {
    // No license key: Nutrient runs in its trial mode, which is enough to
    // render and search locally.
    SDK.setLicenseKey(nil, options: nil)
  }

  var body: some Scene {
    WindowGroup {
      DocumentView(model: model)
    }
  }
}
