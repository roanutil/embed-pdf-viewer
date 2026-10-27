public import CoreGraphics
public import EmbedPDFBindings
import Foundation
import ImageIO

/// Configuration for stamp previews and dynamic asset evaluation.
public typealias StampConfig = EmbedPDFBindings.StampConfig
/// The metadata and ordered asset identifiers of a PDF-backed stamp library.
public typealias StampLibrary = EmbedPDFBindings.StampLibrary
/// A library asset's identifier, label, intrinsic PDF-point size and source page.
public typealias StampAsset = EmbedPDFBindings.StampAsset
/// Whether an asset represents a stamp, signature or initials.
public typealias StampAssetKind = EmbedPDFBindings.StampAssetKind
/// An ink, text, image or PDF source used to author an asset.
public typealias StampMark = EmbedPDFBindings.StampMark
/// The source, library and metadata for a new stamp asset.
public typealias StampAddAssetInput = EmbedPDFBindings.StampAddAssetInput
/// Library metadata and fallback asset settings used during PDF import.
public typealias StampImportOptions = EmbedPDFBindings.StampImportOptions
/// The identifier, kind and categories of a newly created library.
public typealias StampCreateLibraryOptions = EmbedPDFBindings.StampCreateLibraryOptions
/// Changes to a library's editable name and categories.
public typealias StampLibraryPatch = EmbedPDFBindings.StampLibraryPatch
/// Changes to an asset's label, subject and categories.
public typealias StampAssetPatch = EmbedPDFBindings.StampAssetPatch
/// Library, kind and category criteria for listing assets.
public typealias StampAssetFilter = EmbedPDFBindings.StampAssetFilter
/// Applied, skipped and failed placements from stamping several pages.
public typealias StampBatchResult = EmbedPDFBindings.StampBatchResult
/// The page and error associated with a failed stamp placement.
public typealias StampBatchFailure = EmbedPDFBindings.StampBatchFailure
/// The error details associated with a stamp operation.
public typealias StampErrorInfo = EmbedPDFBindings.StampErrorInfo
/// The library or asset mutation that changed a library's canonical bytes.
public typealias StampLibraryChangeReason = EmbedPDFBindings.StampLibraryChangeReason
/// A library, asset or document arming change.
public typealias StampEvent = EmbedPDFBindings.StampEvent
/// A persisted library identifier paired with its canonical PDF bytes.
public typealias StoredStampLibrary = EmbedPDFBindings.StoredStampLibrary
/// Host storage for listing, writing and deleting canonical library PDFs.
///
/// `list()` supplies identifier/byte pairs to restore. `put(id:bytes:)` replaces a
/// library's stored PDF, and `delete(id:)` removes it. Methods are synchronous host
/// callbacks; report storage failures with `HostError`.
public typealias StampLibraryStore = EmbedPDFBindings.StampLibraryStore

/// A stamp preview: the encoded bytes and, decoded once, the image.
public struct StampPreview: @unchecked Sendable {
  /// Encoded preview bytes, or `nil` when rendering supplied no bytes.
  public let data: Data?
  /// The encoded preview's MIME type, when supplied by the engine.
  public let mimeType: String?
  /// `nil` when there are no bytes or ImageIO can't read them.
  public let image: CGImage?

  init(_ preview: StampAssetPreview) {
    data = preview.bytes
    mimeType = preview.mimeType
    image = preview.bytes.flatMap { bytes in
      CGImageSourceCreateWithData(bytes as CFData, nil).flatMap {
        CGImageSourceCreateImageAtIndex($0, 0, nil)
      }
    }
  }
}

