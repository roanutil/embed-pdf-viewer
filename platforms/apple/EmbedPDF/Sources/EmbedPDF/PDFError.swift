internal import EmbedPDFBindings

public struct PDFError: Error, Sendable, Equatable {
  public let name: String
  public let code: String?
  public let detail: String

  init(_ error: ViewerError) {
    switch error {
    case let .Kernel(name, code, detail):
      self.name = name
      self.code = code
      self.detail = detail
    case .Closed:
      name = "Closed"
      code = "instance-closed"
      detail = "The viewer has shut down."
    }
  }

  init(_ error: any Error) {
    if let error = error as? ViewerError {
      self.init(error)
    } else if let error = error as? PDFError {
      self = error
    } else {
      self = PDFError(name: "Error", code: nil, detail: "\(error)")
    }
  }

  init(name: String, code: String?, detail: String) {
    self.name = name
    self.code = code
    self.detail = detail
  }
}

func bridged<T>(_ body: () async throws -> T) async throws(PDFError) -> T {
  do {
    return try await body()
  } catch {
    throw PDFError(error)
  }
}
