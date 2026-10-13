# `dispatchPointer` round trip at 120 Hz

A pointer move reaches its Swift handler in well under one 120 Hz frame. Across three runs of
`PointerLatencyTests.movesReachTheirHandlerWithinAFrameAt120Hz`, the worst p95 was 126 µs against a
budget of 8.33 ms, about 1.5% of a frame. The worst single sample was 4.87 ms, still inside the frame.
Plan 02's gestures can build on `dispatch_pointer` as it is.

## Setup

| | |
|---|---|
| Machine | Apple M4 Max, 64 GB. The local dev machine, not the perf Mac. Up to two other agents ran `xcodebuild` at the same time, so the machine was not idle. |
| macOS | 27.0.1 (26A434) |
| Xcode | 27.0 (27A266a) |
| Simulator | iPhone 17, iOS 27.2 (`5AF6C4A7-9E80-4E87-9F1D-D83A59571B13`), the simulator runtime `simctl create` picks by default |
| Rust | 1.98.0, `PROFILE=release` from `build-xcframework.sh` |
| Swift | Debug, the `xcodebuild test` default |
| Code | `cb501ed39` on `research/swift-interaction`, plus the test in the commit that adds this document |

The test registers one Swift `InteractionHandler` at priority 1, sends a down, then 1,200 moves
paced at 8.33 ms with `Task.sleep(until:)`, then an up. It stamps each move with
`ContinuousClock.now` just before `dispatchPointer` and again when `onMove` runs on the kernel
thread. Latency is the difference. That covers the uniffi call, the post to the kernel thread, the
hub's routing, and the callback back into Swift. It excludes UIKit's touch delivery and anything a
real handler does with the sample.

Run it with:

```bash
cd platforms/apple/EmbedPDF
TEST_RUNNER_EMBEDPDF_LATENCY=1 xcodebuild test -scheme EmbedPDF-Package \
  -destination 'platform=iOS Simulator,id=<UDID>' \
  -only-testing:EmbedPDFTests/BaseSuite/PointerLatencyTests
```

The plain `EMBEDPDF_LATENCY=1` the plan names doesn't reach the test runner. `xcodebuild` forwards
only variables prefixed `TEST_RUNNER_`.

## Results

| Run | p50 | p95 | max |
|---|---|---|---|
| 1 | 47.9 µs | 73.2 µs | 4.87 ms |
| 2 | 54.5 µs | 126.3 µs | 0.93 ms |
| 3 | 43.7 µs | 89.0 µs | 2.39 ms |

p50 sits near the 11 µs to 12 µs kernel-thread hop that `swift-bridge-benchmarks.md` measured, plus
the callback into Swift and the record lift on both sides. The max varies by 5x between runs and
lands on no particular sample, which reads as scheduler noise from the concurrent builds rather than
anything in the path. We haven't profiled it.

The simulator stands in for a device here, as it does for the bridge numbers. A device row waits on
B5's signed run. The cost of trusting these numbers now is that an A-series phone's slower cores
could stretch the tail. With p95 at 1.5% of the frame, it would take a slowdown of more than 60x to
miss the budget.
