import EmbedPDF
import SwiftUI

@MainActor
public struct AttachmentPanel: View {
  private let model: AttachmentModel
  public init(model: AttachmentModel) { self.model = model }

  public var body: some View {
    ModelScope(model: model) { AttachmentPanelContent(model: $0) }
  }
}

@MainActor
private struct AttachmentPanelContent: View {
  let model: AttachmentModel
  @State private var importing = false
  @State private var exporting = false
  @State private var content: AttachmentContent?
  @State private var deleting: AttachedFile?
  @State private var downloadTask: Task<Void, Never>?

  @State private var importTask: Task<Void, Never>?
  @State private var active = true

  var body: some View {
    VStack(alignment: .leading, spacing: 0) {
      HStack {
        Text("Attachments").font(.headline)
        Spacer()
        Button("Reload", systemImage: "arrow.clockwise") { Task { await model.reload() } }
        Button("Import files…", systemImage: "plus") { importing = true }
          .disabled(!model.canAdd)
          .keyboardShortcut("o", modifiers: [.command, .shift])
          .accessibilityIdentifier("attachmentImport")
      }.padding()
      if let error = model.error { Text(error.detail).foregroundStyle(.red).padding(.horizontal) }
      if model.status == .loading { ProgressView("Loading attachments…").padding() }
      List(model.files, id: \.id) { file in
        HStack {
          Button {
            Task { await model.select([file]) }
          } label: {
            VStack(alignment: .leading) {
              Text(file.name)
              if let description = file.description { Text(description).font(.caption) }
              if let size = file.size { Text(ByteCountFormatter.string(fromByteCount: Int64(clamping: size), countStyle: .file)).font(.caption) }
            }
            Spacer()
            if model.selectedFiles.contains(where: { $0.id == file.id }) { Image(systemName: "checkmark").accessibilityLabel("Selected") }
          }
          if model.pendingIDs.contains(file.id) { ProgressView().accessibilityLabel("Working") }
          if case .page = file.home {
            Button("Show on page", systemImage: "doc.text.magnifyingglass") { Task { await model.reveal(file) } }
              .labelStyle(.iconOnly)
          }
        }.buttonStyle(.borderless)
      }
      .overlay {
        if model.files.isEmpty && model.status == .ready { ContentUnavailableView("No attachments", systemImage: "paperclip") }
      }
      HStack {
        Button("Save copy…", systemImage: "square.and.arrow.up") {
          guard let file = model.selectedFiles.first else { return }
          downloadTask?.cancel()
          downloadTask = Task {
            guard let result = await model.download(file), active, !Task.isCancelled else { return }
            content = result
            exporting = true
          }
        }
        .disabled(!model.canDownload || model.selectedFiles.count != 1)
        .accessibilityIdentifier("attachmentExport")
        Button("Remove…", systemImage: "trash", role: .destructive) { deleting = model.selectedFiles.first }
          .disabled(!model.canRemoveSelection || model.selectedFiles.count != 1)
      }.padding()
    }
    .disabled(model.performing)
    .task(id: ObjectIdentifier(model)) { await model.task() }
    .onAppear { active = true }
    .onDisappear {
      active = false
      importTask?.cancel(); importTask = nil
      downloadTask?.cancel(); downloadTask = nil
      content = nil; exporting = false; deleting = nil
    }
    .fileImporter(isPresented: $importing, allowedContentTypes: [.data], allowsMultipleSelection: true) { result in
      guard active else { return }
      switch result {
      case let .success(urls):
        importTask?.cancel()
        importTask = Task { if active, !Task.isCancelled { await model.importFiles(urls) } }
      case let .failure(error): model.fileOperationFailed(error)
      }
    }
    .fileExporter(isPresented: $exporting, document: content.map { AttachmentExportDocument(data: $0.data) },
      contentType: AttachmentExportDocument.contentType(for: content?.mimeType), defaultFilename: content?.name ?? "Attachment") { result in
        guard active else { return }
        if case let .failure(error) = result { model.fileOperationFailed(error) }
        content = nil
      }
    .confirmationDialog("Remove attachment?", isPresented: $deleting.controlPresented, presenting: deleting) { file in
      Button("Remove", role: .destructive) { Task { if active { await model.remove(file) } } }
      Button("Cancel", role: .cancel) {}
    } message: { file in Text(file.name) }
  }
}
