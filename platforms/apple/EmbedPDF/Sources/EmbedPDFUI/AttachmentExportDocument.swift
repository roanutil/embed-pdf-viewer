import Foundation
import SwiftUI
import UniformTypeIdentifiers

public struct AttachmentExportDocument: FileDocument {
  public static let readableContentTypes: [UTType] = [.data]
  public static let writableContentTypes: [UTType] = [.data]
  public var data: Data

  public init(data: Data) { self.data = data }
  public init(configuration: ReadConfiguration) throws {
    guard let data = configuration.file.regularFileContents else { throw CocoaError(.fileReadCorruptFile) }
    self.data = data
  }
  public func fileWrapper(configuration: WriteConfiguration) throws -> FileWrapper {
    FileWrapper(regularFileWithContents: data)
  }
  /// Arbitrary attachment bytes use the document's declared writable type.
  /// The suggested filename retains the attachment's original extension.
  public static func contentType(for _: String?) -> UTType { .data }
}
