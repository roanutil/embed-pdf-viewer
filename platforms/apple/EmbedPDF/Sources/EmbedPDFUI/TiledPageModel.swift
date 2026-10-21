import CoreGraphics
import EmbedPDF
import Observation

/// One page painted from the render plugin's two planes: a base raster held
/// to the full-page budget, and the tile plane above it once the view's
/// demand outgrows that budget.
///
/// Everything here is in the page's unrotated space: demand, visible rect,
/// and tile rects. A host showing a rotated page rotates the whole view.
///
/// The FFI reports base rasters through `RenderCapability.events`, but no
/// event says a tile finished, so `update` polls `plan()` until nothing is in
/// flight and the plan's `stamp` stops changing.
@MainActor
@Observable
public final class TiledPageModel {
  /// A tile with its pixels, placed in page points.
  public struct Tile: Identifiable, Equatable {
    public let id: String
    public let rect: Rect
    public let z: Double
    public let image: CGImage

    public static func == (lhs: Tile, rhs: Tile) -> Bool {
      lhs.id == rhs.id && lhs.rect == rhs.rect && lhs.z == rhs.z
    }
  }

  public private(set) var base: CGImage?
  public private(set) var tiles: [Tile] = []

  public let page: PageRef
  public let includeAnnotations: Bool

  @ObservationIgnored private let render: RenderCapability
  @ObservationIgnored private let demand: RenderViewDemand
  @ObservationIgnored private var baseKey: String?
  @ObservationIgnored private var images: [String: CGImage] = [:]
  @ObservationIgnored private var painted: Set<String> = []
  @ObservationIgnored private var lastStamp: String?
  @ObservationIgnored private var pollTask: Task<Void, Never>?

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

  /// `demand` is the view's handle from `RenderCapability.viewDemand(_:)`,
  /// shared by every page the view shows.
  public init(render: RenderCapability, demand: RenderViewDemand, page: PageRef, includeAnnotations: Bool = true) {
    self.render = render
    self.demand = demand
    self.page = page
    self.includeAnnotations = includeAnnotations
  }

  /// Declares what the view needs, refreshes the base raster, and paints
  /// tiles until the plan settles.
  ///
  /// - Parameters:
  ///   - deviceScale: Device pixels per page point at the current zoom.
  ///   - unrotatedWidth: The page's width in points, without its rotation.
  ///   - visibleRect: The visible region in unrotated page points, y-down;
  ///     `nil` is the whole page and `.zero` is a page built but off screen.
  ///   - onPainted: Called once, when the base is current and every tile
  ///     the plan wants has its pixels: what the view shows from then on is
  ///     sharp. The update keeps polling after it, until the plan settles.
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
    await pollUntilSettled(onPainted: onPainted)
  }

  /// Report that `key`'s tile has been on screen, so the plan can drop the
  /// coarser tiles under it.
  public func tileDisplayed(_ key: String) async {
    guard !painted.contains(key), images[key] != nil else { return }
    painted.insert(key)
    try? await demand.markPainted(page, key: key)
    startPolling()
  }

  /// Stops painting the page and drops its pixels. The raster store keeps
  /// its cached bytes for the next time the page shows.
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
    guard pollTask == nil else { return }
    pollTask = Task { [weak self] in
      await self?.pollUntilSettled()
      self?.pollTask = nil
    }
  }

  private func pollUntilSettled(onPainted: (@MainActor () -> Void)? = nil) async {
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
