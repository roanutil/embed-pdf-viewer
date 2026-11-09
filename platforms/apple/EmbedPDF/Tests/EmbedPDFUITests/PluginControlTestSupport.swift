import CustomDump
import Foundation
import Observation
import Testing

func pluginFixture(_ name: String = "report") throws -> Data {
  let url = try #require(Bundle.module.url(forResource: name, withExtension: "pdf", subdirectory: "Fixtures"))
  return try Data(contentsOf: url)
}

@MainActor
func expectObserved<Value: Equatable>(
  _ read: @escaping @MainActor () -> Value,
  equals expected: Value,
  during action: @MainActor () async throws -> Void
) async throws {
  let (stream, continuation) = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingNewest(1))
  @MainActor func arm() {
    withObservationTracking { _ = read() } onChange: { continuation.yield(()) }
  }
  arm()
  defer { continuation.finish() }
  try await action()
  var iterator = stream.makeAsyncIterator()
  while read() != expected {
    guard await iterator.next() != nil else { throw CancellationError() }
    arm()
  }
  expectNoDifference(read(), expected)
}
