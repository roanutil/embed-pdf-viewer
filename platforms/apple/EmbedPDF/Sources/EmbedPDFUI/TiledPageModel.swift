import CoreGraphics
import EmbedPDF
import Observation

/// Main-actor raster state for one page with a budgeted base and detail tiles.
///
/// Retain one model per page, sharing a `RenderViewDemand` across pages in the
/// same reader surface. Demand, visibility, and tile rectangles all use the
/// page's unrotated point space, with x rightward and y downward. Rotate the
/// composed view as a whole when displaying a rotated page.
///
/// ``update(deviceScale:unrotatedWidth:visibleRect:onPainted:)`` polls for tiles
/// rather than subscribing to render events. The host must request an update
/// after pixel changes. Errors are suppressed; a missing base or incomplete
/// tiles do not provide a diagnostic error property.
@MainActor
@Observable
public final class TiledPageModel {
  /// An owned tile image positioned in unrotated page points.
  ///
  /// Equality compares identity, rectangle, and stacking order, not image bytes.
  public struct Tile: Identifiable, Equatable {
    /// Opaque raster-source key used for display acknowledgments.
    public let id: String
    /// Placement rectangle in unrotated page points with a top-left origin.
    public let rect: Rect
    /// Stacking order; larger values draw above coarser tiles.
    public let z: Double
    /// Immutable image that owns its pixel buffer independently of the document.
    public let image: CGImage

    /// Compares tile identity and placement without comparing pixel bytes.
    public static func == (lhs: Tile, rhs: Tile) -> Bool {
      lhs.id == rhs.id && lhs.rect == rhs.rect && lhs.z == rhs.z
    }
  }

  /// Budgeted whole-page raster, initially `nil`; retained by ``release()``.
  public private(set) var base: CGImage?
  /// Current plan's available tile images in paint order; initially empty.
  public private(set) var tiles: [Tile] = []

  /// Stable page reference served by this model for its entire lifetime.
  public let page: PageRef
  /// Whether both raster planes include annotations; fixed at initialization.
  public let includeAnnotations: Bool

  @ObservationIgnored private let render: RenderCapability
  @ObservationIgnored private let demand: RenderViewDemand
  @ObservationIgnored private var baseKey: String?
  @ObservationIgnored private var images: [String: CGImage] = [:]
  @ObservationIgnored private var painted: Set<String> = []
  @ObservationIgnored private var lastStamp: String?
  @ObservationIgnored private var pollTask: Task<Void, Never>?
  /// Bumped for each background poll so a finished one can't clear a newer one.
  @ObservationIgnored private var pollGeneration = 0
  /// Updates currently polling. Background polling waits while any is live, so
  /// only one loop reconciles against the plan at a time.
  @ObservationIgnored private var updatePolls = 0
  @ObservationIgnored private var livePolls = 0
  /// The most poll loops ever live at once. Tests check it stays at one.
  @ObservationIgnored private(set) var peakPolls = 0

  /// Between polls while tiles are in flight. The controller re-plans an
  /// arrival on its own about 16 ms later.
  @ObservationIgnored static let pollInterval: Duration = .milliseconds(32)
  /// How long the plan must stay unchanged, with nothing in flight, to count
  /// as settled. Longer than the tile plane's default `settleMs` of 150, which
  /// holds fetches back after a demand change.
  @ObservationIgnored static let quietPeriod: Duration = .milliseconds(250)
  /// The longest an update keeps polling, so a plan that never fills can't
  /// poll forever.
  @ObservationIgnored static let pollLimit: Duration = .seconds(5)

  /// Creates a tiled raster model for one page in a reader surface.
  ///
  /// - Parameters:
  ///   - render: Render capability for the page's open document.
  ///   - demand: Surface handle from `RenderCapability.viewDemand(_:)`, shared
  ///     across this surface's pages. The owner must dispose it when done.
  ///   - page: Page belonging to the renderer's document.
  ///   - includeAnnotations: Whether to include annotations; defaults to `true`.
  ///
  /// The model does not dispose the shared demand handle or close the document.
  public init(render: RenderCapability, demand: RenderViewDemand, page: PageRef, includeAnnotations: Bool = true) {
    self.render = render
    self.demand = demand
    self.page = page
    self.includeAnnotations = includeAnnotations
  }

