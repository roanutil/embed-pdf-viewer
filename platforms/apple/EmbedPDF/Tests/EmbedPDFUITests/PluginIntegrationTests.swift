import CustomDump
import EmbedPDF
import EmbedPDFUI
import Foundation
import Testing

private final class IntegrationFeedback: PlatformFeedback, Sendable {
  func selection() {}
  func impact(weight: FeedbackImpactWeight?) {}
  func notify(kind: FeedbackNotifyKind) {}
}

@MainActor
@Suite(.serialized, .timeLimit(.minutes(1)))
struct PluginIntegrationTests {
  func config(scope: [String]? = nil) -> ViewerConfig {
    ViewerConfig(scope: scope, metadata: true, search: SearchConfig(), render: RenderConfig(),
      interaction: true, feedback: FeedbackConfig(provider: IntegrationFeedback()), stage: StageConfig(),
      selection: true, link: true, annotation: "", history: true, outline: true, attachment: true,
      form: FormConfig(), redaction: RedactionConfig(), measurement: MeasurementConfig(), stamp: StampConfig(),
      pageEdit: true, viewManager: true, shell: true,
      i18n: I18nConfig(locale: "en", locales: [LocalePack(code: "en", name: "English", dir: nil, translations: [:])]),
      commands: true, actions: ActionsConfig(openSequence: .off), signature: SignatureConfig())
  }

  @Test func allPluginsAndThreeModelsEditObserveSaveAndCloseTogether() async throws {
    let viewer = try PDFViewer(config())
    try await viewer.start()
    let document = try await viewer.open(pluginFixture())
    let history = try await viewer.history(for: document)
    let outline = try await viewer.outline(for: document)
    let attachments = try await viewer.attachments(for: document)
    let annotations = try await viewer.annotations(for: document)
    let historyModel = HistoryModel(viewer: viewer, history: history)
    let outlineModel = OutlineModel(viewer: viewer, outline: outline)
    let attachmentModel = AttachmentModel(viewer: viewer, attachments: attachments)
    let historyTask = Task { await historyModel.task() }
    let outlineTask = Task { await outlineModel.task() }
    let attachmentTask = Task { await attachmentModel.task() }
    defer { historyTask.cancel(); outlineTask.cancel(); attachmentTask.cancel() }
    try await expectObserved({ outlineModel.status }, equals: .ready) {}
    try await expectObserved({ attachmentModel.status }, equals: .ready) {}
    try await expectObserved({ historyModel.canUndo }, equals: true) {
      try await expectObserved({ outlineModel.rows.contains { $0.item.title == "Saved bookmark" } }, equals: true) {
        _ = try await outline.create(OutlineItemInput(title: "Saved bookmark", clearTarget: true))
      }
    }
    let bytes = Data([0, 255, 42])
    try await expectObserved({ attachmentModel.files.count }, equals: 1) {
      _ = try await attachments.add([AttachmentFileSource(data: bytes, name: "Saved.bin")])
    }
    let page = try await viewer.pages(of: document)[0].page
    let note = try await annotations.create(
      NoteAnnotationDraft(rect: Rect(x: 40, y: 40, width: 24, height: 24), contents: "Saved note"), on: page)
    let undoEvents = history.events
    await historyModel.undo()
    for await event in undoEvents { if case .undone = event { break } }
    let undone = try await annotations.get(note)
    #expect(undone == nil)
    let redoEvents = history.events
    await historyModel.redo()
    for await event in redoEvents { if case .undone = event { break } }
    let redone = try await annotations.get(note)
    expectNoDifference(redone?.contents, "Saved note")
    let saved = try await viewer.save(document)
    let reopened = try await viewer.open(saved)
    let savedOutline = try await viewer.outline(for: reopened)
    let savedFiles = try await viewer.attachments(for: reopened)
    try await savedOutline.ensureLoaded()
    try await savedFiles.ensureLoaded()
    let bookmark = try await savedOutline.list().first { $0.title == "Saved bookmark" }
    #expect(bookmark != nil)
    let file = try #require(try await savedFiles.files().first { $0.name == "Saved.bin" })
    let content = try await savedFiles.download(file)
    expectNoDifference(content.data, bytes)
    try await viewer.close(document)
    await historyTask.value; await outlineTask.value; await attachmentTask.value
    expectNoDifference(historyModel.canUndo, false)
    expectNoDifference(outlineModel.canEdit, false)
    expectNoDifference(attachmentModel.canAdd, false)
    try await viewer.destroy()
  }

  @Test func lockedAndAbsentCapabilitiesRetainTypedErrors() async throws {
    let viewer = try PDFViewer(config())
    try await viewer.start()
    let locked = try await viewer.open(pluginFixture("encrypted-hello"))
    expectNoDifference(locked.status, .locked)
    await #expect { _ = try await viewer.history(for: locked) } throws: { ($0 as? PDFError)?.code == .notReady }
    await #expect { _ = try await viewer.outline(for: locked) } throws: { ($0 as? PDFError)?.code == .notReady }
    await #expect { _ = try await viewer.attachments(for: locked) } throws: { ($0 as? PDFError)?.code == .notReady }
    try await viewer.destroy()
    let bare = try PDFViewer(ViewerConfig())
    try await bare.start()
    let document = try await bare.open(pluginFixture())
    await #expect { _ = try await bare.history(for: document) } throws: { ($0 as? PDFError)?.code == .unsupported }
    await #expect { _ = try await bare.outline(for: document) } throws: { ($0 as? PDFError)?.code == .unsupported }
    await #expect { _ = try await bare.attachments(for: document) } throws: { ($0 as? PDFError)?.code == .unsupported }
    try await bare.destroy()
  }

  @Test func fullConfigStillEnforcesScopeAndCancellation() async throws {
    let viewer = try PDFViewer(config(scope: ["doc.open"]))
    try await viewer.start()
    let document = try await viewer.open(pluginFixture())
    let outline = try await viewer.outline(for: document)
    let attachments = try await viewer.attachments(for: document)
    #expect(try await !outline.canEdit())
    #expect(try await !attachments.canAdd())
    await #expect { _ = try await outline.create(OutlineItemInput(title: "Denied")) } throws: { ($0 as? PDFError)?.code == .permissionDenied }
    try await viewer.destroy()
    let allowed = try PDFViewer(config())
    try await allowed.start()
    let next = try await allowed.open(pluginFixture())
    let files = try await allowed.attachments(for: next)
    let cancelled = Task {
      withUnsafeCurrentTask { $0?.cancel() }
      await #expect { _ = try await files.add([AttachmentFileSource(data: Data(), name: "Cancelled")]) } throws: { ($0 as? PDFError)?.code == .operationCancelled }
    }
    await cancelled.value
    try await files.ensureLoaded()
    let empty = try await files.files()
    expectNoDifference(empty, [])
    try await allowed.destroy()
  }
}
