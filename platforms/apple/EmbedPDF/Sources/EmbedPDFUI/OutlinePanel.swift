import EmbedPDF
import Observation
import SwiftUI

@MainActor
public struct OutlinePanel: View {
  private let model: OutlineModel
  private let onActivation: (LinkActivation) -> Void
  @State private var editor: OutlineEditor?
  @State private var moving: OutlineMoveEditor?
  @State private var deleting: OutlineItem?

  public init(model: OutlineModel, onActivation: @escaping (LinkActivation) -> Void = { _ in }) {
    self.model = model
    self.onActivation = onActivation
  }

  public var body: some View {
    VStack(alignment: .leading, spacing: 0) {
      HStack {
        Text("Bookmarks").font(.headline)
        Spacer()
        Button("Reload", systemImage: "arrow.clockwise") { Task { await model.reload() } }
        Button("Add bookmark", systemImage: "plus") { editor = OutlineEditor() }
          .disabled(!model.canEdit)
          .accessibilityIdentifier("outlineAdd")
      }.padding()
      if model.incomplete { Text("Some bookmarks could not be loaded.").padding(.horizontal) }
      if let error = model.error { Text(error.detail).foregroundStyle(.red).padding(.horizontal).accessibilityIdentifier("outlineError") }
      if model.status == .loading { ProgressView("Loading bookmarks…").padding() }
      List(model.rows, id: \.item.itemRef) { row in
        HStack {
          if row.hasChildren {
            Button(row.expanded ? "Collapse" : "Expand", systemImage: row.expanded ? "chevron.down" : "chevron.right") {
              Task { await model.toggle(row.item.itemRef) }
            }.labelStyle(.iconOnly)
              .accessibilityValue(row.expanded ? "Expanded" : "Collapsed")
          }
          Button {
            Task { if let result = await model.activate(row.item.itemRef) { onActivation(result) } }
          } label: {
            HStack {
              Text(row.item.title).bold(row.item.bold).italic(row.item.italic)
              Spacer()
              if model.currentItem == row.item.itemRef { Image(systemName: "location.fill").accessibilityLabel("Current location") }
              if model.selectedItems.contains(row.item.itemRef) { Image(systemName: "checkmark").accessibilityLabel("Selected") }
            }.contentShape(Rectangle())
          }
          .accessibilityHint("Level \(row.depth + 1). Activate bookmark.")
          .contextMenu {
            Button("Select") { Task { await model.select([row.item.itemRef]) } }
            if model.canEdit {
              Button("Edit…") { editor = OutlineEditor(item: row.item) }
              Button("Add child…") { editor = OutlineEditor(parent: row.item.itemRef) }
              Button("Add sibling…") { editor = OutlineEditor(parent: row.item.parent, position: .after(item: row.item.itemRef)) }
              Button("Move…") { moving = OutlineMoveEditor(item: row.item) }
              Button("Delete…", role: .destructive) { deleting = row.item }
            }
          }
        }
        .buttonStyle(.borderless)
        .padding(.leading, CGFloat(row.depth) * 16)
      }
      .overlay {
        if model.rows.isEmpty && model.status == .ready {
          ContentUnavailableView("No bookmarks", systemImage: "bookmark")
        }
      }
    }
    .disabled(model.performing)
    .task(id: ObjectIdentifier(model)) { await model.task() }
    .sheet(item: $editor) { editor in OutlineEditForm(model: model, editor: editor) }
    .sheet(item: $moving) { editor in OutlineMoveForm(model: model, editor: editor) }
    .confirmationDialog("Delete bookmark and its children?", isPresented: $deleting.controlPresented, presenting: deleting) { item in
      Button("Delete", role: .destructive) { Task { await model.delete([item.itemRef]) } }
      Button("Cancel", role: .cancel) {}
    } message: { item in Text(item.title) }
  }
}

@MainActor
@Observable
private final class OutlineEditor: Identifiable {
  let id = UUID()
  let item: OutlineItem?
  let parent: OutlineItemRef?
  let position: OutlinePosition?
  var title: String
  var destination = 0
  var uri = ""
  var defaultColor: Bool
  var color: String
  var bold: Bool
  var italic: Bool
  var open: Bool

