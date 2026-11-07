import EmbedPDF
public import EmbedPDFBindings
import Observation

@MainActor
@Observable
public final class OutlineModel {
  public private(set) var items: [OutlineItem] = []
  public private(set) var rows: [OutlineRow] = []
  public private(set) var selectedItems: [OutlineItemRef] = []
  public private(set) var currentItem: OutlineItemRef?
  public private(set) var status: ResourceStatus = .idle
  public private(set) var incomplete = false
  public private(set) var canEdit = false
  public private(set) var performing = false
  public private(set) var error: PDFError?
  public private(set) var lastActivation: LinkActivation?
  @ObservationIgnored private let viewer: PDFViewer
  @ObservationIgnored private let outline: OutlineCapability
  @ObservationIgnored private let snapshots = SnapshotRefresh()
  @ObservationIgnored private var observation: UInt64 = 0

  public init(viewer: PDFViewer, outline: OutlineCapability) {
    self.viewer = viewer
    self.outline = outline
  }

  public func task() async {
    observation &+= 1
    let session = observation
    defer { if observation == session { snapshots.invalidate() } }
    await observeCapability(viewer: viewer, events: outline.events,
      initial: {
        guard self.observation == session else { return }
        do { try await self.outline.ensureLoaded() } catch { self.error = controlError(error) }
        await self.refresh()
      },
      refresh: { if self.observation == session { await self.refresh() } },
      receive: { _ in if self.observation == session { await self.refresh() } })
    if observation == session { await refresh() }
  }

  public func refresh() async {
    await snapshots.run(load: {
      let items = try await self.outline.list()
      let rows = try await self.outline.rows()
      let selected = try await self.outline.selectedItems()
      let current = try await self.outline.currentItem()
      let status = try await self.outline.status()
      let incomplete = try await self.outline.isIncomplete()
      let editable = try await self.outline.canEdit()
      return (items, rows, selected, current, status, incomplete, editable)
    }, apply: {
      (self.items, self.rows, self.selectedItems, self.currentItem, self.status, self.incomplete, self.canEdit) = $0
    }, failure: {
      self.items = []; self.rows = []; self.selectedItems = []; self.currentItem = nil
      self.canEdit = false
      self.error = controlError($0)
    })
  }

  public func reload() async { _ = await perform { try await self.outline.refresh() } }
  public func select(_ refs: [OutlineItemRef]) async { _ = await perform { try await self.outline.select(refs) } }
  public func expand(_ ref: OutlineItemRef) async { _ = await perform { try await self.outline.expand(ref) } }
  public func collapse(_ ref: OutlineItemRef) async { _ = await perform { try await self.outline.collapse(ref) } }
  public func toggle(_ ref: OutlineItemRef) async { _ = await perform { try await self.outline.toggle(ref) } }
  public func create(_ input: OutlineItemInput, options: OutlineCreateOptions = OutlineCreateOptions(select: true)) async -> OutlineItem? {
    await perform { try await self.outline.create(input, options: options) }
  }
  public func update(_ ref: OutlineItemRef, _ patch: OutlineItemPatch) async -> OutlineItem? {
    await perform { try await self.outline.update(ref, patch) }
  }
  public func move(_ refs: [OutlineItemRef], to parent: OutlineItemRef?, position: OutlinePosition = .end) async {
    _ = await perform { try await self.outline.move(refs, to: parent, position: position) }
  }
  public func delete(_ refs: [OutlineItemRef]) async { _ = await perform { try await self.outline.delete(refs) } }
  public func activate(_ ref: OutlineItemRef) async -> LinkActivation? {
    let result = await perform {
      try await self.outline.select([ref])
      return try await self.outline.activate(ref)
    }
    lastActivation = result
    return result
  }

  private func perform<Value>(_ action: () async throws -> Value) async -> Value? {
    guard !performing else { return nil }
    performing = true
    error = nil
    defer { performing = false }
    do {
      let value = try await action()
      await refresh()
      return value
    } catch {
      await refresh()
      self.error = controlError(error)
      return nil
    }
  }
}
