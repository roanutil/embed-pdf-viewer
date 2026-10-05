import Foundation
import Observation
import PSPDFKit

@MainActor
@Observable
final class SearchModel {
  enum Status { case idle, searching, complete, error }

  var text = ""
  private(set) var hits: [SearchResult] = []
  private(set) var activeIndex: Int?
  private(set) var status: Status = .idle
  @ObservationIgnored private let search: TextSearch
  @ObservationIgnored private let relay = SearchRelay()

  init(document: NutrientDocument) {
    search = TextSearch(document: document.document)
    search.maximumNumberOfSearchResults = 0
    search.delegate = relay
  }

  func searchSubmitted() async {
    guard !text.isEmpty else { return await clearButtonTapped() }
    status = .searching
    let term = text
    let interval = signposter.beginInterval("search", id: signposter.makeSignpostID())
    let results = await relay.run(search, term: term)
    signposter.endInterval("search", interval)
    switch results {
    case let .some(found):
      hits = found
      activeIndex = found.isEmpty ? nil : 0
      status = .complete
    case .none:
      status = .idle
    }
  }

  func nextButtonTapped() async {
    guard !hits.isEmpty else { return }
    activeIndex = ((activeIndex ?? -1) + 1) % hits.count
  }

  func previousButtonTapped() async {
    guard !hits.isEmpty else { return }
    activeIndex = ((activeIndex ?? 0) - 1 + hits.count) % hits.count
  }

  func clearButtonTapped() async {
    search.cancelAllOperations()
    hits = []
    activeIndex = nil
    status = .idle
  }
}

/// Bridges `TextSearch`'s delegate callbacks to one awaited result per search.
final class SearchRelay: NSObject, TextSearchDelegate, @unchecked Sendable {
  private let lock = NSLock()
  private var continuation: CheckedContinuation<UncheckedBox<[SearchResult]>?, Never>?

  @MainActor
  func run(_ search: TextSearch, term: String) async -> [SearchResult]? {
    let boxed: UncheckedBox<[SearchResult]>? = await runBoxed(search, term: term)
    return boxed?.value
  }

  @MainActor
  private func runBoxed(_ search: TextSearch, term: String) async -> UncheckedBox<[SearchResult]>? {
    await withCheckedContinuation { continuation in
      lock.lock()
      let previous = self.continuation
      self.continuation = continuation
      lock.unlock()
      previous?.resume(returning: nil)
      search.search(for: term)
    }
  }

  private func finish(_ results: [SearchResult]?) {
    lock.lock()
    let continuation = self.continuation
    self.continuation = nil
    lock.unlock()
    continuation?.resume(returning: results.map(UncheckedBox.init))
  }

  func didFinish(
    _ textSearch: TextSearch, term searchTerm: String, searchResults: [SearchResult],
    isFullSearch: Bool, pageTextFound: Bool
  ) {
    finish(searchResults)
  }

  func didCancel(_ textSearch: TextSearch, term searchTerm: String, isFullSearch: Bool) {
    finish(nil)
  }

  func didFail(_ textSearch: TextSearch, withError error: any Error) {
    finish(nil)
  }
}
