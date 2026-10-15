public import EmbedPDFBindings
public import Foundation

public typealias PageEditPlacement = EmbedPDFBindings.PageEditPlacement
public typealias PageEditTurn = EmbedPDFBindings.PageEditTurn
public typealias PageEditRotation = EmbedPDFBindings.PageEditRotation
public typealias PageEditBlankOptions = EmbedPDFBindings.PageEditBlankOptions
public typealias PageEditBytesOptions = EmbedPDFBindings.PageEditBytesOptions
public typealias PageEditLayout = EmbedPDFBindings.PageEditLayout
public typealias PageEditInsertion = EmbedPDFBindings.PageEditInsertion

/// Structural page edits, addressed by `PageRef`: rotate, move, delete,
/// insert and extract. Get one with `PDFViewer.pageEdit(for:)`; the viewer
/// needs `ViewerConfig(pageEdit: true)`. Page-list changes arrive on
/// `PDFViewer.documentEvents` as `.pagesChanged`; this capability has no
/// stream of its own. Cancelling a task skips an edit that hasn't started;
/// one that has started lands. After its document closes, every call
/// throws `instance-closed`.
public struct PageEditCapability: Sendable {
  let handle: PageEditHandle
  let viewer: PDFViewer

  /// Whether this caller may edit pages (`doc.pages.assemble`).
  public func canEdit() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canEdit() }
  }

  /// Turn each page from its own rotation.
  public func rotate(_ pages: [PageRef], by turn: PageEditTurn) async throws(PDFError) -> PageEditLayout {
    try await bridgedCancellable { token in try await handle.rotateBy(pages: pages, turn: turn, cancel: token) }
  }

  public func setRotation(_ rotation: PageEditRotation, of pages: [PageRef]) async throws(PDFError)
    -> PageEditLayout
  {
    try await bridgedCancellable { token in
      try await handle.setRotation(pages: pages, rotation: rotation, cancel: token)
    }
  }

  /// Reorder pages as a contiguous block at `placement`. `.end` puts the
  /// block last. An index counts the pages left once the moved ones are
  /// detached, as the engine does, so an `.index` or an `.after` past them
  /// is refused with `invalid-input`, as it is in the TypeScript plugin.
  /// (That plugin's `'end'` sends the full page count and is refused too;
  /// here it works.)
  public func movePages(_ pages: [PageRef], to placement: PageEditPlacement) async throws(PDFError)
    -> PageEditLayout
  {
    try await bridgedCancellable { token in
      try await handle.movePages(pages: pages, placement: placement, cancel: token)
    }
  }

  /// The engine refuses deleting every page.
  public func deletePages(_ pages: [PageRef]) async throws(PDFError) -> PageEditLayout {
    try await bridgedCancellable { token in try await handle.deletePages(pages: pages, cancel: token) }
  }

  public func insertBlank(_ options: PageEditBlankOptions = PageEditBlankOptions()) async throws(PDFError)
    -> PageEditInsertion
  {
    try await bridgedCancellable { token in try await handle.insertBlank(options: options, cancel: token) }
  }

  /// Pages of another PDF, given as its bytes.
  public func insert(contentsOf data: Data, options: PageEditBytesOptions = PageEditBytesOptions())
    async throws(PDFError) -> PageEditInsertion
  {
    try await bridgedCancellable { token in
      try await handle.insertFromBytes(bytes: data, options: options, cancel: token)
    }
  }

  /// Pages of another open document.
  public func insert(_ pages: [PageRef], from document: PDFDocument, at placement: PageEditPlacement? = nil)
    async throws(PDFError) -> PageEditInsertion
  {
    try await bridgedCancellable { token in
      try await handle.insertFromDocument(
        documentId: document.id, pages: pages, placement: placement, cancel: token)
    }
  }

  /// Copies of pages; by default right after the last of them.
  public func duplicate(_ pages: [PageRef], at placement: PageEditPlacement? = nil) async throws(PDFError)
    -> PageEditInsertion
  {
    try await bridgedCancellable { token in
      try await handle.duplicate(pages: pages, placement: placement, cancel: token)
    }
  }

  /// A new PDF from a page subset.
  public func extract(_ pages: [PageRef]) async throws(PDFError) -> Data {
    try await bridgedCancellable { token in try await handle.extract(pages: pages, cancel: token) }
  }
}

extension PDFViewer {
  public func pageEdit(for document: PDFDocument) async throws(PDFError) -> PageEditCapability {
    let handle = try await bridged { try await host.pageEdit(documentId: document.id) }
    return PageEditCapability(handle: handle, viewer: self)
  }
}
