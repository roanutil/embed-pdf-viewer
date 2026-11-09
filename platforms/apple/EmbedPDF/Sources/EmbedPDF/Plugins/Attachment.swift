public import EmbedPDFBindings

public typealias AttachmentRef = EmbedPDFBindings.AttachmentRef
public typealias AttachmentHome = EmbedPDFBindings.AttachmentHome
public typealias AttachedFile = EmbedPDFBindings.AttachedFile
public typealias AttachmentFileSource = EmbedPDFBindings.AttachmentFileSource
public typealias AttachmentContent = EmbedPDFBindings.AttachmentContent
public typealias AttachmentEvent = EmbedPDFBindings.AttachmentEvent

/// Embedded files from the document and, when installed, its annotation plugin.
public struct AttachmentCapability: Sendable {
  let handle: AttachmentHandle
  let viewer: PDFViewer

  public func files() async throws(PDFError) -> [AttachedFile] {
    try await bridged { try await handle.files() }
  }

  public func file(_ id: String) async throws(PDFError) -> AttachedFile? {
    try await bridged { try await handle.file(id: id) }
  }

  public func selectedFiles() async throws(PDFError) -> [AttachedFile] {
    try await bridged { try await handle.selectedFiles() }
  }

  public func select(_ files: [AttachedFile]) async throws(PDFError) {
    try await bridged { try await handle.select(files: files) }
  }

  public func isPending(_ file: AttachedFile) async throws(PDFError) -> Bool {
    try await bridged { try await handle.isPending(file: file) }
  }

  public func status() async throws(PDFError) -> ResourceStatus {
    try await bridged { try await handle.status() }
  }

  public func canDownload() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canDownload() }
  }

  public func canAdd() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canAdd() }
  }

  public func canRemove(_ file: AttachedFile) async throws(PDFError) -> Bool {
    try await bridged { try await handle.canRemove(file: file) }
  }

  public func reveal(_ file: AttachedFile) async throws(PDFError) {
    try await bridged { try await handle.reveal(file: file) }
  }

  public func ensureLoaded() async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.ensureLoaded(cancel: token) }
  }

  public func refresh() async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.refresh(cancel: token) }
  }

  public func add(_ sources: [AttachmentFileSource]) async throws(PDFError) -> [AttachedFile] {
    try await bridgedCancellable { token in try await handle.add(sources: sources, cancel: token) }
  }

  public func remove(_ file: AttachedFile) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.remove(file: file, cancel: token) }
  }

  public func download(_ file: AttachedFile) async throws(PDFError) -> AttachmentContent {
    try await bridgedCancellable { token in try await handle.download(file: file, cancel: token) }
  }

  public var events: AsyncStream<AttachmentEvent> { viewer.registry.stream { handle.subscribe(listener: $0) } }
}

extension EventRelay: AttachmentEventListener where Event == AttachmentEvent {
  func onEvent(event: AttachmentEvent) { continuation.yield(event) }
  func onEnd() { continuation.finish() }
}

extension PDFViewer {
  public func attachments(for document: DocumentInfo) async throws(PDFError) -> AttachmentCapability {
    let handle = try await bridged { try await host.attachments(documentId: document.id) }
    return AttachmentCapability(handle: handle, viewer: self)
  }
}
