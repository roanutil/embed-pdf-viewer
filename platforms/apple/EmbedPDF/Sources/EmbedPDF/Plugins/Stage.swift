public import EmbedPDFBindings

/// The viewport camera's world-space origin and dimensionless zoom multiplier.
public typealias Camera = EmbedPDFBindings.Camera
/// A named fit intent used to resolve a stage zoom level.
public typealias ZoomMode = EmbedPDFBindings.ZoomMode
/// A fixed zoom level, requested page width or height, or named fit intent.
public typealias ZoomSpec = EmbedPDFBindings.ZoomSpec
/// A zero-based page display index and fractions across and down it for retaining a viewpoint.
public typealias ViewAnchor = EmbedPDFBindings.ViewAnchor
/// Insets for a page frame, used when resolving stage layout and fit bounds.
public typealias PageFrameInsets = EmbedPDFBindings.PageFrameInsets
/// A named start, center, or end alignment.
public typealias StageAlign = EmbedPDFBindings.StageAlign
/// A named alignment or viewport fraction at which to place the subject's center.
public typealias StageAlignValue = EmbedPDFBindings.StageAlignValue
/// The horizontal and vertical alignment of a stage item.
public typealias StageAlignment = EmbedPDFBindings.StageAlignment
/// Horizontal and vertical alignment, each a named stop or viewport fraction.
public typealias StageAlignmentValue = EmbedPDFBindings.StageAlignmentValue
/// The left-to-right or right-to-left reading direction used by the layout.
public typealias StageDirection = EmbedPDFBindings.StageDirection
/// How pages are paired into spreads.
public typealias StageSpreadMode = EmbedPDFBindings.StageSpreadMode
/// How page dimensions are sized within the stage layout.
public typealias StageSizingMode = EmbedPDFBindings.StageSizingMode
/// Camera position expressed as viewport-unit native scroll offsets and extents.
public typealias StageScrollMetrics = EmbedPDFBindings.StageScrollMetrics
/// The initial settings and responsive rules for a document's viewport lens.
public typealias StageConfig = EmbedPDFBindings.StageConfig
/// How the stage flows between layout items during navigation and scrolling.
public typealias StageFlow = EmbedPDFBindings.StageFlow
/// How pages are arranged into stage layout items.
public typealias StageLayout = EmbedPDFBindings.StageLayout
/// Whether camera navigation jumps immediately or animates smoothly.
public typealias StageScrollBehavior = EmbedPDFBindings.StageScrollBehavior
/// The column sizing rule for a stage grid layout.
public typealias StageGridColumns = EmbedPDFBindings.StageGridColumns
/// Spacing between stage layout items.
public typealias StageGap = EmbedPDFBindings.StageGap
/// An alignment value used when a page arrives in the viewport.
public typealias StageArrivalAlignValue = EmbedPDFBindings.StageArrivalAlignValue
/// Horizontal and vertical alignment for page arrival.
public typealias StageArrivalAlignment = EmbedPDFBindings.StageArrivalAlignment
/// Selected page-arrival alignment axes to override.
public typealias StagePartialArrivalAlignment = EmbedPDFBindings.StagePartialArrivalAlignment
/// The effective layout, zoom, and navigation settings of a stage lens.
public typealias StageSettings = EmbedPDFBindings.StageSettings
/// Selected stage settings to update without replacing the others.
public typealias StageSettingsPatch = EmbedPDFBindings.StageSettingsPatch
/// A setting identified in a stage settings change.
public typealias StageSettingKey = EmbedPDFBindings.StageSettingKey
/// An orientation used to match responsive stage settings.
public typealias StageOrientation = EmbedPDFBindings.StageOrientation
/// Viewport constraints for matching a responsive stage rule.
public typealias StageBoxQuery = EmbedPDFBindings.StageBoxQuery
/// A named viewport query and settings to apply when it matches.
public typealias StageResponsiveRule = EmbedPDFBindings.StageResponsiveRule
/// Transforms between intrinsic PDF points, view units, and device pixels for a laid-out page.
public typealias StagePageTransform = EmbedPDFBindings.StagePageTransform
/// A laid-out page, its viewport position, transform, and visible page-space rectangle.
public typealias StageVisiblePage = EmbedPDFBindings.StageVisiblePage
/// The current page anchor and zoom intent for restoring a viewpoint.
public typealias StageViewpoint = EmbedPDFBindings.StageViewpoint
/// Stage settings, cursor, and anchor suitable for restoring a document view.
public typealias StageViewState = EmbedPDFBindings.StageViewState
/// Scroll offsets in viewport units and optional animation behavior.
public typealias StageScrollToOptions = EmbedPDFBindings.StageScrollToOptions
/// Alignment and animation options for page navigation.
public typealias StageGoToOptions = EmbedPDFBindings.StageGoToOptions
/// A named alignment used to reveal a page region.
public typealias StageRevealWord = EmbedPDFBindings.StageRevealWord
/// A numeric or named alignment value for one reveal axis.
public typealias StageRevealAnchorValue = EmbedPDFBindings.StageRevealAnchorValue
/// The horizontal and vertical anchoring used to reveal a region.
public typealias StageRevealAnchor = EmbedPDFBindings.StageRevealAnchor
/// The zoom intent to apply while revealing a region.
public typealias StageRevealZoom = EmbedPDFBindings.StageRevealZoom
/// Alignment, zoom, and animation options for revealing page content.
public typealias StageRevealOptions = EmbedPDFBindings.StageRevealOptions
/// The page and page-space point beneath a viewport position.
public typealias StagePageHit = EmbedPDFBindings.StagePageHit
/// Whether the current zoom intent uses a fit mode or a custom level.
public typealias StageZoomMode = EmbedPDFBindings.StageZoomMode
/// A fit intent or numeric zoom level to apply to the stage.
public typealias StageZoomTarget = EmbedPDFBindings.StageZoomTarget
/// A synchronous kernel-thread callback offering an initial view, or nil to defer to another provider.
public typealias StageInitialViewProvider = EmbedPDFBindings.StageInitialViewProvider
/// An event describing stage camera, layout, and viewport changes.
public typealias StageEvent = EmbedPDFBindings.StageEvent
/// An event describing a current-page change.
public typealias StagePageChangedEvent = EmbedPDFBindings.StagePageChangedEvent
/// An event describing a resolved zoom change.
public typealias StageZoomChangedEvent = EmbedPDFBindings.StageZoomChangedEvent
/// An event describing a camera change.
public typealias StageCameraChangedEvent = EmbedPDFBindings.StageCameraChangedEvent
/// An event describing stage motion ending.
public typealias StageMotionEndedEvent = EmbedPDFBindings.StageMotionEndedEvent
/// An event describing an effective settings change.
public typealias StageSettingsChangedEvent = EmbedPDFBindings.StageSettingsChangedEvent
/// An event describing a viewport-size change.
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

  /// Returns the camera origin in world units and its resolved zoom factor.
  public func camera() async throws(PDFError) -> Camera { try await bridged { try await handle.camera() } }
  /// Returns the container size last reported to the stage, in logical view units.
  public func viewportSize() async throws(PDFError) -> Size { try await bridged { try await handle.viewportSize() } }
  /// Returns the camera as native scroll offsets and extents in viewport units.
  public func scrollMetrics() async throws(PDFError) -> StageScrollMetrics { try await bridged { try await handle.scrollMetrics() } }
  /// Returns the laid-out pages currently on screen, or an empty list before initial placement.
  public func visiblePages() async throws(PDFError) -> [StageVisiblePage] { try await bridged { try await handle.visiblePages() } }
  /// Returns whether the page appears in the current visible-page set.
  public func isPageVisible(_ page: PageRef) async throws(PDFError) -> Bool { try await bridged { try await handle.isPageVisible(page: page) } }
  /// Returns the cursor page's zero-based display index.
  public func currentPageIndex() async throws(PDFError) -> UInt32 { try await bridged { try await handle.currentPageIndex() } }
  /// Returns the cursor page's reference, or nil before the document has pages.
  public func currentPage() async throws(PDFError) -> PageRef? { try await bridged { try await handle.currentPage() } }
  /// Returns the pages in the current layout item, which can be a page, spread, or grid row.
  public func currentItemPages() async throws(PDFError) -> [PageInfo] { try await bridged { try await handle.currentItemPages() } }
  /// Returns a page's laid-out frame and transform, or nil before placement or for an unknown page.
  public func pageFrame(_ page: PageRef) async throws(PDFError) -> StageVisiblePage? { try await bridged { try await handle.pageFrame(page: page) } }
  /// Hit-tests a viewport-space point and returns its page-space position, or nil over a gap.
  public func page(at point: Point) async throws(PDFError) -> StagePageHit? { try await bridged { try await handle.pageAt(point: point) } }
  /// Converts a logical viewport point to intrinsic PDF page coordinates without clamping;
  /// returns nil when the page is not in the visible set.
  public func viewportToPage(_ page: PageRef, _ point: Point) async throws(PDFError) -> Point? { try await bridged { try await handle.viewportToPage(page: page, point: point) } }
  /// Converts intrinsic PDF page coordinates to layout world coordinates, or returns nil when
  /// the page has no layout frame.
  public func pageToWorld(_ page: PageRef, _ point: Point) async throws(PDFError) -> Point? { try await bridged { try await handle.pageToWorld(page: page, point: point) } }
  /// Converts an intrinsic page rectangle to its viewport-space bounding box, or returns nil
  /// when the page has no layout frame.
  public func pageRectToViewport(_ page: PageRef, _ rect: Rect) async throws(PDFError) -> Rect? { try await bridged { try await handle.pageRectToViewport(page: page, rect: rect) } }
  /// Converts a layout world point to logical viewport coordinates using the current camera.
  public func worldToViewport(_ point: Point) async throws(PDFError) -> Point { try await bridged { try await handle.worldToViewport(point: point) } }
  /// Converts a logical viewport point to layout world coordinates using the current camera.
  public func viewportToWorld(_ point: Point) async throws(PDFError) -> Point { try await bridged { try await handle.viewportToWorld(point: point) } }
  /// Converts intrinsic PDF page coordinates to logical viewport coordinates, or returns nil
  /// when the page has no layout frame.
  public func pageToViewport(_ page: PageRef, _ point: Point) async throws(PDFError) -> Point? { try await bridged { try await handle.pageToViewport(page: page, point: point) } }
  /// Returns this lens's view rotation in degrees; it does not rotate the document's saved pages.
  public func viewRotation() async throws(PDFError) -> Double { try await bridged { try await handle.viewRotation() } }
  /// Returns the camera's resolved zoom multiplier.
  public func zoomLevel() async throws(PDFError) -> Double { try await bridged { try await handle.zoomLevel() } }
  /// Returns the active fit intent, or custom mode for a fixed zoom level.
  public func zoomMode() async throws(PDFError) -> StageZoomMode { try await bridged { try await handle.zoomMode() } }
  /// Returns the current page anchor and zoom intent; throws when no layout item can supply the anchor.
  public func viewpoint() async throws(PDFError) -> StageViewpoint { try await bridged { try await handle.viewpoint() } }

  // MARK: Motion

  /// Starts inertial camera motion with horizontal and vertical velocity in viewport units per second.
  public func fling(velocityX: Double, velocityY: Double) async throws(PDFError) { try await bridged { try await handle.fling(velocityX: velocityX, velocityY: velocityY) } }
  /// Returns whether the stage currently has active camera motion.
  public func isMoving() async throws(PDFError) -> Bool { try await bridged { try await handle.isMoving() } }
  /// Stops the stage's current camera animation or inertial motion.
  public func stopMotion() async throws(PDFError) { try await bridged { try await handle.stopMotion() } }
  /// Queues a camera change using a world-space origin and zoom; errors arrive through the
  /// viewer's reportedErrors stream.
  public func setCamera(_ camera: Camera) { handle.setCamera(camera: camera) }
  /// Queues a camera pan by logical viewport deltas; errors arrive through the viewer's
  /// reportedErrors stream.
  public func panBy(dx: Double, dy: Double) { handle.panBy(dx: dx, dy: dy) }
  /// Sets absolute logical scroll offsets, clamped to stage bounds; smooth behavior starts an
  /// animation rather than waiting for it to finish.
  public func scrollTo(_ options: StageScrollToOptions) async throws(PDFError) { try await bridged { try await handle.scrollTo(options: options) } }
  /// Queues a relative logical scroll offset; errors arrive through the viewer's reportedErrors stream.
  public func scrollBy(_ options: StageScrollToOptions) { handle.scrollBy(options: options) }
  /// Queues a multiplicative zoom about a logical viewport point; errors arrive through the
  /// viewer's reportedErrors stream.
  public func zoomAround(_ point: Point, factor: Double) { handle.zoomAround(point: point, factor: factor) }
  /// Brackets continuous camera manipulation and stops existing motion; pair each call with
  /// endGesture. Elastic overscroll is enabled only when elastic is true.
  public func beginGesture(elastic: Bool? = nil) async throws(PDFError) { try await bridged { try await handle.beginGesture(elastic: elastic) } }
  /// Ends one level of the gesture bracket and settles elastic overscroll when the outermost gesture ends.
  public func endGesture() async throws(PDFError) { try await bridged { try await handle.endGesture() } }

  // MARK: Navigation

  /// Navigates to a zero-based page display index using optional alignment and animation settings.
  public func goToPageIndex(_ index: UInt32, options: StageGoToOptions? = nil) async throws(PDFError) { try await bridged { try await handle.goToPageIndex(index: index, options: options) } }
  /// Navigates to the supplied durable page identity using optional alignment and animation settings.
  public func goToPage(_ page: PageRef, options: StageGoToOptions? = nil) async throws(PDFError) { try await bridged { try await handle.goToPage(page: page, options: options) } }
  /// Navigates to the document's first page.
  public func goToFirstPage(options: StageGoToOptions? = nil) async throws(PDFError) { try await bridged { try await handle.goToFirstPage(options: options) } }
  /// Navigates to the document's last page.
  public func goToLastPage(options: StageGoToOptions? = nil) async throws(PDFError) { try await bridged { try await handle.goToLastPage(options: options) } }
  /// Returns whether navigation can advance beyond the current layout item.
  public func canGoNext() async throws(PDFError) -> Bool { try await bridged { try await handle.canGoNext() } }
  /// Returns whether navigation can move before the current layout item.
  public func canGoPrevious() async throws(PDFError) -> Bool { try await bridged { try await handle.canGoPrevious() } }
  /// Navigates to the next layout item according to the active flow and layout.
  public func nextPage(options: StageGoToOptions? = nil) async throws(PDFError) { try await bridged { try await handle.nextPage(options: options) } }
  /// Navigates to the previous layout item according to the active flow and layout.
  public func previousPage(options: StageGoToOptions? = nil) async throws(PDFError) { try await bridged { try await handle.previousPage(options: options) } }
  /// Restores the stage's default view and placement.
  public func resetView() async throws(PDFError) { try await bridged { try await handle.resetView() } }
  /// Brings the supplied page into view using optional alignment, zoom, and animation settings.
  public func reveal(_ page: PageRef, options: StageRevealOptions? = nil) async throws(PDFError) { try await bridged { try await handle.reveal(page: page, options: options) } }
  /// Reveals the page at a zero-based display index.
  public func revealIndex(_ index: UInt32, options: StageRevealOptions? = nil) async throws(PDFError) { try await bridged { try await handle.revealIndex(index: index, options: options) } }

  // MARK: Settings

  /// Returns the effective stage settings after current configuration and responsive rules.
  public func settings() async throws(PDFError) -> StageSettings { try await bridged { try await handle.settings() } }
  /// Restores settings from the stage's original configuration, then reapplies matching responsive rules.
  public func resetSettings() async throws(PDFError) { try await bridged { try await handle.resetSettings() } }
  /// Applies the supplied settings patch while preserving fields absent from the patch.
  public func updateSettings(_ patch: StageSettingsPatch) async throws(PDFError) { try await bridged { try await handle.updateSettings(patch: patch) } }
  /// Replaces the viewport-responsive rules used by this stage.
  public func setResponsiveRules(_ rules: [StageResponsiveRule]) async throws(PDFError) { try await bridged { try await handle.setResponsiveRules(rules: rules) } }
  /// Returns whether the named responsive rule currently matches this stage's viewport.
  public func matchesRule(_ name: String) async throws(PDFError) -> Bool { try await bridged { try await handle.matchesRule(name: name) } }
  /// Returns the names of responsive rules currently matching the viewport.
  public func activeRules() async throws(PDFError) -> [String] { try await bridged { try await handle.activeRules() } }
  /// Restores settings, cursor, and anchor from a previously captured view state.
  public func applyViewState(_ view: StageViewState) async throws(PDFError) { try await bridged { try await handle.applyViewState(view: view) } }

  // MARK: Zoom

  /// Applies the stage's double-tap zoom behavior about a logical viewport point.
  public func doubleTapZoom(_ point: Point) async throws(PDFError) { try await bridged { try await handle.doubleTapZoom(point: point) } }
  /// Multiplies zoom by 1.2 about the configured zoom-alignment point.
  public func zoomIn() async throws(PDFError) { try await bridged { try await handle.zoomIn() } }
  /// Divides zoom by 1.2 about the configured zoom-alignment point.
  public func zoomOut() async throws(PDFError) { try await bridged { try await handle.zoomOut() } }
  /// Applies a fit intent or fixed zoom level, optionally anchored about a logical viewport point.
  public func zoomTo(_ target: StageZoomTarget, around: Point? = nil) async throws(PDFError) { try await bridged { try await handle.zoomTo(target: target, around: around) } }
  /// Multiplies the current zoom by factor, optionally anchored about a logical viewport point.
  public func zoomBy(_ factor: Double, around: Point? = nil) async throws(PDFError) { try await bridged { try await handle.zoomBy(factor: factor, around: around) } }
  /// Changes the zoom intent to fit the current item's width.
  public func fitWidth() async throws(PDFError) { try await bridged { try await handle.fitWidth() } }
  /// Changes the zoom intent to fit the current page item in the viewport.
  public func fitPage() async throws(PDFError) { try await bridged { try await handle.fitPage() } }
  /// Changes the zoom intent to fit the full layout scene.
  public func fitAll() async throws(PDFError) { try await bridged { try await handle.fitAll() } }
  /// Fits the current item's width without increasing zoom beyond 100 percent.
  public func fitAutomatic() async throws(PDFError) { try await bridged { try await handle.fitAutomatic() } }
  /// Sets this lens's display rotation in degrees without modifying saved page rotation; use
  /// quarter-turn values (0, 90, 180, or 270).
  public func setViewRotation(_ degrees: Double) async throws(PDFError) { try await bridged { try await handle.setViewRotation(degrees: degrees) } }
  /// Rotates this lens one quarter-turn: delta equal to 90 rotates clockwise; every other
  /// value rotates counterclockwise. Saved page rotation is unchanged.
  public func rotateViewBy(_ delta: Double) async throws(PDFError) { try await bridged { try await handle.rotateViewBy(delta: delta) } }

  // MARK: Lifecycle

  /// Reports container dimensions in logical view units; the first positive size places the
  /// document, and later sizes preserve the anchor and update fit modes.
  public func setViewportSize(_ size: Size) async throws(PDFError) { try await bridged { try await handle.setViewportSize(size: size) } }
  /// Reports device pixels per logical view unit; only a positive, changed ratio is applied.
  public func setDevicePixelRatio(_ ratio: Double) async throws(PDFError) { try await bridged { try await handle.setDevicePixelRatio(ratio: ratio) } }
  /// Recomputes the active zoom intent while retaining the anchor; does nothing before initial placement.
  public func refit() async throws(PDFError) { try await bridged { try await handle.refit() } }
  /// Captures settings, cursor, and anchor for restoring this document view.
  public func viewState() async throws(PDFError) -> StageViewState { try await bridged { try await handle.viewState() } }
  /// Returns this stage's identity for stamping pointer samples and scoping interaction handlers.
  public func lensId() async throws(PDFError) -> String { try await bridged { try await handle.lensId() } }

  /// Offers an initial view candidate; the highest-priority non-nil candidate wins.
  ///
  /// `provider` is asked on the viewer's kernel thread when the stage
  /// first places this document.
  public func provideInitialView(_ provider: some StageInitialViewProvider, priority: Double = 0)
    async throws(PDFError) -> Registration
  {
    Registration(handle: try await bridged { try await handle.provideInitialView(priority: priority, provider: provider) })
  }

  /// Resolves initial-view providers by priority and applies the first non-nil result, or
  /// places the default view.
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
  /// Returns this document's viewport lens; configure stage support before requesting it.
  public func stage(for document: DocumentInfo) async throws(PDFError) -> StageCapability {
    let handle = try await bridged { try await host.stage(documentId: document.id) }
    return StageCapability(handle: handle, viewer: self)
  }
}
