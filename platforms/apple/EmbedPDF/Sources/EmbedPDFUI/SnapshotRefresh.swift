@MainActor
final class SnapshotRefresh {
  private var version: UInt64 = 0

  func invalidate() { version &+= 1 }

  func run<Value>(
    load: @MainActor () async throws -> Value,
    apply: @MainActor (Value) -> Void,
    failure: @MainActor (any Error) -> Void
  ) async {
    version &+= 1
    let request = version
    do {
      let value = try await load()
      guard request == version, !Task.isCancelled else { return }
      apply(value)
    } catch {
      guard request == version, !Task.isCancelled else { return }
      failure(error)
    }
  }
}
