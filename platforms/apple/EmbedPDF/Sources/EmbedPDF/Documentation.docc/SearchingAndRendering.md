# Search text and render pages

Find document content and create page images within an existing workspace.

## Before you begin

Start a viewer configured with `SearchConfig()` and `RenderConfig()`, then open a ready document. Install `StageConfig()` as well if your UI needs search navigation to reveal a hit through the stage. Your application supplies the search field, results list, and highlights.

## Choose independent or session search

Use ``SearchCapability/findAll(_:snippets:)`` for an independent query that returns all hits in page order without replacing the document's interactive search session. Use ``SearchCapability/search(_:from:)`` when you need current-query state, progress, active-hit navigation, and a session your UI can clear.

The following helper receives an already open document and viewer:

```swift
import EmbedPDF

func findReferences(
  to text: String, in document: DocumentInfo, viewer: PDFViewer
) async throws -> [SearchHit] {
  let search = try await viewer.search(in: document)
  return try await search.findAll(SearchQuery(text: text), snippets: false)
}
```

Hit geometry needs `doc.text.search`; snippets also need `doc.text.copy`. Passing `snippets: nil` asks for snippets and falls back to none when they are denied. Search matches extractable text, so an image-only scan can produce no hits without being a search failure.

For session search, inspect the result status as well as the hit count. A new search supersedes the previous scan, an empty query clears the session, and cancellation can retain partial hits. Use `hits()` to read the current hit snapshot, `nextHit()` and `previousHit()` to navigate, and `clear()` to remove the session. Hit navigation indices are zero-based and `goToHit` wraps modulo the hit count. No active hit is represented by `nil`.

## Render at an intentional resolution

This helper renders a supplied page reference at a caller-chosen pixel width:

```swift
import EmbedPDF

func renderPreview(
  page: PageRef, in document: DocumentInfo,
  viewer: PDFViewer, widthInPixels: Double
) async throws -> RenderedPage {
  let render = try await viewer.render(in: document)
  return try await render.renderPage(
    page, options: RenderPageOptions(width: widthInPixels))
}
```

Widths and scales must be positive. An explicit width is in device pixels across the unrotated page. `scale` is device pixels per PDF point, defaulting to one when neither size option is supplied. ``RenderedPage`` supplies raster `width`, `height`, effective `scale`, and an immutable `CGImage` that owns its pixels.

Use `renderThumbnail` for a bounded thumbnail demand. For batch rendering, inspect both `applied` and `failed`; individual page failures do not imply an entirely failed batch. Batch cancellation can throw for the operation as a whole. Plan memory usage around actual pixel area rather than the number of views currently allocated.

## Refresh changed page pixels

Use render events and epochs to determine when cached pixels are stale. Call `invalidate` when your custom integration knows that a page's pixels changed and needs the render plugin to advance its epoch. A direct render call returns an image; it does not update an arbitrary SwiftUI image already held by your app.

For a scrolling reader, use stage transforms and visibility to choose demand. A tiled custom reader additionally owns a `RenderViewDemand`, updates its view, acknowledges displayed tiles, and calls `dispose()` at the end of the surface's lifetime. The `EmbedPDFUI` tiled-page guide provides a compositor example.

## Verify the result

Search for text visible in a known document, confirm that hit page references belong to that document, and render one matching page. Check the image's dimensions and compare its content with the hit geometry after stage conversion. Handle a zero-hit result separately from a thrown permission or input error.

Next, use <doc:CoordinatesAndPages> to place search overlays or <doc:EditingAndSaving> to persist changes.

## Related APIs

- ``SearchCapability``
- ``SearchQuery``
- ``SearchHit``
- ``RenderCapability``
- ``RenderedPage``
