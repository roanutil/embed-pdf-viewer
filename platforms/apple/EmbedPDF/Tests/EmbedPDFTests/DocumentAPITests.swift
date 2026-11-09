import EmbedPDF
import Foundation
import Testing

private final class DocumentTransport: @unchecked Sendable {
  let lock = NSLock()
  private var statuses: [Int] = []
  private var body = Data()
  private var paused = false
  private var requests = 0
  private var stops = 0

  func reset(statuses: [Int], body: Data, paused: Bool = false) {
    lock.withLock {
      self.statuses = statuses
      self.body = body
      self.paused = paused
      requests = 0
      stops = 0
    }
  }
  func begin() -> (Int, Data, Bool) {
    lock.withLock {
      requests += 1
      return (statuses.isEmpty ? 200 : statuses.removeFirst(), body, paused)
    }
  }
  func stop() { lock.withLock { stops += 1 } }
  var counts: (requests: Int, stops: Int) { lock.withLock { (requests, stops) } }
}

private final class DocumentURLProtocol: URLProtocol, @unchecked Sendable {
  static let transport = DocumentTransport()
  override class func canInit(with request: URLRequest) -> Bool { request.url?.host == "embedpdf.test" }
  override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
  override func startLoading() {
    let (status, data, paused) = Self.transport.begin()
    guard !paused, let url = request.url,
      let response = HTTPURLResponse(url: url, statusCode: status, httpVersion: nil, headerFields: nil)
    else { return }
    client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
    client?.urlProtocol(self, didLoad: data)
    client?.urlProtocolDidFinishLoading(self)
  }
  override func stopLoading() { Self.transport.stop() }
}

extension BaseSuite {
  @Suite struct DocumentAPITests {
    @Test func documentControlsUseTheCurrentKernelRegistry() async throws {
      let viewer = try PDFViewer(ViewerConfig(metadata: true))
      try await viewer.start()
      let first = try await viewer.open(Fixture.data("report"))
      let second = try await viewer.open(Fixture.data("report"), activate: false)
      #expect(try await viewer.activeDocumentID() == first.id)
      try await viewer.rename(first, to: "")
      #expect(try await viewer.document(id: first.id)?.name == "")
      try await viewer.setActive(second)
      let page = try #require(try await viewer.page(at: 0))
      #expect(try await viewer.page(page.page) == page)
      #expect(try await viewer.index(of: page.page) == 0)
      #expect(try await viewer.pageRevision() == 0)
      try await viewer.move(second, to: 0)
      #expect(try await viewer.documentOrder() == [second.id, first.id])
      try await viewer.swap(first, second)
      #expect(try await viewer.documentOrder() == [first.id, second.id])
      try await viewer.setDocumentOrder([second.id, first.id])
      let metadata = try await viewer.metadata(for: first)
      _ = try await metadata.update(MetadataPatch(title: "Saved layer title"))
      let bytes = try await viewer.saveLayer(first)
      let restored = try await viewer.open(base: Fixture.data("report"), layer: bytes)
      #expect(restored.pageCount == 14)
      let restoredMetadata = try await viewer.metadata(for: restored)
      try await restoredMetadata.ensureLoaded()
      #expect(try await restoredMetadata.snapshot()?.title == "Saved layer title")
      try await viewer.close(second)
      #expect(try await viewer.document(id: second.id) == nil)
      #expect(try await viewer.page(page.page, in: second) == nil)
      #expect(try await viewer.pageRevision(in: second) == -1)
      try await viewer.closeAll()
      #expect(try await viewer.documentCount() == 0)
      #expect(try await viewer.activeDocument() == nil)
      try await viewer.destroy()
    }

    @Test func settingsPreserveOmissionsAndClearWithoutGrantingDownload() async throws {
      let viewer = try PDFViewer(ViewerConfig(scope: ["doc.open"]))
      try await viewer.start()
      let initial = try await viewer.settings()
      let revisions = viewer.observeStateRevisions()
      var iterator = revisions.makeAsyncIterator()
      try await viewer.updateSettings(ViewerSettingsPatch(accent: "#123456", pageBackground: "transparent"))
      #expect(await iterator.next() != nil)
      try await viewer.updateSettings(ViewerSettingsPatch(clear: [.accent]))
      let current = try await viewer.settings()
      #expect(current.accent == nil)
      #expect(current.pageBackground == "transparent")
      #expect(current.pageShadow == initial.pageShadow)
      try await viewer.resetSettings()
      #expect(try await viewer.settings() == initial)
      let document = try await viewer.open(Fixture.data("report"))
      #expect(try await !viewer.canDownload(document))
      #expect(try await !viewer.allows("doc.download", in: document))
      await #expect { _ = try await viewer.saveLayer(document) } throws: { ($0 as? PDFError)?.code == "permission-denied" }
      try await viewer.destroy()
    }

    @Test func urlSessionProviderKeepsFailedTabsAndRefetchesOnRetry() async throws {
      DocumentURLProtocol.transport.reset(statuses: [404, 200], body: try Fixture.data("report"))
      let configuration = URLSessionConfiguration.ephemeral
      configuration.protocolClasses = [DocumentURLProtocol.self]
      let session = URLSession(configuration: configuration)
      defer { session.invalidateAndCancel() }
      let viewer = try PDFViewer(ViewerConfig(), documentFetcher: URLSessionDocumentFetcher(session: session))
      try await viewer.start()
      await #expect { _ = try await viewer.open(URL(string: "https://embedpdf.test/report.pdf")!) } throws: { ($0 as? PDFError)?.code == "not-found" }
      let failed = try #require(try await viewer.documents().first)
      #expect(failed.status == .failed)
      let retried = try await viewer.retry(failed)
      #expect(retried.status == .ready)
      #expect(try await viewer.documents().count == 1)
      #expect(retried.pageCount == 14)
      #expect(DocumentURLProtocol.transport.counts.requests == 2)
      try await viewer.destroy()
    }

    @Test func defaultUrlSessionProviderCancelsTheLoadingTab() async throws {
      DocumentURLProtocol.transport.reset(statuses: [200], body: Data(), paused: true)
      #expect(URLProtocol.registerClass(DocumentURLProtocol.self))
      defer { URLProtocol.unregisterClass(DocumentURLProtocol.self) }
      let viewer = try PDFViewer(ViewerConfig())
      try await viewer.start()
      let opening = Task { try await viewer.open(URL(string: "https://embedpdf.test/held.pdf")!) }
      for _ in 0..<500 {
        if DocumentURLProtocol.transport.counts.requests == 1 { break }
        try await Task.sleep(for: .milliseconds(10))
      }
      #expect(DocumentURLProtocol.transport.counts.requests == 1)
      #expect(try await viewer.documents().first?.status == .loading)
      opening.cancel()
      await #expect { _ = try await opening.value } throws: { ($0 as? PDFError)?.code == "operation-cancelled" }
      #expect(try await viewer.documents().isEmpty)
      for _ in 0..<500 {
        if DocumentURLProtocol.transport.counts.stops == 1 { break }
        try await Task.sleep(for: .milliseconds(10))
      }
      #expect(DocumentURLProtocol.transport.counts.stops == 1)
      try await viewer.destroy()
    }
  }
}
