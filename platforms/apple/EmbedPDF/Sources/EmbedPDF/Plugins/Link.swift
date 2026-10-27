public import EmbedPDFBindings

/// A PDF link destination, URI, named action, or other supported target.
public typealias PdfLinkTarget = EmbedPDFBindings.PdfLinkTarget
/// A PDF navigation destination with its page and view parameters.
public typealias PdfDestination = EmbedPDFBindings.PdfDestination
/// A link target in page space: a destination, URI, named action, or other
/// supported target, as the link plugin's `targetJson` decodes.
public typealias PageLinkTarget = EmbedPDFBindings.PageLinkTarget
/// A navigation destination in page space, measured from the top-left of the
/// page's visible box with y pointing down.
public typealias PageDestination = EmbedPDFBindings.PageDestination
/// The durable identity of an annotation within a document.
public typealias AnnotationRef = EmbedPDFBindings.AnnotationRef
/// A clickable page-space region, its target as the engine's JSON, and optional annotation identity.
public typealias LinkItem = EmbedPDFBindings.LinkItem
/// The hover behaviors associated with link interaction.
public typealias LinkHoverFlags = EmbedPDFBindings.LinkHoverFlags
/// A page link or standalone target to resolve, label, or activate.
public typealias LinkInput = EmbedPDFBindings.LinkInput
/// What activating a target would do, without performing the activation.
public typealias LinkResolution = EmbedPDFBindings.LinkResolution
/// The activation outcome; URI outcomes require the application to open the URL.
public typealias LinkActivation = EmbedPDFBindings.LinkActivation
/// An event describing link loading and activation.
public typealias LinkEvent = EmbedPDFBindings.LinkEvent
/// An event describing a link activation after built-in handling.
public typealias LinkActivatedEvent = EmbedPDFBindings.LinkActivatedEvent
/// An event describing a page's links finishing loading.
public typealias LinkLoadedEvent = EmbedPDFBindings.LinkLoadedEvent

/// A document's links: where they are, what they target, and activating
/// them. Get one with `PDFViewer.link(for:)`; the viewer needs
/// `ViewerConfig(interaction: true, link: true)`. Taps reach links once
/// `registerTapHandler()` has run. It keeps the viewer
/// alive. After its document closes, every call throws `instance-closed`.
///
/// The plugin never opens a URL. A `.uri` activation, returned or on
/// `events`, is the app's to open.
///
/// Targets cross as the engine's JSON: `LinkItem.targetJson` is the page-space
/// `PageLinkTarget` record (`{"kind": "uri", "uri": …}`), and a
/// `.destination` resolution or activation carries the `PageDestination`
/// record.
public struct LinkCapability: Sendable {
  let handle: LinkHandle
  let viewer: PDFViewer

  /// Empty until the page loads (`ensureLoaded`).
  public func links(on page: PageRef) async throws(PDFError) -> [LinkItem] {
    try await bridged { try await handle.links(page: page) }
  }

  /// Returns the cached link with the supplied identifier on the page, or nil when unavailable.
  public func link(_ id: String, on page: PageRef) async throws(PDFError) -> LinkItem? {
    try await bridged { try await handle.link(page: page, id: id) }
  }

  /// Returns the smallest link under a point in intrinsic PDF page coordinates, or nil over no link.
  public func link(at point: Point, on page: PageRef) async throws(PDFError) -> LinkItem? {
    try await bridged { try await handle.linkAt(page: page, point: point) }
  }

  /// Returns every link in document page order, loading pages as needed.
  public func allLinks() async throws(PDFError) -> [LinkItem] {
    try await bridged { try await handle.allLinks() }
  }

  /// Loads the page's links when needed so cached lookup and hit-testing can use them.
  public func ensureLoaded(_ page: PageRef) async throws(PDFError) {
    try await bridged { try await handle.ensureLoaded(page: page) }
  }

  /// Returns whether this page's links have been loaded.
  public func isLoaded(_ page: PageRef) async throws(PDFError) -> Bool {
    try await bridged { try await handle.isLoaded(page: page) }
  }

  /// Describes how the target would activate without navigating or opening a URI.
  ///
  /// - Parameter targetJSON: The engine's page-space `PageLinkTarget` JSON, such as a
  ///   ``LinkItem``'s `targetJson`.
  public func resolve(targetJSON: String) async throws(PDFError) -> LinkResolution {
    try await bridged { try await handle.resolve(targetJson: targetJSON) }
  }

  /// Activates the supplied page link and returns the outcome; URI outcomes must be
  /// handled by the application.
  public func activate(_ link: LinkItem) async throws(PDFError) -> LinkActivation {
    try await bridged { try await handle.activate(input: .link(page: link.page, id: link.id)) }
  }

  /// Activates a target that isn't a link on a page and returns the outcome; URI outcomes
  /// must be handled by the application.
  ///
  /// - Parameter targetJSON: The engine's page-space `PageLinkTarget` JSON.
  public func activate(targetJSON: String) async throws(PDFError) -> LinkActivation {
    try await bridged { try await handle.activate(input: .target(targetJson: targetJSON)) }
  }

  /// Activates the link at a point in intrinsic PDF page coordinates, or returns nil over no link.
  public func activate(at point: Point, on page: PageRef) async throws(PDFError) -> LinkActivation? {
    try await bridged { try await handle.activateAt(page: page, point: point) }
  }

  /// Returns a display label for the supplied page link.
  public func label(for link: LinkItem) async throws(PDFError) -> String {
    try await bridged { try await handle.label(input: .link(page: link.page, id: link.id)) }
  }

  /// A label for a target that isn't a link on a page (an annotation's
  /// link, an outline entry): the URL, `Go to destination`, ...
  public func label(forTargetJSON targetJSON: String) async throws(PDFError) -> String {
    try await bridged { try await handle.label(input: .target(targetJson: targetJSON)) }
  }

  /// Returns whether the active interaction tool enables link navigation.
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
  /// Returns this document's link capability; enable interaction and link support in the
  /// viewer configuration.
  public func link(for document: DocumentInfo) async throws(PDFError) -> LinkCapability {
    let handle = try await bridged { try await host.link(documentId: document.id) }
    return LinkCapability(handle: handle, viewer: self)
  }
}
