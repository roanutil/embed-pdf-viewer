public import EmbedPDFBindings

public typealias PlatformFeedback = EmbedPDFBindings.PlatformFeedback
public typealias FeedbackConfig = EmbedPDFBindings.FeedbackConfig
public typealias FeedbackImpactWeight = EmbedPDFBindings.FeedbackImpactWeight
public typealias FeedbackNotifyKind = EmbedPDFBindings.FeedbackNotifyKind

/// The workspace's haptics. Get it with `PDFViewer.feedback()`; the viewer
/// needs `ViewerConfig(feedback: FeedbackConfig(provider: ...))`. It keeps
/// the viewer alive, and after `destroy` every call throws
/// `instance-closed`. It has no event stream: what it does reaches the
/// `PlatformFeedback` provider.
public struct FeedbackCapability: Sendable {
  let handle: FeedbackHandle
  let viewer: PDFViewer

  public func selection() async throws(PDFError) {
    try await bridged { try await handle.selection() }
  }

  public func impact(_ weight: FeedbackImpactWeight? = nil) async throws(PDFError) {
    try await bridged { try await handle.impact(weight: weight) }
  }

  public func notify(_ kind: FeedbackNotifyKind) async throws(PDFError) {
    try await bridged { try await handle.notify(kind: kind) }
  }
}

extension PDFViewer {
  public func feedback() async throws(PDFError) -> FeedbackCapability {
    FeedbackCapability(handle: try await bridged { try await host.feedback() }, viewer: self)
  }
}
