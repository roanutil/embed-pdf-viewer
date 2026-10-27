import Foundation

/// How sharp a view should render a page: full resolution while any part of
/// it is on screen, a small placeholder otherwise.
///
/// Lazy stacks keep neighbor pages built, and accessibility clients build
/// more of them. A page that renders at full resolution whenever its view
/// exists holds megabytes for pages nobody can see.
public struct PageRenderResolution: Hashable, Sendable {
  /// Device pixels across the page's displayed width.
  public let deviceWidth: Double
  /// Whether this is the on-screen render rather than the placeholder.
  public let isFull: Bool

  /// Device pixels for a page that is built but not on screen.
  public static let offScreenDeviceWidth: Double = 256

  /// Full-resolution widths round up to this many pixels, so small layout
  /// changes don't each start a new render.
  public static let deviceWidthStep: Double = 128

  /// Creates an explicit render demand without rounding or validating its width.
  ///
  /// - Parameters:
  ///   - deviceWidth: Desired raster width in device pixels.
  ///   - isFull: Whether this demand represents an on-screen page.
  public init(deviceWidth: Double, isFull: Bool) {
    self.deviceWidth = deviceWidth
    self.isFull = isFull
  }

  /// The resolution for a page laid out `viewWidth` points wide.
  ///
  /// On-screen widths round upward to multiples of ``deviceWidthStep``, with
  /// a minimum of 64 pixels. Off-screen pages always use
  /// ``offScreenDeviceWidth`` and ignore the supplied dimensions.
  ///
  /// - Parameters:
  ///   - viewWidth: The page's displayed width in layout points.
  ///   - displayScale: Device pixels per layout point, typically SwiftUI's display scale.
  ///   - zoomScale: Additional magnification; values below one are treated as one.
  ///   - isOnScreen: Whether any portion of the page is visible.
  /// - Returns: A resolution suitable for use as a render task's identity.
  public static func forPage(
    viewWidth: Double,
    displayScale: Double,
    zoomScale: Double = 1,
    isOnScreen: Bool
  ) -> Self {
    guard isOnScreen else {
      return Self(deviceWidth: offScreenDeviceWidth, isFull: false)
    }
    let pixels = viewWidth * displayScale * max(zoomScale, 1)
    let width = max(deviceWidthStep / 2, (pixels / deviceWidthStep).rounded(.up) * deviceWidthStep)
    return Self(deviceWidth: width, isFull: true)
  }

  /// `deviceWidth` measured across a page whose displayed width differs from
  /// its unrotated width, for `RenderPageOptions(width:)`, which renders the
  /// unrotated page.
  ///
  /// - Parameters:
  ///   - unrotatedWidth: The page's unrotated width in page units.
  ///   - displayedWidth: The corresponding displayed width in page units,
  ///     including any rotation. The denominator is clamped to at least one.
  /// - Returns: The rounded width to pass to `RenderPageOptions(width:)`.
  public func unrotatedDeviceWidth(unrotatedWidth: Double, displayedWidth: Double) -> Double {
    (deviceWidth * unrotatedWidth / max(displayedWidth, 1)).rounded()
  }
}
