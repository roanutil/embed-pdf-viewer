public import EmbedPDFBindings

public typealias HistoryChangeLabel = EmbedPDFBindings.HistoryChangeLabel
public typealias HistoryLabelValue = EmbedPDFBindings.HistoryLabelValue
public typealias HistorySettings = EmbedPDFBindings.HistorySettings
public typealias HistorySettingsPatch = EmbedPDFBindings.HistorySettingsPatch
public typealias HistoryUndoFailure = EmbedPDFBindings.HistoryUndoFailure
public typealias HistoryEvent = EmbedPDFBindings.HistoryEvent

/// Document history. Undo and redo return when submitted; events report confirmation or refusal.
public struct HistoryCapability: Sendable {
  let handle: HistoryHandle
  let viewer: PDFViewer

  public func canUndo() async throws(PDFError) -> Bool { try await bridged { try await handle.canUndo() } }
  public func canRedo() async throws(PDFError) -> Bool { try await bridged { try await handle.canRedo() } }
  public func undoLabel() async throws(PDFError) -> HistoryChangeLabel? { try await bridged { try await handle.undoLabel() } }
  public func redoLabel() async throws(PDFError) -> HistoryChangeLabel? { try await bridged { try await handle.redoLabel() } }
  public func undo() async throws(PDFError) { try await bridged { try await handle.undo() } }
  public func redo() async throws(PDFError) { try await bridged { try await handle.redo() } }
  public func clear() async throws(PDFError) { try await bridged { try await handle.clear() } }
  public func settings() async throws(PDFError) -> HistorySettings { try await bridged { try await handle.settings() } }
  public func updateSettings(_ patch: HistorySettingsPatch) async throws(PDFError) { try await bridged { try await handle.updateSettings(patch: patch) } }
  public func resetSettings() async throws(PDFError) { try await bridged { try await handle.resetSettings() } }
  public var events: AsyncStream<HistoryEvent> { viewer.registry.stream { handle.subscribe(listener: $0) } }
}

extension EventRelay: HistoryEventListener where Event == HistoryEvent {
  func onEvent(event: HistoryEvent) { continuation.yield(event) }
  func onEnd() { continuation.finish() }
}

extension PDFViewer {
  public func history(for document: DocumentInfo) async throws(PDFError) -> HistoryCapability {
    let handle = try await bridged { try await host.history(documentId: document.id) }
    return HistoryCapability(handle: handle, viewer: self)
  }
}
