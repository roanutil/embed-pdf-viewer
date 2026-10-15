import EmbedPDF
import Foundation
import Testing

@Suite(.serialized, .timeLimit(.minutes(1)))
struct BaseSuite {}

enum Fixture {
  static func data(_ name: String) throws -> Data {
    let url = try #require(Bundle.module.url(forResource: name, withExtension: "pdf", subdirectory: "Fixtures"))
    return try Data(contentsOf: url)
  }

  static func text(_ name: String, ext: String) throws -> String {
    let url = try #require(Bundle.module.url(forResource: name, withExtension: ext, subdirectory: "Fixtures"))
    return try String(contentsOf: url, encoding: .utf8)
  }
}

/// The `<member>CancelsWhenTheTaskIsCancelled` check: run `body` in a task
/// that is already cancelled when `body` starts, and expect
/// `operation-cancelled`.
///
/// `bridgedCancellable` installs its handler in a cancelled task, so the
/// call's `CancelToken` is cancelled before the call is posted, and the
/// kernel job (`call_promise_cancellable`) skips the member and answers
/// `operation-cancelled`: every time, and without the member's effect. A
/// cancel that arrives while the job runs aborts the member's promise
/// instead, and whether that wins against the engine is timing, so these
/// tests don't try; the Rust tests hold an engine reply to pin that case.
func expectCancelledBeforeItStarts<T: Sendable>(
  _ body: @escaping @Sendable () async throws -> T,
  sourceLocation: SourceLocation = #_sourceLocation
) async {
  let task = Task {
    withUnsafeCurrentTask { $0?.cancel() }
    return try await body()
  }
  let result = await task.result
  switch result {
  case .success:
    Issue.record("a call made in a cancelled task finished", sourceLocation: sourceLocation)
  case let .failure(error):
    #expect((error as? PDFError)?.code == "operation-cancelled", "\(error)", sourceLocation: sourceLocation)
  }
}
