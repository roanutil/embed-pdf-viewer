public import EmbedPDFBindings

public typealias Camera = EmbedPDFBindings.Camera
public typealias ZoomMode = EmbedPDFBindings.ZoomMode
public typealias ZoomSpec = EmbedPDFBindings.ZoomSpec
public typealias ViewAnchor = EmbedPDFBindings.ViewAnchor
public typealias PageFrameInsets = EmbedPDFBindings.PageFrameInsets
public typealias StageAlign = EmbedPDFBindings.StageAlign
public typealias StageAlignValue = EmbedPDFBindings.StageAlignValue
public typealias StageAlignment = EmbedPDFBindings.StageAlignment
public typealias StageAlignmentValue = EmbedPDFBindings.StageAlignmentValue
public typealias StageDirection = EmbedPDFBindings.StageDirection
public typealias StageSpreadMode = EmbedPDFBindings.StageSpreadMode
public typealias StageSizingMode = EmbedPDFBindings.StageSizingMode
public typealias StageScrollMetrics = EmbedPDFBindings.StageScrollMetrics
public typealias StageConfig = EmbedPDFBindings.StageConfig
public typealias StageFlow = EmbedPDFBindings.StageFlow
public typealias StageLayout = EmbedPDFBindings.StageLayout
public typealias StageScrollBehavior = EmbedPDFBindings.StageScrollBehavior
public typealias StageGridColumns = EmbedPDFBindings.StageGridColumns
public typealias StageGap = EmbedPDFBindings.StageGap
public typealias StageArrivalAlignValue = EmbedPDFBindings.StageArrivalAlignValue
public typealias StageArrivalAlignment = EmbedPDFBindings.StageArrivalAlignment
public typealias StagePartialArrivalAlignment = EmbedPDFBindings.StagePartialArrivalAlignment
public typealias StageSettings = EmbedPDFBindings.StageSettings
public typealias StageSettingsPatch = EmbedPDFBindings.StageSettingsPatch
public typealias StageSettingKey = EmbedPDFBindings.StageSettingKey
public typealias StageOrientation = EmbedPDFBindings.StageOrientation
public typealias StageBoxQuery = EmbedPDFBindings.StageBoxQuery
public typealias StageResponsiveRule = EmbedPDFBindings.StageResponsiveRule
public typealias StagePageTransform = EmbedPDFBindings.StagePageTransform
public typealias StageVisiblePage = EmbedPDFBindings.StageVisiblePage
public typealias StageViewpoint = EmbedPDFBindings.StageViewpoint
public typealias StageViewState = EmbedPDFBindings.StageViewState
public typealias StageScrollToOptions = EmbedPDFBindings.StageScrollToOptions
public typealias StageGoToOptions = EmbedPDFBindings.StageGoToOptions
public typealias StageRevealWord = EmbedPDFBindings.StageRevealWord
public typealias StageRevealAnchorValue = EmbedPDFBindings.StageRevealAnchorValue
public typealias StageRevealAnchor = EmbedPDFBindings.StageRevealAnchor
public typealias StageRevealZoom = EmbedPDFBindings.StageRevealZoom
public typealias StageRevealOptions = EmbedPDFBindings.StageRevealOptions
public typealias StagePageHit = EmbedPDFBindings.StagePageHit
public typealias StageZoomMode = EmbedPDFBindings.StageZoomMode
public typealias StageZoomTarget = EmbedPDFBindings.StageZoomTarget
public typealias StageInitialViewProvider = EmbedPDFBindings.StageInitialViewProvider
public typealias StageEvent = EmbedPDFBindings.StageEvent
public typealias StagePageChangedEvent = EmbedPDFBindings.StagePageChangedEvent
public typealias StageZoomChangedEvent = EmbedPDFBindings.StageZoomChangedEvent
public typealias StageCameraChangedEvent = EmbedPDFBindings.StageCameraChangedEvent
public typealias StageMotionEndedEvent = EmbedPDFBindings.StageMotionEndedEvent
public typealias StageSettingsChangedEvent = EmbedPDFBindings.StageSettingsChangedEvent
public typealias StageViewportChangedEvent = EmbedPDFBindings.StageViewportChangedEvent

