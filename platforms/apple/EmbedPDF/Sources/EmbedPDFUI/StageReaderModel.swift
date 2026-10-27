import CoreGraphics
import EmbedPDF
import Observation

/// Main-actor state and gesture forwarding for a stage-based scrolling reader.
///
/// The stage owns layout; this model mirrors its visible pages and camera.
/// Retain one model per reader in SwiftUI state. ``StageReaderView`` starts its
/// event task and reports viewport geometry; custom hosts must do both.
/// Stage and render capabilities must refer to the same open document and
/// retain the viewer without preventing explicit document closure.
@MainActor
@Observable
public final class StageReaderModel {
  /// Pages currently intersecting the stage viewport, with screen transforms.
  ///
  /// Initially empty; refreshed after viewport and camera changes.
  public private(set) var visiblePages: [StageVisiblePage] = []
  /// Latest camera snapshot, or `nil` before the first successful refresh.
  public private(set) var camera: Camera?
  /// Zero-based current page index; initially zero until read from the stage.
  public private(set) var currentPageIndex: UInt32 = 0
  /// Most recent stage failure; successful operations do not clear it.
  public private(set) var error: PDFError?
  /// Stage that owns page layout, camera state, and motion for this reader.
  @ObservationIgnored public let stage: StageCapability
  /// Renderer for the same document as ``stage``.
  @ObservationIgnored public let render: RenderCapability
  @ObservationIgnored private var lastTranslation = CGSize.zero
  @ObservationIgnored private var lastMagnification: CGFloat = 1
  @ObservationIgnored private var gestureOpen = false
  /// The last drag call, which the next one waits for. SwiftUI hands a
  /// gesture's change and end to separate tasks, and each call leaves the
  /// main actor while it reaches the stage, so without this an end could
  /// reach the stage before the begin it closes.
  @ObservationIgnored private var lastDragCall: Task<Void, Never>?

  /// Full-page raster area limit of 16,777,216 pixels (64 MiB in RGBA8).
  ///
  /// This is an area budget, not a limit of 4096 pixels on each dimension.
  public nonisolated static let maxPagePixels: Double = 4096 * 4096

  /// Caps a requested full-page raster scale to ``maxPagePixels``.
  ///
  /// ``StageReaderView`` uses whole-page images, so zoom beyond this cap can
  /// look soft. Use ``TiledPageImage`` for a custom reader with tile detail.
  ///
  /// - Parameters:
  ///   - scale: Requested device pixels per page point; supply a positive value.
  ///   - contentWidth: Page content width in points, without display scaling.
  ///   - contentHeight: Page content height in the same coordinate space.
  /// - Returns: The smaller of `scale` and the budgeted scale; returns `scale`
  ///   unchanged when the content area is zero or negative.
  public nonisolated static func renderScale(
    _ scale: Double, contentWidth: Double, contentHeight: Double
  ) -> Double {
    let area = contentWidth * contentHeight
    guard area > 0 else { return scale }
    return min(scale, (maxPagePixels / area).squareRoot())
  }

  /// Creates a reader model for one open document.
  ///
  /// - Parameters:
  ///   - stage: Stage capability from a viewer configured with `StageConfig`.
  ///   - render: Renderer from the same document with `RenderConfig` enabled.
  public init(stage: StageCapability, render: RenderCapability) {
    self.stage = stage
    self.render = render
  }

  /// Observes stage events for the displayed reader's lifetime.
  ///
  /// Camera-related events refresh visible pages and camera; page events
  /// update the current index. Does not perform an initial refresh: call
  /// ``sizeChanged(_:displayScale:)`` to establish the viewport. Cancel the
  /// owning view task on removal; closure or destruction also ends the stream.
  /// ``StageReaderView`` calls this automatically.
  public func task() async {
    for await event in stage.events {
      switch event {
      case .cameraChanged, .motionEnded, .viewportChanged, .zoomChanged, .settingsChanged:
        await refresh()
      case let .pageChanged(event):
        currentPageIndex = event.pageIndex
      }
    }
  }

