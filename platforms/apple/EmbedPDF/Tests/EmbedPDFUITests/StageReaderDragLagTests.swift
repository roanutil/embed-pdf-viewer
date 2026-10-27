import EmbedPDF
import EmbedPDFUI
import Foundation
import Observation
import Testing

/// Timestamps every write of `model.camera`: one per `refresh`, whether a
/// drag call or the stage event loop made it.
@MainActor
final class CameraWrites {
  var times: [ContinuousClock.Instant] = []

  func track(_ model: StageReaderModel) {
    withObservationTracking {
      _ = model.camera
    } onChange: { [weak self] in
      // `camera` is written on the main actor, and `onChange` runs in its
      // `willSet`, so this is on the main actor too.
      MainActor.assumeIsolated {
        self?.times.append(.now)
        self?.track(model)
      }
    }
  }
}

/// Opt in with EMBEDPDF_LATENCY=1 (TEST_RUNNER_EMBEDPDF_LATENCY=1 through
/// xcodebuild): it drives a two-second drag, and its numbers mean something
/// only on an idle machine.
@MainActor
@Suite(.serialized, .enabled(if: ProcessInfo.processInfo.environment["EMBEDPDF_LATENCY"] == "1"))
struct StageReaderDragLagTests {
  /// A drag at 120 Hz through the path `StageReaderView` uses: each
  /// `.onChanged` starts its own task that calls `dragChanged`. Measures each
  /// call from the change to its return (its turn in the drag chain, the
  /// pan, its refresh), how long the chain runs on after the last change,
  /// and how long the event loop's refreshes (one per `cameraChanged`) keep
  /// writing the camera after that.
  @Test func a120HzDragKeepsUp() async throws {
    let viewer = try PDFViewer(ViewerConfig(render: RenderConfig(), stage: StageConfig()))
    try await viewer.start()
    let url = try #require(Bundle.module.url(forResource: "report", withExtension: "pdf", subdirectory: "Fixtures"))
    let document = try await viewer.open(try Data(contentsOf: url))
    let model = StageReaderModel(
      stage: try await viewer.stage(for: document), render: try await viewer.render(in: document))
    let loop = Task { await model.task() }
    defer { loop.cancel() }
    await model.sizeChanged(CGSize(width: 390, height: 844), displayScale: 3)
    try await Task.sleep(for: .milliseconds(200))

    let writes = CameraWrites()
    writes.track(model)
    let clock = ContinuousClock()
    let count = 240
    let period = Duration.nanoseconds(8_333_333)
    var calls: [Task<Duration, Never>] = []
    let start = clock.now
    for i in 1...count {
      let issued = clock.now
      // 2 points a frame: 240 points a second, a slow scroll.
      calls.append(Task { @MainActor in
        await model.dragChanged(translation: CGSize(width: 0, height: -2 * Double(i)))
        return clock.now - issued
      })
      try await Task.sleep(until: start + period * i, clock: clock)
    }
    let lastChange = clock.now
    var latencies: [Duration] = []
    for call in calls { latencies.append(await call.value) }
    let chainDrained = clock.now - lastChange
    // Quiet for 250 ms: the event loop has nothing left to refresh.
    var quietSince = writes.times.last ?? lastChange
    while clock.now - quietSince < .milliseconds(250) {
      try await Task.sleep(for: .milliseconds(10))
      quietSince = max(quietSince, writes.times.last ?? quietSince)
    }
    let loopTail = (writes.times.last ?? lastChange) - lastChange
    await model.dragEnded(velocity: .zero)

    latencies.sort()
    let ms = { (d: Duration) in String(format: "%.2f", Double(d.components.attoseconds) / 1e15 + Double(d.components.seconds) * 1000) }
    let p = { (q: Double) in latencies[min(latencies.count - 1, Int(Double(latencies.count) * q))] }
    print(
      "stage drag 120Hz: changes=\(count) p50=\(ms(p(0.5)))ms p95=\(ms(p(0.95)))ms p99=\(ms(p(0.99)))ms max=\(ms(latencies.last!))ms"
        + " chainDrained=\(ms(chainDrained))ms cameraWrites=\(writes.times.count) loopTail=\(ms(loopTail))ms")
    let camera = try await model.stage.camera()
    #expect(model.camera == camera)
  }
}
