public import EmbedPDFBindings

/// The host provider that delivers selection, impact, and notification haptics.
public typealias PlatformFeedback = EmbedPDFBindings.PlatformFeedback
/// Configuration that installs a platform feedback provider.
public typealias FeedbackConfig = EmbedPDFBindings.FeedbackConfig
/// The requested intensity of impact feedback.
public typealias FeedbackImpactWeight = EmbedPDFBindings.FeedbackImpactWeight
/// The outcome conveyed by notification feedback.
public typealias FeedbackNotifyKind = EmbedPDFBindings.FeedbackNotifyKind

/// The workspace's haptics. Get it with `PDFViewer.feedback()`; the viewer
/// needs `ViewerConfig(feedback: FeedbackConfig(provider: ...))`. It keeps
/// the viewer alive, and after `destroy` every call throws
/// `instance-closed`. It has no event stream: what it does reaches the
/// `PlatformFeedback` provider.
public struct FeedbackCapability: Sendable {
  let handle: FeedbackHandle
  let viewer: PDFViewer

  /// Requests selection-change haptics from the configured platform provider.
  public func selection() async throws(PDFError) {
    try await bridged { try await handle.selection() }
  }

  /// Requests impact haptics; nil leaves the weight choice to the provider.
  public func impact(_ weight: FeedbackImpactWeight? = nil) async throws(PDFError) {
    try await bridged { try await handle.impact(weight: weight) }
  }

  /// Requests notification haptics conveying the supplied outcome.
  public func notify(_ kind: FeedbackNotifyKind) async throws(PDFError) {
    try await bridged { try await handle.notify(kind: kind) }
  }
}

extension PDFViewer {
  /// Returns the workspace feedback capability; configure a platform provider before requesting it.
  public func feedback() async throws(PDFError) -> FeedbackCapability {
    FeedbackCapability(handle: try await bridged { try await host.feedback() }, viewer: self)
  }
}
