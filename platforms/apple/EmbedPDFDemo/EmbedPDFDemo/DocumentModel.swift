import EmbedPDF
import EmbedPDFUI
import Foundation
import Observation

@MainActor
@Observable
final class DocumentModel {
  struct Opened {
    let document: DocumentInfo
    let pages: [PageInfo]
    let render: RenderCapability
    let search: SearchModel
    let metadata: MetadataCapability
    let annotations: AnnotationCapability
    let history: HistoryModel
    let outline: OutlineModel
    let reader: StageReaderModel
  }

  struct PasswordPrompt: Identifiable {
    let id = UUID()
    let data: Data
    let name: String
    var password = ""
  }

  struct MetadataSheet: Identifiable {
    let id = UUID()
    let metadata: DocumentMetadata
    let custom: CustomMetadata
  }

  private(set) var opened: Opened?
  var error: PDFError?
  private(set) var isOpening = false
  private(set) var canAddNote = false
  var isImporting = false
  var passwordPrompt: PasswordPrompt?
  var metadataSheet: MetadataSheet?
  @ObservationIgnored private var viewer: PDFViewer?

  func task() async {
    guard viewer == nil else { return }
    do {
      let viewer = try PDFViewer(ViewerConfig(metadata: true, search: SearchConfig(), render: RenderConfig(), interaction: true, stage: StageConfig(), annotation: "", history: true, outline: true))
      try await viewer.start()
      self.viewer = viewer
    } catch {
      self.error = error
      return
    }
    await open(bundled: "report")
  }

  func sampleButtonTapped(_ name: String) async {
    await open(bundled: name)
  }

  func openButtonTapped() {
    isImporting = true
  }

  func fileImported(_ result: Result<URL, any Error>) async {
    guard case let .success(url) = result else { return }
    let accessing = url.startAccessingSecurityScopedResource()
    defer { if accessing { url.stopAccessingSecurityScopedResource() } }
    guard let data = try? Data(contentsOf: url) else { return }
    await open(data, name: url.lastPathComponent)
  }

  func passwordSubmitted() async {
    guard let prompt = passwordPrompt else { return }
    passwordPrompt = nil
    await open(prompt.data, name: prompt.name, password: prompt.password)
  }

  func metadataButtonTapped() async {
    guard let opened else { return }
    do {
      try await opened.metadata.refresh()
      try await opened.metadata.refreshCustom()
      if let metadata = try await opened.metadata.snapshot() {
        metadataSheet = MetadataSheet(metadata: metadata, custom: try await opened.metadata.customSnapshot() ?? [:])
      }
    } catch {
      self.error = error
    }
  }

  func addNoteButtonTapped() async {
    guard let opened, let page = opened.pages.first?.page else { return }
    do {
      _ = try await opened.annotations.create(
        #"{"subtype":"text","rect":{"x":72,"y":72,"width":24,"height":24},"contents":"A note"}"#, on: page)
      error = nil
    } catch { self.error = error }
    canAddNote = (try? await opened.annotations.canCreate()) ?? false
  }

  func errorDismissed() {
    error = nil
  }

  private func open(bundled name: String) async {
    guard
      let url = Bundle.main.url(forResource: name, withExtension: "pdf"),
      let data = try? Data(contentsOf: url)
    else { return }
    await open(data, name: "\(name).pdf")
  }

  private func open(_ data: Data, name: String, password: String? = nil) async {
    guard let viewer else { return }
    isOpening = true
    canAddNote = false
    defer { isOpening = false }
    do {
      if let previous = opened?.document {
        opened = nil
        try? await viewer.close(previous)
      }
      let document = try await viewer.open(data, name: name, password: password)
      opened = Opened(
        document: document,
        pages: try await viewer.pages(of: document),
        render: try await viewer.render(in: document),
        search: SearchModel(search: try await viewer.search(in: document)),
        metadata: try await viewer.metadata(for: document),
        annotations: try await viewer.annotations(for: document),
        history: HistoryModel(viewer: viewer, history: try await viewer.history(for: document)),
        outline: OutlineModel(viewer: viewer, outline: try await viewer.outline(for: document)),
        reader: StageReaderModel(stage: try await viewer.stage(for: document), render: try await viewer.render(in: document))
      )
      canAddNote = try await opened?.annotations.canCreate() ?? false
      error = nil
    } catch where error.detail.contains("DocPassword") {
      passwordPrompt = PasswordPrompt(data: data, name: name)
    } catch {
      self.error = error
    }
  }
}
