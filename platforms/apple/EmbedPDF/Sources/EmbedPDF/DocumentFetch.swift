public import EmbedPDFBindings
import Foundation

/// The default transport for URL document sources. The kernel owns loading
/// tabs, HTTP refusals, PDF validation and retry; URLSession owns the request.
/// Supply a session to customize headers, cookies, authentication or transport.
public final class URLSessionDocumentFetcher: DocumentFetcher, Sendable {
  private let session: URLSession

  public init(session: URLSession = .shared) {
    self.session = session
  }

  public func fetch(url: String, completion: DocumentFetchCompletion) -> any DocumentFetchCancellation {
    guard let url = URL(string: url) else {
      completion.failWith(failure: HostFailure(name: "InvalidURL", code: "invalid-input", detail: "The document URL is invalid."))
      return EmptyDocumentFetchCancellation()
    }
    let task = session.dataTask(with: url) { data, response, error in
      if let error {
        let cancelled = (error as NSError).code == NSURLErrorCancelled && (error as NSError).domain == NSURLErrorDomain
        completion.failWith(failure: HostFailure(name: cancelled ? "AbortError" : "URLSessionError", code: cancelled ? "operation-cancelled" : nil, detail: error.localizedDescription))
      } else if let data, let response {
        let status = (response as? HTTPURLResponse)?.statusCode ?? 200
        guard let status = UInt16(exactly: status) else {
          completion.failWith(failure: HostFailure(name: "InvalidResponse", code: "operation-failed", detail: "The response has an invalid HTTP status."))
          return
        }
        completion.finish(status: status, bytes: data)
      } else {
        completion.failWith(failure: HostFailure(name: "InvalidResponse", code: "operation-failed", detail: "The document request returned no response."))
      }
    }
    task.resume()
    return URLSessionFetchCancellation(task: task)
  }
}

private final class URLSessionFetchCancellation: DocumentFetchCancellation, Sendable {
  let task: URLSessionDataTask
  init(task: URLSessionDataTask) { self.task = task }
  func cancel() { task.cancel() }
}

private final class EmptyDocumentFetchCancellation: DocumentFetchCancellation, Sendable {
  func cancel() {}
}
