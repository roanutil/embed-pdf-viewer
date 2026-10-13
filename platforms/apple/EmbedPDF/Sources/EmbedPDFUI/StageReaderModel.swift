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
  public private(set) var currentPageIndex: UInt32 = 0
  public private(set) var error: PDFError?
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

  public init(stage: StageCapability, render: RenderCapability) {
    self.stage = stage
    self.render = render
  }

  /// Run for the view's lifetime: mirrors the stage's camera and page.
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
      currentPageIndex = try await stage.currentPageIndex()
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
