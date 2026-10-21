import EmbedPDF
import SwiftUI

/// Paints a `TiledPageModel`: the base raster stretched over the page, and
/// the tiles placed over it in page points, coarse under fine.
///
/// The view is laid out in the page's unrotated space. Give it the page's
/// unrotated aspect ratio, and rotate it as a whole for a rotated page.
public struct TiledPageImage: View {
  let model: TiledPageModel
  let unrotatedSize: CGSize
  let deviceScale: Double
  let visibleRect: Rect?
  let revision: Int
  let onPhase: (@MainActor (Phase) -> Void)?

  /// Where an update is, for hosts that time renders.
  public enum Phase: Sendable {
    /// A new demand is being declared.
    case began
    /// The base is current and every tile the plan wants has its pixels.
    case painted
    /// The tile plan has stopped changing.
    case settled
  }

  /// - Parameters:
  ///   - unrotatedSize: The page's size in points, without its rotation.
  ///   - deviceScale: Device pixels per page point at the current zoom.
  ///   - visibleRect: Unrotated page points; `nil` is the whole page and
  ///     `.zero` is a page built but off screen.
  ///   - revision: Change it when the page's pixels change, after
  ///     `RenderCapability.invalidate`, so the base is fetched again.
  ///   - onPhase: Called as each update begins and settles. A cancelled
  ///     update never settles.
  public init(
    model: TiledPageModel,
    unrotatedSize: CGSize,
    deviceScale: Double,
    visibleRect: Rect?,
    revision: Int = 0,
    onPhase: (@MainActor (Phase) -> Void)? = nil
  ) {
    self.model = model
    self.unrotatedSize = unrotatedSize
    self.deviceScale = deviceScale
    self.visibleRect = visibleRect
    self.revision = revision
    self.onPhase = onPhase
  }

  public var body: some View {
    GeometryReader { geometry in
      let scale = geometry.size.width / max(unrotatedSize.width, 1)
      ZStack(alignment: .topLeading) {
        if let base = model.base {
          Image(decorative: base, scale: 1)
            .resizable()
            .frame(width: geometry.size.width, height: geometry.size.height)
        }
        ForEach(model.tiles) { tile in
          TileView(tile: tile, scale: scale, model: model)
        }
      }
    }
    .task(id: Demand(deviceScale: deviceScale, visibleRect: visibleRect, revision: revision)) {
      onPhase?(.began)
      await model.update(
        deviceScale: deviceScale, unrotatedWidth: unrotatedSize.width, visibleRect: visibleRect,
        onPainted: { onPhase?(.painted) })
      guard !Task.isCancelled else { return }
      onPhase?(.settled)
    }
    .onDisappear {
      Task { await model.release() }
    }
  }

  private struct Demand: Hashable {
    let deviceScale: Double
    let visibleRect: Rect?
    let revision: Int
  }
}

private struct TileView: View {
  let tile: TiledPageModel.Tile
  let scale: Double
  let model: TiledPageModel

  var body: some View {
    Image(decorative: tile.image, scale: 1)
      .resizable()
      .frame(width: tile.rect.width * scale, height: tile.rect.height * scale)
      .offset(x: tile.rect.x * scale, y: tile.rect.y * scale)
      .zIndex(tile.z)
      // Two frames after it appears, as the web host does, so the tile is on
      // screen before the plan may drop what's under it.
      .task(id: tile.id) {
        try? await Task.sleep(for: .milliseconds(34))
        guard !Task.isCancelled else { return }
        await model.tileDisplayed(tile.id)
      }
  }
}
