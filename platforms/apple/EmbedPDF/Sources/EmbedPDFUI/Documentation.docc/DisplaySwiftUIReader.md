# Display a SwiftUI reader

Show the open document's visible pages with scrolling and zoom gestures.

## Before you begin

Follow **Render your first PDF page** in the `EmbedPDF` module documentation to
install the products and start a viewer.
Enable both `RenderConfig()` and `StageConfig()` in its `ViewerConfig` before
startup. Open an unlocked document and keep the viewer and document available
while their UI is displayed. This guide uses iOS 17 or macOS 14 or later.

## Add the reader surface

The following complete view is an integration excerpt: its caller supplies an
already started `PDFViewer` and one open `DocumentInfo`. Add `ReaderSurface.swift`
to your app target. Retain the model in SwiftUI state so body evaluations do not
create new event subscriptions.

```swift
import EmbedPDF
import EmbedPDFUI
import SwiftUI

@MainActor
struct ReaderSurface: View {
  let viewer: PDFViewer
  let document: DocumentInfo
  @State private var model: StageReaderModel?
  @State private var failure: String?

  var body: some View {
    Group {
      if let model {
        VStack {
          Text("Page \(Int(model.currentPageIndex) + 1)")
          StageReaderView(model: model)
          if let error = model.error {
            Text(error.detail).foregroundStyle(.red)
          }
        }
      } else if let failure {
        ContentUnavailableView(
          "Reader unavailable", systemImage: "doc", description: Text(failure))
      } else {
        ProgressView()
      }
    }
    .task(id: document.id) {
      model = nil
      failure = nil
      do {
        let stage = try await viewer.stage(for: document)
        let render = try await viewer.render(in: document)
        guard !Task.isCancelled else { return }
        model = StageReaderModel(stage: stage, render: render)
      } catch {
        guard !Task.isCancelled else { return }
        failure = (error as? PDFError)?.detail ?? String(describing: error)
      }
    }
  }
}
```

Give `ReaderSurface` a bounded area, for example as a screen's main content.
``StageReaderView`` reports its geometry and device scale, observes stage events
in a SwiftUI task, and forwards gestures. Do not also run ``StageReaderModel/task()``
for the same model. `currentPageIndex` is zero-based, so the label adds one.

The surface renders whole-page images with an area cap of 16,777,216 pixels per
page. Deep zoom can therefore look soft. For a custom surface that adds detail
tiles, continue with <doc:RenderTiledPages>.

## Own document lifetime and accessibility

Removing the reader cancels its SwiftUI observation task; it does not close the
PDF or destroy the viewer. The document owner should remove its UI before
calling `PDFViewer.close(_:)`, and call `PDFViewer.destroy()` when finished with
the whole viewer. Both operations can throw, so handle their errors in the
owning screen or session. Capabilities retain the viewer, but explicit closure
still invalidates them. Use a new model for a newly opened document.

Page rasters are decorative images. The page label above gives location, but
does not make PDF text accessible. Supply accessible document text or an
alternative reading experience appropriate for your application. This reader
does not add text selection or annotation controls.

## Verify the result

The first page should appear inside the reader's bounds. Drag upward to reveal
later pages, pinch to zoom, and double-tap to change zoom. If the area stays
empty, confirm the document has pages and the parent gives the reader nonzero
width and height. Check the displayed error when acquiring capabilities or
updating the stage fails; a stored stage error is not cleared by later success.

## Related APIs

- ``StageReaderView``
- ``StageReaderModel/sizeChanged(_:displayScale:)``
- ``PageImageView``
- <doc:RenderTiledPages>