  /// Declares raster demand, refreshes the base, and polls for detail tiles.
  ///
  /// Polls at 32-millisecond intervals until the plan stays quiet for 250
  /// milliseconds, the calling task is cancelled, a plan request fails, or
  /// five seconds elapse. Returns without throwing and suppresses capability
  /// failures; returning does not guarantee that every requested pixel exists.
  /// Retains an earlier image if refreshing fails. Cancel an earlier update
  /// before starting another for the same model.
  ///
  /// - Parameters:
  ///   - deviceScale: Positive device pixels per page point at the current zoom.
  ///   - unrotatedWidth: Positive page width in points, without its rotation.
  ///   - visibleRect: The visible region in unrotated page points, y-down;
  ///     `nil` is the whole page and a zero-area rectangle is a page built but off screen.
  ///   - onPainted: Main-actor callback invoked at most once after the base is
  ///     current and every tile in a nonwaiting plan has pixels. It can run
  ///     for a base-only plan. It acknowledges available pixels, not display
  ///     completion, and may never run on error, cancellation, or timeout.
  public func update(
    deviceScale: Double,
    unrotatedWidth: Double,
    visibleRect: Rect?,
    onPainted: (@MainActor () -> Void)? = nil
  ) async {
    do {
      try await demand.setDemand(
        page,
        RenderPageViewDemand(desiredDeviceWidth: deviceScale * unrotatedWidth, visibleRect: visibleRect),
        includeAnnotations: includeAnnotations)
      guard !Task.isCancelled else { return }
      let key = try await render.sourceKey(page, scale: deviceScale, includeAnnotations: includeAnnotations)
      if key != baseKey {
        let rendered = try await render.renderSource(
          page, scale: deviceScale, includeAnnotations: includeAnnotations)
        guard !Task.isCancelled else { return }
        base = rendered.image
        baseKey = key
      }
    } catch {
      return
    }
    pollTask?.cancel()
    pollTask = nil
    updatePolls += 1
    defer { updatePolls -= 1 }
    await pollUntilSettled(onPainted: onPainted)
  }

  /// Acknowledges that a tile has appeared so coarser covering tiles can retire.
  ///
  /// - Parameter key: A key from ``Tile/id`` currently held by this model.
  ///
  /// Ignores unknown and already acknowledged keys; suppresses capability
  /// errors and starts background polling. ``TiledPageImage`` calls this after
  /// a short display delay; custom painters should call it after presentation.
  public func tileDisplayed(_ key: String) async {
    guard !painted.contains(key), images[key] != nil else { return }
    painted.insert(key)
    try? await demand.markPainted(page, key: key)
    startPolling()
  }

  /// Releases this page's tile demand and clears local tile images.
  ///
  /// Cancels background polling started by ``tileDisplayed(_:)`` and suppresses
  /// release failures. The base raster and its key remain retained, and the
  /// render store may keep cached bytes. Cancel any caller-owned ``update(deviceScale:unrotatedWidth:visibleRect:onPainted:)``
  /// task before releasing to avoid repopulating tiles. Dispose the shared
  /// demand separately after all pages stop using it; discard this model to
  /// release its retained base image.
  public func release() async {
    pollTask?.cancel()
    pollTask = nil
    tiles = []
    images = [:]
    painted = []
    lastStamp = nil
    try? await demand.release(page)
  }

  // MARK: - Polling

  private func startPolling() {
    guard pollTask == nil, updatePolls == 0 else { return }
    pollGeneration += 1
    let generation = pollGeneration
    pollTask = Task { [weak self] in
      await self?.pollUntilSettled()
      if self?.pollGeneration == generation {
        self?.pollTask = nil
      }
    }
  }

  private func pollUntilSettled(onPainted: (@MainActor () -> Void)? = nil) async {
    livePolls += 1
    peakPolls = max(peakPolls, livePolls)
    defer { livePolls -= 1 }
    var onPainted = onPainted
    let clock = ContinuousClock()
    let deadline = clock.now + Self.pollLimit
    var quietSince = clock.now
    while !Task.isCancelled, clock.now < deadline {
      guard let plan = try? await demand.plan(page) else { return }
      let changed = plan.stamp != lastStamp
      lastStamp = plan.stamp
      await reconcile(plan)
      let waiting = !plan.fetching.isEmpty || (plan.engaged && plan.paint.isEmpty)
      if !waiting, plan.paint.allSatisfy({ images[$0.key] != nil }) {
        onPainted?()
        onPainted = nil
      }
      if changed || waiting {
        quietSince = clock.now
      } else if clock.now - quietSince >= Self.quietPeriod {
        return
      }
      try? await Task.sleep(for: Self.pollInterval)
    }
  }

  private func reconcile(_ plan: RenderTilePlan) async {
    for source in plan.paint where images[source.key] == nil {
      guard let tile = try? await demand.tileImage(page, key: source.key) else { continue }
      guard !Task.isCancelled else { return }
      images[source.key] = tile.image
    }
    let wanted = Set(plan.paint.map(\.key))
    for key in Set(images.keys).subtracting(wanted) {
      images[key] = nil
      if painted.remove(key) != nil {
        try? await demand.markUnpainted(page, key: key)
      }
    }
    let next = plan.paint.compactMap { source in
      images[source.key].map { Tile(id: source.key, rect: source.rect, z: source.z, image: $0) }
    }
    if next != tiles {
      tiles = next
    }
  }
}
