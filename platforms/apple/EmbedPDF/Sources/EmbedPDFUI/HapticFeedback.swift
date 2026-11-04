import EmbedPDF

#if os(iOS)
  import UIKit
#elseif os(macOS)
  import AppKit
#endif

/// Adapts viewer feedback requests to iOS or macOS haptic feedback.
///
/// Safe to call from the viewer's kernel thread: each method schedules a
/// main-actor task and returns before the platform delivers feedback. Actual haptic
/// output depends on the device and its settings.
/// macOS feedback requires a compatible haptic trackpad. Selection uses
/// alignment feedback on macOS; impact and notification use
/// generic feedback. macOS does not distinguish impact weights or outcomes.
public final class HapticFeedback: PlatformFeedback, Sendable {
  /// Creates a stateless platform feedback adapter.
  public init() {}

  /// Schedules a selection-change haptic on the main actor.
  public func selection() {
    #if os(iOS)
      Task { @MainActor in UISelectionFeedbackGenerator().selectionChanged() }
    #elseif os(macOS)
      Task { @MainActor in
        NSHapticFeedbackManager.defaultPerformer.perform(.alignment, performanceTime: .default)
      }
    #endif
  }

  /// Schedules an impact haptic on the main actor.
  ///
  /// - Parameter weight: Requested intensity; `nil` uses medium feedback.
  public func impact(weight: FeedbackImpactWeight?) {
    #if os(iOS)
      Task { @MainActor in
        let style: UIImpactFeedbackGenerator.FeedbackStyle =
          switch weight {
          case .light: .light
          case .heavy: .heavy
          case .medium, nil: .medium
          }
        UIImpactFeedbackGenerator(style: style).impactOccurred()
      }
    #elseif os(macOS)
      Task { @MainActor in
        NSHapticFeedbackManager.defaultPerformer.perform(.generic, performanceTime: .default)
      }
    #endif
  }

  /// Schedules success, warning, or error notification feedback.
  ///
  /// - Parameter kind: Notification category; macOS uses generic feedback for all categories.
  public func notify(kind: FeedbackNotifyKind) {
    #if os(iOS)
      Task { @MainActor in
        let type: UINotificationFeedbackGenerator.FeedbackType =
          switch kind {
          case .success: .success
          case .warning: .warning
          case .error: .error
          }
        UINotificationFeedbackGenerator().notificationOccurred(type)
      }
    #elseif os(macOS)
      Task { @MainActor in
        NSHapticFeedbackManager.defaultPerformer.perform(.generic, performanceTime: .default)
      }
    #endif
  }
}
