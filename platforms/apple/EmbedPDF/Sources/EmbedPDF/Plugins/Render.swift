public import CoreGraphics
public import EmbedPDFBindings

public typealias RenderSettings = EmbedPDFBindings.RenderSettings
public typealias RenderSettingsPatch = EmbedPDFBindings.RenderSettingsPatch
public typealias RenderEncodingFormat = EmbedPDFBindings.RenderEncodingFormat
public typealias RenderTilesSettings = EmbedPDFBindings.RenderTilesSettings
public typealias RenderTilesPatch = EmbedPDFBindings.RenderTilesPatch
public typealias RenderTilesSetting = EmbedPDFBindings.RenderTilesSetting
public typealias RenderTilesOptions = EmbedPDFBindings.RenderTilesOptions
public typealias RenderPrefetchOptions = EmbedPDFBindings.RenderPrefetchOptions
public typealias RenderSnapshotSettings = EmbedPDFBindings.RenderSnapshotSettings
public typealias RenderSnapshotPatch = EmbedPDFBindings.RenderSnapshotPatch
public typealias RenderSnapshotBox = EmbedPDFBindings.RenderSnapshotBox
import Foundation

public typealias RenderConfig = EmbedPDFBindings.RenderConfig
public typealias RenderFullPageOptions = EmbedPDFBindings.RenderFullPageOptions
public typealias RenderQuantize = EmbedPDFBindings.RenderQuantize
public typealias RenderPageOptions = EmbedPDFBindings.RenderPageOptions
public typealias RenderRotation = EmbedPDFBindings.RenderRotation
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
  /// Device pixels per PDF point across the rendered region's rotated width.
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

/// A capture's immediate metadata and separate image wait. Keep the viewer
/// alive until the image resolves; returned pixels outlive it.
public struct RenderSnapshot: Sendable {
  public let page: PageRef
  public let rect: Rect
  private let handle: RenderSnapshotImageHandle

  init(_ snapshot: EmbedPDFBindings.RenderSnapshot) {
    page = snapshot.page
    rect = snapshot.rect
    handle = snapshot.image
  }

  /// Cancelling this task ends only its wait. Clearing the snapshot or
  /// closing the document cancels a pending capture for every reader.
  public func image() async throws(PDFError) -> RenderedPage {
    let pixels = try await bridgedCancellable { token in
      try await handle.image(cancel: token)
    }
    return try RenderedPage(pixels)
  }
}

/// A document's page rasters. Get one with `PDFViewer.render(in:)`; the
/// viewer needs `ViewerConfig(render: RenderConfig())`. It keeps the viewer
/// alive. After its document closes, every call throws `instance-closed`.
///
/// A picture's layers, `includeAnnotations` and `includeFormFields`, default
/// to ``ContentInclusion/whenPermitted``: annotations when the session may read
/// them (`doc.annotate.read`), form fields when it may read them
/// (`doc.forms.read`) and annotations are not ``ContentInclusion/never``. That is
/// the engine's rule, `resolve_page_layers` in
/// `crates/plugins/render/src/model.rs`. A call that renders throws
/// `permission-denied` for ``ContentInclusion/always`` on a layer the session may
/// not read; `renderEpoch`, `sourceKey` and `RenderViewDemand.setDemand`
/// leave that layer out instead.
public struct RenderCapability: Sendable {

  /// Current settings shared by every document using this plugin registration.
  /// Encoding preferences are stored; native page images remain raw pixels and this host provides no encoder.
  public func settings() async throws(PDFError) -> RenderSettings {
    try await bridged { try await handle.settings() }
  }

  /// Merge the supplied fields. Clear flags remove nullable values; omission preserves them.
  public func updateSettings(_ patch: RenderSettingsPatch) async throws(PDFError) {
    try await bridged { try await handle.updateSettings(patch: patch) }
  }