/// The workspace's stamp libraries. Get it with `PDFViewer.stamps()`; the
/// viewer needs `ViewerConfig(stamp: StampConfig())`, plus
/// `interaction: true, annotation: AnnotationConfig()` to place. One
/// capability for the whole viewer, and it keeps the viewer alive.
///
/// Libraries open on their own engine thread. On that engine an asset can
/// come from PDF bytes (a library, or `StampAddAssetInput.source`); ink and
/// text marks and `createAsset(from:in:on:)` need annotation appearance
/// export, which the native engine doesn't have yet.
public struct StampCapability: Sendable {
  let handle: StampHandle
  let viewer: PDFViewer

  /// Every library, or those of the given kinds (`stamps`, `signatures`).
  public func libraries(kinds: [String]? = nil) async throws(PDFError) -> [StampLibrary] {
    try await bridged { try await handle.libraries(kinds: kinds) }
  }

  /// Returns library metadata for `id`, or `nil` for an unknown library.
  public func library(_ id: String) async throws(PDFError) -> StampLibrary? {
    try await bridged { try await handle.library(id: id) }
  }

  /// Lists assets matching `filter`; `nil` includes every asset.
  public func assets(_ filter: StampAssetFilter? = nil) async throws(PDFError) -> [StampAsset] {
    try await bridged { try await handle.assets(filter: filter) }
  }

  /// Returns asset metadata for `id`, or `nil` for an unknown asset.
  public func asset(_ id: String) async throws(PDFError) -> StampAsset? {
    try await bridged { try await handle.asset(id: id) }
  }

  /// The preview cached at import. `nil` for an unknown asset.
  public func assetPreview(_ id: String) async throws(PDFError) -> StampPreview? {
    try await bridged { try await handle.assetPreview(id: id) }.map(StampPreview.init)
  }

  /// A fresh render `width` device pixels wide.
  public func renderAssetPreview(_ id: String, width: Double) async throws(PDFError) -> StampPreview? {
    try await bridgedCancellable { token in
      try await handle.renderAssetPreview(id: id, width: width, cancel: token)
    }.map(StampPreview.init)
  }

  /// The asset's own one-page PDF.
  public func assetBytes(_ id: String) async throws(PDFError) -> Data? {
    try await bridged { try await handle.readAssetBytes(id: id) }
  }

  /// The library's canonical PDF.
  public func exportLibrary(_ id: String) async throws(PDFError) -> Data {
    try await bridgedCancellable { token in try await handle.exportLibrary(id: id, cancel: token) }
  }

  /// Whether `document` (the active document when `nil`) can take a stamp.
  public func canPlace(in document: DocumentInfo? = nil) async throws(PDFError) -> Bool {
    try await bridged { try await handle.canPlace(documentId: document?.id) }
  }

