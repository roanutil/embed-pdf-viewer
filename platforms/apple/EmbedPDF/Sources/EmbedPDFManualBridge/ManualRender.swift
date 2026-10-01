// `render_page` over the hand-written C ABI, for Task 11's benchmarks only.
// The CGImage reads the raster where Rust keeps it: `CGDataProvider`'s
// release callback frees it, so the pixels are never copied.
public import CoreGraphics
public import EmbedPDF
internal import EpdfManualFFI

public final class ManualRender: @unchecked Sendable {
  let raw: OpaquePointer
  let viewer: PDFViewer

  init(raw: OpaquePointer, viewer: PDFViewer) {
    self.raw = raw
    self.viewer = viewer
  }

  public static func open(_ viewer: PDFViewer, _ document: PDFDocument) async throws(ManualError)
    -> ManualRender
  {
    let kernel = viewer.experimentalKernelToken
    let raw = try await manualCall({ ctx in
      document.id.withCString { epdf_render_open(kernel, $0, ctx, trampoline) }
    }) { result in OpaquePointer(result!) }
    return ManualRender(raw: raw, viewer: viewer)
  }

  /// Render one page at `scale` device pixels a point.
  public func renderPage(_ page: PageRef, scale: Double) async throws(ManualError) -> CGImage {
    let pixels = try await manualCallCancellable({ ctx in
      epdf_render_page(raw, page.pageObjectNumber, scale, ctx, trampoline)
    }) { result in result!.load(as: EpdfPixels.self) }
    // Memory order R, G, B, A with straight alpha, as the uniffi path's
    // `RenderedPage` reads it.
    precondition(pixels.format == UInt32(EPDF_PIXEL_FORMAT_RGBA8))
    guard
      let provider = CGDataProvider(
        dataInfo: pixels.owner, data: pixels.data, size: pixels.len,
        releaseData: { info, _, _ in epdf_pixels_free(info) })
    else {
      epdf_pixels_free(pixels.owner)
      throw ManualError(kind: -1, code: "invalid-state", detail: "no CGDataProvider")
    }
    // From here the provider owns the raster and frees it when it goes.
    guard
      let space = CGColorSpace(name: CGColorSpace.sRGB),
      let image = CGImage(
        width: Int(pixels.width), height: Int(pixels.height), bitsPerComponent: 8,
        bitsPerPixel: 32, bytesPerRow: Int(pixels.stride), space: space,
        bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue), provider: provider,
        decode: nil, shouldInterpolate: true, intent: .defaultIntent)
    else {
      throw ManualError(
        kind: -1, code: "invalid-state",
        detail: "Core Graphics refused a \(pixels.width)x\(pixels.height) raster")
    }
    return image
  }

  deinit {
    epdf_render_free(raw)
  }
}
