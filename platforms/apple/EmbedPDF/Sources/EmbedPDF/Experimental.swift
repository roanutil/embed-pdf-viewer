// Task 11's experimental bridge benchmarks reach the host through these.
// Package.swift defines EMBEDPDF_EXPERIMENTAL_BRIDGE only when
// EMBEDPDF_EXPERIMENTAL_BRIDGE=1 is set at manifest evaluation, against an
// XCFramework built with `build-xcframework.sh --experimental-manual-bridge`.
// A normal build compiles nothing here.
#if EMBEDPDF_EXPERIMENTAL_BRIDGE
  internal import EmbedPDFBindings

  extension PDFViewer {
    /// The token the manual bridge's `epdf_*_open` calls take.
    package var experimentalKernelToken: UInt64 {
      host.experimentalKernelToken()
    }

    /// uniffi plus the hop to the kernel thread, and nothing else.
    package func benchNoopHop() async throws(PDFError) {
      try await bridged { try await host.benchNoopHop() }
    }

    /// uniffi's entry cost alone: returns at once on the calling thread.
    package static func benchNoop() {
      EmbedPDFBindings.benchNoop()
    }
  }
#endif
