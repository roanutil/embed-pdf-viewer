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
  }

  private(set) var opened: Opened?
  var error: PDFError?
  private(set) var isOpening = false
  var isImporting = false
  var passwordPrompt: PasswordPrompt?
  var metadataSheet: MetadataSheet?
  @ObservationIgnored private var viewer: PDFViewer?

  func task() async {
    guard viewer == nil else { return }
    do {
      let viewer = try PDFViewer(ViewerConfig(metadata: true, search: SearchConfig(), render: RenderConfig()))
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
      if let metadata = try await opened.metadata.snapshot() {
        metadataSheet = MetadataSheet(metadata: metadata)
      }
    } catch {
      self.error = error
    }
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
        metadata: try await viewer.metadata(for: document)
      )
      error = nil
    } catch where error.detail.contains("DocPassword") {
      passwordPrompt = PasswordPrompt(data: data, name: name)
    } catch {
      self.error = error
    }
  }
}
