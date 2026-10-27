public import EmbedPDFBindings
import Foundation

/// A named document UI surface's open state, exclusive tag, and optional JSON props.
public typealias ShellSurface = EmbedPDFBindings.ShellSurface
/// An exclusive group tag and JSON object props used when opening a surface.
public typealias ShellOpenOptions = EmbedPDFBindings.ShellOpenOptions
/// The document shell's surface and menu state for persistence and restoration.
public typealias ShellSnapshot = EmbedPDFBindings.ShellSnapshot
/// An event describing a document surface opening, with the props it was opened with.
public typealias ShellSurfaceOpenedEvent = EmbedPDFBindings.ShellSurfaceOpenedEvent
/// An event describing a document surface closing.
public typealias ShellSurfaceClosedEvent = EmbedPDFBindings.ShellSurfaceClosedEvent
/// An event describing a document menu's state change.
public typealias ShellMenuEvent = EmbedPDFBindings.ShellMenuEvent
/// An event describing surface and menu state changes.
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

  /// Returns whether the named document surface is currently open.
  public func isOpen(_ id: String) async throws(PDFError) -> Bool {
    try await bridged { try await handle.isOpen(id: id) }
  }

  /// `nil` for a surface never opened. The TypeScript plugin keeps
  /// surfaces in a plain object and reads `surfaces[id]`, so an id naming
  /// an `Object.prototype` member (`"constructor"`, `"toString"`) is found
  /// there; it is found here too, closed, with no tag or props.
  public func surface(_ id: String) async throws(PDFError) -> ShellSurface? {
    try await bridged { try await handle.surface(id: id) }
  }

  /// Returns the document surfaces that are currently open.
  public func openSurfaces() async throws(PDFError) -> [ShellSurface] {
    try await bridged { try await handle.openSurfaces() }
  }

  /// A tagged open closes every other open surface with the same tag.
  public func open(_ id: String, exclusive: String? = nil, propsJSON: String? = nil) async throws(PDFError) {
    let options = ShellOpenOptions(exclusive: exclusive, propsJson: propsJSON)
    try await bridged { try await handle.openSurface(id: id, options: options) }
  }

  /// Opens a named surface with encoded object props; throws invalid-input if encoding fails
  /// or the value is not a JSON object.
  public func open<Props: Encodable & Sendable>(_ id: String, exclusive: String? = nil, props: Props)
    async throws(PDFError)
  {
    try await open(id, exclusive: exclusive, propsJSON: try encoded(props))
  }

  /// Closes the named surface in this document's shell state.
  public func close(_ id: String) async throws(PDFError) {
    try await bridged { try await handle.closeSurface(id: id) }
  }

  /// Toggles the named surface, applying the exclusive tag and JSON object props when opening it.
  public func toggle(_ id: String, exclusive: String? = nil, propsJSON: String? = nil) async throws(PDFError) {
    let options = ShellOpenOptions(exclusive: exclusive, propsJson: propsJSON)
    try await bridged { try await handle.toggleSurface(id: id, options: options) }
  }

  /// Change a surface's props without reopening it.
  public func updateProps(of id: String, propsJSON: String) async throws(PDFError) {
    try await bridged { try await handle.updateSurfaceProps(id: id, propsJson: propsJSON) }
  }

  /// Replaces a surface's props with an encoded JSON object without reopening it; throws
  /// invalid-input for invalid props.
  public func updateProps<Props: Encodable & Sendable>(of id: String, props: Props) async throws(PDFError) {
    try await updateProps(of: id, propsJSON: try encoded(props))
  }

  /// Closes every document surface while retaining the shell's surface records.
  public func closeAll() async throws(PDFError) {
    try await bridged { try await handle.closeAllSurfaces() }
  }

  /// Returns whether the named document menu is currently open.
  public func isMenuOpen(_ id: String) async throws(PDFError) -> Bool {
    try await bridged { try await handle.isMenuOpen(id: id) }
  }

  /// In opening order; the last is topmost.
  public func openMenus() async throws(PDFError) -> [String] {
    try await bridged { try await handle.openMenus() }
  }

  /// Opens the named menu in this document's menu stack.
  public func openMenu(_ id: String) async throws(PDFError) {
    try await bridged { try await handle.openMenu(id: id) }
  }

  /// Closes the named document menu.
  public func closeMenu(_ id: String) async throws(PDFError) {
    try await bridged { try await handle.closeMenu(id: id) }
  }

  /// Toggles the named menu's presence in the document's menu stack.
  public func toggleMenu(_ id: String) async throws(PDFError) {
    try await bridged { try await handle.toggleMenu(id: id) }
  }

  /// Closes every menu in this document's shell.
  public func closeAllMenus() async throws(PDFError) {
    try await bridged { try await handle.closeAllMenus() }
  }

  /// Everything this document's shell holds, for persist and restore.
  public func snapshot() async throws(PDFError) -> ShellSnapshot {
    try await bridged { try await handle.snapshot() }
  }

  /// Restores the document's surface and menu state from a previously captured snapshot.
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
  /// Decodes the surface's props as `Props`, or returns nil when it has none.
  ///
  /// - Throws: A JSON decoding error when props do not match the requested type.
  public func props<Props: Decodable>(as type: Props.Type) throws -> Props? {
    try propsJson.map { try JSONDecoder().decode(type, from: Data($0.utf8)) }
  }
}

extension ShellSurfaceOpenedEvent {
  /// Decodes this event's JSON props as the requested type, returning nil when no props are
  /// present and throwing a decoding error on mismatch.
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
  /// Returns this document's UI surface state; enable shell support in the viewer
  /// configuration before requesting it.
  public func shell(for document: DocumentInfo) async throws(PDFError) -> ShellCapability {
    let handle = try await bridged { try await host.shell(documentId: document.id) }
    return ShellCapability(handle: handle, viewer: self)
  }
}
