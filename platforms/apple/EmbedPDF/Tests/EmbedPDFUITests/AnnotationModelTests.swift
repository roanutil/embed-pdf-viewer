import CustomDump
import EmbedPDF
@testable import EmbedPDFUI
import Testing

@MainActor
@Suite(.serialized, .timeLimit(.minutes(1)))
struct AnnotationModelTests {
  func opened(scope: [String]? = nil) async throws -> (PDFViewer, DocumentInfo, AnnotationCapability) {
    let viewer = try PDFViewer(ViewerConfig(scope: scope, interaction: true, annotation: ""))
    try await viewer.start()
    let document = try await viewer.open(pluginFixture())
    return (viewer, document, try await viewer.annotations(for: document))
  }

  @Test func aNewModelAllowsNothing() async throws {
    let (viewer, _, annotations) = try await opened()
    let model = AnnotationModel(annotations: annotations)
    expectNoDifference(model.canRead, false)
    expectNoDifference(model.canCreate, false)
    expectNoDifference(model.error, nil)
    try await viewer.destroy()
  }

  @Test func theObservingModelFollowsTheKernelUntilTheDocumentCloses() async throws {
    let (viewer, document, annotations) = try await opened()
    let model = AnnotationModel(annotations: annotations)
    let observing = Task { await model.task() }
    defer { observing.cancel() }
    try await expectObserved({ model.canCreate }, equals: true) {}
    let canRead = try await annotations.canRead()
    let canCreate = try await annotations.canCreate()
    expectNoDifference(model.canRead, canRead)
    expectNoDifference(model.canCreate, canCreate)
    try await expectObserved({ model.canRead }, equals: false) { try await viewer.close(document) }
    await observing.value
    expectNoDifference(model.canCreate, false)
    expectNoDifference(model.error?.code, .instanceClosed)
    try await viewer.destroy()
  }

  @Test func aRefreshNotifiesObserversOfEachProperty() async throws {
    let (viewer, _, annotations) = try await opened()
    let model = AnnotationModel(annotations: annotations)
    #expect(await observationFires({ model.canRead }) { await model.refresh() })
    let other = AnnotationModel(annotations: annotations)
    #expect(await observationFires({ other.canCreate }) { await other.refresh() })
    try await viewer.destroy()
  }

  @Test func aScopeWithoutAnnotationsReadsFalse() async throws {
    let (viewer, _, annotations) = try await opened(scope: ["doc.open", "doc.render"])
    let model = AnnotationModel(annotations: annotations)
    await model.refresh()
    let canRead = try await annotations.canRead()
    expectNoDifference(model.canRead, canRead)
    expectNoDifference(model.canRead, false)
    expectNoDifference(model.canCreate, false)
    expectNoDifference(model.error, nil)
    try await viewer.destroy()
  }

  @Test func aCancelledTaskReleasesTheModel() async throws {
    let (viewer, _, annotations) = try await opened()
    weak var released: AnnotationModel?
    do {
      let model = AnnotationModel(annotations: annotations)
      released = model
      let observing = Task { await model.task() }
      try await expectObserved({ model.canRead }, equals: true) {}
      observing.cancel()
      await observing.value
    }
    #expect(released == nil)
    try await viewer.destroy()
  }

  @Test func anOlderReadCannotOverwriteANewerOne() async throws {
    let (viewer, document, annotations) = try await opened()
    let model = AnnotationModel(annotations: annotations)
    let (reached, release) = holdingFirstRead(of: model.snapshots)
    let stale = Task { await model.refresh() }
    var held = reached.makeAsyncIterator()
    _ = await held.next()
    try await viewer.close(document)
    await model.refresh()
    expectNoDifference(model.error?.code, .instanceClosed)
    release()
    await stale.value
    expectNoDifference(model.canRead, false)
    expectNoDifference(model.canCreate, false)
    expectNoDifference(model.error?.code, .instanceClosed)
    try await viewer.destroy()
  }
}
