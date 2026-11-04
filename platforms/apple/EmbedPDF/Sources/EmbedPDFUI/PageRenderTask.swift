/// Owns one page's asynchronous render work, including superseded work still unwinding.
@MainActor
final class PageRenderTask {
  private var task: Task<Void, Never>?
  private var generation = 0

  deinit { task?.cancel() }

  /// Replace current work, waiting for it to finish before touching the next demand.
  func run(_ operation: @escaping @MainActor () async -> Void) async {
    generation += 1
    let current = generation
    let previous = task
    previous?.cancel()
    let next = Task {
      await previous?.value
      guard !Task.isCancelled else { return }
      await operation()
    }
    task = next
    await withTaskCancellationHandler {
      await next.value
    } onCancel: {
      next.cancel()
    }
    if generation == current { task = nil }
  }

  /// Continue observing tile arrivals only when no foreground work owns the page.
  func startIfIdle(_ operation: @escaping @MainActor () async -> Void) {
    guard task == nil else { return }
    generation += 1
    let current = generation
    task = Task { [weak self] in
      guard !Task.isCancelled else { return }
      await operation()
      if self?.generation == current { self?.task = nil }
    }
  }
}
