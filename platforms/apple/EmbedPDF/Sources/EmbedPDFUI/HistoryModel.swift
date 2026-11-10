import EmbedPDF
import Foundation
import Observation

@MainActor
@Observable
public final class HistoryModel {
  public private(set) var canUndo = false
  public private(set) var canRedo = false
  public private(set) var undoLabel: HistoryChangeLabel?
  public private(set) var redoLabel: HistoryChangeLabel?
  public private(set) var undoTitle = String(localized: "Undo")
  public private(set) var redoTitle = String(localized: "Redo")
  public private(set) var submitting = false
  public private(set) var notice: String?
  public private(set) var error: PDFError?
  @ObservationIgnored private let viewer: PDFViewer
  @ObservationIgnored private let history: HistoryCapability
  @ObservationIgnored private let translator: I18nCapability?
  @ObservationIgnored private let snapshots = SnapshotRefresh()
  @ObservationIgnored private var observation: UInt64 = 0

  public init(viewer: PDFViewer, history: HistoryCapability, translator: I18nCapability? = nil) {
    self.viewer = viewer
    self.history = history
    self.translator = translator
  }

  public func task() async {
    observation &+= 1
    let session = observation
    defer { if observation == session { snapshots.invalidate() } }
    await observeCapability(
      viewer: viewer, events: history.events,
      initial: { if self.observation == session { await self.refresh() } },
      refresh: { if self.observation == session { await self.refresh() } },
      receive: { if self.observation == session { await self.receive($0) } })
    if observation == session { await refresh() }
  }

  public func refresh() async {
    await snapshots.run(load: {
      let canUndo = try await self.history.canUndo()
      let canRedo = try await self.history.canRedo()
      let undo = try await self.history.undoLabel()
      let redo = try await self.history.redoLabel()
      let undoTitle = try await self.title(for: undo, redo: false)
      let redoTitle = try await self.title(for: redo, redo: true)
      return (canUndo, canRedo, undo, redo, undoTitle, redoTitle)
    }, apply: { snapshot in
      self.canUndo = snapshot.0
      self.canRedo = snapshot.1
      self.undoLabel = snapshot.2
      self.redoLabel = snapshot.3
      self.undoTitle = snapshot.4
      self.redoTitle = snapshot.5
    }, failure: {
      self.canUndo = false
      self.canRedo = false
      self.error = controlError($0)
    })
  }

  public func undo() async { await submit { try await self.history.undo() } }
  public func redo() async { await submit { try await self.history.redo() } }

  func receive(_ event: HistoryEvent) async {
    switch event {
    case let .undoFailed(_, _, _, failure):
      error = PDFError(name: "PluginError", code: PDFError.Code(rawValue: failure.code), detail: failure.message)
      notice = nil
    case let .undone(_, redo, skipped):
      error = nil
      notice = skipped == 0 ? nil : redo
        ? String(localized: "Some changes could not be redone.")
        : String(localized: "Some changes could not be undone.")
    case .settingsChanged: break
    }
    await refresh()
  }

  private func submit(_ action: () async throws -> Void) async {
    guard !submitting else { return }
    submitting = true
    error = nil
    notice = nil
    defer { submitting = false }
    do { try await action() } catch { self.error = controlError(error) }
    await refresh()
  }

  private func title(for label: HistoryChangeLabel?, redo: Bool) async throws -> String {
    let verb = redo ? String(localized: "Redo") : String(localized: "Undo")
    guard let label, let translator else { return verb }
    let params = label.values.mapValues { value -> I18nParam in
      switch value {
      case let .text(value): .text(value: value)
      case let .number(value): .number(value: value)
      }
    }
    let description = try await translator.t(label.key, params: params, fallback: String(localized: "change"))
    return "\(verb) \(description)"
  }
}
