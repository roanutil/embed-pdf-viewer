import SwiftUI

@MainActor
public struct HistoryControls: View {
  let model: HistoryModel
  @Environment(\.dynamicTypeSize) private var dynamicTypeSize

  public init(model: HistoryModel) { self.model = model }

  public var body: some View {
    let buttons = dynamicTypeSize.isAccessibilitySize
      ? AnyLayout(VStackLayout(alignment: .leading, spacing: 8))
      : AnyLayout(HStackLayout())
    VStack(alignment: .leading, spacing: 4) {
      buttons {
        Button(model.undoTitle, systemImage: "arrow.uturn.backward") { Task { await model.undo() } }
          .disabled(!model.canUndo || model.submitting)
          .keyboardShortcut("z", modifiers: .command)
          .accessibilityIdentifier("historyUndo")
        Button(model.redoTitle, systemImage: "arrow.uturn.forward") { Task { await model.redo() } }
          .disabled(!model.canRedo || model.submitting)
          .keyboardShortcut("z", modifiers: [.command, .shift])
          .accessibilityIdentifier("historyRedo")
      }
      if let error = model.error {
        Text(error.detail).font(.caption).foregroundStyle(.red)
          .accessibilityIdentifier("historyError")
      }
      if let notice = model.notice { Text(notice).font(.caption).foregroundStyle(.secondary) }
    }
    .task(id: ObjectIdentifier(model)) { await model.task() }
  }
}
