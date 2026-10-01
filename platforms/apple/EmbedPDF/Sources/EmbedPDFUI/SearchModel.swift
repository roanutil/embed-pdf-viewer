import EmbedPDF
import Observation

@MainActor
@Observable
public final class SearchModel {
  public var text = ""
  public private(set) var hits: [SearchHit] = []
  public private(set) var activeIndex: UInt32?
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
      activeIndex = try await search.activeHitIndex()
    } catch {
      self.error = error
    }
    for await event in events {
      if case let .activeHitChanged(event) = event {
        activeIndex = event.index
      }
    }
  }

  public func searchSubmitted() async {
    do {
      _ = try await search.search(SearchQuery(text: text))
      hits = try await search.hits()
      status = try await search.status()
      activeIndex = try await search.activeHitIndex()
      error = nil
    } catch {
      self.error = error
      status = .error
    }
  }

  public func nextButtonTapped() async {
    do {
      _ = try await search.nextHit()
      activeIndex = try await search.activeHitIndex()
    } catch {
      self.error = error
    }
  }

  public func previousButtonTapped() async {
    do {
      _ = try await search.previousHit()
      activeIndex = try await search.activeHitIndex()
    } catch {
      self.error = error
    }
  }

  public func clearButtonTapped() async {
    do {
      try await search.clear()
      hits = []
      activeIndex = nil
      status = try await search.status()
    } catch {
      self.error = error
    }
  }
}