/// A document's viewport lens: camera, zoom, scroll, layout and
/// navigation. Get one with `PDFViewer.stage(for:)`; the viewer needs
/// `ViewerConfig(stage: StageConfig())`. It keeps the viewer alive. After
/// its document closes, every call throws `instance-closed`.
///
/// `setCamera`, `panBy`, `scrollBy` and `zoomAround` return at once and run
/// in order; their errors arrive on `PDFViewer.reportedErrors`.
public struct StageCapability: Sendable {
  let handle: StageHandle
  let viewer: PDFViewer

  // MARK: Read

  public func camera() async throws(PDFError) -> Camera { try await bridged { try await handle.camera() } }
  public func viewportSize() async throws(PDFError) -> Size { try await bridged { try await handle.viewportSize() } }
  public func scrollMetrics() async throws(PDFError) -> StageScrollMetrics { try await bridged { try await handle.scrollMetrics() } }
  public func visiblePages() async throws(PDFError) -> [StageVisiblePage] { try await bridged { try await handle.visiblePages() } }
  public func isPageVisible(_ page: PageRef) async throws(PDFError) -> Bool { try await bridged { try await handle.isPageVisible(page: page) } }
  public func currentPageIndex() async throws(PDFError) -> UInt32 { try await bridged { try await handle.currentPageIndex() } }
  public func currentPage() async throws(PDFError) -> PageInfo? { try await bridged { try await handle.currentPage() } }
  public func currentItemPages() async throws(PDFError) -> [PageInfo] { try await bridged { try await handle.currentItemPages() } }
  public func pageFrame(_ page: PageRef) async throws(PDFError) -> StageVisiblePage? { try await bridged { try await handle.pageFrame(page: page) } }
  public func page(at point: Point) async throws(PDFError) -> StagePageHit? { try await bridged { try await handle.pageAt(point: point) } }
  public func viewportToPage(_ page: PageRef, _ point: Point) async throws(PDFError) -> Point? { try await bridged { try await handle.viewportToPage(page: page, point: point) } }
  public func pageToWorld(_ page: PageRef, _ point: Point) async throws(PDFError) -> Point? { try await bridged { try await handle.pageToWorld(page: page, point: point) } }
  public func pageRectToViewport(_ page: PageRef, _ rect: Rect) async throws(PDFError) -> Rect? { try await bridged { try await handle.pageRectToViewport(page: page, rect: rect) } }
  public func worldToViewport(_ point: Point) async throws(PDFError) -> Point { try await bridged { try await handle.worldToViewport(point: point) } }
  public func viewportToWorld(_ point: Point) async throws(PDFError) -> Point { try await bridged { try await handle.viewportToWorld(point: point) } }
  public func pageToViewport(_ page: PageRef, _ point: Point) async throws(PDFError) -> Point? { try await bridged { try await handle.pageToViewport(page: page, point: point) } }
  public func viewRotation() async throws(PDFError) -> Double { try await bridged { try await handle.viewRotation() } }
  public func zoomLevel() async throws(PDFError) -> Double { try await bridged { try await handle.zoomLevel() } }
  public func zoomMode() async throws(PDFError) -> StageZoomMode { try await bridged { try await handle.zoomMode() } }
  public func viewpoint() async throws(PDFError) -> StageViewpoint { try await bridged { try await handle.viewpoint() } }

  // MARK: Motion

