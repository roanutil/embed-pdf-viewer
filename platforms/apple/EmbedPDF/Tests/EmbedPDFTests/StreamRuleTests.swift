import CustomDump
@testable import EmbedPDF
import Foundation
import Testing

extension BaseSuite {
  @Suite struct StreamRuleTests {
    /// A viewer whose throwing handler reports one `HostError` per pointer down.
    func viewerThatReports() async throws -> (PDFViewer, InteractionCapability, PageRef) {
      let viewer = try PDFViewer(ViewerConfig(interaction: true))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      let page = try await viewer.pages(of: document)[0].page
      let interaction = try await viewer.interaction(for: document)
      _ = try await interaction.registerHandler(RecordingHandler(throwsOnDown: true), id: "throws", priority: 5)
      return (viewer, interaction, page)
    }

    func take(_ count: Int, from stream: AsyncStream<PDFError>) async -> [String] {
      var names: [String] = []
      for await error in stream {
        names.append(error.name)
        if names.count == count { break }
      }
      return names
    }

    @Test func everyReportedErrorObserverReceivesEveryError() async throws {
      let (viewer, interaction, page) = try await viewerThatReports()
      let first = viewer.observeReportedErrors()
      let second = viewer.observeReportedErrors()
      async let firstNames = take(3, from: first)
      async let secondNames = take(3, from: second)
      for _ in 0..<3 { interaction.dispatchPointer(sample(.down, page: page)) }
      let (a, b) = await (firstNames, secondNames)
      expectNoDifference(a, ["HostError", "HostError", "HostError"])
      expectNoDifference(b, a)
      try await viewer.destroy()
    }

    @Test func cancellingOneErrorObserverLeavesTheOthersLiveAndDestroyFinishesThem() async throws {
      let (viewer, interaction, page) = try await viewerThatReports()
      let survivor = viewer.observeReportedErrors()
      let cancelled = Task { for await _ in viewer.observeReportedErrors() {} }
      cancelled.cancel()
      await cancelled.value
      async let names = take(1, from: survivor)
      interaction.dispatchPointer(sample(.down, page: page))
      let received = await names
      expectNoDifference(received, ["HostError"])
      try await viewer.destroy()
      var late = viewer.observeReportedErrors().makeAsyncIterator()
      let end = await late.next()
      expectNoDifference(end, nil)
    }

    @Test func reportedErrorsKeepTheNewestSixteenPerObserver() async {
      let hub = ReportedErrorHub()
      let stream = hub.stream()
      for index in 0..<20 { hub.yield(PDFError(name: "E\(index)", code: .operationFailed, detail: "")) }
      hub.finish()
      var names: [String] = []
      for await error in stream { names.append(error.name) }
      expectNoDifference(names, (4..<20).map { "E\($0)" })
    }
  }

  /// The stored streams are deprecated but keep their single-consumer contract.
  @Suite struct DeprecatedStreamTests {
    @available(*, deprecated)
    @Test func storedStateRevisionsStillDeliver() async throws {
      let (viewer, _, metadata) = try await MetadataTests().opened()
      try await metadata.ensureLoaded()
      var legacy = viewer.stateRevisions.makeAsyncIterator()
      let baseline = await legacy.next()
      _ = try await metadata.update(MetadataPatch(title: "Legacy"))
      let next = await legacy.next()
      #expect(try #require(next) > #require(baseline))
      try await viewer.destroy()
    }

    @available(*, deprecated)
    @Test func storedReportedErrorsStillDeliverAndFinish() async throws {
      let (viewer, interaction, page) = try await StreamRuleTests().viewerThatReports()
      async let names = StreamRuleTests().take(1, from: viewer.reportedErrors)
      interaction.dispatchPointer(sample(.down, page: page))
      let received = await names
      expectNoDifference(received, ["HostError"])
      try await viewer.destroy()
      var rest = 0
      for await _ in viewer.reportedErrors { rest += 1 }
      expectNoDifference(rest, 0)
    }
  }
}
