// The search plugin over the hand-written C ABI (`epdf_manual.h`), for
// Task 11's benchmarks only. The method names match `SearchCapability` for
// the members it measures.
public import EmbedPDF
internal import EpdfManualFFI
import Foundation

/// A document's search capability through the manual bridge.
public final class ManualSearch: @unchecked Sendable {
  let raw: OpaquePointer
  let viewer: PDFViewer

  init(raw: OpaquePointer, viewer: PDFViewer) {
    self.raw = raw
    self.viewer = viewer
  }

  public static func open(_ viewer: PDFViewer, _ document: DocumentInfo) async throws(ManualError)
    -> ManualSearch
  {
    let kernel = viewer.experimentalKernelToken
    let raw = try await manualCall({ ctx in
      document.id.withCString { epdf_search_open(kernel, $0, ctx, trampoline) }
    }) { result in OpaquePointer(result!) }
    return ManualSearch(raw: raw, viewer: viewer)
  }

  public func hitCount() async throws(ManualError) -> UInt32 {
    try await manualCall({ ctx in
      epdf_search_hit_count(raw, ctx, trampoline)
    }) { result in result!.load(as: UInt32.self) }
  }

  /// The session's hits, read in place: nothing is copied until an element
  /// is read.
  public func hits() async throws(ManualError) -> ManualHits {
    let hits = try await manualCall({ ctx in
      epdf_search_hits(raw, ctx, trampoline)
    }) { result in result!.load(as: EpdfHits.self) }
    return ManualHits(hits)
  }

  /// Start a session search; answers its hit count.
  public func search(_ text: String) async throws(ManualError) -> UInt32 {
    try await manualCallCancellable({ ctx in
      text.withCString { epdf_search_search(raw, $0, ctx, trampoline) }
    }) { result in result!.load(as: UInt32.self) }
  }

  /// Every hit, touching no session state.
  public func findAll(_ text: String) async throws(ManualError) -> ManualHits {
    let hits = try await manualCallCancellable({ ctx in
      text.withCString { epdf_search_find_all(raw, $0, ctx, trampoline) }
    }) { result in result!.load(as: EpdfHits.self) }
    return ManualHits(hits)
  }

  /// Session events. Finishes when the document closes.
  public var events: AsyncStream<ManualSearchEvent> {
    let (stream, continuation) = AsyncStream<ManualSearchEvent>.makeStream()
    let sink = Unmanaged.passRetained(EventSink(continuation)).toOpaque()
    let subscription = Carried(value: epdf_search_subscribe(raw, sink, onSearchEvent, releaseSink))
    continuation.onTermination = { _ in
      epdf_subscription_free(subscription.value)
    }
    return stream
  }

  deinit {
    epdf_search_free(raw)
  }
}

/// A session event, decoded on the kernel thread.
public enum ManualSearchEvent: Sendable {
  case started(text: String, operationID: String)
  case progress(scanned: Double, total: Double, hitCount: UInt32, operationID: String)
  case completed(hitCount: UInt32, operationID: String)
  case cancelled(operationID: String, reason: Int32)
  case failed(code: String, message: String, operationID: String)
  case activeHitChanged(index: UInt32, hit: SearchHit?)
  case cleared
}

final class EventSink: @unchecked Sendable {
  let continuation: AsyncStream<ManualSearchEvent>.Continuation

  init(_ continuation: AsyncStream<ManualSearchEvent>.Continuation) {
    self.continuation = continuation
  }
}