  /// Restore the values supplied at registration, preserving registered host resources.
  public func resetSettings() async throws(PDFError) {
    try await bridged { try await handle.resetSettings() }
  }

  let handle: RenderHandle
  let viewer: PDFViewer

  public func canSnapshot() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canSnapshot() }
  }

  /// Read the existing capture without starting a render.
  public func snapshot() async throws(PDFError) -> RenderSnapshot? {
    try await bridged { try await handle.snapshot() }.map(RenderSnapshot.init)
  }

  public func clearSnapshot() async throws(PDFError) {
    try await bridged { try await handle.clearSnapshot() }
  }

  /// The capture tool's dragged or held box in page points.
  public func snapshotBox(on page: PageRef) async throws(PDFError) -> Rect? {
    try await bridged { try await handle.snapshotBox(page: page) }
  }

  /// Captures and clears (`nil`). Finishes when the document closes.
  public var snapshotChanges: AsyncStream<RenderSnapshot?> {
    viewer.registry.stream { handle.subscribeSnapshots(listener: $0) }
  }

  /// Whether page rasters would be served to this session (`doc.render`).
  public func canRender() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canRender() }
  }

  /// Whether the pages' thumbnails would be served to this session
  /// (`doc.render.thumbnail`, which `doc.render` includes). A session that
  /// may not render pages may still have them.
  public func canRenderThumbnail() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canRenderThumbnail() }
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

  /// The page's thumbnail: the whole page, 512 pixels on its long side,
  /// sharing the raster cache. Needs only `doc.render.thumbnail`; throws
  /// `permission-denied` without it, otherwise like `renderPage`.
  public func renderThumbnail(
    _ page: PageRef, includeAnnotations: ContentInclusion = .whenPermitted, includeFormFields: ContentInclusion = .whenPermitted
  ) async throws(PDFError) -> RenderedPage {
    try await renderThumbnail(
      page, engineAnnotations: includeAnnotations.engineValue, engineFormFields: includeFormFields.engineValue)
  }

  /// `nil` is ``ContentInclusion/whenPermitted``, `true` ``ContentInclusion/always`` and
  /// `false` ``ContentInclusion/never``.
  @available(*, deprecated, message: "Pass an ContentInclusion: leave it out for nil, .always for true, .never for false.")
  @_disfavoredOverload
  public func renderThumbnail(
    _ page: PageRef, includeAnnotations: Bool? = nil, includeFormFields: Bool? = nil
  ) async throws(PDFError) -> RenderedPage {
    try await renderThumbnail(page, engineAnnotations: includeAnnotations, engineFormFields: includeFormFields)
  }

  private func renderThumbnail(
    _ page: PageRef, engineAnnotations: Bool?, engineFormFields: Bool?
  ) async throws(PDFError) -> RenderedPage {
    let image = try await bridgedCancellable { token in
      try await handle.renderThumbnail(
        page: page, includeAnnotations: engineAnnotations, includeFormFields: engineFormFields, cancel: token)
    }
    return try RenderedPage(image)
  }

  /// Render several pages, `concurrency` at a time. The default, 4, is the
  /// engine's (`DEFAULT_BATCH_CONCURRENCY` in
  /// `crates/plugins/render/src/controller.rs`), and the engine runs 0 as 1.
  /// A page that fails is in `failed`; cancelling the task throws
  /// `operation-cancelled` for the whole batch. A `concurrency` below zero
  /// or past `UInt32.max` throws `invalid-input`.
  public func renderPages(
    _ pages: [PageRef], options: RenderPageOptions = RenderPageOptions(),
    concurrency: Int = 4
  ) async throws(PDFError) -> RenderBatchResult {
    let concurrency: UInt32 = try concurrency.toWire("concurrency")
    return try await renderPages(pages, options: options, wireConcurrency: concurrency)
  }

  @available(*, deprecated, message: "Pass an Int: renderPages(_:options:concurrency:) takes an Int concurrency.")
  @_disfavoredOverload
  public func renderPages(
    _ pages: [PageRef], options: RenderPageOptions = RenderPageOptions(),
    concurrency: UInt32?
  ) async throws(PDFError) -> RenderBatchResult {
    try await renderPages(pages, options: options, wireConcurrency: concurrency)
  }

  private func renderPages(
    _ pages: [PageRef], options: RenderPageOptions, wireConcurrency concurrency: UInt32?
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
  public func renderEpoch(
    _ page: PageRef, includeAnnotations: ContentInclusion = .whenPermitted, includeFormFields: ContentInclusion = .whenPermitted
  ) async throws(PDFError) -> UInt64 {
    try await renderEpoch(
      page, engineAnnotations: includeAnnotations.engineValue, engineFormFields: includeFormFields.engineValue)
  }

  /// `nil` is ``ContentInclusion/whenPermitted``, `true` ``ContentInclusion/always`` and
  /// `false` ``ContentInclusion/never``.
  @available(*, deprecated, message: "Pass an ContentInclusion: leave it out for nil, .always for true, .never for false.")
  @_disfavoredOverload
  public func renderEpoch(_ page: PageRef, includeAnnotations: Bool? = nil, includeFormFields: Bool? = nil) async throws(PDFError)
    -> UInt64
  {
    try await renderEpoch(page, engineAnnotations: includeAnnotations, engineFormFields: includeFormFields)
  }

  private func renderEpoch(_ page: PageRef, engineAnnotations: Bool?, engineFormFields: Bool?) async throws(PDFError)
    -> UInt64
  {
    try await bridged {
      try await handle.renderEpoch(page: page, includeAnnotations: engineAnnotations, includeFormFields: engineFormFields)
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
  /// `renderScale`), conformed through the policy's render points. With
  /// `thumbnail`, the page's thumbnail instead (`scale` is ignored), which
  /// needs only `doc.render.thumbnail`. Leaving `thumbnail` out is `false`,
  /// as in the engine (`RenderHandle::render_source` in
  /// `crates/plugins/render/src/ffi.rs`).
  public func renderSource(
    _ page: PageRef, scale: Double, includeAnnotations: ContentInclusion = .whenPermitted,
    includeFormFields: ContentInclusion = .whenPermitted, thumbnail: Bool = false
  ) async throws(PDFError) -> RenderedPage {
    try await renderSource(
      page, scale: scale, engineAnnotations: includeAnnotations.engineValue,
      engineFormFields: includeFormFields.engineValue, engineThumbnail: thumbnail)
  }

  /// `nil` layers are ``ContentInclusion/whenPermitted``, `true` ``ContentInclusion/always``
  /// and `false` ``ContentInclusion/never``; a `nil` `thumbnail` is `false`.
  @available(*, deprecated, message: "Pass an ContentInclusion for each layer (leave it out for nil) and a Bool thumbnail (nil is false).")
  @_disfavoredOverload
  public func renderSource(
    _ page: PageRef, scale: Double, includeAnnotations: Bool? = nil, includeFormFields: Bool? = nil,
    thumbnail: Bool? = nil
  ) async throws(PDFError) -> RenderedPage {
    try await renderSource(
      page, scale: scale, engineAnnotations: includeAnnotations, engineFormFields: includeFormFields,
      engineThumbnail: thumbnail)
  }

  private func renderSource(
    _ page: PageRef, scale: Double, engineAnnotations: Bool?, engineFormFields: Bool?, engineThumbnail: Bool?
  ) async throws(PDFError) -> RenderedPage {
    let image = try await bridgedCancellable { token in
      try await handle.renderSource(
        page: page, scale: scale, includeAnnotations: engineAnnotations, includeFormFields: engineFormFields,
        thumbnail: engineThumbnail, cancel: token)
    }
    return try RenderedPage(image)
  }

  /// The identity of the raster `renderSource` would produce, the
  /// thumbnail's with `thumbnail`. Leaving `thumbnail` out is `false`, as in
  /// the engine (`RenderHandle::source_key` in `crates/plugins/render/src/ffi.rs`).
  public func sourceKey(
    _ page: PageRef, scale: Double, includeAnnotations: ContentInclusion = .whenPermitted,
    includeFormFields: ContentInclusion = .whenPermitted, thumbnail: Bool = false
  ) async throws(PDFError) -> String {
    try await sourceKey(
      page, scale: scale, engineAnnotations: includeAnnotations.engineValue,
      engineFormFields: includeFormFields.engineValue, engineThumbnail: thumbnail)
  }

  /// `nil` layers are ``ContentInclusion/whenPermitted``, `true` ``ContentInclusion/always``
  /// and `false` ``ContentInclusion/never``; a `nil` `thumbnail` is `false`.
  @available(*, deprecated, message: "Pass an ContentInclusion for each layer (leave it out for nil) and a Bool thumbnail (nil is false).")
  @_disfavoredOverload
  public func sourceKey(
    _ page: PageRef, scale: Double, includeAnnotations: Bool? = nil, includeFormFields: Bool? = nil,
    thumbnail: Bool? = nil
  ) async throws(PDFError) -> String {
    try await sourceKey(
      page, scale: scale, engineAnnotations: includeAnnotations, engineFormFields: includeFormFields,
      engineThumbnail: thumbnail)
  }

  private func sourceKey(
    _ page: PageRef, scale: Double, engineAnnotations: Bool?, engineFormFields: Bool?, engineThumbnail: Bool?
  ) async throws(PDFError) -> String {
    try await bridged {
      try await handle.sourceKey(
        page: page, scale: scale, includeAnnotations: engineAnnotations, includeFormFields: engineFormFields,
        thumbnail: engineThumbnail)
    }
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

  /// What this view wants for a page right now. The layers follow
  /// ``RenderCapability``'s rule.
  public func setDemand(
    _ page: PageRef, _ demand: RenderPageViewDemand, includeAnnotations: ContentInclusion = .whenPermitted,
    includeFormFields: ContentInclusion = .whenPermitted
  ) async throws(PDFError) {
    try await setDemand(
      page, demand, engineAnnotations: includeAnnotations.engineValue, engineFormFields: includeFormFields.engineValue)
  }

  /// `nil` is ``ContentInclusion/whenPermitted``, `true` ``ContentInclusion/always`` and
  /// `false` ``ContentInclusion/never``.
  @available(*, deprecated, message: "Pass an ContentInclusion: leave it out for nil, .always for true, .never for false.")
  @_disfavoredOverload
  public func setDemand(_ page: PageRef, _ demand: RenderPageViewDemand, includeAnnotations: Bool? = nil, includeFormFields: Bool? = nil) async throws(PDFError) {
    try await setDemand(page, demand, engineAnnotations: includeAnnotations, engineFormFields: includeFormFields)
  }

  private func setDemand(
    _ page: PageRef, _ demand: RenderPageViewDemand, engineAnnotations: Bool?, engineFormFields: Bool?
  ) async throws(PDFError) {
    try await bridged { try await handle.setDemand(page: page, demand: demand, includeAnnotations: engineAnnotations, includeFormFields: engineFormFields) }
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
        name: "InvalidState", code: .invalidState,
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

extension EventRelay: RenderSnapshotListener where Event == RenderSnapshot? {
  func onSnapshot(snapshot: EmbedPDFBindings.RenderSnapshot?) {
    continuation.yield(snapshot.map(RenderSnapshot.init))
  }

  func onEnd() { continuation.finish() }
}

extension PDFViewer {
  public func render(in document: DocumentInfo) async throws(PDFError) -> RenderCapability {
    let handle = try await bridged { try await host.render(documentId: document.id) }
    return RenderCapability(handle: handle, viewer: self)
  }
}
