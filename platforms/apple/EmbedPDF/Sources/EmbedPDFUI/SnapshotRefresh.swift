@MainActor
final class SnapshotRefresh {
  private var version: UInt64 = 0
  /// Runs after a read finishes and before its result is applied or dropped.
  /// Tests hold a read here so a newer one can overtake it.
  var readFinished: (@MainActor () async -> Void)?

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
      await readFinished?()
      guard request == version, !Task.isCancelled else { return }
      apply(value)
    } catch {
      await readFinished?()
      guard request == version, !Task.isCancelled else { return }
      failure(error)
    }
  }
}
