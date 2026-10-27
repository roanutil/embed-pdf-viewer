import EmbedPDF
import Observation

/// Observable search controls for one document's search session.
///
/// Create on the main actor from an enabled search capability and retain in
/// SwiftUI state. Call ``task()`` in a view task to follow active-hit changes.
/// Submit and navigate through this model to keep its snapshots current;
/// external searches do not refresh ``hits`` or ``status`` automatically.
/// The capability retains its viewer, but closing the document invalidates it.
@MainActor
@Observable
public final class SearchModel {
  /// Editable query text, initially empty; changing it does not start a search.
  public var text = ""
  /// Hits captured after the last successful submission; initially empty.
  public private(set) var hits: [SearchHit] = []
  /// Zero-based index in the search session, or `nil` when no hit is active.
  public private(set) var activeIndex: UInt32?
  /// Session status read at task startup, submission, or clear; initially `.idle`.
  ///
  /// This snapshot does not track intermediate search-progress events.
  public private(set) var status: SearchStatus = .idle
  /// Last caught capability error, cleared only by a successful submission.
  ///
  /// A successful navigation or clear does not remove an earlier error.
  public private(set) var error: PDFError?
  @ObservationIgnored let search: SearchCapability

  /// Creates controls for a document with `SearchConfig` enabled.
  ///
  /// - Parameter search: The document's live search capability.
  public init(search: SearchCapability) {
    self.search = search
  }

  /// Reads the initial status and active index, then follows active-hit events.
  ///
  /// Run once per displayed model in a SwiftUI `.task`; cancel that task when
  /// the owner disappears. The stream also ends when the document closes or
  /// viewer is destroyed. This method does not load preexisting hits or mirror
  /// external status changes. Initial read failures are stored in ``error``.
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

  /// Searches for ``text`` and snapshots hits, status, and the active index.
  ///
  /// An empty query clears the underlying session. Cancelling the calling task
  /// cancels the scan; the capability may return partial hits and a cancelled
  /// status. This model does not expose scan progress while awaiting completion.
  /// A thrown error is stored in ``error`` and sets ``status`` to `.error`;
  /// earlier snapshots may remain. Serialize submissions in the host if needed.
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

  /// Advances the session's active hit and refreshes ``activeIndex``.
  ///
  /// Uses the capability's default reveal behavior. Errors are stored in
  /// ``error``; ``hits`` and ``status`` are not refreshed.
  public func nextButtonTapped() async {
    do {
      _ = try await search.nextHit()
      activeIndex = try await search.activeHitIndex()
    } catch {
      self.error = error
    }
  }

  /// Moves to the previous session hit and refreshes ``activeIndex``.
  ///
  /// Uses the capability's default reveal behavior. Errors are stored in
  /// ``error``; ``hits`` and ``status`` are not refreshed.
  public func previousButtonTapped() async {
    do {
      _ = try await search.previousHit()
      activeIndex = try await search.activeHitIndex()
    } catch {
      self.error = error
    }
  }

  /// Clears the search session, empties ``hits``, and removes the active index.
  ///
  /// Reads the resulting session status. Leaves ``text`` and any previous
  /// ``error`` unchanged; a thrown error replaces ``error``. This is session
  /// cleanup, not document or viewer destruction.
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
