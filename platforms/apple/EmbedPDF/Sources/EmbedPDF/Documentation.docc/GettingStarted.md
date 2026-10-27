# Render your first PDF page

Install the Swift package, open a PDF, and return an image of its first page.

## Before you begin

Use an iOS 17 or later app and Xcode 27.1 or later for the pinned binary engine. The package manifest uses Swift tools 6.2; the handwritten modules compile in Swift 6 mode. The supplied engine contains arm64 device and arm64 simulator slices. Use an Apple silicon simulator or a supported device.

The Swift package lives at `platforms/apple/EmbedPDF` in the repository. This directory, rather than the repository root, contains `Package.swift`. These instructions use that local package and do not assume a separately tagged Swift package release.

## Add the package to your app

1. In Xcode, choose **File > Add Package Dependencies**, then **Add Local**.
2. Select the repository's `platforms/apple/EmbedPDF` directory.
3. Add `EmbedPDF` to your application target. Add `EmbedPDFUI` too if you want SwiftUI views and models.

The default package configuration downloads and verifies the pinned engine XCFramework. You do not need to build Rust for this route. Source engine builds and experimental bridge flags are contributor options, not prerequisites for an app integration.

## Open and render a file

Add the following function to your app. Supply a readable local PDF URL, such as a resource copied into your app bundle. The example deliberately reports a locked document to its caller; an application that accepts encrypted files should follow <doc:DocumentsAndLifetime> to prompt for a password.

```swift
import EmbedPDF
import Foundation

enum FirstPageError: Error {
  case documentNotReady(DocumentStatus)
  case noPages
}

func renderFirstPage(at url: URL) async throws -> RenderedPage {
  let data = try Data(contentsOf: url)
  let viewer = try PDFViewer(ViewerConfig(render: RenderConfig()))
  do {
    try await viewer.start()
    let document = try await viewer.open(data, name: url.lastPathComponent)
    guard document.status == .ready else {
      throw FirstPageError.documentNotReady(document.status)
    }
    let pages = try await viewer.pages(of: document)
    guard let first = pages.first else { throw FirstPageError.noPages }
    let render = try await viewer.render(in: document)
    let image = try await render.renderPage(
      first.page, options: RenderPageOptions(width: 1024))
    try await viewer.close(document)
    try await viewer.destroy()
    return image
  } catch {
    try? await viewer.destroy()
    throw error
  }
}
```

This function owns a short-lived workspace. A reader application should instead retain its viewer and capabilities while the document remains on screen. The returned ``RenderedPage`` owns its pixels, so its `image` remains usable after the workspace closes.

File loading can also throw Foundation errors; PDF operations throw ``PDFError``. For large files, arrange file I/O off the main actor rather than reading synchronously in a UI event handler. If a URL comes from a document picker, your app must manage security-scoped access while reading it. The SDK accepts bytes and does not manage that access for you.

## Verify the result

Call `renderFirstPage(at:)` from an asynchronous app operation. For an ordinary unencrypted PDF, the returned image has positive `width` and `height` and contains the first page at approximately 1024 pixels across its unrotated width. Inspect its `CGImage` in Xcode or display it with a SwiftUI image. If the result is `documentNotReady(.locked)`, request a password; if the engine reports `unsupported`, check that the rendering plugin was configured.

Next, use the `EmbedPDFUI` module's reader guide to display a scrolling document, or follow <doc:SearchingAndRendering> to search and render within a retained workspace.

## Related APIs

- ``PDFViewer``
- ``PDFViewer/open(_:name:password:)``
- ``PDFViewer/render(in:)``
- ``RenderCapability/renderPage(_:options:)``
- ``RenderedPage``
