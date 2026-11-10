import EmbedPDF
import EmbedPDFUI
import SwiftUI

struct DocumentView: View {
  @Bindable var model: DocumentModel
  @State private var showingBookmarks = false
  @State private var showingAttachments = false

  var body: some View {
    NavigationStack {
      Group {
        if let opened = model.opened {
          PagesView(opened: opened)
        } else if model.isOpening {
          ProgressView("Opening…")
        } else {
          ContentUnavailableView("No document", systemImage: "doc", description: Text("Open a PDF from Files or a sample."))
        }
      }
      .navigationTitle(model.opened?.document.name ?? "EmbedPDF")
      .navigationBarTitleDisplayMode(.inline)
      .toolbar { toolbar }
    }
    .task { await model.task() }
    .fileImporter(isPresented: $model.isImporting, allowedContentTypes: [.pdf]) { result in
      Task { await model.fileImported(result) }
    }
    .alert("Password required", isPresented: $model.passwordPrompt.isPresented, presenting: model.passwordPrompt) { _ in
      SecureField("Password", text: $model.passwordPrompt.password)
      Button("Open") { Task { await model.passwordSubmitted() } }
      Button("Cancel", role: .cancel) {}
    }
    .alert("Something went wrong", isPresented: $model.error.isPresented, presenting: model.error) { _ in
      Button("OK") { model.errorDismissed() }
    } message: { error in
      Text(error.detail)
    }
    .sheet(isPresented: $showingBookmarks) {
      if let opened = model.opened {
        NavigationStack {
          OutlinePanel(model: opened.outline)
            .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { showingBookmarks = false } } }
        }
      }
    }
    .sheet(isPresented: $showingAttachments) {
      if let opened = model.opened {
        NavigationStack {
          AttachmentPanel(model: opened.attachments)
            .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { showingAttachments = false } } }
        }
      }
    }
    .sheet(item: $model.metadataSheet) { sheet in
      MetadataView(metadata: sheet.metadata, custom: sheet.custom)
    }
  }

  @ToolbarContentBuilder
  private var toolbar: some ToolbarContent {
    ToolbarItem(placement: .topBarLeading) {
      Menu {
        Button("Open from Files…", systemImage: "folder") { model.openButtonTapped() }
        Button("Sample: report.pdf") { Task { await model.sampleButtonTapped("report") } }
        Button("Sample: encrypted-hello.pdf (hôtel)") { Task { await model.sampleButtonTapped("encrypted-hello") } }
      } label: {
        Label("Open", systemImage: "doc.badge.plus")
      }
    }
    ToolbarItem(placement: .topBarTrailing) {
      Button("Add note", systemImage: "note.text.badge.plus") { Task { await model.addNoteButtonTapped() } }
        .labelStyle(.iconOnly)
        .disabled(!model.canAddNote)
        .accessibilityIdentifier("addNote")
    }
    ToolbarItem(placement: .topBarTrailing) {
      Button("Bookmarks", systemImage: "bookmark") { showingBookmarks = true }
        .disabled(model.opened == nil)
        .accessibilityIdentifier("showBookmarks")
    }
    ToolbarItem(placement: .topBarTrailing) {
      Button("Attachments", systemImage: "paperclip") { showingAttachments = true }
        .disabled(model.opened == nil)
        .accessibilityIdentifier("showAttachments")
    }
    ToolbarItem(placement: .topBarTrailing) {
      Button("Info", systemImage: "info.circle") { Task { await model.metadataButtonTapped() } }
        .disabled(model.opened == nil)
    }
  }
}

private struct PagesView: View {
  let opened: DocumentModel.Opened
  var body: some View {
    StageReaderView(model: opened.reader)
      .accessibilityIdentifier("pages")
      .safeAreaInset(edge: .bottom) {
        VStack(spacing: 0) {
          HistoryControls(model: opened.history).padding([.horizontal, .top])
          SearchBar(search: opened.search)
        }.background(.bar)
      }
      .task(id: opened.document.id) { await opened.search.task() }
  }
}

private struct SearchBar: View {
  @Bindable var search: SearchModel

  var body: some View {
    HStack {
      TextField("Search", text: $search.text)
        .textFieldStyle(.roundedBorder)
        .submitLabel(.search)
        .accessibilityIdentifier("searchField")
        .onSubmit { Task { await search.searchSubmitted() } }
      Text(summary)
        .accessibilityIdentifier("searchSummary")
        .font(.caption.monospacedDigit())
        .foregroundStyle(.secondary)
      Button("Previous", systemImage: "chevron.up") { Task { await search.previousButtonTapped() } }
        .labelStyle(.iconOnly)
        .disabled(search.hits.isEmpty)
      Button("Next", systemImage: "chevron.down") { Task { await search.nextButtonTapped() } }
        .labelStyle(.iconOnly)
        .disabled(search.hits.isEmpty)
      Button("Clear", systemImage: "xmark.circle.fill") { Task { await search.clearButtonTapped() } }
        .labelStyle(.iconOnly)
        .disabled(search.text.isEmpty && search.hits.isEmpty)
    }
    .padding()
    .background(.bar)
  }

  private var summary: String {
    switch search.status {
    case .searching: "…"
    case .idle: ""
    default:
      if let index = search.activeHitIndex, !search.hits.isEmpty {
        "\(index + 1)/\(search.hits.count)"
      } else {
        "\(search.hits.count) hits"
      }
    }
  }
}

extension Optional {
  fileprivate var isPresented: Bool {
    get { self != nil }
    set {
      guard !newValue else { return }
      self = nil
    }
  }
}

extension Optional where Wrapped == DocumentModel.PasswordPrompt {
  fileprivate var password: String {
    get { self?.password ?? "" }
    set { self?.password = newValue }
  }
}
