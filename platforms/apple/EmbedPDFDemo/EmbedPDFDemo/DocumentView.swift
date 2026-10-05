import EmbedPDF
import EmbedPDFUI
import SwiftUI

struct DocumentView: View {
  @Bindable var model: DocumentModel

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
    .sheet(item: $model.metadataSheet) { sheet in
      MetadataView(metadata: sheet.metadata)
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
      Button("Info", systemImage: "info.circle") { Task { await model.metadataButtonTapped() } }
        .disabled(model.opened == nil)
    }
  }
}

private struct PagesView: View {
  let opened: DocumentModel.Opened
  @Environment(\.displayScale) private var displayScale

  var body: some View {
    let search = opened.search
    ScrollViewReader { proxy in
      GeometryReader { geometry in
        ScrollView {
          LazyVStack(spacing: 12) {
            ForEach(opened.pages, id: \.index) { page in
              PageImageView(render: opened.render, page: page.page, scale: scale(for: page, width: geometry.size.width))
                .aspectRatio(page.width / page.height, contentMode: .fit)
                .background(.white)
                .overlay { highlight(on: page) }
                .shadow(radius: 2)
                .id(page.index)
            }
          }
          .padding()
        }
        .background(Color(.secondarySystemBackground))
      }
      .safeAreaInset(edge: .bottom) {
        SearchBar(search: search)
      }
      .onChange(of: search.activeIndex) { _, index in
        scrollToHit(index, proxy: proxy)
      }
    }
    .task(id: opened.document.id) { await search.task() }
  }

  private func scale(for page: PageInfo, width: CGFloat) -> Double {
    let available = max(width - 32, 100)
    return (available / page.width * displayScale).rounded(.up)
  }

  @ViewBuilder
  private func highlight(on page: PageInfo) -> some View {
    if let index = opened.search.activeIndex,
      Int(index) < opened.search.hits.count,
      opened.search.hits[Int(index)].pageIndex == page.index
    {
      RoundedRectangle(cornerRadius: 2).stroke(.yellow, lineWidth: 4)
    }
  }

  private func scrollToHit(_ index: UInt32?, proxy: ScrollViewProxy) {
    guard let index, Int(index) < opened.search.hits.count else { return }
    withAnimation { proxy.scrollTo(opened.search.hits[Int(index)].pageIndex, anchor: .top) }
  }
}

private struct SearchBar: View {
  @Bindable var search: SearchModel

  var body: some View {
    HStack {
      TextField("Search", text: $search.text)
        .textFieldStyle(.roundedBorder)
        .submitLabel(.search)
        .onSubmit { Task { await search.searchSubmitted() } }
      Text(summary)
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
      if let index = search.activeIndex, !search.hits.isEmpty {
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
