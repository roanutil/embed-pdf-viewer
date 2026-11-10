import EmbedPDF
import Foundation

@MainActor
func observeCapability<Event: Sendable>(
  viewer: PDFViewer,
  events: AsyncStream<Event>,
  initial: @escaping @MainActor @Sendable () async -> Void,
  refresh: @escaping @MainActor @Sendable () async -> Void,
  receive: @escaping @MainActor @Sendable (Event) async -> Void
) async {
  let revisions = viewer.observeStateRevisions()
  await withTaskGroup(of: Void.self) { group in
    group.addTask { for await event in events { await receive(event) } }
    group.addTask { for await _ in revisions { await refresh() } }
    await initial()
    _ = await group.next()
    group.cancelAll()
  }
}

func controlError(_ error: any Error) -> PDFError {
  if let error = error as? PDFError { return error }
  return PDFError(
    name: String(describing: type(of: error)),
    code: error is CancellationError ? .operationCancelled : .operationFailed,
    detail: error.localizedDescription)
}
