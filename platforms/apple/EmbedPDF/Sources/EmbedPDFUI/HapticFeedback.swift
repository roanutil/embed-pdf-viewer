import EmbedPDF
import UIKit

/// `PlatformFeedback` over UIKit's feedback generators. The viewer calls it
/// on its kernel thread; each call hops to the main actor, where UIKit
/// wants the generators.
public final class HapticFeedback: PlatformFeedback, Sendable {
  public init() {}

  public func selection() {
    Task { @MainActor in UISelectionFeedbackGenerator().selectionChanged() }
  }

  public func impact(weight: FeedbackImpactWeight?) {
    Task { @MainActor in
      let style: UIImpactFeedbackGenerator.FeedbackStyle =
        switch weight {
        case .light: .light
        case .heavy: .heavy
        case .medium, nil: .medium
        }
      UIImpactFeedbackGenerator(style: style).impactOccurred()
    }
  }

  public func notify(kind: FeedbackNotifyKind) {
    Task { @MainActor in
      let type: UINotificationFeedbackGenerator.FeedbackType =
        switch kind {
        case .success: .success
        case .warning: .warning
        case .error: .error
        }
      UINotificationFeedbackGenerator().notificationOccurred(type)
    }
  }
}
