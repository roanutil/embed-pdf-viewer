import EmbedPDF
import Foundation
import Testing

/// Stamps each move's arrival on the kernel thread.
final class StampingHandler: InteractionHandler, @unchecked Sendable {
  let lock = NSLock()
  var arrivals: [Int: ContinuousClock.Instant] = [:]

  func enabledFor(tool: InteractionTool) -> Bool { true }
  func onDown(sample: PointerSample) throws -> Bool { true }
  func onMove(sample: PointerSample) {
    let now = ContinuousClock.now
    lock.withLock { arrivals[Int(sample.viewport.x)] = now }
  }
}

extension BaseSuite {
  /// Opt in with EMBEDPDF_LATENCY=1: it takes ten seconds and its numbers
  /// mean something only on an idle machine.
  @Suite(.enabled(if: ProcessInfo.processInfo.environment["EMBEDPDF_LATENCY"] == "1"))
  struct PointerLatencyTests {
    @Test func movesReachTheirHandlerWithinAFrameAt120Hz() async throws {
      let viewer = try PDFViewer(ViewerConfig(interaction: true))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      let page = try await viewer.pages(of: document)[0].page
      let interaction = try await viewer.interaction(for: document)
      let handler = StampingHandler()
      _ = try await interaction.registerHandler(handler, id: "stamp", priority: 1)

      let count = 1200
      let period = Duration.nanoseconds(8_333_333)
      var sent: [Int: ContinuousClock.Instant] = [:]
      interaction.dispatchPointer(sample(.down, page: page, x: -1))
      var next = ContinuousClock.now
      for i in 0..<count {
        sent[i] = ContinuousClock.now
        interaction.dispatchPointer(sample(.move, page: page, x: Double(i)))
        next += period
        try await Task.sleep(until: next, clock: .continuous)
      }
      interaction.dispatchPointer(sample(.up, page: page, x: Double(count)))
      _ = try await interaction.cursor()

      let arrivals = handler.lock.withLock { handler.arrivals }
      let latencies = (0..<count).compactMap { i in arrivals[i].map { $0 - sent[i]! } }.sorted()
      #expect(latencies.count == count)
      let p50 = latencies[count / 2]
      let p95 = latencies[count * 95 / 100]
      let max = latencies[count - 1]
      print("pointer latency p50=\(p50) p95=\(p95) max=\(max)")
      #expect(p95 < period)
    }
  }
}
