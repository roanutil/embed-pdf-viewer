import EmbedPDF
import SwiftUI

/// Renders one PDF page as a SwiftUI image with loading and error states.
///
/// A task restarts when the page, scale, or visibility changes. Cancelled
/// results are ignored; a previous image remains visible during a scale
/// change. Render failures show an error only when no image is already held.
/// The image is decorative: hosts should provide a page accessibility label
/// and any accessible document text needed by their interface.
public struct PageImageView: View {
  let render: RenderCapability
  let page: PageRef
  let scale: Double
  let isOnScreen: Bool
  @State private var rendered: RenderedPage?
  @State private var error: PDFError?
  @State private var renderedPage: PageRef?

  /// Creates a page image whose raster resolution follows its visibility.
  ///
  /// - Parameters:
  ///   - render: Render capability for the page's open document.
  ///   - page: Stable page reference belonging to that document.
  ///   - scale: Positive device pixels per PDF point for an on-screen page.
  ///   - isOnScreen: Whether to use `scale`; defaults to `true`. When `false`,
  ///     renders at `PageRenderResolution.offScreenDeviceWidth` instead.
  ///
  /// Lazy scroll hosts can drive visibility with
  /// ``SwiftUICore/View/trackingScrollVisibility(_:)``. Changing document pixels
  /// alone does not restart this view's task; recreate its identity after
  /// invalidation if the page, scale, and visibility remain unchanged.
  public init(render: RenderCapability, page: PageRef, scale: Double, isOnScreen: Bool = true) {
    self.render = render
    self.page = page
    self.scale = scale
    self.isOnScreen = isOnScreen
  }

  /// The current page image, loading indicator, or render error.
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
    .task(id: TaskID(page: page, scale: scale, isOnScreen: isOnScreen)) { await task() }
  }

  private struct TaskID: Hashable {
    let page: PageRef
    let scale: Double
    let isOnScreen: Bool
  }

  private func task() async {
    if renderedPage != page {
      rendered = nil
      renderedPage = nil
    }
    error = nil
    do {
      let options = isOnScreen
        ? RenderPageOptions(scale: scale)
        : RenderPageOptions(width: PageRenderResolution.offScreenDeviceWidth)
      let image = try await render.renderPage(page, options: options)
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
