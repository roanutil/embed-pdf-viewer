import EmbedPDF
import Observation

/// The viewer's open documents, for SwiftUI.
///
/// ``activeDocument`` and ``documentCount`` are snapshots of the kernel, read
/// again after each state revision while ``task()`` runs. A snapshot is one
/// revision behind the kernel until its refresh lands: read right after an
/// open or a close, it still holds the value from before. The `PDFViewer`
/// reads answer with the kernel's current value. A refresh that started
/// earlier never overwrites one that started later. A failed read, as when
/// the viewer is destroyed, empties the model and sets ``error``; ``task()``
/// ends with one more refresh, so a destroyed viewer leaves it empty.
@MainActor
@Observable
public final class ViewerModel {
  /// The active tab, which may be loading, failed or locked; `nil` when no
  /// document is open.
  public private(set) var activeDocument: DocumentInfo?
  /// Open tabs, including loading, failed and locked ones.
  public private(set) var documentCount = 0
  public private(set) var error: PDFError?
  @ObservationIgnored private let viewer: PDFViewer
  @ObservationIgnored let snapshots = SnapshotRefresh()
  @ObservationIgnored private var observation: UInt64 = 0

  public init(viewer: PDFViewer) {
    self.viewer = viewer
  }

  /// Observes until the viewer is destroyed or the task is cancelled.
  public func task() async {
    observation &+= 1
    let session = observation
    defer { if observation == session { snapshots.invalidate() } }
    let revisions = viewer.observeStateRevisions()
    await refresh()
    for await _ in revisions {
      guard observation == session else { return }
      await refresh()
    }
    if observation == session { await refresh() }
  }

  public func refresh() async {
    await snapshots.run(load: {
      let activeDocument = try await self.viewer.activeDocument()
      let documentCount: Int = try await self.viewer.documentCount()
      return (activeDocument, documentCount)
    }, apply: { snapshot in
      (self.activeDocument, self.documentCount) = snapshot
      self.error = nil
    }, failure: {
      self.activeDocument = nil
      self.documentCount = 0
      self.error = controlError($0)
    })
  }
}
