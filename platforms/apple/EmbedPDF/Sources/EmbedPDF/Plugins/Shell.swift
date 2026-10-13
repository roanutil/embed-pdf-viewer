public import EmbedPDFBindings
import Foundation

public typealias ShellSurface = EmbedPDFBindings.ShellSurface
public typealias ShellOpenOptions = EmbedPDFBindings.ShellOpenOptions
public typealias ShellSnapshot = EmbedPDFBindings.ShellSnapshot
public typealias ShellSurfaceEvent = EmbedPDFBindings.ShellSurfaceEvent
public typealias ShellMenuEvent = EmbedPDFBindings.ShellMenuEvent
public typealias ShellEvent = EmbedPDFBindings.ShellEvent

/// Which named surfaces (panels, sheets, inspectors) and menus are open in
/// one document. Pure state: your views decide what a surface looks like
/// and bind to this. Get one with `PDFViewer.shell(for:)`; the viewer needs
/// `ViewerConfig(shell: true)`. A surface's props are a JSON object, given
/// as text or as any `Encodable` that encodes to an object. After its
/// document closes, every call throws `instance-closed`.
public struct ShellCapability: Sendable {
  let handle: ShellHandle
  let viewer: PDFViewer

  public func isOpen(_ id: String) async throws(PDFError) -> Bool {
    try await bridged { try await handle.isOpen(id: id) }
  }

  /// `nil` for a surface never opened.
  public func surface(_ id: String) async throws(PDFError) -> ShellSurface? {
    try await bridged { try await handle.surface(id: id) }
  }

  public func openSurfaces() async throws(PDFError) -> [ShellSurface] {
    try await bridged { try await handle.openSurfaces() }
  }

  /// A tagged open closes every other open surface with the same tag.
  public func open(_ id: String, exclusive: String? = nil, propsJSON: String? = nil) async throws(PDFError) {
    let options = ShellOpenOptions(exclusive: exclusive, propsJson: propsJSON)
    try await bridged { try await handle.openSurface(id: id, options: options) }
  }

  public func open<Props: Encodable & Sendable>(_ id: String, exclusive: String? = nil, props: Props)
    async throws(PDFError)
  {
    try await open(id, exclusive: exclusive, propsJSON: try encoded(props))
  }

  public func close(_ id: String) async throws(PDFError) {
    try await bridged { try await handle.closeSurface(id: id) }
  }

  public func toggle(_ id: String, exclusive: String? = nil, propsJSON: String? = nil) async throws(PDFError) {
    let options = ShellOpenOptions(exclusive: exclusive, propsJson: propsJSON)
    try await bridged { try await handle.toggleSurface(id: id, options: options) }
  }

  /// Change a surface's props without reopening it.
  public func updateProps(of id: String, propsJSON: String) async throws(PDFError) {
    try await bridged { try await handle.updateSurfaceProps(id: id, propsJson: propsJSON) }
  }

  public func updateProps<Props: Encodable & Sendable>(of id: String, props: Props) async throws(PDFError) {
    try await updateProps(of: id, propsJSON: try encoded(props))
  }

  public func closeAll() async throws(PDFError) {
    try await bridged { try await handle.closeAllSurfaces() }
  }

  public func isMenuOpen(_ id: String) async throws(PDFError) -> Bool {
    try await bridged { try await handle.isMenuOpen(id: id) }
  }

  /// In opening order; the last is topmost.
  public func openMenus() async throws(PDFError) -> [String] {
    try await bridged { try await handle.openMenus() }
  }

  public func openMenu(_ id: String) async throws(PDFError) {
    try await bridged { try await handle.openMenu(id: id) }
  }

  public func closeMenu(_ id: String) async throws(PDFError) {
    try await bridged { try await handle.closeMenu(id: id) }
  }

  public func toggleMenu(_ id: String) async throws(PDFError) {
    try await bridged { try await handle.toggleMenu(id: id) }
  }

  public func closeAllMenus() async throws(PDFError) {
    try await bridged { try await handle.closeAllMenus() }
  }

  /// Everything this document's shell holds, for persist and restore.
  public func snapshot() async throws(PDFError) -> ShellSnapshot {
    try await bridged { try await handle.snapshot() }
  }

  public func applySnapshot(_ snapshot: ShellSnapshot) async throws(PDFError) {
    try await bridged { try await handle.applySnapshot(snapshot: snapshot) }
  }

  /// Surface and menu events. Finishes when the document closes (at once
  /// if it already has) or the viewer is destroyed.
  public var events: AsyncStream<ShellEvent> {
    viewer.registry.stream { handle.subscribe(listener: $0) }
  }

  private func encoded(_ props: some Encodable) throws(PDFError) -> String {
    do {
      return String(decoding: try JSONEncoder().encode(props), as: UTF8.self)
    } catch {
      throw PDFError(name: "EncodingError", code: "invalid-input", detail: "\(error)")
    }
  }
}

extension ShellSurface {
  /// The props decoded as `Props`, or `nil` when the surface has none.
  public func props<Props: Decodable>(as type: Props.Type) throws -> Props? {
    try propsJson.map { try JSONDecoder().decode(type, from: Data($0.utf8)) }
  }
}

extension ShellSurfaceEvent {
  public func props<Props: Decodable>(as type: Props.Type) throws -> Props? {
    try propsJson.map { try JSONDecoder().decode(type, from: Data($0.utf8)) }
  }
}

extension EventRelay: ShellEventListener where Event == ShellEvent {
  func onEvent(event: ShellEvent) {
    continuation.yield(event)
  }

  func onEnd() {
    continuation.finish()
  }
}

extension PDFViewer {
  public func shell(for document: PDFDocument) async throws(PDFError) -> ShellCapability {
    let handle = try await bridged { try await host.shell(documentId: document.id) }
    return ShellCapability(handle: handle, viewer: self)
  }
}
