import CoreGraphics
import Foundation
import os
import PSPDFKit

// The same signpost names EmbedPDF emits, so the two traces line up:
// `renderPage.engine` here is the counterpart of EmbedPDF's `renderPage.rust`.
let signposter = OSSignposter(subsystem: "com.embedpdf", category: .pointsOfInterest)

func measured<T>(_ name: StaticString, _ body: () async throws -> T) async rethrows -> T {
  let state = signposter.beginInterval(name, id: signposter.makeSignpostID())
  defer { signposter.endInterval(name, state) }
  return try await body()
}

func measured<T>(_ name: StaticString, _ body: () throws -> T) rethrows -> T {
  let state = signposter.beginInterval(name, id: signposter.makeSignpostID())
  defer { signposter.endInterval(name, state) }
  return try body()
}

struct DemoError: Error, Sendable {
  let detail: String
  var needsPassword = false
}

struct PageInfo: Hashable, Sendable {
  let index: Int
  let width: Double
  let height: Double
}

struct RenderedPage: @unchecked Sendable {
  let image: CGImage
  let scale: Double
}

struct DocumentMetadata: Sendable {
  var title, author, subject, keywords, creator, producer, created, modified: String?
}

/// A Nutrient `Document` opened from memory, with the operations the demo
/// times. `Document` is thread-safe for these reads; the wrapper only exists
/// to carry it across tasks.
final class NutrientDocument: @unchecked Sendable {
  let document: Document
  let name: String

  init(data: Data, name: String, password: String?) throws(DemoError) {
    document = measured("open") {
      let document = Document(dataProviders: [DataContainerProvider(data: data)])
      _ = document.pageCount
      return document
    }
    self.name = name
    if document.isLocked {
      guard let password, document.unlock(withPassword: password) else {
        throw DemoError(detail: "DocPasswordRequired", needsPassword: true)
      }
    }
    guard document.isValid else {
      throw DemoError(detail: "Nutrient could not open \(name)")
    }
  }

  func pages() -> [PageInfo] {
    measured("pages") {
      (0..<Int(document.pageCount)).compactMap { index in
        document.pageInfoForPage(at: PageIndex(index)).map {
          PageInfo(index: index, width: $0.size.width, height: $0.size.height)
        }
      }
    }
  }

  /// Render through Nutrient's render queue with its default cache policy,
  /// the counterpart of EmbedPDF's raster store.
  func renderPage(_ page: PageInfo, scale: Double) async throws -> RenderedPage {
    try await measured("renderPage") {
      let request = MutableRenderRequest(document: document)
      request.pageIndex = PageIndex(page.index)
      request.imageSize = CGSize(width: page.width * scale, height: page.height * scale)
      request.imageScale = 1
      let task = try RenderTask(request: request)
      let image = try await measured("renderPage.engine") { try await run(task) }
      guard let cgImage = image.cgImage else { throw DemoError(detail: "no CGImage") }
      return RenderedPage(image: cgImage, scale: scale)
    }
  }

  func metadata() -> DocumentMetadata {
    let info = PDFMetadata(document: document)
    func value(_ key: PDFMetadata.Key) -> String? {
      guard let object = info?[key] else { return nil }
      if let date = object as? Date { return date.formatted() }
      return "\(object)"
    }
    return DocumentMetadata(
      title: value(.title), author: value(.author), subject: value(.subject),
      keywords: value(.keywords), creator: value(.creator), producer: value(.producer),
      created: value(.creationDate), modified: value(.modificationDate))
  }

  private func run(_ renderTask: RenderTask) async throws -> UIImage {
    // RenderTask is thread-safe (its properties are atomic) but not marked
    // Sendable; the box carries it into the cancellation handler.
    let box = UncheckedBox(renderTask)
    let once = Once<UIImage>()
    return try await withTaskCancellationHandler {
      let task = box.value
      return try await withCheckedThrowingContinuation { continuation in
        once.attach(continuation)
        task.completionHandler = { image, error in
          if let image {
            once.finish(.success(image))
          } else {
            once.finish(.failure(error ?? DemoError(detail: "render failed")))
          }
        }
        SDK.shared.renderManager.renderQueue.schedule(task)
      }
    } onCancel: {
      box.value.cancel()
      once.finish(.failure(CancellationError()))
    }
  }
}

struct UncheckedBox<Value>: @unchecked Sendable {
  let value: Value
  init(_ value: Value) { self.value = value }
}

/// Resumes a continuation exactly once, whichever of completion and
/// cancellation arrives first; a cancelled render task may never complete.
final class Once<Value: Sendable>: @unchecked Sendable {
  private let lock = NSLock()
  private var continuation: CheckedContinuation<Value, any Error>?
  private var pending: Result<Value, any Error>?
  private var done = false

  func attach(_ continuation: CheckedContinuation<Value, any Error>) {
    lock.lock()
    if let pending {
      self.pending = nil
      lock.unlock()
      continuation.resume(with: pending)
      return
    }
    self.continuation = continuation
    lock.unlock()
  }

  func finish(_ result: Result<Value, any Error>) {
    lock.lock()
    guard !done else {
      lock.unlock()
      return
    }
    done = true
    guard let continuation else {
      pending = result
      lock.unlock()
      return
    }
    self.continuation = nil
    lock.unlock()
    continuation.resume(with: result)
  }
}
