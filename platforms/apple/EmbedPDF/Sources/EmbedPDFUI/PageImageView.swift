import EmbedPDF
import SwiftUI

public struct PageImageView: View {
  let render: RenderCapability
  let page: PageRef
  let scale: Double
  @State private var rendered: RenderedPage?
  @State private var error: PDFError?

  public init(render: RenderCapability, page: PageRef, scale: Double) {
    self.render = render
    self.page = page
    self.scale = scale
  }

  public var body: some View {
    Group {
      if let rendered {
        Image(decorative: rendered.image, scale: rendered.scale)
          .resizable()
          .aspectRatio(contentMode: .fit)
      } else if let error {
        ContentUnavailableView(
          "Page unavailable", systemImage: "doc", description: Text(error.detail))
      } else {
        ProgressView()
      }
    }
    .task(id: scale) { await task() }
  }

  private func task() async {
    do {
      rendered = try await render.renderPage(page, options: RenderPageOptions(scale: scale))
      error = nil
    } catch {
      guard !Task.isCancelled else { return }
      self.error = error
    }
  }
}