  public func fling(velocityX: Double, velocityY: Double) async throws(PDFError) { try await bridged { try await handle.fling(velocityX: velocityX, velocityY: velocityY) } }
  public func isMoving() async throws(PDFError) -> Bool { try await bridged { try await handle.isMoving() } }
  public func stopMotion() async throws(PDFError) { try await bridged { try await handle.stopMotion() } }
  public func setCamera(_ camera: Camera) { handle.setCamera(camera: camera) }
  public func panBy(dx: Double, dy: Double) { handle.panBy(dx: dx, dy: dy) }
  public func scrollTo(_ options: StageScrollToOptions) async throws(PDFError) { try await bridged { try await handle.scrollTo(options: options) } }
  public func scrollBy(_ options: StageScrollToOptions) { handle.scrollBy(options: options) }
  public func zoomAround(_ point: Point, factor: Double) { handle.zoomAround(point: point, factor: factor) }
  public func beginGesture(elastic: Bool? = nil) async throws(PDFError) { try await bridged { try await handle.beginGesture(elastic: elastic) } }
  public func endGesture() async throws(PDFError) { try await bridged { try await handle.endGesture() } }

  // MARK: Navigation

  public func goToPageIndex(_ index: UInt32, options: StageGoToOptions? = nil) async throws(PDFError) { try await bridged { try await handle.goToPageIndex(index: index, options: options) } }
  public func goToPage(_ page: PageRef, options: StageGoToOptions? = nil) async throws(PDFError) { try await bridged { try await handle.goToPage(page: page, options: options) } }
  public func goToFirstPage(options: StageGoToOptions? = nil) async throws(PDFError) { try await bridged { try await handle.goToFirstPage(options: options) } }
  public func goToLastPage(options: StageGoToOptions? = nil) async throws(PDFError) { try await bridged { try await handle.goToLastPage(options: options) } }
  public func canGoNext() async throws(PDFError) -> Bool { try await bridged { try await handle.canGoNext() } }
  public func canGoPrevious() async throws(PDFError) -> Bool { try await bridged { try await handle.canGoPrevious() } }
  public func nextPage(options: StageGoToOptions? = nil) async throws(PDFError) { try await bridged { try await handle.nextPage(options: options) } }
  public func previousPage(options: StageGoToOptions? = nil) async throws(PDFError) { try await bridged { try await handle.previousPage(options: options) } }
  public func resetView() async throws(PDFError) { try await bridged { try await handle.resetView() } }
  public func reveal(_ page: PageRef, options: StageRevealOptions? = nil) async throws(PDFError) { try await bridged { try await handle.reveal(page: page, options: options) } }
  public func revealRect(_ page: PageRef, _ rect: Rect, options: StageRevealOptions? = nil) async throws(PDFError) { try await bridged { try await handle.revealRect(page: page, rect: rect, options: options) } }
  public func revealIndex(_ index: UInt32, options: StageRevealOptions? = nil) async throws(PDFError) { try await bridged { try await handle.revealIndex(index: index, options: options) } }

  // MARK: Settings

  public func settings() async throws(PDFError) -> StageSettings { try await bridged { try await handle.settings() } }
  public func resetSettings() async throws(PDFError) { try await bridged { try await handle.resetSettings() } }
  public func updateSettings(_ patch: StageSettingsPatch) async throws(PDFError) { try await bridged { try await handle.updateSettings(patch: patch) } }
  public func setResponsiveRules(_ rules: [StageResponsiveRule]) async throws(PDFError) { try await bridged { try await handle.setResponsiveRules(rules: rules) } }
  public func matchesRule(_ name: String) async throws(PDFError) -> Bool { try await bridged { try await handle.matchesRule(name: name) } }
  public func activeRules() async throws(PDFError) -> [String] { try await bridged { try await handle.activeRules() } }
  public func setFlow(_ flow: StageFlow) async throws(PDFError) { try await bridged { try await handle.setFlow(flow: flow) } }
  public func setLayout(_ layout: StageLayout) async throws(PDFError) { try await bridged { try await handle.setLayout(layout: layout) } }
  public func setSpread(_ spread: StageSpreadMode) async throws(PDFError) { try await bridged { try await handle.setSpread(spread: spread) } }
  public func setSizing(_ sizing: StageSizingMode) async throws(PDFError) { try await bridged { try await handle.setSizing(sizing: sizing) } }
  public func applyViewState(_ view: StageViewState) async throws(PDFError) { try await bridged { try await handle.applyViewState(view: view) } }

