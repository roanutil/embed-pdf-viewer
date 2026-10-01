// Task 11: the uniffi bridge against the hand-written C bridge, on the same
// viewer and the same workloads. Timing never fails a test; each test only
// asserts that both bridges answered the same thing. Results are one JSON
// line per workload and bridge, prefixed `BENCH `, for a script to collect
// from the xcodebuild log.
import CoreGraphics
import Darwin
import EmbedPDF
import EmbedPDFManualBridge
import Foundation
import Testing

enum Fixture {
  static func data(_ name: String) throws -> Data {
    let url = try #require(
      Bundle.module.url(forResource: name, withExtension: "pdf", subdirectory: "Fixtures"))
    return try Data(contentsOf: url)
  }
}

/// One workload's numbers for one bridge.
struct BenchResult: Encodable {
  let workload: String
  let bridge: String
  let n: Int
  let runs: Int
  /// Per operation, over every sample of every run.
  let median_ns: Double
  let p95_ns: Double
  /// Each run's mean per operation: the run-to-run spread.
  let run_means_ns: [Double]
  var extra: [String: Double] = [:]
}

func report(_ result: BenchResult) {
  let encoder = JSONEncoder()
  encoder.outputFormatting = [.sortedKeys]
  let line = String(decoding: try! encoder.encode(result), as: UTF8.self)
  print("BENCH \(line)")
}

func nanoseconds(_ d: Duration) -> Double {
  let (s, atto) = d.components
  return Double(s) * 1e9 + Double(atto) / 1e9
}

func percentile(_ sorted: [Double], _ p: Double) -> Double {
  guard !sorted.isEmpty else { return .nan }
  let rank = p * Double(sorted.count - 1)
  let low = Int(rank.rounded(.down))
  let high = min(low + 1, sorted.count - 1)
  return sorted[low] + (sorted[high] - sorted[low]) * (rank - Double(low))
}

let clock = ContinuousClock()
let warmups = 200
let runs = 5

/// 200 warm-up iterations (or `n`, if fewer), then 5 runs of `n`. Each
/// sample is one operation, or the mean of `batch` operations when one is
/// too short to time alone.
func measure(
  _ workload: String, _ bridge: String, n: Int, batch: Int = 1, emit: Bool = true,
  _ body: () async throws -> Void
) async throws -> BenchResult {
  for _ in 0..<min(warmups, n) { try await body() }
  var samples: [Double] = []
  var means: [Double] = []
  samples.reserveCapacity(runs * n / batch)
  for _ in 0..<runs {
    let runStart = clock.now
    var done = 0
    while done < n {
      let start = clock.now
      for _ in 0..<batch { try await body() }
      samples.append(nanoseconds(clock.now - start) / Double(batch))
      done += batch
    }
    means.append(nanoseconds(clock.now - runStart) / Double(n))
  }
  samples.sort()
  let result = BenchResult(
    workload: workload, bridge: bridge, n: n, runs: runs,
    median_ns: percentile(samples, 0.5), p95_ns: percentile(samples, 0.95), run_means_ns: means)
  if emit { report(result) }
  return result
}

/// The process's current footprint and its peak so far, in bytes.
func memory() -> (footprint: Double, peak: Double) {
  var info = task_vm_info_data_t()
  var count = mach_msg_type_number_t(
    MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<integer_t>.size)
  let status = withUnsafeMutablePointer(to: &info) {
    $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
      task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count)
    }
  }
  guard status == KERN_SUCCESS else { return (.nan, .nan) }
  return (Double(info.phys_footprint), Double(info.resident_size_peak))
}

