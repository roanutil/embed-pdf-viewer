import EmbedPDF
import UIKit

/// Adapts viewer feedback requests to UIKit haptic feedback generators.
///
/// Safe to call from the viewer's kernel thread: each method schedules a
/// main-actor task and returns before UIKit delivers feedback. Actual haptic
/// output depends on the device and its settings.
public final class HapticFeedback: PlatformFeedback, Sendable {
  /// Creates a stateless UIKit feedback adapter.
  public init() {}

  /// Schedules a selection-change haptic on the main actor.
  public func selection() {
    Task { @MainActor in UISelectionFeedbackGenerator().selectionChanged() }
  }

  /// Schedules an impact haptic on the main actor.
  ///
  /// - Parameter weight: Requested intensity; `nil` uses medium feedback.
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

  /// Schedules success, warning, or error notification feedback.
  ///
  /// - Parameter kind: Notification category mapped to UIKit on the main actor.
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