  init(item: OutlineItem? = nil, parent: OutlineItemRef? = nil, position: OutlinePosition? = nil) {
    self.item = item
    self.parent = parent
    self.position = position
    title = item?.title ?? ""
    defaultColor = item?.color == nil
    color = item?.color ?? "#000000"
    bold = item?.bold ?? false
    italic = item?.italic ?? false
    open = item?.open ?? true
  }
}

private struct OutlineEditForm: View {
  let model: OutlineModel
  @Bindable var editor: OutlineEditor
  @Environment(\.dismiss) private var dismiss

  var body: some View {
    NavigationStack {
      Form {
        TextField("Title", text: $editor.title).accessibilityIdentifier("outlineTitle")
        Picker("Destination", selection: $editor.destination) {
          Text(editor.item == nil ? "Current view" : "Keep destination").tag(0)
          Text("Web address").tag(1)
          Text("None").tag(2)
        }
        if editor.destination == 1 { TextField("Address", text: $editor.uri) }
        Toggle("Default color", isOn: $editor.defaultColor)
        if !editor.defaultColor { TextField("Color (#RRGGBB)", text: $editor.color) }
        Toggle("Bold", isOn: $editor.bold)
        Toggle("Italic", isOn: $editor.italic)
        if editor.item != nil { Toggle("Open by default", isOn: $editor.open) }
        if let error = model.error { Text(error.detail).foregroundStyle(.red) }
      }
      .navigationTitle(editor.item == nil ? "New bookmark" : "Edit bookmark")
      .toolbar {
        ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
        ToolbarItem(placement: .confirmationAction) {
          Button("Save") { Task { await save() } }
            .disabled(model.performing || !model.canEdit || (editor.destination == 1 && editor.uri.isEmpty))
            .accessibilityIdentifier("outlineSave")
        }
      }
    }
    .frame(minWidth: 280, minHeight: 360)
  }

  private func save() async {
    let target: PageLinkTarget? = editor.destination == 1 ? .uri(uri: editor.uri) : nil
    if let item = editor.item {
      var clear: [OutlinePatchKey] = []
      if editor.destination == 2 { clear.append(.target) }
      if editor.defaultColor { clear.append(.color) }
      if await model.update(item.itemRef, OutlineItemPatch(title: editor.title, target: target,
        color: editor.defaultColor ? nil : editor.color, bold: editor.bold, italic: editor.italic, open: editor.open, clear: clear)) != nil { dismiss() }
    } else {
      if await model.create(OutlineItemInput(title: editor.title.isEmpty ? nil : editor.title,
        parent: editor.parent, position: editor.position, target: target, clearTarget: editor.destination == 2,
        color: editor.defaultColor ? nil : editor.color, bold: editor.bold, italic: editor.italic)) != nil { dismiss() }
    }
  }
}

@MainActor
@Observable
private final class OutlineMoveEditor: Identifiable {
  let id = UUID()
  let item: OutlineItem
  var parent: OutlineItemRef?
  var first = false
  init(item: OutlineItem) { self.item = item; parent = item.parent }
}

private struct OutlineMoveForm: View {
  let model: OutlineModel
  @Bindable var editor: OutlineMoveEditor
  @Environment(\.dismiss) private var dismiss
  var body: some View {
    NavigationStack {
      Form {
        Picker("Parent", selection: $editor.parent) {
          Text("Document root").tag(Optional<OutlineItemRef>.none)
          ForEach(model.items, id: \.itemRef) { item in
            Text(item.title).tag(Optional(item.itemRef))
          }
        }
        Toggle("Place first", isOn: $editor.first)
        if let error = model.error { Text(error.detail).foregroundStyle(.red) }
      }
      .navigationTitle("Move bookmark")
      .toolbar {
        ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
        ToolbarItem(placement: .confirmationAction) {
          Button("Move") {
            Task {
              await model.move([editor.item.itemRef], to: editor.parent, position: editor.first ? .start : .end)
              if model.error == nil { dismiss() }
            }
          }.disabled(model.performing || !model.canEdit)
        }
      }
    }.frame(minWidth: 280, minHeight: 240)
  }
}
