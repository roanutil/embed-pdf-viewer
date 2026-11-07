import Foundation
import SwiftUI
import UniformTypeIdentifiers

public struct AttachmentExportDocument: FileDocument {
  public static let readableContentTypes: [UTType] = [.data]
  public var data: Data

  public init(data: Data) { self.data = data }
  public init(configuration: ReadConfiguration) throws {
    guard let data = configuration.file.regularFileContents else { throw CocoaError(.fileReadCorruptFile) }
    self.data = data
  }
  public func fileWrapper(configuration: WriteConfiguration) throws -> FileWrapper {
    FileWrapper(regularFileWithContents: data)
  }
  public static func contentType(for mimeType: String?) -> UTType {
    guard let mimeType, let type = UTType(mimeType: mimeType), type.isDeclared else { return .data }
    return type
  }
}
