# Render tiled pages

Keep a budgeted whole-page image visible while detail tiles arrive at deep zoom.

## Before you begin

Start a viewer with `RenderConfig()` and open a document as described in
**Render your first PDF page** in the `EmbedPDF` module documentation.
This task builds a single-page surface; you
supply zoom, scrolling, and visibility calculations. ``StageReaderView`` uses
whole-page images and does not automatically adopt this tiled compositor.

## Create one surface demand and one model per page

This integration excerpt defines `TiledReaderSession.swift` and
`TiledReaderPage.swift`. Its factory receives a live viewer and document, throws
setup failures to its caller, and rejects a document without pages or with a rotated first page. Use a unique
`surfaceID` for each independent reader surface. Backing render handles with the
same ID share a tile surface within the document.

```swift
import EmbedPDF
import EmbedPDFUI
import SwiftUI

@MainActor
final class TiledReaderSession {
  let model: TiledPageModel
  let pageSize: CGSize
  private let demand: RenderViewDemand

  private init(model: TiledPageModel, pageSize: CGSize, demand: RenderViewDemand) {
    self.model = model
    self.pageSize = pageSize
    self.demand = demand
  }

  enum SetupError: Error { case noPages, rotatedPage }

  static func open(
    viewer: PDFViewer, document: DocumentInfo, surfaceID: String
  ) async throws -> TiledReaderSession {
    guard let page = try await viewer.pages(of: document).first else {
      throw SetupError.noPages
    }
    guard page.rotation == 0 else { throw SetupError.rotatedPage }
    let render = try await viewer.render(in: document)
    let demand = try await render.viewDemand(surfaceID)
    let model = TiledPageModel(render: render, demand: demand, page: page.page)
    return TiledReaderSession(
      model: model, pageSize: CGSize(width: page.width, height: page.height),
      demand: demand)
  }

  // Call after removing the surface and cancelling its rendering tasks.
  func stop() async throws {
    await model.release()
    try await demand.dispose()
  }
}

@MainActor
struct TiledReaderPage: View {
  let session: TiledReaderSession
  let deviceScale: Double
  let visibleRect: Rect?
  let revision: Int

  var body: some View {
    TiledPageImage(
      model: session.model,
      unrotatedSize: session.pageSize,
      deviceScale: deviceScale,
      visibleRect: visibleRect,
      revision: revision)
      .aspectRatio(session.pageSize.width / session.pageSize.height, contentMode: .fit)
      .accessibilityLabel("PDF page 1")
  }
}
```

Retain the session in your screen's state for the displayed page's lifetime.
Handle `open` errors in that screen before constructing `TiledReaderPage`.
For multiple pages, create one model per page using the same `demand` handle;
dispose the handle only after every page in that surface is finished.

## Supply the visible region and scale

`deviceScale` is device pixels per **unrotated PDF page point**, including zoom.
For a page displayed at 300 layout points wide from an unrotated width of 600
page points on a 3× display, pass `1.5` before further zoom. Use positive page
sizes and scale.

`visibleRect` uses unrotated page points, x rightward and y downward from the
top-left. Pass `nil` for the full page and `Rect(x: 0, y: 0, width: 0, height: 0)` for a built but off-screen
page. Restricting the rectangle lets the planner concentrate detail on the
visible region. Supply dimensions matching the page's unrotated aspect ratio;
if the PDF page is rotated, rotate the entire composed surface and convert
visibility back to unrotated space before passing it here. The example rejects
rotated pages to keep the example free of host rotation and placement code.
`PageInfo.width` and `height` already provide the unrotated dimensions; extending
the example requires the host to rotate the composed surface and its placement.

After changing page pixels, first call `RenderCapability.invalidate` for the
affected page, handle its error, and then increment `revision` in the host.
Changing `revision` restarts the view's update even if scale and visibility are
unchanged. Invalidating alone does not restart the SwiftUI task.

## Handle completion and cleanup

The view handles display acknowledgments so the planner can retire coarser
covering tiles. Its optional `.painted` phase means the current nonwaiting plan
has pixels, including base-only plans. `.settled` means the update returned
without task cancellation; a suppressed failure or the five-second polling
limit can also produce it. Neither phase is a rendering error channel.

The model suppresses render and planning failures. If your application needs
detailed diagnostics or retry policy, use the core rendering capability and
its events alongside the surface. Do not treat a missing tile as success just
because `.settled` arrived.

On disappearance the view releases page demand. To finish the whole surface,
remove it, allow its task cancellation, then call `session.stop()` and handle
any disposal error. ``TiledPageModel/release()`` keeps the base raster; discard
the session and model to release that image. Close the document only after its
surfaces have stopped, and destroy the viewer when its owning session ends.

## Verify the result

At a small scale, the base image should appear with no detail tiles. At a deep
scale and a restricted visible rectangle, tile detail should cover that region.
If it does not, check scale units, rectangle coordinates, render permissions,
and document lifetime. The page accessibility label is only location metadata;
provide document text separately when your UI needs accessible reading.

## Related APIs

- ``TiledPageImage``
- ``TiledPageModel``
- <doc:DisplaySwiftUIReader>
