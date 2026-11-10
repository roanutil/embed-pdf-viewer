import EmbedPDF
import Observation

/// A document's search session for SwiftUI.
///
/// ``status``, ``activeHitIndex``, ``hitCount``, ``progress`` and ``hits``
/// are snapshots of the kernel, read again after each state revision and
/// search event while ``task()`` runs, and after each action. A snapshot is
/// one revision behind the kernel until its refresh lands: read right after a
/// change, it still holds the value from before. The `SearchCapability` reads
/// answer with the kernel's current value. A refresh that started earlier
/// never overwrites one that started later. A failed read, as when the
/// document closes, empties the model and sets ``error``; ``task()`` ends with
/// one more refresh, so a closed document leaves the model empty.
///
/// ``error`` is the latest failure of an action or a read. A successful
/// background refresh does not clear it; a successful ``searchSubmitted()``
/// does.
@MainActor
@Observable
public final class SearchModel {
  public var text = ""
  public private(set) var hits: [SearchHit] = []
  /// The active hit's position in `hits`, `nil` when no hit is active.
  public private(set) var activeHitIndex: Int?
  /// ``activeHitIndex`` as the engine's `UInt32`.
  @available(*, deprecated, renamed: "activeHitIndex")
  public var activeIndex: UInt32? { activeHitIndex.flatMap { UInt32(exactly: $0) } }
  public private(set) var status: SearchStatus = .idle
  /// Hits found so far in the whole document.
  public private(set) var hitCount = 0
  public private(set) var progress = SearchProgress(pagesSearched: 0, pageCount: 0)
  public private(set) var error: PDFError?
  private var pageHitCounts: [PageRef: Int] = [:]
  @ObservationIgnored private var query: SearchQuery?
  /// Set by a submitted search and cleared only when a hit list is applied,
  /// so a refresh that supersedes the submitted one still rereads it.
  @ObservationIgnored private var hitsOutdated = false
  @ObservationIgnored let search: SearchCapability
  @ObservationIgnored let snapshots = SnapshotRefresh()
  @ObservationIgnored private var observation: UInt64 = 0

  public init(search: SearchCapability) {
    self.search = search
  }

  /// Hits found so far on one page, counted from ``hits``.
  public func hitCount(on page: PageRef) -> Int { pageHitCounts[page] ?? 0 }

  public func task() async {
    observation &+= 1
    let session = observation
    defer { if observation == session { snapshots.invalidate() } }
    await observeCapability(
      viewer: search.viewer, events: search.events,
      initial: { if self.observation == session { await self.refresh() } },
      refresh: { if self.observation == session { await self.refresh() } },
      receive: { _ in if self.observation == session { await self.refresh() } })
    if observation == session { await refresh() }
  }

  public func refresh() async {
    await snapshots.run(load: {
      let status = try await self.search.status()
      let activeHitIndex = try await self.search.activeHitIndex()
      let hitCount: Int = try await self.search.hitCount()
      let progress = try await self.search.progress()
      let query = try await self.search.query()
      // Between searches the hit list changes only with the query, the count
      // or the status, and a revision is often a scroll or a zoom: keep the
      // list then. A submitted search always rereads it, since a rerun after
      // an edit can find as many hits in other places.
      let unchanged = !self.hitsOutdated && query == self.query && hitCount == self.hits.count
        && status == self.status
      let hits: [SearchHit]? = unchanged ? nil : try await self.search.hits()
      return (status, activeHitIndex, hitCount, progress, query, hits)
    }, apply: { snapshot in
      self.status = snapshot.0
      self.activeHitIndex = snapshot.1
      self.hitCount = snapshot.2
      self.progress = snapshot.3
      self.query = snapshot.4
      if let hits = snapshot.5 {
        self.hitsOutdated = false
        self.hits = hits
        self.pageHitCounts = hits.reduce(into: [:]) { counts, hit in counts[hit.page, default: 0] += 1 }
      }
    }, failure: {
      self.status = .idle
      self.activeHitIndex = nil
      self.hitCount = 0
      self.progress = SearchProgress(pagesSearched: 0, pageCount: 0)
      self.query = nil
      self.hits = []
      self.pageHitCounts = [:]
      self.error = controlError($0)
    })
  }

  public func searchSubmitted() async {
    do {
      _ = try await search.search(SearchQuery(text: text))
      error = nil
    } catch {
      snapshots.invalidate()
      self.error = error
      status = .error
      return
    }
    hitsOutdated = true
    await refresh()
  }

  public func nextButtonTapped() async {
    do {
      _ = try await search.nextHit()
    } catch {
      self.error = error
      return
    }
    await refresh()
  }

  public func previousButtonTapped() async {
    do {
      _ = try await search.previousHit()
    } catch {
      self.error = error
      return
    }
    await refresh()
  }

  public func clearButtonTapped() async {
    do {
      try await search.clear()
    } catch {
      self.error = error
      return
    }
    await refresh()
  }
}