  // MARK: Zoom

  public func doubleTapZoom(_ point: Point) async throws(PDFError) { try await bridged { try await handle.doubleTapZoom(point: point) } }
  public func zoomIn() async throws(PDFError) { try await bridged { try await handle.zoomIn() } }
  public func zoomOut() async throws(PDFError) { try await bridged { try await handle.zoomOut() } }
  public func zoomTo(_ target: StageZoomTarget, around: Point? = nil) async throws(PDFError) { try await bridged { try await handle.zoomTo(target: target, around: around) } }
  public func zoomBy(_ factor: Double, around: Point? = nil) async throws(PDFError) { try await bridged { try await handle.zoomBy(factor: factor, around: around) } }
  public func fitWidth() async throws(PDFError) { try await bridged { try await handle.fitWidth() } }
  public func fitPage() async throws(PDFError) { try await bridged { try await handle.fitPage() } }
  public func fitAll() async throws(PDFError) { try await bridged { try await handle.fitAll() } }
  public func fitAutomatic() async throws(PDFError) { try await bridged { try await handle.fitAutomatic() } }
  public func setViewRotation(_ degrees: Double) async throws(PDFError) { try await bridged { try await handle.setViewRotation(degrees: degrees) } }
  public func rotateViewBy(_ delta: Double) async throws(PDFError) { try await bridged { try await handle.rotateViewBy(delta: delta) } }

  // MARK: Lifecycle

  public func setViewportSize(_ size: Size) async throws(PDFError) { try await bridged { try await handle.setViewportSize(size: size) } }
  public func setDevicePixelRatio(_ ratio: Double) async throws(PDFError) { try await bridged { try await handle.setDevicePixelRatio(ratio: ratio) } }
  public func refit() async throws(PDFError) { try await bridged { try await handle.refit() } }
  public func viewState() async throws(PDFError) -> StageViewState { try await bridged { try await handle.viewState() } }
  public func lensId() async throws(PDFError) -> String { try await bridged { try await handle.lensId() } }

  /// `provider` is asked on the viewer's kernel thread when the stage
  /// first places this document.
  public func provideInitialView(_ provider: some StageInitialViewProvider, priority: Double = 0)
    async throws(PDFError) -> Registration
  {
    Registration(handle: try await bridged { try await handle.provideInitialView(priority: priority, provider: provider) })
  }

  public func placeInitial() async throws(PDFError) { try await bridged { try await handle.placeInitial() } }

  /// Let a pan tool drag this lens: registers the stage's pan-scroll
  /// handler with the document's pointer hub, scoped to `lensId()`, as the
  /// web's `<Stage>` does. `panFallback` (`nil` is true) lets any other tool
  /// pan over the gaps between pages. Throws `unsupported` without
  /// `ViewerConfig(interaction: true)`.
  public func registerScrollHandler(panFallback: Bool? = nil) async throws(PDFError) -> Registration {
    Registration(handle: try await bridged { try await handle.registerScrollHandler(panFallback: panFallback) })
  }

  /// Finishes when the document closes (at once if it already has) or the
  /// viewer is destroyed.
  public var events: AsyncStream<StageEvent> {
    viewer.registry.stream { handle.subscribe(listener: $0) }
  }
}

extension EventRelay: StageEventListener where Event == StageEvent {
  func onEvent(event: StageEvent) { continuation.yield(event) }
  func onEnd() { continuation.finish() }
}

extension PDFViewer {
  public func stage(for document: DocumentInfo) async throws(PDFError) -> StageCapability {
    let handle = try await bridged { try await host.stage(documentId: document.id) }
    return StageCapability(handle: handle, viewer: self)
  }
}
