import CustomDump
import EmbedPDF
@testable import EmbedPDFUI
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

  func opened() async throws -> (PDFViewer, DocumentInfo, SearchCapability, SearchModel) {
    let viewer = try PDFViewer(ViewerConfig(search: SearchConfig(), pageEdit: true))
    try await viewer.start()
    let document = try await viewer.open(pluginFixture())
    let search = try await viewer.search(in: document)
    return (viewer, document, search, SearchModel(search: search))
  }

  @Test func resubmittingAfterAnEditRereadsHitsWhoseCountDidNotChange() async throws {
    let (viewer, document, search, model) = try await opened()
    model.text = "the"
    await model.searchSubmitted()
    let before = model.hits
    let page = try #require(before.first).page
    _ = try await viewer.pageEdit(for: document).movePages([page], to: .end)
    await model.searchSubmitted()
    let kernel = try await search.hits()
    // The rerun finds as many hits, at other positions.
    expectNoDifference(kernel.count, before.count)
    #expect(kernel != before)
    expectNoDifference(model.hits, kernel)
    try await viewer.destroy()
  }

  @Test func aRefreshThatOvertakesTheSubmittedOneStillRereadsHits() async throws {
    let (viewer, document, search, model) = try await opened()
    model.text = "the"
    await model.searchSubmitted()
    let before = model.hits
    let page = try #require(before.first).page
    _ = try await viewer.pageEdit(for: document).movePages([page], to: .end)
    // A revision lands while the submitted refresh is reading: the newer
    // refresh supersedes it and must still reread the hit list.
    var overtaken = false
    model.snapshots.readFinished = {
      guard !overtaken else { return }
      overtaken = true
      await model.refresh()
    }
    await model.searchSubmitted()
    model.snapshots.readFinished = nil
    #expect(overtaken)
    let kernel = try await search.hits()
    expectNoDifference(kernel.count, before.count)
    #expect(kernel != before)
    expectNoDifference(model.hits, kernel)
    try await viewer.destroy()
  }

  @Test func closingTheDocumentEndsTheTaskAndEmptiesTheModel() async throws {
    let (viewer, document, _, model) = try await opened()
    let observing = Task { await model.task() }
    model.text = "the"
    await model.searchSubmitted()
    #expect(model.hitCount > 1)
    try await viewer.close(document)
    await observing.value
    expectNoDifference(model.hits, [])
    expectNoDifference(model.hitCount, 0)
    expectNoDifference(model.activeHitIndex, nil)
    expectNoDifference(model.status, .idle)
    expectNoDifference(model.progress, SearchProgress(pagesSearched: 0, pageCount: 0))
    expectNoDifference(model.error?.code, .instanceClosed)
    try await viewer.destroy()
  }

  @Test func aRefreshNotifiesObserversOfEachProperty() async throws {
    let (viewer, _, search, model) = try await opened()
    _ = try await search.search(SearchQuery(text: "the"))
    _ = try await search.nextHit()
    let page = try #require(try await search.hits().first).page
    let changes = [
      armObservation { model.status }, armObservation { model.activeHitIndex },
      armObservation { model.hitCount }, armObservation { model.progress },
      armObservation { model.hitCount(on: page) },
    ]
    await model.refresh()
    expectNoDifference(changes.map(\.value), [true, true, true, true, true])
    try await viewer.destroy()
  }

  @Test func aBackgroundRefreshKeepsAnActionsError() async throws {
    // Without doc.text.search the search itself is refused, while the
    // session reads still answer.
    let viewer = try PDFViewer(ViewerConfig(scope: ["doc.open", "doc.render"], search: SearchConfig()))
    try await viewer.start()
    let model = SearchModel(search: try await viewer.search(in: try await viewer.open(pluginFixture())))
    model.text = "the"
    await model.searchSubmitted()
    let refused = try #require(model.error)
    await model.refresh()
    expectNoDifference(model.error, refused)
    expectNoDifference(model.status, .error)
    try await viewer.destroy()
  }

  @Test func clearingAfterARefusedSearchLeavesTheErrorStatus() async throws {
    let viewer = try PDFViewer(ViewerConfig(scope: ["doc.open", "doc.render"], search: SearchConfig()))
    try await viewer.start()
    let model = SearchModel(search: try await viewer.search(in: try await viewer.open(pluginFixture())))
    model.text = "the"
    await model.searchSubmitted()
    expectNoDifference(model.status, .error)
    await model.clearButtonTapped()
    expectNoDifference(model.status, .idle)
    try await viewer.destroy()
  }

  @Test func thePluginsOwnRerunAfterAnEditReachesTheObservingModel() async throws {
    let (viewer, document, search, model) = try await opened()
    model.text = "the"
    await model.searchSubmitted()
    let before = model.hits
    let page = try #require(before.first).page
    let deadline = ContinuousClock.now + .seconds(10)
    func rerunFinished() async -> Bool {
      guard (try? await search.status()) == .complete, let hits = try? await search.hits() else { return false }
      return hits != before
    }
    // Hold every read until the rerun has finished, so the model never applies
    // the searching state in between: the count, query and status it then reads
    // are the ones it already holds, and only the search events say the hits
    // moved.
    model.snapshots.readFinished = {
      while ContinuousClock.now < deadline, !(await rerunFinished()) {
        try? await Task.sleep(for: .milliseconds(20))
      }
    }
    let task = Task { await model.task() }
    defer { task.cancel() }
    // Nobody resubmits: the plugin reruns the query 250 ms after the move.
    _ = try await viewer.pageEdit(for: document).movePages([page], to: .end)
    let kernel = try await {
      while ContinuousClock.now < deadline, !(await rerunFinished()) {
        try await Task.sleep(for: .milliseconds(20))
      }
      return try await search.hits()
    }()
    while ContinuousClock.now < deadline, model.hits != kernel {
      try await Task.sleep(for: .milliseconds(20))
    }
    model.snapshots.readFinished = nil
    // The rerun finds as many hits, at other positions.
    expectNoDifference(kernel.count, before.count)
    #expect(kernel != before)
    expectNoDifference(model.hits, kernel)
    try await viewer.destroy()
  }

  @Test func submittingASearchFillsHitsAndNavigationMovesTheActiveHit() async throws {
    let (model, _) = try await model()
    model.text = "the"
    await model.searchSubmitted()
    expectNoDifference(model.status, .complete)
    #expect(model.hits.count > 1)
    expectNoDifference(model.activeHitIndex, 0)
    await model.nextButtonTapped()
    expectNoDifference(model.activeHitIndex, 1)
    await model.previousButtonTapped()
    expectNoDifference(model.activeHitIndex, 0)
  }

  /// The `UInt32` spelling reads the same hit as ``SearchModel/activeHitIndex``.
  @available(*, deprecated)
  @Test func theUInt32ActiveIndexFollowsActiveHitIndex() async throws {
    let (model, _) = try await model()
    expectNoDifference(model.activeIndex, nil)
    model.text = "the"
    await model.searchSubmitted()
    await model.nextButtonTapped()
    expectNoDifference(model.activeHitIndex, 1)
    expectNoDifference(model.activeIndex, 1)
  }

  @Test func clearingEmptiesTheModel() async throws {
    let (model, _) = try await model()
    model.text = "the"
    await model.searchSubmitted()
    await model.clearButtonTapped()
    expectNoDifference(model.hits, [])
    expectNoDifference(model.activeHitIndex, nil)
    expectNoDifference(model.status, .idle)
  }

  @Test func activeHitChangesFromTheCapabilityReachTheModel() async throws {
    let (model, search) = try await model()
    model.text = "the"
    await model.searchSubmitted()
    let task = Task { await model.task() }
    defer { task.cancel() }
    _ = try await search.goToHit(2)
    let deadline = ContinuousClock.now + .seconds(10)
    while model.activeHitIndex != 2, ContinuousClock.now < deadline {
      try await Task.sleep(for: .milliseconds(20))
    }
    expectNoDifference(model.activeHitIndex, 2)
  }

  @Test func aNewModelHasNoSearch() async throws {
    let (model, _) = try await model()
    expectNoDifference(model.status, .idle)
    expectNoDifference(model.activeHitIndex, nil)
    expectNoDifference(model.hitCount, 0)
    expectNoDifference(model.progress, SearchProgress(pagesSearched: 0, pageCount: 0))
    expectNoDifference(model.hits, [])
    expectNoDifference(model.error, nil)
  }

  @Test func aSearchRunElsewhereReachesTheObservingModel() async throws {
    let (model, search) = try await model()
    let observing = Task { await model.task() }
    defer { observing.cancel() }
    _ = try await search.search(SearchQuery(text: "the"))
    let hitCount: Int = try await search.hitCount()
    #expect(hitCount > 1)
    try await expectObserved({ model.hitCount }, equals: hitCount) {}
    try await expectObserved({ model.status }, equals: try await search.status()) {}
    let progress = try await search.progress()
    let activeHitIndex: Int? = try await search.activeHitIndex()
    expectNoDifference(model.progress, progress)
    expectNoDifference(model.activeHitIndex, activeHitIndex)
    expectNoDifference(model.hits.count, hitCount)
    let page = try #require(model.hits.first).page
    let onPage: Int = try await search.hitCount(on: page)
    expectNoDifference(model.hitCount(on: page), onPage)
    try await expectObserved({ model.hitCount }, equals: 0) { try await search.clear() }
    expectNoDifference(model.hitCount(on: page), 0)
    expectNoDifference(model.hits, [])
  }

  @Test func aCancelledTaskReleasesTheModel() async throws {
    weak var released: SearchModel?
    do {
      let (model, search) = try await model()
      released = model
      let observing = Task { await model.task() }
      _ = try await search.search(SearchQuery(text: "the"))
      let hitCount: Int = try await search.hitCount()
      try await expectObserved({ model.hitCount }, equals: hitCount) {}
      observing.cancel()
      await observing.value
    }
    #expect(released == nil)
  }

  @Test func anOlderReadCannotOverwriteANewerOne() async throws {
    let (model, _) = try await model()
    let (reached, release) = holdingFirstRead(of: model.snapshots)
    let stale = Task { await model.refresh() }
    var held = reached.makeAsyncIterator()
    _ = await held.next()
    model.text = "the"
    await model.searchSubmitted()
    let hitCount = model.hitCount
    #expect(hitCount > 1)
    release()
    await stale.value
    expectNoDifference(model.hitCount, hitCount)
    expectNoDifference(model.status, .complete)
    expectNoDifference(model.activeHitIndex, 0)
  }
}