private let onSearchEvent: EpdfSearchEvent = { ctx, kind, payload in
  let sink = Unmanaged<EventSink>.fromOpaque(ctx!).takeUnretainedValue()
  func string(_ s: UnsafePointer<CChar>?) -> String { s.map { String(cString: $0) } ?? "" }
  switch kind {
  case EPDF_SEARCH_EVENT_STARTED:
    let e = payload!.load(as: EpdfSearchStarted.self)
    sink.continuation.yield(.started(text: string(e.text), operationID: string(e.operation_id)))
  case EPDF_SEARCH_EVENT_PROGRESS:
    let e = payload!.load(as: EpdfSearchProgress.self)
    sink.continuation.yield(
      .progress(
        scanned: e.scanned, total: e.total, hitCount: e.hit_count,
        operationID: string(e.operation_id)))
  case EPDF_SEARCH_EVENT_COMPLETED:
    let e = payload!.load(as: EpdfSearchCompleted.self)
    sink.continuation.yield(
      .completed(hitCount: e.hit_count, operationID: string(e.operation_id)))
  case EPDF_SEARCH_EVENT_CANCELLED:
    let e = payload!.load(as: EpdfSearchCancelled.self)
    sink.continuation.yield(.cancelled(operationID: string(e.operation_id), reason: e.reason))
  case EPDF_SEARCH_EVENT_FAILED:
    let e = payload!.load(as: EpdfSearchFailed.self)
    sink.continuation.yield(
      .failed(code: string(e.code), message: string(e.message), operationID: string(e.operation_id)))
  case EPDF_SEARCH_EVENT_ACTIVE_HIT_CHANGED:
    let e = payload!.load(as: EpdfSearchActiveHitChanged.self)
    let hit = e.hits.map { hits in
      SearchHit(hits.pointee.hits[0], segments: hits.pointee.segments)
    }
    sink.continuation.yield(.activeHitChanged(index: e.index, hit: hit))
  case EPDF_SEARCH_EVENT_CLEARED:
    sink.continuation.yield(.cleared)
  case EPDF_SEARCH_EVENT_END:
    sink.continuation.finish()
  default:
    break
  }
}

private let releaseSink: EpdfRelease = { ctx in
  let sink = Unmanaged<EventSink>.fromOpaque(ctx!).takeRetainedValue()
  sink.continuation.finish()
}

/// A hit list read in place from the bridge's arrays, freed with the last
/// reference. Elements are decoded as they are read.
public final class ManualHits: RandomAccessCollection, @unchecked Sendable {
  let raw: EpdfHits

  init(_ raw: EpdfHits) {
    self.raw = raw
  }

  public var startIndex: Int { 0 }
  public var endIndex: Int { raw.hit_count }

  public subscript(position: Int) -> ManualHit {
    ManualHit(raw: raw.hits[position], segments: raw.segments, owner: self)
  }

  deinit {
    epdf_hits_free(raw.owner)
  }
}

/// One hit, viewed in place. It keeps its list alive.
public struct ManualHit: @unchecked Sendable {
  let raw: EpdfSearchHit
  let segments: UnsafePointer<EpdfSegment>?
  let owner: ManualHits

  public var pageObjectNumber: UInt32 { raw.page_object_number }
  public var pageIndex: UInt32 { raw.page_index }
  public var charStart: Double { raw.char_start }
  public var charCount: Double { raw.char_count }
  public var segmentCount: Int { Int(raw.segment_count) }
}

extension Rect {
  init(_ r: EpdfRect) {
    self.init(x: r.x, y: r.y, width: r.width, height: r.height)
  }
}

extension Point {
  init(_ p: EpdfPoint) {
    self.init(x: p.x, y: p.y)
  }
}

extension SearchHit {
  /// A Swift value, decoded from the hit and copied out of the bridge.
  public init(_ hit: ManualHit) {
    self.init(hit.raw, segments: hit.segments)
  }

  init(_ h: EpdfSearchHit, segments: UnsafePointer<EpdfSegment>?) {
    var decoded: [TextSegment] = []
    decoded.reserveCapacity(Int(h.segment_count))
    if let segments {
      for i in Int(h.segment_offset)..<Int(h.segment_offset + h.segment_count) {
        let s = segments[i]
        decoded.append(
          TextSegment(
            quad: TextQuad(
              upperStart: Point(s.quad.upper_start), upperEnd: Point(s.quad.upper_end),
              lowerStart: Point(s.quad.lower_start), lowerEnd: Point(s.quad.lower_end)),
            rect: Rect(s.rect), advance: s.advance))
      }
    }
    var snippet: SearchSnippet?
    if h.has_snippet != 0 {
      let bytes = UnsafeBufferPointer(start: h.snippet_text, count: h.snippet_len)
      snippet = SearchSnippet(
        text: String(decoding: bytes, as: UTF8.self), matchStart: h.snippet_match_start,
        matchLength: h.snippet_match_length)
    }
    self.init(
      page: PageRef(pageObjectNumber: h.page_object_number), pageIndex: h.page_index,
      charStart: h.char_start, charCount: h.char_count, segments: decoded,
      bounds: h.has_bounds != 0 ? Rect(h.bounds) : nil, snippet: snippet)
  }
}
