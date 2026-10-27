import EmbedPDF
import SwiftUI

/// Composes a page's budgeted base image and detail tiles in SwiftUI.
///
/// Give the view the page's unrotated aspect ratio and bounded dimensions;
/// rotate the completed view as a whole if needed. The view starts updates
/// when scale, visibility, or revision changes, acknowledges displayed tiles,
/// and releases page demand on disappearance. Retain the model in the host.
/// Images are decorative; the host supplies accessible labels and text.
public struct TiledPageImage: View {
  let model: TiledPageModel
  let unrotatedSize: CGSize
  let deviceScale: Double
  let visibleRect: Rect?
  let revision: Int
  let onPhase: (@MainActor (Phase) -> Void)?

  /// Main-actor notifications for a view-driven raster update.
  ///
  /// These describe update execution, not guaranteed successful rendering.
  public enum Phase: Sendable {
    /// The view is about to submit its current scale and visible region.
    case began
    /// The base is current and a nonwaiting plan has all its requested pixels.
    ///
    /// May occur for a base-only plan; does not confirm physical display.
    case painted
    /// The update returned while its SwiftUI task was not cancelled.
    ///
    /// Can also occur after a suppressed error or the model's polling timeout;
    /// it is not proof that the tile plan settled successfully.
    case settled
  }

  /// Creates a tiled image driven by the supplied page model.
  ///
  /// - Parameters:
  ///   - model: Retained model for one page and the surface's shared demand.
  ///   - unrotatedSize: The page's size in points, without its rotation.
  ///   - deviceScale: Positive device pixels per page point at the current zoom.
  ///   - visibleRect: Unrotated page points; `nil` is the whole page and
  ///     a zero-area rectangle is a page built but off screen.
  ///   - revision: Host revision token, initially zero. Change it after
  ///     `RenderCapability.invalidate` to restart the update and check the base
  ///     key, even when scale and visibility are unchanged.
  ///   - onPhase: Optional main-actor update observer. A cancelled task does
  ///     not send `.settled`; `.painted` is not guaranteed for every update.
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

  /// The base-and-tile composition, with demand updates and page cleanup.
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
