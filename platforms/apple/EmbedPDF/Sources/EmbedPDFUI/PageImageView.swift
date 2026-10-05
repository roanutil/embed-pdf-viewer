import EmbedPDF
import SwiftUI

public struct PageImageView: View {
  let render: RenderCapability
  let page: PageRef
  let scale: Double
  @State private var rendered: RenderedPage?
  @State private var error: PDFError?
  @State private var renderedPage: PageRef?

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
    .task(id: TaskID(page: page, scale: scale)) { await task() }
  }

  private struct TaskID: Hashable {
    let page: PageRef
    let scale: Double
  }

  private func task() async {
    if renderedPage != page {
      rendered = nil
      renderedPage = nil
    }
    error = nil
    do {
      let image = try await render.renderPage(page, options: RenderPageOptions(scale: scale))
      // A newer task owns the view now; this result is for a page or scale
      // it no longer shows.
      guard !Task.isCancelled else { return }
      rendered = image
      renderedPage = page
    } catch {
      guard !Task.isCancelled else { return }
      self.error = error
    }
  }
}
