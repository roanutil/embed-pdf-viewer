import SwiftUI

extension View {
  /// Tracks scroll visibility for selecting a page's raster resolution.
  ///
  /// On iOS 18 and later, writes `true` when the scroll visibility threshold
  /// of 1% is met. On iOS 17, uses appear/disappear callbacks; lazy stacks may
  /// report built neighboring pages as visible even outside the viewport.
  ///
  /// - Parameter isOnScreen: Destination binding, typically passed to
  ///   ``PageImageView/init(render:page:scale:isOnScreen:)``. The modifier writes
  ///   it as visibility changes; choose the initial value in the host.
  /// - Returns: A view that reports visibility through the supplied binding.
  public func trackingScrollVisibility(_ isOnScreen: Binding<Bool>) -> some View {
    modifier(ScrollVisibilityModifier(isOnScreen: isOnScreen))
  }
}

private struct ScrollVisibilityModifier: ViewModifier {
  @Binding var isOnScreen: Bool

  func body(content: Content) -> some View {
    if #available(iOS 18, *) {
      content.onScrollVisibilityChange(threshold: 0.01) { isOnScreen = $0 }
    } else {
      content
        .onAppear { isOnScreen = true }
        .onDisappear { isOnScreen = false }
    }
  }
}
