import EmbedPDF
import Observation

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
  public private(set) var error: PDFError?
  @ObservationIgnored let search: SearchCapability

  public init(search: SearchCapability) {
    self.search = search
  }

  public func task() async {
    let events = search.events
    do {
      status = try await search.status()
      activeHitIndex = try await search.activeHitIndex()
    } catch {
      self.error = error
    }
    for await event in events {
      if case let .activeHitChanged(event) = event {
        do {
          activeHitIndex = try event.index?.toInt("active hit index")
        } catch {
          self.error = error
        }
      }
    }
  }

  public func searchSubmitted() async {
    do {
      _ = try await search.search(SearchQuery(text: text))
      hits = try await search.hits()
      status = try await search.status()
      activeHitIndex = try await search.activeHitIndex()
      error = nil
    } catch {
      self.error = error
      status = .error
    }
  }

  public func nextButtonTapped() async {
    do {
      _ = try await search.nextHit()
      activeHitIndex = try await search.activeHitIndex()
    } catch {
      self.error = error
    }
  }

  public func previousButtonTapped() async {
    do {
      _ = try await search.previousHit()
      activeHitIndex = try await search.activeHitIndex()
    } catch {
      self.error = error
    }
  }

  public func clearButtonTapped() async {
    do {
      try await search.clear()
      hits = []
      activeHitIndex = nil
      status = try await search.status()
    } catch {
      self.error = error
    }
  }
}
