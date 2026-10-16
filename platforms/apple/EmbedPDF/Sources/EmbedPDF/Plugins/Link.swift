public import EmbedPDFBindings

public typealias PdfLinkTarget = EmbedPDFBindings.PdfLinkTarget
public typealias PdfDestination = EmbedPDFBindings.PdfDestination
public typealias AnnotationRef = EmbedPDFBindings.AnnotationRef
public typealias AnnotationRevisionToken = EmbedPDFBindings.AnnotationRevisionToken
public typealias LinkItem = EmbedPDFBindings.LinkItem
public typealias LinkHoverFlags = EmbedPDFBindings.LinkHoverFlags
public typealias LinkInput = EmbedPDFBindings.LinkInput
public typealias LinkResolution = EmbedPDFBindings.LinkResolution
public typealias LinkActivation = EmbedPDFBindings.LinkActivation
public typealias LinkEvent = EmbedPDFBindings.LinkEvent
public typealias LinkActivatedEvent = EmbedPDFBindings.LinkActivatedEvent
public typealias LinkLoadedEvent = EmbedPDFBindings.LinkLoadedEvent

/// A document's links: where they are, what they target, and activating
/// them. Get one with `PDFViewer.link(for:)`; the viewer needs
/// `ViewerConfig(interaction: true, link: true)`. Taps reach links once
/// `registerTapHandler()` has run. It keeps the viewer
/// alive. After its document closes, every call throws `instance-closed`.
///
/// The plugin never opens a URL. A `.uri` activation, returned or on
/// `events`, is the app's to open.
public struct LinkCapability: Sendable {
  let handle: LinkHandle
  let viewer: PDFViewer

  /// Empty until the page loads (`ensureLoaded`).
  public func links(on page: PageRef) async throws(PDFError) -> [LinkItem] {
    try await bridged { try await handle.links(page: page) }
  }

  public func link(_ id: String, on page: PageRef) async throws(PDFError) -> LinkItem? {
    try await bridged { try await handle.link(page: page, id: id) }
  }

  /// The smallest link under a page point.
  public func link(at point: Point, on page: PageRef) async throws(PDFError) -> LinkItem? {
    try await bridged { try await handle.linkAt(page: page, point: point) }
  }

  public func allLinks() async throws(PDFError) -> [LinkItem] {
    try await bridged { try await handle.allLinks() }
  }

  public func ensureLoaded(_ page: PageRef) async throws(PDFError) {
    try await bridged { try await handle.ensureLoaded(page: page) }
  }

  public func isLoaded(_ page: PageRef) async throws(PDFError) -> Bool {
    try await bridged { try await handle.isLoaded(page: page) }
  }

  public func resolve(_ target: PdfLinkTarget) async throws(PDFError) -> LinkResolution {
    try await bridged { try await handle.resolve(target: target) }
  }

  public func activate(_ link: LinkItem) async throws(PDFError) -> LinkActivation {
    try await bridged { try await handle.activate(input: .link(page: link.page, id: link.id)) }
  }

  public func activate(_ target: PdfLinkTarget) async throws(PDFError) -> LinkActivation {
    try await bridged { try await handle.activate(input: .target(target: target)) }
  }

  /// `nil` when no link is under the point.
  public func activate(at point: Point, on page: PageRef) async throws(PDFError) -> LinkActivation? {
    try await bridged { try await handle.activateAt(page: page, point: point) }
  }

  public func label(for link: LinkItem) async throws(PDFError) -> String {
    try await bridged { try await handle.label(input: .link(page: link.page, id: link.id)) }
  }

  /// A label for a target that isn't a link on a page (an annotation's
  /// link, an outline entry): the URL, `Go to destination`, ...
  public func label(for target: PdfLinkTarget) async throws(PDFError) -> String {
    try await bridged { try await handle.label(input: .target(target: target)) }
  }

  public func isNavigationEngaged() async throws(PDFError) -> Bool {
    try await bridged { try await handle.isNavigationEngaged() }
  }

  /// Let taps on the interaction hub follow links, as the web's link
  /// anchors do: under a `link-nav` tool, a tap that goes down and up on one
  /// link activates it, and the outcome arrives on `events` (a `.uri` is
  /// the app's to open). A reader that forwards its pointer samples with
  /// `InteractionCapability.dispatchPointer` calls this once per document.
  /// A drag that starts on a link still scrolls.
  public func registerTapHandler() async throws(PDFError) -> Registration {
    Registration(handle: try await bridged { try await handle.registerTapHandler() })
  }

  /// Finishes when the document closes (at once if it already has) or the
  /// viewer is destroyed.
  public var events: AsyncStream<LinkEvent> {
    viewer.registry.stream { handle.subscribe(listener: $0) }
  }
}

extension EventRelay: LinkEventListener where Event == LinkEvent {
  func onEvent(event: LinkEvent) {
    continuation.yield(event)
  }

  func onEnd() {
    continuation.finish()
  }
}

extension PDFViewer {
  public func link(for document: DocumentInfo) async throws(PDFError) -> LinkCapability {
    let handle = try await bridged { try await host.link(documentId: document.id) }
    return LinkCapability(handle: handle, viewer: self)
  }
}