/// `image` drawn into an sRGB RGBA bitmap; the pixel at (x, y), top row
/// first. Drawing makes Core Graphics honour the image's declared format.
func drawnPixel(_ image: CGImage, x: Int, y: Int) -> [UInt8] {
  let (width, height) = (image.width, image.height)
  var bytes = [UInt8](repeating: 0, count: width * height * 4)
  bytes.withUnsafeMutableBytes { raw in
    let context = CGContext(
      data: raw.baseAddress, width: width, height: height, bitsPerComponent: 8,
      bytesPerRow: width * 4, space: CGColorSpace(name: CGColorSpace.sRGB)!,
      bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
  }
  let start = (y * width + x) * 4
  return Array(bytes[start..<start + 4])
}

/// A viewer with search and render over `fixture`, and both bridges' handles.
struct Opened {
  let viewer: PDFViewer
  let document: PDFDocument
  let pages: [PageInfo]
  let search: SearchCapability
  let render: RenderCapability
  let manualSearch: ManualSearch
  let manualRender: ManualRender

  init(_ fixture: String) async throws {
    viewer = try PDFViewer(ViewerConfig(search: SearchConfig(), render: RenderConfig()))
    try await viewer.start()
    document = try await viewer.open(Fixture.data(fixture))
    pages = try await viewer.pages(of: document)
    search = try await viewer.search(in: document)
    render = try await viewer.render(in: document)
    manualSearch = try await ManualSearch.open(viewer, document)
    manualRender = try await ManualRender.open(viewer, document)
  }
}

func ratio(_ uniffi: BenchResult, _ manual: BenchResult) -> Double {
  uniffi.median_ns / manual.median_ns
}

@Suite(.serialized, .timeLimit(.minutes(30)))
struct BridgeBenchmarks {
  @Test func noop() async throws {
    _ = try await measure("noop", "uniffi", n: 100_000, batch: 100) { PDFViewer.benchNoop() }
    _ = try await measure("noop", "manual", n: 100_000, batch: 100) { ManualBench.noop() }
  }

  @Test func noopHop() async throws {
    let o = try await Opened("report")
    _ = try await measure("noop_hop", "uniffi", n: 10_000) { try await o.viewer.benchNoopHop() }
    _ = try await measure("noop_hop", "manual", n: 10_000) {
      try await ManualBench.noopHop(o.viewer)
    }
  }

  @Test func hitCount() async throws {
    let o = try await Opened("report")
    let found = try await o.search.search(SearchQuery(text: "the"))
    #expect(try await o.manualSearch.hitCount() == found.hitCount)
    #expect(try await o.search.hitCount() == found.hitCount)
    _ = try await measure("hit_count", "uniffi", n: 10_000) { _ = try await o.search.hitCount() }
    _ = try await measure("hit_count", "manual", n: 10_000) {
      _ = try await o.manualSearch.hitCount()
    }
  }

  @Test func hits() async throws {
    let o = try await Opened("report")
    let found = try await o.search.search(SearchQuery(text: "e"))
    let typed = try await o.search.hits()
    let manual = try await o.manualSearch.hits()
    #expect(typed.count == Int(found.hitCount))
    #expect(manual.count == typed.count)
    #expect(SearchHit(manual[0]) == typed[0])
    #expect(manual.map(SearchHit.init) == typed)
    let segments = typed.reduce(0) { $0 + $1.segments.count }
    let extra = ["hits": Double(typed.count), "segments": Double(segments)]

    var u = try await measure("hits_lazy", "uniffi", n: 200, emit: false) { _ = try await o.search.hits() }
    u.extra = extra
    report(u)
    var m = try await measure("hits_lazy", "manual", n: 200, emit: false) {
      let hits = try await o.manualSearch.hits()
      _ = hits.count
    }
    m.extra = extra
    report(m)
    var e = try await measure("hits_eager", "manual", n: 200, emit: false) {
      _ = Array(try await o.manualSearch.hits().map(SearchHit.init))
    }
    e.extra = extra
    report(e)
  }

  @Test func search() async throws {
    let o = try await Opened("report")
    let typed = try await o.search.search(SearchQuery(text: "the"))
    let manual = try await o.manualSearch.search("the")
    #expect(manual == typed.hitCount)
    _ = try await measure("search", "uniffi", n: 50) {
      _ = try await o.search.search(SearchQuery(text: "the"))
    }
    _ = try await measure("search", "manual", n: 50) { _ = try await o.manualSearch.search("the") }
  }

  @Test func findAll() async throws {
    let o = try await Opened("report")
    let typed = try await o.search.findAll(SearchQuery(text: "e"))
    let manual = try await o.manualSearch.findAll("e")
    #expect(manual.map(SearchHit.init) == typed)
    _ = try await measure("find_all", "uniffi", n: 20) {
      _ = try await o.search.findAll(SearchQuery(text: "e"))
    }
    _ = try await measure("find_all", "manual", n: 20) {
      _ = Array(try await o.manualSearch.findAll("e").map(SearchHit.init))
    }
  }

  /// 20 session scans of "e", each with a fresh subscription: the time from
  /// starting the scan to the first progress event, the events per scan,
  /// and progress events a second over the scan.
  @Test func progressEvents() async throws {
    let o = try await Opened("report")
    struct Scan {
      var first: Double = .nan
      var total: Double = .nan
      var events = 0
    }

    func summarize(_ bridge: String, _ scans: [Scan]) {
      let firsts = scans.map(\.first).sorted()
      let totals = scans.map(\.total)
      let events = scans.reduce(0) { $0 + $1.events }
      let seconds = totals.reduce(0, +) / 1e9
      report(
        BenchResult(
          workload: "progress_events", bridge: bridge, n: scans.count, runs: 1,
          median_ns: percentile(firsts, 0.5), p95_ns: percentile(firsts, 0.95),
          run_means_ns: [totals.reduce(0, +) / Double(totals.count)],
          extra: [
            "events_per_scan": Double(events) / Double(scans.count),
            "events_per_s": Double(events) / seconds,
            "scan_median_ns": percentile(totals.sorted(), 0.5),
          ]))
    }

    var uniffi: [Scan] = []
    for i in 0..<(20 + 20) {
      let events = o.search.events
      var scan = Scan()
      let start = clock.now
      async let result = o.search.search(SearchQuery(text: "e"))
      for await event in events {
        switch event {
        case .progress:
          scan.events += 1
          if scan.first.isNaN { scan.first = nanoseconds(clock.now - start) }
        case .completed:
          scan.total = nanoseconds(clock.now - start)
        default: break
        }
        if !scan.total.isNaN { break }
      }
      _ = try await result
      if i >= 20 { uniffi.append(scan) }
    }

    var manual: [Scan] = []
    for i in 0..<(20 + 20) {
      let events = o.manualSearch.events
      var scan = Scan()
      let start = clock.now
      async let result = o.manualSearch.search("e")
      for await event in events {
        switch event {
        case .progress:
          scan.events += 1
          if scan.first.isNaN { scan.first = nanoseconds(clock.now - start) }
        case .completed:
          scan.total = nanoseconds(clock.now - start)
        default: break
        }
        if !scan.total.isNaN { break }
      }
      _ = try await result
      if i >= 20 { manual.append(scan) }
    }

    #expect(uniffi.allSatisfy { $0.events > 0 })
    #expect(manual.map(\.events) == uniffi.map(\.events))
    summarize("uniffi", uniffi)
    summarize("manual", manual)
  }

  /// The `colors` fixture is four 20-point squares: red, green, blue, then
  /// (0.2, 0.4, 0.6). Both bridges must name the bytes' real order.
  @Test func rasterFormatsAgree() async throws {
    let o = try await Opened("colors")
    let page = o.pages[0].page
    let typed = try await o.render.renderPage(page, options: RenderPageOptions(scale: 0.5))
    let manual = try await o.manualRender.renderPage(page, scale: 0.5)
    #expect(manual.width == typed.width && manual.height == typed.height)
    #expect(drawnPixel(manual, x: 5, y: 5) == [255, 0, 0, 255])
    #expect(drawnPixel(typed.image, x: 5, y: 5) == [255, 0, 0, 255])
    #expect(drawnPixel(manual, x: 35, y: 5) == drawnPixel(typed.image, x: 35, y: 5))
  }

  /// report.pdf's first page at 1x, 2x and 3x. Each bridge records its
  /// footprint growth while it runs and the process's peak after it.
  @Test(arguments: [1.0, 2.0, 3.0])
  func raster(scale: Double) async throws {
    let o = try await Opened("report")
    let page = o.pages[0].page
    let typed = try await o.render.renderPage(page, options: RenderPageOptions(scale: scale))
    let manual = try await o.manualRender.renderPage(page, scale: scale)
    #expect(manual.width == typed.width && manual.height == typed.height)
    let name = "raster_\(Int(scale))x"
    let pixels = ["width": Double(typed.width), "height": Double(typed.height)]

    var before = memory()
    var footprint = before.footprint
    var m = try await measure(name, "manual", n: 50, emit: false) {
      let image = try await o.manualRender.renderPage(page, scale: scale)
      footprint = max(footprint, memory().footprint)
      _ = image.width
    }
    m.extra = pixels.merging([
      "footprint_growth_bytes": footprint - before.footprint,
      "peak_growth_bytes": memory().peak - before.peak,
    ]) { a, _ in a }
    report(m)

    before = memory()
    footprint = before.footprint
    var u = try await measure(name, "uniffi", n: 50, emit: false) {
      let rendered = try await o.render.renderPage(page, options: RenderPageOptions(scale: scale))
      footprint = max(footprint, memory().footprint)
      _ = rendered.image.width
    }
    u.extra = pixels.merging([
      "footprint_growth_bytes": footprint - before.footprint,
      "peak_growth_bytes": memory().peak - before.peak,
    ]) { a, _ in a }
    report(u)
  }
}