  /// Returns whether the asset engine exposes the operations required to import libraries.
  public func canImport() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canImport() }
  }

  /// Returns the new library's id.
  public func createLibrary(_ name: String, options: StampCreateLibraryOptions = StampCreateLibraryOptions())
    async throws(PDFError) -> String
  {
    try await bridgedCancellable { token in
      try await handle.createLibrary(name: name, options: options, cancel: token)
    }
  }

  /// Changes a library's editable metadata and updates its canonical PDF.
  public func updateLibrary(_ id: String, _ patch: StampLibraryPatch) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.updateLibrary(id: id, patch: patch, cancel: token) }
  }

  /// Returns the library's id. A PDF without a `/Names /Pages` registry
  /// imports one asset per page. Throws `invalid-input` for bytes that
  /// aren't a PDF.
  public func importLibrary(_ data: Data, options: StampImportOptions = StampImportOptions())
    async throws(PDFError) -> String
  {
    try await bridgedCancellable { token in
      try await handle.importLibrary(bytes: data, options: options, cancel: token)
    }
  }

  /// Removes a library and its assets from the viewer catalog.
  public func deleteLibrary(_ id: String) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.deleteLibrary(id: id, cancel: token) }
  }

  /// Returns the new asset's id.
  public func createAsset(_ input: StampAddAssetInput) async throws(PDFError) -> String {
    try await bridgedCancellable { token in try await handle.createAsset(input: input, cancel: token) }
  }

  /// Throws `unsupported`: the native engine can't export annotation
  /// appearances yet.
  public func createAsset(
    from annotations: [AnnotationRef], in document: DocumentInfo, on page: PageRef,
    input: StampAddAssetInput = StampAddAssetInput()
  ) async throws(PDFError) -> String {
    try await bridgedCancellable { token in
      try await handle.createAssetFromAnnotations(
        documentId: document.id, page: page, annotations: annotations, input: input, cancel: token)
    }
  }

  /// Changes an asset's editable metadata and updates its library PDF.
  public func updateAsset(_ id: String, _ patch: StampAssetPatch) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.updateAsset(id: id, patch: patch, cancel: token) }
  }

  /// Removes an asset from its library and updates the library PDF.
  public func deleteAsset(_ id: String) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.deleteAsset(id: id, cancel: token) }
  }

  /// Returns the asset's new id (ids carry the library).
  public func moveAsset(_ id: String, to libraryId: String) async throws(PDFError) -> String {
    try await bridgedCancellable { token in
      try await handle.moveAsset(id: id, toLibraryId: libraryId, cancel: token)
    }
  }

  /// Copies an asset within its library and returns the new identifier.
  ///
  /// `label` optionally replaces the display label.
  public func duplicateAsset(_ id: String, label: String? = nil) async throws(PDFError) -> String {
    try await bridgedCancellable { token in try await handle.duplicateAsset(id: id, label: label, cancel: token) }
  }

  /// The next tap in `document` places the asset.
  ///
  /// - Parameters:
  ///   - assetId: The library asset to place on the next tap.
  ///   - document: The target document, with annotation and interaction installed.
  ///   - targetWidth: Optional placed width in page-space points; the asset's
  ///     intrinsic size is used when no width override is supplied.
  public func arm(_ assetId: String, in document: DocumentInfo, targetWidth: Double? = nil) async throws(PDFError) {
    try await bridgedCancellable { token in
      try await handle.armAsset(documentId: document.id, assetId: assetId, targetWidth: targetWidth, cancel: token)
    }
  }

  /// Place the asset at `point` (page space) and return the annotation.
  ///
  /// - Parameters:
  ///   - assetId: The library asset to place.
  ///   - document: The open document to receive the annotation.
  ///   - page: A page reference belonging to the target document.
  ///   - point: The target location in page space.
  ///   - targetWidth: Width in page-space points; `nil` uses the asset's intrinsic size.
  ///   - rotation: Rotation in degrees; `nil` uses zero.
  public func place(
    _ assetId: String, in document: DocumentInfo, on page: PageRef, at point: Point,
    targetWidth: Double? = nil, rotation: Double? = nil
  ) async throws(PDFError) -> AnnotationRef {
    try await bridgedCancellable { token in
      try await handle.placeAsset(
        documentId: document.id, assetId: assetId, page: page, at: point, targetWidth: targetWidth,
        rotation: rotation, cancel: token)
    }
  }

  /// The same placement on every page (`nil`) or those listed.
  public func place(
    _ assetId: String, in document: DocumentInfo, onPages pages: [PageRef]?, at point: Point,
    targetWidth: Double? = nil, rotation: Double? = nil
  ) async throws(PDFError) -> StampBatchResult {
    try await bridgedCancellable { token in
      try await handle.placeAssetOnPages(
        documentId: document.id, assetId: assetId, pages: pages, at: point, targetWidth: targetWidth,
        rotation: rotation, cancel: token)
    }
  }

  /// Cancels the prepared asset placement in `document`.
  public func disarm(in document: DocumentInfo) async throws(PDFError) {
    try await bridged { try await handle.disarm(documentId: document.id) }
  }

  /// Returns the asset awaiting placement in `document`, or `nil` when none is armed.
  public func armedAsset(in document: DocumentInfo) async throws(PDFError) -> StampAsset? {
    try await bridged { try await handle.armedAsset(documentId: document.id) }
  }

  /// Imports every library `store` holds and returns their ids. One that
  /// fails to import is skipped.
  public func restore(from store: some StampLibraryStore) async throws(PDFError) -> [String] {
    try await bridgedCancellable { token in try await handle.restore(store: store, cancel: token) }
  }

  /// Writes each changed library to `store` once changes stop for
  /// `debounce` (250 ms when `nil`), until `unregister()`, which flushes
  /// pending writes. Dropping the registration keeps it writing, as every
  /// `Registration` does; it stops when the viewer is destroyed.
  public func persist(to store: some StampLibraryStore, except: [String] = [], debounce: Duration? = nil)
    async throws(PDFError) -> Registration
  {
    let ms = debounce.map { Double($0.components.seconds) * 1000 + Double($0.components.attoseconds) / 1e15 }
    let handle = handle
    return Registration(handle: try await bridged {
      try await handle.persist(store: store, except: except, debounceMs: ms)
    })
  }

  /// Library, asset and arming changes. Finishes when the viewer is
  /// destroyed.
  public var events: AsyncStream<StampEvent> {
    viewer.registry.stream { handle.subscribe(listener: $0) }
  }
}

