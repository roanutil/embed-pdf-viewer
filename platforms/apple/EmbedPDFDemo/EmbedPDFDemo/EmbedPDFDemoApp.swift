import SwiftUI

@main
struct EmbedPDFDemoApp: App {
  @State private var model = DocumentModel()

  var body: some Scene {
    WindowGroup {
      DocumentView(model: model)
    }
  }
}