  /// Sets the viewport and device pixel ratio, then refreshes reader state.
  ///
  /// - Parameters:
  ///   - size: Viewport dimensions in SwiftUI layout points, before device scale.
  ///   - displayScale: Device pixels per layout point, usually the environment's
  ///     `displayScale` value.
  ///
  /// Stores failures in ``error`` and still attempts a camera/page refresh.
  public func sizeChanged(_ size: CGSize, displayScale: CGFloat) async {
    do {
      try await stage.setDevicePixelRatio(Double(displayScale))
      try await stage.setViewportSize(Size(width: size.width, height: size.height))
      currentPageIndex = try await stage.currentPageIndex()
    } catch {
      self.error = error
    }
    await refresh()
  }

  /// Pans by the change in a drag's cumulative translation.
  ///
  /// Opens an elastic stage gesture on the first change. Calls are serialized
  /// with earlier drag changes and ends, even when delivered from separate tasks.
  /// - Parameter translation: Cumulative SwiftUI drag translation in layout
  ///   points from the gesture's start; positive values follow the finger.
  ///
  /// Finish each drag with ``dragEnded(velocity:)``. Stores failures in ``error``.
  public func dragChanged(translation: CGSize) async {
    await afterEarlierDragCalls { await $0.applyDragChange(translation) }
  }

  /// Ends an open drag gesture and starts a fling for nonzero velocity.
  ///
  /// Waits for earlier drag calls, then refreshes the current page and camera.
  /// - Parameter velocity: SwiftUI finger velocity in layout points per second;
  ///   use `.zero` to stop without initiating a fling.
  ///
  /// Stores failures in ``error``.
  public func dragEnded(velocity: CGSize) async {
    await afterEarlierDragCalls { await $0.applyDragEnd(velocity) }
  }

  private func afterEarlierDragCalls(
    _ work: @escaping @MainActor @Sendable (StageReaderModel) async -> Void
  ) async {
    let previous = lastDragCall
    let call = Task { @MainActor in
      await previous?.value
      await work(self)
    }
    lastDragCall = call
    await call.value
  }

  private func applyDragChange(_ translation: CGSize) async {
    do {
      if !gestureOpen {
        gestureOpen = true
        lastTranslation = .zero
        try await stage.beginGesture(elastic: true)
      }
      // `panBy` follows the finger: a finger moving up scrolls down.
      stage.panBy(
        dx: translation.width - lastTranslation.width,
        dy: translation.height - lastTranslation.height)
      lastTranslation = translation
    } catch {
      self.error = error
    }
    await refresh()
  }

  private func applyDragEnd(_ velocity: CGSize) async {
    do {
      if gestureOpen {
        gestureOpen = false
        try await stage.endGesture()
      }
      if velocity != .zero {
        try await stage.fling(velocityX: velocity.width, velocityY: velocity.height)
      }
      currentPageIndex = try await stage.currentPageIndex()
    } catch {
      self.error = error
    }
    await refresh()
  }

  /// Applies the incremental ratio of a cumulative magnification around a point.
  ///
  /// - Parameters:
  ///   - magnification: Positive cumulative factor from the gesture's start,
  ///     where `1` is unchanged.
  ///   - location: Anchor in the reader viewport's local layout points.
  ///
  /// Call ``magnifyEnded()`` when the gesture ends to reset the stored factor.
  public func magnifyChanged(_ magnification: CGFloat, at location: CGPoint) {
    let factor = magnification / lastMagnification
    lastMagnification = magnification
    stage.zoomAround(Point(x: location.x, y: location.y), factor: Double(factor))
  }

  /// Resets cumulative magnification tracking to one and refreshes reader state.
  public func magnifyEnded() async {
    lastMagnification = 1
    await refresh()
  }

  /// Requests the stage's double-tap zoom and refreshes reader state.
  ///
  /// - Parameter location: Tap position in viewport-local layout points.
  ///
  /// Stores zoom or refresh failures in ``error``.
  public func doubleTapped(at location: CGPoint) async {
    do {
      try await stage.doubleTapZoom(Point(x: location.x, y: location.y))
    } catch {
      self.error = error
    }
    await refresh()
  }

  private func refresh() async {
    do {
      visiblePages = try await stage.visiblePages()
      camera = try await stage.camera()
    } catch {
      self.error = error
    }
  }
}