extension EventRelay: StampEventListener where Event == StampEvent {
  func onEvent(event: StampEvent) {
    continuation.yield(event)
  }
}

/// A `StampLibraryStore` over one file per library in `directory`.
public final class FileStampLibraryStore: StampLibraryStore, Sendable {
  /// The directory containing one percent-encoded library identifier and `.pdf` file per library.
  public let directory: URL

  /// Creates a file-backed store, creating `directory` and its parents if needed.
  ///
  /// Throws the file-system error if the directory cannot be created.
  public init(directory: URL) throws {
    self.directory = directory
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
  }

  private func url(_ id: String) -> URL {
    let safe = id.addingPercentEncoding(withAllowedCharacters: .alphanumerics) ?? id
    return directory.appending(path: "\(safe).pdf")
  }

  /// Reads `.pdf` files in filename order and decodes their filenames into library identifiers.
  ///
  /// Throws `HostError.Failed` if listing or reading a library fails.
  public func list() throws -> [StoredStampLibrary] {
    do {
      return try FileManager.default.contentsOfDirectory(at: directory, includingPropertiesForKeys: nil)
        .filter { $0.pathExtension == "pdf" }
        .sorted { $0.lastPathComponent < $1.lastPathComponent }
        .map { url in
          let id = url.deletingPathExtension().lastPathComponent.removingPercentEncoding ?? ""
          return StoredStampLibrary(id: id, bytes: try Data(contentsOf: url))
        }
    } catch {
      throw HostError.Failed(detail: "\(error)")
    }
  }

  /// Atomically writes or replaces the PDF bytes for `id`.
  ///
  /// Throws `HostError.Failed` if writing fails.
  public func put(id: String, bytes: Data) throws {
    do {
      try bytes.write(to: url(id), options: .atomic)
    } catch {
      throw HostError.Failed(detail: "\(error)")
    }
  }

  /// Deletes the stored PDF for `id`; an already missing file is ignored.
  ///
  /// Throws `HostError.Failed` for other deletion failures.
  public func delete(id: String) throws {
    do {
      try FileManager.default.removeItem(at: url(id))
    } catch CocoaError.fileNoSuchFile {
    } catch {
      throw HostError.Failed(detail: "\(error)")
    }
  }
}

extension PDFViewer {
  /// Returns the viewer-wide stamp library capability.
  ///
  /// Enable `ViewerConfig.stamp` before starting the viewer. The capability retains the viewer.
  ///
  /// - Throws: `PDFError` if the stamp plugin is unavailable.
  public func stamps() async throws(PDFError) -> StampCapability {
    let handle = try await bridged { try await host.stamp() }
    return StampCapability(handle: handle, viewer: self)
  }
}
