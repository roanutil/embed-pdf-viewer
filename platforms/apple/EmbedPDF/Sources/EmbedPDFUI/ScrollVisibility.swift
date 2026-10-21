import SwiftUI

extension View {
  /// Writes whether any part of this view is on screen in its scroll view,
  /// for `PageImageView(isOnScreen:)` and other renders that should drop to
  /// a placeholder off screen.
  ///
  /// iOS 18 reports scroll visibility directly. iOS 17 falls back to
  /// appear and disappear, which in a lazy stack also counts the neighbors
  /// the stack keeps built.
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
