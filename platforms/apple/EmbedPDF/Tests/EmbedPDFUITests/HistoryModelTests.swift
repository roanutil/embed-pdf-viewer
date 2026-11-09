import CustomDump
import EmbedPDF
@testable import EmbedPDFUI
import Testing

@MainActor
@Suite(.serialized, .timeLimit(.minutes(1)))
struct HistoryModelTests {
  func opened() async throws -> (PDFViewer, DocumentInfo, HistoryCapability, HistoryModel) {
    let viewer = try PDFViewer(ViewerConfig(history: true, pageEdit: true))
    try await viewer.start()
    let document = try await viewer.open(pluginFixture())
    let history = try await viewer.history(for: document)
    return (viewer, document, history, HistoryModel(viewer: viewer, history: history))
  }

  @Test func emptyHistoryNeverLeavesTheControlsSubmitting() async throws {
    let (viewer, _, _, model) = try await opened()
    await model.refresh()
    expectNoDifference(model.canUndo, false)
    expectNoDifference(model.canRedo, false)
    await model.undo()
    await model.redo()
    expectNoDifference(model.submitting, false)
    expectNoDifference(model.error, nil)
    _ = HistoryControls(model: model)
    try await viewer.destroy()
  }

  @Test func externalEditsAndUndoReachTheObservingModel() async throws {
    let (viewer, document, _, model) = try await opened()
    let observing = Task { await model.task() }
    defer { observing.cancel() }
    let edit = try await viewer.pageEdit(for: document)
    let page = try await viewer.pages(of: document)[0].page
    try await expectObserved({ model.canUndo }, equals: true) {
      _ = try await edit.rotate([page], by: .clockwise)
    }
    #expect(model.undoLabel != nil)
    try await expectObserved({ model.canRedo }, equals: true) { await model.undo() }
    expectNoDifference(model.submitting, false)
    observing.cancel()
    await observing.value
    try await viewer.destroy()
  }

  @Test func closingADocumentStopsObservationAndDoesNotAffectItsReplacement() async throws {
    let (viewer, document, _, model) = try await opened()
    let observing = Task { await model.task() }
    try await viewer.close(document)
    await observing.value
    expectNoDifference(model.canUndo, false)
    expectNoDifference(model.error?.code, "instance-closed")
    let replacement = try await viewer.open(pluginFixture())
    let next = HistoryModel(viewer: viewer, history: try await viewer.history(for: replacement))
    await next.refresh()
    expectNoDifference(next.error, nil)
    expectNoDifference(next.canUndo, false)
    try await viewer.destroy()
  }

  @Test func refusalAndSkippedPartsArePresentedWithoutWaitingForAnotherEvent() async throws {
    let (viewer, _, _, model) = try await opened()
    let label = HistoryChangeLabel(key: "test.edit", values: [:])
    await model.receive(.undoFailed(label: label, redo: false, reason: .refused,
      error: PluginErrorInfo(code: "permission-denied", message: "Editing is not allowed", capability: "history", permission: nil)))
    expectNoDifference(model.error?.code, "permission-denied")
    await model.receive(.undone(label: label, redo: false, skipped: 2))
    #expect(model.notice != nil)
    expectNoDifference(model.error, nil)
    try await viewer.destroy()
  }
}
