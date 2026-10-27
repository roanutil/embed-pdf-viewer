import EmbedPDF
import EmbedPDFUI
import Testing

@Suite struct HapticFeedbackTests {
  @Test func acceptsEveryFeedbackRequestWithoutRequiringHapticHardware() async throws {
    let viewer = try PDFViewer(ViewerConfig(feedback: FeedbackConfig(provider: HapticFeedback())))
    try await viewer.start()
    let feedback = try await viewer.feedback()
    try await feedback.selection()
    for weight: FeedbackImpactWeight? in [nil, .light, .medium, .heavy] {
      try await feedback.impact(weight)
    }
    for kind: FeedbackNotifyKind in [.success, .warning, .error] {
      try await feedback.notify(kind)
    }
    try await viewer.destroy()
  }
}
