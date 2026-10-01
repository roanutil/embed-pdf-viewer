import CoreGraphics
import CustomDump
import EmbedPDF
import Testing

extension BaseSuite {
  @Suite struct RenderTests {
    func opened() async throws -> (PDFViewer, PDFDocument, PageRef, RenderCapability) {
      let viewer = try PDFViewer(ViewerConfig(render: RenderConfig()))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("colors"))
      let page = try await viewer.pages(of: document)[0].page
      return (viewer, document, page, try await viewer.render(in: document))
    }

    @Test func aPageRendersToACGImageThatOutlivesTheViewer() async throws {
      var image: CGImage?
      do {
        let (viewer, _, page, render) = try await opened()
        let rendered = try await render.renderPage(page, options: RenderPageOptions(scale: 1))
        #expect(rendered.width > 0 && rendered.height > 0)
        expectNoDifference(rendered.scale, 1)
        expectNoDifference(rendered.image.width, rendered.width)
        image = rendered.image
        try await viewer.destroy()
      }
      let survivor = try #require(image)
      #expect(survivor.width > 0)
      #expect(survivor.dataProvider?.data != nil)
    }

    /// The `colors` fixture is an 80 by 20 point page of four 20-point
    /// squares: red, green, blue, then (0.2, 0.4, 0.6). Drawn through Core
    /// Graphics, each keeps its color only if the image's pixel format names
    /// the bytes' real order; a red and blue swap would turn the first
    /// square blue.
    @Test func redStaysRed() async throws {
      let (_, _, page, render) = try await opened()
      let rendered = try await render.renderPage(page, options: RenderPageOptions(scale: 0.5))
      expectNoDifference(rendered.width, 40)
      expectNoDifference(rendered.height, 10)
      let pixels = try drawn(rendered.image)
      expectNoDifference(pixels.at(x: 5, y: 5), [255, 0, 0, 255])
      expectNoDifference(pixels.at(x: 15, y: 5), [0, 255, 0, 255])
      expectNoDifference(pixels.at(x: 25, y: 5), [0, 0, 255, 255])
      expectNoDifference(pixels.at(x: 35, y: 5), [51, 102, 153, 255])
    }

    @Test func aBadScaleIsRefused() async throws {
      let (_, _, page, render) = try await opened()
      await #expect {
        _ = try await render.renderPage(page, options: RenderPageOptions(scale: 0))
      } throws: { error in
        (error as? PDFError)?.code == "invalid-input"
      }
    }

    @Test func aThumbnailAndABatchRender() async throws {
      let (viewer, document, page, render) = try await opened()
      let thumbnail = try await render.renderThumbnail(page, maxWidth: 40)
      expectNoDifference(thumbnail.width, 40)
      let pages = try await viewer.pages(of: document).map(\.page)
      let batch = try await render.renderPages(pages, options: RenderPageOptions(width: 30))
      expectNoDifference(batch.applied.map(\.page), pages)
      expectNoDifference(batch.applied.map(\.rendered.width), pages.map { _ in 30 })
      expectNoDifference(batch.failed, [])
    }

    @Test func invalidatingIsObservedAndBumpsTheEpoch() async throws {
      let (_, _, page, render) = try await opened()
      let before = try await render.renderEpoch(page)
      let events = render.events
      try await render.invalidate([page])
      for await event in events {
        guard case let .invalidated(pages, scope) = event else { continue }
        expectNoDifference(pages, [page])
        expectNoDifference(scope, .content)
        break
      }
      let after = try await render.renderEpoch(page)
      expectNoDifference(after, before + 1)
    }

    @Test func aClosedDocumentsCapabilityThrows() async throws {
      let (viewer, document, page, render) = try await opened()
      try await viewer.close(document)
      await #expect {
        _ = try await render.renderPage(page)
      } throws: { error in
        (error as? PDFError)?.code == "instance-closed"
      }
    }

    /// `image` drawn into an sRGB RGBA bitmap, top row first.
    func drawn(_ image: CGImage) throws -> Drawn {
      let (width, height) = (image.width, image.height)
      var bytes = [UInt8](repeating: 0, count: width * height * 4)
      try bytes.withUnsafeMutableBytes { raw in
        let context = try #require(
          CGContext(
            data: raw.baseAddress, width: width, height: height, bitsPerComponent: 8,
            bytesPerRow: width * 4, space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
          )
        )
        context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
      }
      return Drawn(bytes: bytes, width: width)
    }

    struct Drawn {
      let bytes: [UInt8]
      let width: Int

      func at(x: Int, y: Int) -> [UInt8] {
        let start = (y * width + x) * 4
        return Array(bytes[start..<start + 4])
      }
    }
  }
}
