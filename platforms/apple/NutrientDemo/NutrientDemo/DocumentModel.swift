import Foundation
import Observation
import PSPDFKit

@MainActor
@Observable
final class DocumentModel {
  struct Opened {
    let document: NutrientDocument
    let pages: [PageInfo]
    let search: SearchModel
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
  var error: DemoError?
  private(set) var isOpening = false
  var isImporting = false
  var passwordPrompt: PasswordPrompt?
  var metadataSheet: MetadataSheet?

  func task() async {
    guard opened == nil else { return }
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
    metadataSheet = MetadataSheet(metadata: opened.document.metadata())
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
    isOpening = true
    defer { isOpening = false }
    opened = nil
    do {
      let document = try await Task.detached { () throws -> NutrientDocument in
        try NutrientDocument(data: data, name: name, password: password)
      }.value
      let pages = await Task.detached { document.pages() }.value
      opened = Opened(document: document, pages: pages, search: SearchModel(document: document))
      error = nil
    } catch let error as DemoError where error.needsPassword {
      passwordPrompt = PasswordPrompt(data: data, name: name)
    } catch let error as DemoError {
      self.error = error
    } catch {
      self.error = DemoError(detail: "\(error)")
    }
  }
}
