public import EmbedPDFBindings
import Foundation

public final class PDFViewer: Sendable {
  let host: ViewerHost
  let registry = StreamRegistry()
  private let stateContinuation: AsyncStream<UInt64>.Continuation
  private let errorContinuation: AsyncStream<PDFError>.Continuation

  /// Errors the kernel reports rather than throws: a listener that failed,
  /// a foreign handler that threw (`HostError`). Keeps the newest 16. The
  /// stream supports a single consumer.
  public let reportedErrors: AsyncStream<PDFError>

  /// Revisions of the viewer's state. The stream supports a single consumer.
  public let stateRevisions: AsyncStream<UInt64>

  public init(_ config: ViewerConfig) throws(PDFError) {
    let (stream, continuation) = AsyncStream<UInt64>.makeStream(bufferingPolicy: .bufferingNewest(1))
    let (errors, errorContinuation) = AsyncStream<PDFError>.makeStream(bufferingPolicy: .bufferingNewest(16))
    do {
      host = try ViewerHost(config: config, delegate: StateRelay(continuation, errors: errorContinuation))
    } catch {
      throw PDFError(error)
    }
    stateRevisions = stream
    stateContinuation = continuation
    reportedErrors = errors
    self.errorContinuation = errorContinuation
  }

  public func start() async throws(PDFError) {
    try await bridged { try await host.start() }
  }

  public func destroy() async throws(PDFError) {
    try await bridged { try await host.destroy() }
    host.shutdown()
    finishStreams()
  }

  public func open(_ data: Data, name: String? = nil, password: String? = nil) async throws(PDFError) -> PDFDocument {
    try await measured("open") { () async throws(PDFError) in
      try await bridged {
        try await host.openDocument(bytes: data, options: OpenDocumentOptions(name: name, password: password, activate: true))
      }
    }
  }

  /// Unlock a document that opened `.locked` (password-protected and opened
  /// without one). Throws `permission-denied` for a wrong password (the
  /// detail begins with the engine's `DocPasswordIncorrect`); the tab stays
  /// locked.
  public func unlock(_ document: PDFDocument, password: String) async throws(PDFError) {
    try await bridged { try await host.unlockDocument(documentId: document.id, password: password) }
  }

  public func close(_ document: PDFDocument) async throws(PDFError) {
    try await bridged { try await host.closeDocument(documentId: document.id) }
  }

  public func documents() async throws(PDFError) -> [PDFDocument] {
    try await bridged { try await host.documents() }
  }

  /// Throws `not-found` for a document the viewer doesn't have.
  public func pages(of document: PDFDocument) async throws(PDFError) -> [PageInfo] {
    try await measured("pages") { () async throws(PDFError) in
      try await bridged { try await host.pages(documentId: document.id) }
    }
  }

  /// The complete document as PDF bytes, for the app to write wherever it
  /// keeps files. `.incremental` (the default) appends the edits after the
  /// original bytes and keeps every signature valid; `.rewrite` writes a
  /// fresh, smaller file without the revision history, and a signed
  /// document refuses it with `permission-denied`. Needs `doc.download` in
  /// `ViewerConfig.scope`. Cancelling the task throws `operation-cancelled`.
  public func save(_ document: PDFDocument, mode: SaveMode = .incremental) async throws(PDFError) -> Data {
    try await measured("save") { () async throws(PDFError) in
      try await bridgedCancellable { token in
        try await host.save(documentId: document.id, mode: mode, cancel: token)
      }
    }
  }

  public var documentEvents: AsyncStream<DocumentEvent> {
    registry.stream { host.subscribeDocuments(listener: $0) }
  }

  func finishStreams() {
    registry.finishAll()
    stateContinuation.finish()
    errorContinuation.finish()
  }

  deinit {
    host.shutdown()
    finishStreams()
  }
}
