import CustomDump
import EmbedPDF
import Foundation
import Testing

final class RecordingFeedback: PlatformFeedback, @unchecked Sendable {
  let lock = NSLock()
  var calls: [String] = []

  func selection() { lock.withLock { calls.append("selection") } }
  func impact(weight: FeedbackImpactWeight?) { lock.withLock { calls.append("impact \(String(describing: weight))") } }
  func notify(kind: FeedbackNotifyKind) { lock.withLock { calls.append("notify \(kind)") } }
}

extension BaseSuite {
  @Suite struct FeedbackTests {
    @Test func feedbackReachesTheSwiftProvider() async throws {
      let provider = RecordingFeedback()
      let viewer = try PDFViewer(ViewerConfig(feedback: FeedbackConfig(provider: provider)))
      try await viewer.start()
      let feedback = try await viewer.feedback()
      try await feedback.selection()
      try await feedback.impact(.heavy)
      try await feedback.notify(.success)
      expectNoDifference(
        provider.lock.withLock { provider.calls },
        ["selection", "impact Optional(EmbedPDFBindings.FeedbackImpactWeight.heavy)", "notify success"])
    }

    // Feedback has no event stream of its own (its effects reach the
    // provider), so it has no `droppingTheStreamUnsubscribes`.

    @Test func aDestroyedViewersCapabilityThrows() async throws {
      let viewer = try PDFViewer(ViewerConfig(feedback: FeedbackConfig(provider: RecordingFeedback())))
      try await viewer.start()
      let feedback = try await viewer.feedback()
      try await viewer.destroy()
      await #expect {
        try await feedback.selection()
      } throws: { error in
        (error as? PDFError)?.code == "instance-closed"
      }
    }

    @Test func withoutAProviderFeedbackIsNotInstalled() async throws {
      let viewer = try PDFViewer(ViewerConfig())
      try await viewer.start()
      await #expect(throws: PDFError.self) { _ = try await viewer.feedback() }
    }
  }
}
