import EmbedPDF
import Observation

/// What the session may do with a document's annotations, for SwiftUI.
///
/// ``canRead`` and ``canCreate`` are snapshots of the kernel, read again
/// after each state revision and annotation event while ``task()`` runs. A
/// snapshot is one revision behind the kernel until its refresh lands: read
/// right after a change, it still holds the value from before. The
/// `AnnotationCapability` reads answer with the kernel's current value. A
/// refresh that started earlier never overwrites one that started later.
/// Both are `false` until the first refresh lands and after a read fails,
/// as when the document closes.
@MainActor
@Observable
public final class AnnotationModel {
  public private(set) var canRead = false
  public private(set) var canCreate = false
  public private(set) var error: PDFError?
  @ObservationIgnored private let annotations: AnnotationCapability
  @ObservationIgnored let snapshots = SnapshotRefresh()
  @ObservationIgnored private var observation: UInt64 = 0

  public init(annotations: AnnotationCapability) {
    self.annotations = annotations
  }

  /// Observes until the document closes or the task is cancelled.
  public func task() async {
    observation &+= 1
    let session = observation
    defer { if observation == session { snapshots.invalidate() } }
    await observeCapability(
      viewer: annotations.viewer, events: annotations.events,
      initial: { if self.observation == session { await self.refresh() } },
      refresh: { if self.observation == session { await self.refresh() } },
      receive: { _ in if self.observation == session { await self.refresh() } })
    if observation == session { await refresh() }
  }

  public func refresh() async {
    await snapshots.run(load: {
      let canRead = try await self.annotations.canRead()
      let canCreate = try await self.annotations.canCreate()
      return (canRead, canCreate)
    }, apply: { snapshot in
      (self.canRead, self.canCreate) = snapshot
      self.error = nil
    }, failure: {
      self.canRead = false
      self.canCreate = false
      self.error = controlError($0)
    })
  }
}
