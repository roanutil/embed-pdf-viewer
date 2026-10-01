public import EmbedPDFBindings
import Foundation

public final class PDFViewer: Sendable {
  let host: ViewerHost
  public let stateRevisions: AsyncStream<UInt64>

  public init(_ config: ViewerConfig) throws(PDFError) {
    let (stream, continuation) = AsyncStream<UInt64>.makeStream(bufferingPolicy: .bufferingNewest(1))
    do {
      host = try ViewerHost(config: config, delegate: StateRelay(continuation))
    } catch {
      throw PDFError(error)
    }
    stateRevisions = stream
  }

  public func start() async throws(PDFError) {
    try await bridged { try await host.start() }
  }

  public func destroy() async throws(PDFError) {
    try await bridged { try await host.destroy() }
    host.shutdown()
  }

  public func open(_ data: Data, name: String? = nil, password: String? = nil) async throws(PDFError) -> PDFDocument {
    try await bridged {
      try await host.openDocument(bytes: data, options: OpenDocumentOptions(name: name, password: password, activate: true))
    }
  }

  public func unlock(_ document: PDFDocument, password: String) async throws(PDFError) {
    try await bridged { try await host.unlockDocument(documentId: document.id, password: password) }
  }

  public func close(_ document: PDFDocument) async throws(PDFError) {
    try await bridged { try await host.closeDocument(documentId: document.id) }
  }

  public func documents() async throws(PDFError) -> [PDFDocument] {
    try await bridged { try await host.documents() }
  }

  public func pages(of document: PDFDocument) async throws(PDFError) -> [PageInfo] {
    try await bridged { try await host.pages(documentId: document.id) }
  }

  public var documentEvents: AsyncStream<DocumentEvent> {
    EventRelay.stream { host.subscribeDocuments(listener: $0) }
  }

  deinit {
    host.shutdown()
  }
}
