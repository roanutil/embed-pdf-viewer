public import CoreGraphics
public import EmbedPDFBindings
import Foundation

public typealias RenderConfig = EmbedPDFBindings.RenderConfig
public typealias RenderFullPageOptions = EmbedPDFBindings.RenderFullPageOptions
public typealias RenderQuantize = EmbedPDFBindings.RenderQuantize
public typealias RenderPageOptions = EmbedPDFBindings.RenderPageOptions
public typealias RenderInvalidateScope = EmbedPDFBindings.RenderInvalidateScope
public typealias RenderBatchFailure = EmbedPDFBindings.RenderBatchFailure
public typealias RenderEvent = EmbedPDFBindings.RenderEvent
public typealias RenderPolicy = EmbedPDFBindings.RenderPolicy
public typealias RenderLatticePolicy = EmbedPDFBindings.RenderLatticePolicy
public typealias RenderViewport = EmbedPDFBindings.RenderViewport
public typealias RenderPaintSettings = EmbedPDFBindings.RenderPaintSettings
public typealias RenderPageViewDemand = EmbedPDFBindings.RenderPageViewDemand
public typealias RenderTileSource = EmbedPDFBindings.RenderTileSource
public typealias RenderTilePlan = EmbedPDFBindings.RenderTilePlan

/// A rendered page. `image` owns its pixels: it outlives the document and
/// the viewer.
public struct RenderedPage: @unchecked Sendable {
  public let width: Int
  public let height: Int
  /// Device pixels per PDF point across the page's width.
  public let scale: Double
  /// Immutable, so sharing it across tasks is safe.
  public let image: CGImage
}

/// One page of a batch.
public struct RenderedPageResult: Sendable {
  public let page: PageRef
  public let rendered: RenderedPage
}

/// A batch, best effort per page, each list in input order.
public struct RenderBatchResult: Sendable {
  public let applied: [RenderedPageResult]
  public let failed: [RenderBatchFailure]
}

/// A document's page rasters. Get one with `PDFViewer.render(in:)`; the
/// viewer needs `ViewerConfig(render: RenderConfig())`. It keeps the viewer
/// alive. After its document closes, every call throws `instance-closed`.
public struct RenderCapability: Sendable {
  let handle: RenderHandle
  let viewer: PDFViewer

  /// Whether page rasters would be served to this session (`doc.render`).
  public func canRender() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canRender() }
  }

  /// Render one page at exactly the size asked for: `width` device pixels,
  /// or `scale` device pixels a point (1 when neither is given). Throws
  /// `invalid-input` for a size that is not a positive number, `not-found`
  /// for a page not in this document, and `operation-cancelled` when the
  /// task is cancelled.
  public func renderPage(_ page: PageRef, options: RenderPageOptions = RenderPageOptions())
    async throws(PDFError) -> RenderedPage
  {
    try await measured("renderPage") { () async throws(PDFError) in
      let image = try await measured("renderPage.rust") { () async throws(PDFError) in
        try await bridgedCancellable { token in
          try await handle.renderPage(page: page, options: options, cancel: token)
        }
      }
      return try measured("renderPage.cgImage") { () throws(PDFError) in try RenderedPage(image) }
    }
  }

  /// A small raster `maxWidth` device pixels wide.
  public func renderThumbnail(
    _ page: PageRef, maxWidth: Double, includeAnnotations: Bool? = nil, includeFormFields: Bool? = nil
  ) async throws(PDFError) -> RenderedPage {
    let image = try await bridgedCancellable { token in
      try await handle.renderThumbnail(
        page: page, maxWidth: maxWidth, includeAnnotations: includeAnnotations, includeFormFields: includeFormFields, cancel: token)
    }
    return try RenderedPage(image)
  }

  /// Render several pages, `concurrency` at a time (4 when `nil`). A page
  /// that fails is in `failed`; cancelling the task throws
  /// `operation-cancelled` for the whole batch.
  public func renderPages(
    _ pages: [PageRef], options: RenderPageOptions = RenderPageOptions(),
    concurrency: UInt32? = nil
  ) async throws(PDFError) -> RenderBatchResult {
    let batch = try await bridgedCancellable { token in
      try await handle.renderPages(
        pages: pages, options: options, concurrency: concurrency, cancel: token)
    }
    var applied: [RenderedPageResult] = []
    for entry in batch.applied {
      applied.append(RenderedPageResult(page: entry.page, rendered: try RenderedPage(entry.image)))
    }
    return RenderBatchResult(applied: applied, failed: batch.failed)
  }

  /// The version of the raster these options would produce. It goes up when
  /// the page's pixels change.
  public func renderEpoch(_ page: PageRef, includeAnnotations: Bool? = nil, includeFormFields: Bool? = nil) async throws(PDFError)
    -> UInt64
  {
    try await bridged {
      try await handle.renderEpoch(page: page, includeAnnotations: includeAnnotations, includeFormFields: includeFormFields)
    }
  }

  /// Declare that page pixels changed: every page when `pages` is `nil`,
  /// `.content` when `scope` is.
  public func invalidate(_ pages: [PageRef]? = nil, scope: RenderInvalidateScope? = nil)
    async throws(PDFError)
  {
    try await bridged { try await handle.invalidate(pages: pages, scope: scope) }
  }

  public func renderPolicy() async throws(PDFError) -> RenderPolicy {
    try await bridged { try await handle.renderPolicy() }
  }

  /// Render at the scale a view shows the page (a `StagePageTransform`'s
  /// `renderScale`), conformed through the policy's render points.
  public func renderSource(_ page: PageRef, scale: Double, includeAnnotations: Bool? = nil, includeFormFields: Bool? = nil)
    async throws(PDFError) -> RenderedPage
  {
    let image = try await bridgedCancellable { token in
      try await handle.renderSource(page: page, scale: scale, includeAnnotations: includeAnnotations, includeFormFields: includeFormFields, cancel: token)
    }
    return try RenderedPage(image)
  }

  public func sourceKey(_ page: PageRef, scale: Double, includeAnnotations: Bool? = nil, includeFormFields: Bool? = nil) async throws(PDFError) -> String {
    try await bridged { try await handle.sourceKey(page: page, scale: scale, includeAnnotations: includeAnnotations, includeFormFields: includeFormFields) }
  }

  public func conformViewport(_ page: PageRef, scale: Double) async throws(PDFError) -> RenderViewport {
    try await bridged { try await handle.conformViewport(page: page, scale: scale) }
  }

  public func paintSettings() async throws(PDFError) -> RenderPaintSettings {
    try await bridged { try await handle.paintSettings() }
  }

  /// A view's tile surface. Pair with `dispose()`.
  public func viewDemand(_ viewId: String) async throws(PDFError) -> RenderViewDemand {
    RenderViewDemand(handle: try await bridged { try await handle.createViewDemand(viewId: viewId) }, render: self)
  }

  /// Invalidations, and rasters resolving or failing. Finishes when the
  /// document closes (at once if it already has) or the viewer is destroyed.
  public var events: AsyncStream<RenderEvent> {
    viewer.registry.stream { handle.subscribe(listener: $0) }
  }
}

