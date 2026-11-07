import CustomDump
import EmbedPDF
import EmbedPDFUI
import Foundation
import Testing
import UniformTypeIdentifiers

@MainActor
@Suite(.serialized, .timeLimit(.minutes(1)))
struct AttachmentModelTests {
  func opened(scope: [String]? = nil) async throws -> (PDFViewer, DocumentInfo, AttachmentCapability, AttachmentModel) {
    let viewer = try PDFViewer(ViewerConfig(scope: scope, interaction: true, annotation: "", attachment: true))
    try await viewer.start()
    let document = try await viewer.open(pluginFixture())
    let cap = try await viewer.attachments(for: document)
    return (viewer, document, cap, AttachmentModel(viewer: viewer, attachments: cap))
  }

  @Test func importDownloadExportAndBothHomesUseOwnedBytes() async throws {
    let (viewer, document, _, model) = try await opened()
    await model.reload()
    let bytes = Data([0, 255, 13, 10])
    await model.add([AttachmentFileSource(data: bytes, name: "same.bin"), AttachmentFileSource(data: Data(), name: "same.bin")])
    expectNoDifference(model.files.count, 2)
    #expect(model.files[0].id != model.files[1].id)
    let file = try #require(model.files.first { $0.name == "same.bin" })
    await model.select([file])
    expectNoDifference(model.selectedFiles.map(\.id), [file.id])
    #expect(model.canRemoveSelection)
    let content = try #require(await model.download(file))
    expectNoDifference(content.data, bytes)
    expectNoDifference(content.mimeType, nil)
    let exported = AttachmentExportDocument(data: content.data)
    expectNoDifference(exported.data, bytes)
    expectNoDifference(AttachmentExportDocument.contentType(for: "application/x-unknown-example"), .data)
    let temporary = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString + ".bin")
    try bytes.write(to: temporary)
    defer { try? FileManager.default.removeItem(at: temporary) }
    await model.importFiles([temporary])
    expectNoDifference(model.error, nil)
    let imported = try #require(model.files.first { $0.name == temporary.lastPathComponent })
    let importedContent = try #require(await model.download(imported))
    expectNoDifference(importedContent.data, bytes)
    let page = try await viewer.pages(of: document)[0].page
    let annotations = try await viewer.annotations(for: document)
    _ = try await annotations.drafts.createAttachment(AnnotationAttachmentFile(bytes: bytes, name: "page.bin"), at: Point(x: 40, y: 40), on: page)
    await model.refresh()
    let pageFile = try #require(model.files.first { if case .page = $0.home { return true }; return false })
    await model.reveal(pageFile)
    expectNoDifference(model.error, nil)
    await model.remove(file)
    #expect(!model.files.contains { $0.id == file.id })
    _ = AttachmentPanel(model: model)
    try await viewer.destroy()
  }

  @Test func independentModelsObserveImportAndClose() async throws {
    let (viewer, document, cap, first) = try await opened()
    let second = AttachmentModel(viewer: viewer, attachments: cap)
    let one = Task { await first.task() }
    let two = Task { await second.task() }
    defer { one.cancel(); two.cancel() }
    try await expectObserved({ first.status }, equals: .ready) {}
    try await expectObserved({ second.status }, equals: .ready) {}
    try await expectObserved({ first.files.count }, equals: 1) {
      try await expectObserved({ second.files.count }, equals: 1) {
        _ = try await cap.add([AttachmentFileSource(data: Data([42]), name: "External")])
      }
    }
    try await viewer.close(document)
    await one.value; await two.value
    expectNoDifference(first.files.count, 0)
    expectNoDifference(second.canDownload, false)
    let replacement = try await viewer.open(pluginFixture())
    let next = AttachmentModel(viewer: viewer, attachments: try await viewer.attachments(for: replacement))
    await next.reload()
    expectNoDifference(next.error, nil)
    expectNoDifference(next.files.count, 0)
    try await viewer.destroy()
  }

  @Test func fileIOErrorsAndCancellationAndPermissionsAreVisible() async throws {
    let (viewer, _, _, model) = try await opened()
    await model.importFiles([URL(fileURLWithPath: "/nonexistent/embedpdf-parity-file")])
    #expect(model.error != nil)
    expectNoDifference(model.performing, false)
    let task = Task { withUnsafeCurrentTask { $0?.cancel() }; await model.add([AttachmentFileSource(data: Data(), name: "Cancelled")]) }
    await task.value
    expectNoDifference(model.error?.code, "operation-cancelled")
    try await viewer.destroy()
    let (restricted, _, _, denied) = try await opened(scope: ["doc.open"])
    await denied.reload()
    #expect(!denied.canAdd)
    await denied.add([AttachmentFileSource(data: Data(), name: "Denied")])
    expectNoDifference(denied.error?.code, "permission-denied")
    try await restricted.destroy()
  }
}
