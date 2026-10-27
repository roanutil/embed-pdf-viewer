# Work with pages and coordinates

Use durable page references and transform coordinates into the space expected by each API.

## Distinguish identity from order

``PageInfo`` contains a `page` reference, a zero-based `index`, unrotated crop `width` and `height`, and clockwise `rotation` in degrees. A page reference holds the PDF page object's number. It survives reordering within that open document; an index does not.

Get references from `viewer.pages(of:)` or a capability result. Do not convert a displayed page number into an object number. Display `index + 1` to a person if your interface uses ordinary page numbers, and keep the zero-based value for index-taking SDK methods.

## Know the coordinate space

The Swift records ``Point``, ``Size``, and ``Rect`` are shared by multiple APIs and do not encode a coordinate space in their type. Check the receiving method and do not mix spaces merely because the values have the same shape.

| Space | Origin and axes | Typical use |
| --- | --- | --- |
| Page content | Unrotated CropBox top-left, x right and y down; PDF-point scale | Annotation bounds, selection quads, and search geometry |
| Stage world | Laid-out scene before the camera projects it | Page layout and camera operations |
| Stage viewport | Container top-left, x right and y down; layout units | Gesture locations and screen-space overlays |
| Raster | Device pixels | Requested image widths and rendered image dimensions |
| Raw PDF engine | PDF user space, y up; page-box offsets may be nonzero | Internal engine geometry, not a substitute for viewer page coordinates |

The viewer converts raw PDF geometry to crop-relative page content coordinates. Page `/Rotate`, stage camera zoom, and device pixel ratio are presentation transforms; do not apply an extra y flip or rotation to page-space geometry returned by plugins.

Stage layout also has a `viewUnitsPerPoint` setting, and a page can carry a PDF `UserUnit`. Do not assume that a page unit always maps to exactly one UIKit point at the current zoom. Use stage conversion methods for overlay placement.

## Convert through the stage

This helper converts a container-local gesture location into page content coordinates. The caller supplies the intended page reference and a stage capability for the same document:

```swift
import EmbedPDF

func pageLocation(
  for viewportLocation: Point,
  on page: PageRef,
  stage: StageCapability
) async throws -> Point? {
  try await stage.viewportToPage(page, viewportLocation)
}
```

Use `stage.page(at:)` when you first need to determine which page is under the gesture. Use `pageRectToViewport` to position a page-bound overlay. A conversion can return `nil` when no page transform is available; handle that state rather than treating it as `(0, 0)`.

## Choose raster size separately

`RenderPageOptions(width:)` asks for device pixels across the unrotated page. `scale:` is device pixels per PDF point; absent sizing options default to scale one. ``PageRenderResolution`` can convert a displayed page demand to unrotated raster width and reduce off-screen pages to a placeholder resolution.

An image's pixel width is not its layout width. For a page shown 300 layout points wide on a 3× display, a roughly 900-pixel raster is an appropriate starting demand before additional zoom. Let stage transforms supply the scale for stage-driven rendering.

## Verify an overlay

Test an ordinary page and a page rotated 90 degrees. Select or search for known content, convert its page rectangle through the stage, and confirm that the overlay follows that content while zooming and scrolling. Repeat after reordering pages to catch accidental index-based identity.

Next, follow <doc:SearchingAndRendering> for search results and raster operations.