/// One view's tile plane over a document's pages.
public struct RenderViewDemand: Sendable {
  let handle: RenderViewDemandHandle
  let render: RenderCapability

  public func setDemand(_ page: PageRef, _ demand: RenderPageViewDemand, includeAnnotations: Bool? = nil, includeFormFields: Bool? = nil) async throws(PDFError) {
    try await bridged { try await handle.setDemand(page: page, demand: demand, includeAnnotations: includeAnnotations, includeFormFields: includeFormFields) }
  }

  public func plan(_ page: PageRef) async throws(PDFError) -> RenderTilePlan {
    try await bridged { try await handle.plan(page: page) }
  }

  /// `nil` for a key the current plan doesn't paint.
  public func tileImage(_ page: PageRef, key: String) async throws(PDFError) -> RenderedPage? {
    guard let image = try await bridged({ try await handle.tileImage(page: page, key: key) }) else { return nil }
    return try RenderedPage(image)
  }

  public func markPainted(_ page: PageRef, key: String) async throws(PDFError) { try await bridged { try await handle.markPainted(page: page, key: key) } }
  public func markUnpainted(_ page: PageRef, key: String) async throws(PDFError) { try await bridged { try await handle.markUnpainted(page: page, key: key) } }
  public func release(_ page: PageRef) async throws(PDFError) { try await bridged { try await handle.release(page: page) } }
  public func dispose() async throws(PDFError) { try await bridged { try await handle.dispose() } }
}

extension RenderedPage {
  /// Wrap the owned pixels in a `CGImage`. `CGDataProvider` retains the
  /// buffer, so the image depends on nothing in Rust.
  init(_ rendered: RenderedImage) throws(PDFError) {
    let alpha: CGImageAlphaInfo =
      switch rendered.format {
      // Memory order R, G, B, A with straight alpha.
      case .rgba8: .last
      }
    guard
      let provider = CGDataProvider(data: rendered.pixels as CFData),
      let space = CGColorSpace(name: CGColorSpace.sRGB),
      let image = CGImage(
        width: Int(rendered.width), height: Int(rendered.height), bitsPerComponent: 8,
        bitsPerPixel: 32, bytesPerRow: Int(rendered.stride), space: space,
        bitmapInfo: CGBitmapInfo(rawValue: alpha.rawValue), provider: provider, decode: nil,
        shouldInterpolate: true, intent: .defaultIntent)
    else {
      throw PDFError(
        name: "InvalidState", code: "invalid-state",
        detail: "Core Graphics refused a \(rendered.width)x\(rendered.height) raster")
    }
    self.init(
      width: Int(rendered.width), height: Int(rendered.height), scale: rendered.scale,
      image: image)
  }
}

extension EventRelay: RenderEventListener where Event == RenderEvent {
  func onEvent(event: RenderEvent) {
    continuation.yield(event)
  }

  func onEnd() {
    continuation.finish()
  }
}

extension PDFViewer {
  public func render(in document: DocumentInfo) async throws(PDFError) -> RenderCapability {
    let handle = try await bridged { try await host.render(documentId: document.id) }
    return RenderCapability(handle: handle, viewer: self)
  }
}
