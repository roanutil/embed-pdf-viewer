import os

/// Intervals for the calls worth timing, under Points of Interest so they show
/// up in Instruments' Time Profiler and os_signpost tracks without extra setup.
let signposter = OSSignposter(subsystem: "com.embedpdf", category: .pointsOfInterest)

func measured<T>(_ name: StaticString, _ body: () async throws(PDFError) -> T) async throws(PDFError)
  -> T
{
  let state = signposter.beginInterval(name, id: signposter.makeSignpostID())
  defer { signposter.endInterval(name, state) }
  return try await body()
}

func measured<T>(_ name: StaticString, _ body: () throws(PDFError) -> T) throws(PDFError) -> T {
  let state = signposter.beginInterval(name, id: signposter.makeSignpostID())
  defer { signposter.endInterval(name, state) }
  return try body()
}
