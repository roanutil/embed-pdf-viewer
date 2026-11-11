import CustomDump
import EmbedPDF
@testable import EmbedPDFUI
import Testing

@MainActor
@Suite(.serialized, .timeLimit(.minutes(1)))
struct ViewerModelTests {
  func started() async throws -> PDFViewer {
    let viewer = try PDFViewer(ViewerConfig())
    try await viewer.start()
    return viewer
  }

  @Test func aNewModelHoldsNoDocument() async throws {
    let viewer = try await started()
    let model = ViewerModel(viewer: viewer)
    expectNoDifference(model.activeDocument, nil)
    expectNoDifference(model.documentCount, 0)
    expectNoDifference(model.error, nil)
    try await viewer.destroy()
  }

  @Test func opensAndClosesReachTheObservingModel() async throws {
    let viewer = try await started()
    let model = ViewerModel(viewer: viewer)
    let observing = Task { await model.task() }
    defer { observing.cancel() }
    var first: DocumentInfo?
    try await expectObserved({ model.documentCount }, equals: 1) {
      first = try await viewer.open(pluginFixture())
    }
    // The count can reach 1 while the tab still holds its `pending:` ticket;
    // the real id lands with a later revision.
    try await expectObserved({ model.activeDocument?.id }, equals: first?.id) {}
    let second = try await viewer.open(pluginFixture("features"))
    try await expectObserved({ model.activeDocument?.id }, equals: second.id) {}
    expectNoDifference(model.documentCount, 2)
    try await expectObserved({ model.documentCount }, equals: 1) { try await viewer.close(second) }
    let documentCount: Int = try await viewer.documentCount()
    expectNoDifference(model.documentCount, documentCount)
    observing.cancel()
    await observing.value
    try await viewer.destroy()
  }

  @Test func destroyingTheViewerEndsTheTaskAndEmptiesTheModel() async throws {
    let viewer = try await started()
    let model = ViewerModel(viewer: viewer)
    let observing = Task { await model.task() }
    _ = try await viewer.open(pluginFixture())
    try await expectObserved({ model.documentCount }, equals: 1) {}
    try await viewer.destroy()
    await observing.value
    expectNoDifference(model.documentCount, 0)
    expectNoDifference(model.activeDocument, nil)
  }

  @Test func aRefreshNotifiesObserversOfEachProperty() async throws {
    let viewer = try await started()
    let model = ViewerModel(viewer: viewer)
    let document = try await viewer.open(pluginFixture())
    #expect(await observationFires({ model.documentCount }) { await model.refresh() })
    #expect(await observationFires({ model.activeDocument }) {
      _ = try? await viewer.close(document)
      await model.refresh()
    })
    try await viewer.destroy()
  }

  @Test func aCancelledTaskReleasesTheModel() async throws {
    let viewer = try await started()
    weak var released: ViewerModel?
    do {
      let model = ViewerModel(viewer: viewer)
      released = model
      let observing = Task { await model.task() }
      try await expectObserved({ model.documentCount }, equals: 1) {
        _ = try await viewer.open(pluginFixture())
      }
      observing.cancel()
      await observing.value
    }
    #expect(released == nil)
    try await viewer.destroy()
  }

  @Test func anOlderReadCannotOverwriteANewerOne() async throws {
    let viewer = try await started()
    let model = ViewerModel(viewer: viewer)
    let (reached, release) = holdingFirstRead(of: model.snapshots)
    let stale = Task { await model.refresh() }
    var held = reached.makeAsyncIterator()
    _ = await held.next()
    let document = try await viewer.open(pluginFixture())
    await model.refresh()
    expectNoDifference(model.documentCount, 1)
    release()
    await stale.value
    expectNoDifference(model.documentCount, 1)
    expectNoDifference(model.activeDocument?.id, document.id)
    try await viewer.destroy()
  }
}
