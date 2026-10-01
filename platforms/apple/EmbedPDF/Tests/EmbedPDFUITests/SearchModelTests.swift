import CustomDump
import EmbedPDF
import EmbedPDFUI
import Foundation
import Testing

@MainActor
@Suite(.serialized, .timeLimit(.minutes(1)))
struct SearchModelTests {
  func model() async throws -> (SearchModel, SearchCapability) {
    let url = try #require(
      Bundle.module.url(forResource: "report", withExtension: "pdf", subdirectory: "Fixtures"))
    let viewer = try PDFViewer(ViewerConfig(search: SearchConfig()))
    try await viewer.start()
    let document = try await viewer.open(Data(contentsOf: url))
    let search = try await viewer.search(in: document)
    return (SearchModel(search: search), search)
  }

  @Test func submittingASearchFillsHitsAndNavigationMovesTheActiveHit() async throws {
    let (model, _) = try await model()
    model.text = "the"
    await model.searchSubmitted()
    expectNoDifference(model.status, .complete)
    #expect(model.hits.count > 1)
    expectNoDifference(model.activeIndex, 0)
    await model.nextButtonTapped()
    expectNoDifference(model.activeIndex, 1)
    await model.previousButtonTapped()
    expectNoDifference(model.activeIndex, 0)
  }

  @Test func clearingEmptiesTheModel() async throws {
    let (model, _) = try await model()
    model.text = "the"
    await model.searchSubmitted()
    await model.clearButtonTapped()
    expectNoDifference(model.hits, [])
    expectNoDifference(model.activeIndex, nil)
    expectNoDifference(model.status, .idle)
  }

  @Test func activeHitChangesFromTheCapabilityReachTheModel() async throws {
    let (model, search) = try await model()
    model.text = "the"
    await model.searchSubmitted()
    let task = Task { await model.task() }
    defer { task.cancel() }
    let deadline = ContinuousClock.now + .seconds(10)
    var target: UInt32 = 1
    while model.activeIndex != 2, ContinuousClock.now < deadline {
      target = target == 1 ? 2 : 1
      _ = try await search.goToHit(target)
      try await Task.sleep(for: .milliseconds(50))
    }
    expectNoDifference(model.activeIndex, 2)
  }
}
