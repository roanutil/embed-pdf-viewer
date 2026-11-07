import EmbedPDF
import Foundation
import Observation
import UniformTypeIdentifiers

@MainActor
@Observable
public final class AttachmentModel {
  public private(set) var files: [AttachedFile] = []
  public private(set) var selectedFiles: [AttachedFile] = []
  public private(set) var status: ResourceStatus = .idle
  public private(set) var canAdd = false
  public private(set) var canDownload = false
  public private(set) var canRemoveSelection = false
  public private(set) var pendingIDs: Set<String> = []
  public private(set) var performing = false
  public private(set) var error: PDFError?
  @ObservationIgnored private let viewer: PDFViewer
  @ObservationIgnored private let attachments: AttachmentCapability
  @ObservationIgnored private let snapshots = SnapshotRefresh()
  @ObservationIgnored private var observation: UInt64 = 0

  public init(viewer: PDFViewer, attachments: AttachmentCapability) {
    self.viewer = viewer
    self.attachments = attachments
  }

  public func task() async {
    observation &+= 1
    let session = observation
    defer { if observation == session { snapshots.invalidate() } }
    await observeCapability(viewer: viewer, events: attachments.events,
      initial: {
        guard self.observation == session else { return }
        do { try await self.attachments.ensureLoaded() } catch { self.error = controlError(error) }
        await self.refresh()
      },
      refresh: { if self.observation == session { await self.refresh() } },
      receive: { _ in if self.observation == session { await self.refresh() } })
    if observation == session { await refresh() }
  }

  public func refresh() async {
    await snapshots.run(load: {
      let files = try await self.attachments.files()
      let selected = try await self.attachments.selectedFiles()
      let status = try await self.attachments.status()
      let add = try await self.attachments.canAdd()
      let download = try await self.attachments.canDownload()
      var remove = !selected.isEmpty
      for file in selected { if try await !self.attachments.canRemove(file) { remove = false } }
      var pending: Set<String> = []
      for file in files { if try await self.attachments.isPending(file) { pending.insert(file.id) } }
      return (files, selected, status, add, download, remove, pending)
    }, apply: {
      (self.files, self.selectedFiles, self.status, self.canAdd, self.canDownload, self.canRemoveSelection, self.pendingIDs) = $0
    }, failure: {
      self.files = []; self.selectedFiles = []; self.pendingIDs = []
      self.canAdd = false; self.canDownload = false; self.canRemoveSelection = false
      self.error = controlError($0)
    })
  }

  public func reload() async { _ = await perform { try await self.attachments.refresh() } }
  public func select(_ files: [AttachedFile]) async { _ = await perform { try await self.attachments.select(files) } }
  public func add(_ sources: [AttachmentFileSource]) async { _ = await perform { try await self.attachments.add(sources) } }
  public func remove(_ file: AttachedFile) async { _ = await perform { try await self.attachments.remove(file) } }
  public func reveal(_ file: AttachedFile) async { _ = await perform { try await self.attachments.reveal(file) } }
  public func download(_ file: AttachedFile) async -> AttachmentContent? { await perform { try await self.attachments.download(file) } }

  public func importFiles(_ urls: [URL]) async {
    _ = await perform {
      let reading = Task.detached {
        try urls.map { url in
          try Task.checkCancellation()
          let access = url.startAccessingSecurityScopedResource()
          defer { if access { url.stopAccessingSecurityScopedResource() } }
          let bytes = try Data(contentsOf: url, options: .mappedIfSafe)
          try Task.checkCancellation()
          return AttachmentFileSource(data: bytes, name: url.lastPathComponent,
            mimeType: UTType(filenameExtension: url.pathExtension)?.preferredMIMEType)
        }
      }
      let sources = try await withTaskCancellationHandler {
        try await reading.value
      } onCancel: { reading.cancel() }
      try Task.checkCancellation()
      return try await self.attachments.add(sources)
    }
  }

  public func fileOperationFailed(_ failure: any Error) {
    if (failure as NSError).domain == NSCocoaErrorDomain && (failure as NSError).code == CocoaError.userCancelled.rawValue { return }
    error = controlError(failure)
  }

  private func perform<Value>(_ action: () async throws -> Value) async -> Value? {
    guard !performing else { return nil }
    performing = true
    error = nil
    defer { performing = false }
    do {
      try Task.checkCancellation()
      let value = try await action()
      try Task.checkCancellation()
      await refresh()
      return value
    } catch {
      await refresh()
      self.error = controlError(error)
      return nil
    }
  }
}
