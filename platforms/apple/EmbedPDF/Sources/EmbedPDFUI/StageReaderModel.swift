import CoreGraphics
import EmbedPDF
import Observation

/// What a scrolling reader shows, and the gestures it forwards. The stage
/// lays out; this model only mirrors its visible pages and camera onto the
/// main actor and turns SwiftUI gesture values into stage calls.
@MainActor
@Observable
public final class StageReaderModel {
  public private(set) var visiblePages: [StageVisiblePage] = []
  public private(set) var camera: Camera?
  /// The stage's current page as a zero-based display position.
  public private(set) var pageIndex = 0
  /// ``pageIndex`` as the engine's `UInt32`, which always holds it: the
  /// stage reports it as one.
  @available(*, deprecated, message: "Use pageIndex, an Int; this is the same position as a UInt32.")
  public var currentPageIndex: UInt32 { UInt32(exactly: pageIndex) ?? 0 }
  public private(set) var error: PDFError?
  private var renderRevisions: [PageRef: UInt64] = [:]
  @ObservationIgnored public let stage: StageCapability
  @ObservationIgnored public let render: RenderCapability
  @ObservationIgnored private var lastTranslation = CGSize.zero
  @ObservationIgnored private var lastMagnification: CGFloat = 1
  @ObservationIgnored private var gestureOpen = false
  /// The last drag call, which the next one waits for. SwiftUI hands a
  /// gesture's change and end to separate tasks, and each call leaves the
  /// main actor while it reaches the stage, so without this an end could
  /// reach the stage before the begin it closes.
  @ObservationIgnored private var lastDragCall: Task<Void, Never>?

  /// The most pixels one page renders at: 4096 by 4096, 64 MB of RGBA.
  public nonisolated static let maxPagePixels: Double = 4096 * 4096

  /// The scale to render a page at: the stage's `renderScale`, capped so the
  /// whole page stays within `maxPagePixels`. The reader renders pages whole
  /// until it draws the tile plane, so past the cap a deep zoom is soft
  /// rather than hundreds of megabytes a page. Sizes in page points.
  public nonisolated static func renderScale(
    _ scale: Double, contentWidth: Double, contentHeight: Double
  ) -> Double {
    let area = contentWidth * contentHeight
    guard area > 0 else { return scale }
    return min(scale, (maxPagePixels / area).squareRoot())
  }

  public init(stage: StageCapability, render: RenderCapability) {
    self.stage = stage
    self.render = render
  }

  /// Run for the view's lifetime, projecting stage and render events.
  public func task() async {
    let stageEvents = stage.events
    let renderEvents = render.events
    await withTaskGroup(of: Void.self) { group in
      group.addTask {
        for await event in stageEvents {
          guard !Task.isCancelled else { return }
          await self.receiveStage(event)
        }
      }
      group.addTask {
        for await event in renderEvents { await self.receiveRender(event) }
      }
      await refresh()
      _ = await group.next()
      group.cancelAll()
    }
  }

  /// A presentation revision that changes when Rust invalidates this page.
  public func renderRevision(for page: PageRef) -> UInt64 { renderRevisions[page, default: 0] }

  private func receiveStage(_ event: StageEvent) async {
    switch event {
    case .cameraChanged, .motionEnded, .viewportChanged, .zoomChanged, .settingsChanged:
      await refresh()
    case let .pageChanged(event):
      do {
        pageIndex = try event.pageIndex.toInt("page index")
      } catch {
        self.error = error
      }
    }
  }

  private func receiveRender(_ event: RenderEvent) {
    guard !Task.isCancelled, case let .invalidated(pages, _) = event else { return }
    for page in pages { renderRevisions[page, default: 0] &+= 1 }
  }

  public func sizeChanged(_ size: CGSize, displayScale: CGFloat) async {
    do {
      try await stage.setDevicePixelRatio(Double(displayScale))
      try await stage.setViewportSize(Size(width: size.width, height: size.height))
      pageIndex = try await stage.currentPageIndex()
    } catch {
      self.error = error
    }
    await refresh()
  }

  /// Runs after every earlier drag call has finished.
  public func dragChanged(translation: CGSize) async {
    await afterEarlierDragCalls { await $0.applyDragChange(translation) }
  }

  /// `velocity` in points per second, as `DragGesture.Value.velocity`
  /// reports. Runs after every earlier drag call has finished.
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
      pageIndex = try await stage.currentPageIndex()
    } catch {
      self.error = error
    }
    await refresh()
  }

  public func magnifyChanged(_ magnification: CGFloat, at location: CGPoint) {
    let factor = magnification / lastMagnification
    lastMagnification = magnification
    stage.zoomAround(Point(x: location.x, y: location.y), factor: Double(factor))
  }

  public func magnifyEnded() async {
    lastMagnification = 1
    await refresh()
  }

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
